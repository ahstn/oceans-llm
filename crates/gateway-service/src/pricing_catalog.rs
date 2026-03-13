use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::Context;
use gateway_core::{
    GatewayError, ModelPricingRecord, ModelRoute, Money4, PricingCatalogCacheRecord,
    PricingCatalogRepository, PricingLimits, PricingModalities, PricingProvenance,
    PricingResolution, PricingUnpricedReason, ProviderConnection, ResolvedModelPricing,
};
use reqwest::{
    Client, StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

pub const DEFAULT_PRICING_CATALOG_SOURCE_URL: &str = "https://models.dev/api.json";
pub const PRICING_CATALOG_CACHE_KEY: &str = "models_dev_supported_v1";
pub const DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const SUPPORTED_PRICING_PROVIDER_IDS: [&str; 3] =
    ["google-vertex", "google-vertex-anthropic", "openai"];

const REMOTE_SOURCE: &str = "models_dev_api";
const VENDORED_SOURCE: &str = "vendored_models_dev";
const VENDORED_FALLBACK_JSON: &str = include_str!("../data/pricing_catalog_fallback.json");

#[derive(Clone)]
pub struct PricingCatalog<R> {
    repo: Arc<R>,
    client: Client,
    source_url: String,
    catalog_key: String,
    refresh_interval: Duration,
    fallback_snapshot: PricingCatalogSnapshot,
}

impl<R> PricingCatalog<R>
where
    R: PricingCatalogRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self::with_options(
            repo,
            DEFAULT_PRICING_CATALOG_SOURCE_URL.to_string(),
            PRICING_CATALOG_CACHE_KEY.to_string(),
            DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL,
        )
    }

    #[must_use]
    pub fn with_options(
        repo: Arc<R>,
        source_url: String,
        catalog_key: String,
        refresh_interval: Duration,
    ) -> Self {
        Self {
            repo,
            client: Client::new(),
            source_url,
            catalog_key,
            refresh_interval,
            fallback_snapshot: load_vendored_fallback_snapshot(),
        }
    }

    #[cfg(test)]
    fn with_fallback_snapshot(
        repo: Arc<R>,
        source_url: String,
        catalog_key: String,
        refresh_interval: Duration,
        fallback_snapshot: PricingCatalogSnapshot,
    ) -> Self {
        Self {
            repo,
            client: Client::new(),
            source_url,
            catalog_key,
            refresh_interval,
            fallback_snapshot,
        }
    }

    pub async fn refresh_if_stale(&self) -> Result<(), GatewayError> {
        let current = self.load_stored_snapshot().await?;
        let current_fetched_at = current
            .as_ref()
            .map(|snapshot| snapshot.metadata.fetched_at)
            .unwrap_or(self.fallback_snapshot.metadata.fetched_at);
        let now = OffsetDateTime::now_utc();
        if now
            .unix_timestamp()
            .saturating_sub(current_fetched_at.unix_timestamp())
            < self.refresh_interval.as_secs() as i64
        {
            return Ok(());
        }

        let mut request = self.client.get(&self.source_url);
        if let Some(etag) = current
            .as_ref()
            .and_then(|snapshot| snapshot.metadata.etag.clone())
        {
            request = request.header(IF_NONE_MATCH, etag);
        }

        let response = request.send().await.map_err(|error| {
            GatewayError::Internal(format!("pricing catalog refresh request failed: {error}"))
        })?;
        match response.status() {
            StatusCode::NOT_MODIFIED => {
                if current.is_some() {
                    self.repo
                        .touch_pricing_catalog_cache_fetched_at(&self.catalog_key, now)
                        .await?;
                }
                Ok(())
            }
            StatusCode::OK => {
                let etag = response
                    .headers()
                    .get(ETAG)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let body = response.text().await.map_err(|error| {
                    GatewayError::Internal(format!(
                        "pricing catalog refresh body read failed: {error}"
                    ))
                })?;
                let snapshot = project_models_dev_snapshot(&body, REMOTE_SOURCE, etag, now)?;
                let snapshot_json =
                    serde_json::to_string_pretty(&snapshot.document).map_err(|error| {
                        GatewayError::Internal(format!(
                            "failed serializing pricing catalog snapshot: {error}"
                        ))
                    })?;
                self.repo
                    .upsert_pricing_catalog_cache(&PricingCatalogCacheRecord {
                        catalog_key: self.catalog_key.clone(),
                        source: snapshot.metadata.source.clone(),
                        etag: snapshot.metadata.etag.clone(),
                        fetched_at: snapshot.metadata.fetched_at,
                        snapshot_json,
                    })
                    .await?;
                Ok(())
            }
            status => Err(GatewayError::Internal(format!(
                "pricing catalog refresh failed with HTTP {}",
                status.as_u16()
            ))),
        }
    }

    pub async fn resolve_for_provider_connection(
        &self,
        provider: &ProviderConnection,
        route: &ModelRoute,
        occurred_at: OffsetDateTime,
    ) -> Result<PricingResolution, GatewayError> {
        if let Err(error) = self.refresh_if_stale().await {
            warn!(
                provider_key = %provider.provider_key,
                provider_type = %provider.provider_type,
                error = %error,
                "pricing catalog refresh failed; falling back to cached snapshot"
            );
        }

        let snapshot = self.load_snapshot_from_store_or_fallback().await?;
        self.sync_model_pricing_snapshot(&snapshot).await?;
        let (pricing_provider_id, model_id) = match pricing_target_for_route(provider, route) {
            PricingTarget::Exact {
                pricing_provider_id,
                model_id,
            } => (pricing_provider_id, model_id),
            PricingTarget::Unpriced(reason) => {
                return Ok(PricingResolution::Unpriced { reason });
            }
        };

        let Some(record) = self
            .repo
            .resolve_model_pricing_at(&pricing_provider_id, &model_id, occurred_at)
            .await?
        else {
            return Ok(PricingResolution::Unpriced {
                reason: PricingUnpricedReason::ModelNotFound,
            });
        };

        Ok(PricingResolution::Exact {
            pricing: Box::new(resolved_model_pricing(&record)),
        })
    }

    async fn load_snapshot_from_store_or_fallback(
        &self,
    ) -> Result<PricingCatalogSnapshot, GatewayError> {
        Ok(self
            .load_stored_snapshot()
            .await?
            .unwrap_or_else(|| self.fallback_snapshot.clone()))
    }

    async fn load_stored_snapshot(&self) -> Result<Option<PricingCatalogSnapshot>, GatewayError> {
        let Some(cache) = self
            .repo
            .get_pricing_catalog_cache(&self.catalog_key)
            .await?
        else {
            return Ok(None);
        };

        match serde_json::from_str::<PricingCatalogDocument>(&cache.snapshot_json) {
            Ok(document) => Ok(Some(PricingCatalogSnapshot {
                metadata: PricingCatalogSnapshotMetadata {
                    source: cache.source,
                    etag: cache.etag,
                    fetched_at: cache.fetched_at,
                },
                document,
            })),
            Err(error) => {
                warn!(
                    catalog_key = %self.catalog_key,
                    error = %error,
                    "stored pricing catalog cache is invalid; falling back to vendored snapshot"
                );
                Ok(None)
            }
        }
    }

    async fn sync_model_pricing_snapshot(
        &self,
        snapshot: &PricingCatalogSnapshot,
    ) -> Result<(), GatewayError> {
        let active_rows = self.repo.list_active_model_pricing().await?;
        let active_by_target = active_rows
            .into_iter()
            .map(|record| {
                (
                    (
                        record.pricing_provider_id.clone(),
                        record.pricing_model_id.clone(),
                    ),
                    record,
                )
            })
            .collect::<BTreeMap<_, _>>();

        for (pricing_provider_id, provider_document) in &snapshot.document.providers {
            for (pricing_model_id, model_document) in &provider_document.models {
                let desired = build_model_pricing_record(
                    &snapshot.metadata,
                    pricing_provider_id,
                    pricing_model_id,
                    model_document,
                )?;
                let key = (pricing_provider_id.clone(), pricing_model_id.clone());

                match active_by_target.get(&key) {
                    Some(existing) if pricing_record_matches(existing, &desired) => {}
                    Some(existing) => {
                        self.repo
                            .close_model_pricing(
                                existing.model_pricing_id,
                                snapshot.metadata.fetched_at,
                                snapshot.metadata.fetched_at,
                            )
                            .await?;
                        self.repo.insert_model_pricing(&desired).await?;
                    }
                    None => {
                        self.repo.insert_model_pricing(&desired).await?;
                    }
                }
            }
        }

        Ok(())
    }
}

pub async fn fetch_vendored_snapshot(
    source_url: &str,
) -> anyhow::Result<PricingCatalogSnapshotFile> {
    let client = Client::new();
    let response = client
        .get(source_url)
        .send()
        .await
        .with_context(|| format!("failed fetching pricing catalog from `{source_url}`"))?;
    let status = response.status();
    if status != StatusCode::OK {
        anyhow::bail!("pricing catalog fetch returned HTTP {}", status.as_u16());
    }

    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response
        .text()
        .await
        .context("failed reading pricing catalog response body")?;
    let snapshot =
        project_models_dev_snapshot(&body, VENDORED_SOURCE, etag, OffsetDateTime::now_utc())
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    Ok(PricingCatalogSnapshotFile {
        metadata: snapshot.metadata,
        providers: snapshot.document.providers,
    })
}

pub fn snapshot_to_pretty_json(snapshot: &PricingCatalogSnapshotFile) -> anyhow::Result<String> {
    serde_json::to_string_pretty(snapshot).context("failed serializing vendored pricing catalog")
}

pub fn is_supported_pricing_provider_id(value: &str) -> bool {
    SUPPORTED_PRICING_PROVIDER_IDS.contains(&value)
}

#[derive(Debug, Clone)]
enum PricingTarget {
    Exact {
        pricing_provider_id: String,
        model_id: String,
    },
    Unpriced(PricingUnpricedReason),
}

fn pricing_target_for_route(provider: &ProviderConnection, route: &ModelRoute) -> PricingTarget {
    if let Some(reason) = unsupported_billing_modifier(route) {
        return PricingTarget::Unpriced(reason);
    }

    match provider.provider_type.as_str() {
        "openai_compat" => {
            let Some(pricing_provider_id) = provider
                .config
                .get("pricing_provider_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
            else {
                return PricingTarget::Unpriced(
                    PricingUnpricedReason::ProviderPricingSourceMissing,
                );
            };

            if !is_supported_pricing_provider_id(&pricing_provider_id) {
                return PricingTarget::Unpriced(
                    PricingUnpricedReason::UnsupportedPricingProviderId(pricing_provider_id),
                );
            }

            PricingTarget::Exact {
                pricing_provider_id,
                model_id: route.upstream_model.clone(),
            }
        }
        "gcp_vertex" => {
            let mut parts = route.upstream_model.splitn(2, '/');
            let publisher = parts.next().unwrap_or_default();
            let model_id = parts.next().unwrap_or_default();
            if publisher.is_empty() || model_id.is_empty() {
                return PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedVertexPublisher(
                    route.upstream_model.clone(),
                ));
            }

            let pricing_provider_id = match publisher {
                "google" => "google-vertex",
                "anthropic" => "google-vertex-anthropic",
                other => {
                    return PricingTarget::Unpriced(
                        PricingUnpricedReason::UnsupportedVertexPublisher(other.to_string()),
                    );
                }
            };

            if pricing_provider_id == "google-vertex-anthropic" {
                let location = provider
                    .config
                    .get("location")
                    .and_then(Value::as_str)
                    .unwrap_or("global");
                if location != "global" {
                    return PricingTarget::Unpriced(
                        PricingUnpricedReason::UnsupportedVertexLocation(location.to_string()),
                    );
                }
            }

            PricingTarget::Exact {
                pricing_provider_id: pricing_provider_id.to_string(),
                model_id: model_id.to_string(),
            }
        }
        other => PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedPricingProviderId(
            other.to_string(),
        )),
    }
}

fn unsupported_billing_modifier(route: &ModelRoute) -> Option<PricingUnpricedReason> {
    if route.extra_body.contains_key("service_tier") {
        return Some(PricingUnpricedReason::UnsupportedBillingModifier(
            "service_tier".to_string(),
        ));
    }
    if route.extra_body.contains_key("serviceTier") {
        return Some(PricingUnpricedReason::UnsupportedBillingModifier(
            "serviceTier".to_string(),
        ));
    }

    None
}

fn resolved_model_pricing(record: &ModelPricingRecord) -> ResolvedModelPricing {
    ResolvedModelPricing {
        model_pricing_id: record.model_pricing_id,
        pricing_provider_id: record.pricing_provider_id.clone(),
        model_id: record.pricing_model_id.clone(),
        display_name: record.display_name.clone(),
        input_cost_per_million_tokens: record.input_cost_per_million_tokens,
        output_cost_per_million_tokens: record.output_cost_per_million_tokens,
        cache_read_cost_per_million_tokens: record.cache_read_cost_per_million_tokens,
        cache_write_cost_per_million_tokens: record.cache_write_cost_per_million_tokens,
        input_audio_cost_per_million_tokens: record.input_audio_cost_per_million_tokens,
        output_audio_cost_per_million_tokens: record.output_audio_cost_per_million_tokens,
        release_date: record.release_date.clone(),
        last_updated: record.last_updated.clone(),
        effective_start_at: record.effective_start_at,
        effective_end_at: record.effective_end_at,
        limits: record.limits.clone(),
        modalities: record.modalities.clone(),
        provenance: record.provenance.clone(),
    }
}

fn build_model_pricing_record(
    metadata: &PricingCatalogSnapshotMetadata,
    pricing_provider_id: &str,
    pricing_model_id: &str,
    document: &PricingCatalogModelDocument,
) -> Result<ModelPricingRecord, GatewayError> {
    Ok(ModelPricingRecord {
        model_pricing_id: Uuid::new_v4(),
        pricing_provider_id: pricing_provider_id.to_string(),
        pricing_model_id: pricing_model_id.to_string(),
        display_name: document.display_name.clone(),
        input_cost_per_million_tokens: parse_money(document.cost.input.as_deref())?,
        output_cost_per_million_tokens: parse_money(document.cost.output.as_deref())?,
        cache_read_cost_per_million_tokens: parse_money(document.cost.cache_read.as_deref())?,
        cache_write_cost_per_million_tokens: parse_money(document.cost.cache_write.as_deref())?,
        input_audio_cost_per_million_tokens: parse_money(document.cost.input_audio.as_deref())?,
        output_audio_cost_per_million_tokens: parse_money(document.cost.output_audio.as_deref())?,
        release_date: document.release_date.clone(),
        last_updated: document.last_updated.clone(),
        effective_start_at: metadata.fetched_at,
        effective_end_at: None,
        limits: PricingLimits {
            context: document.limit.context,
            input: document.limit.input,
            output: document.limit.output,
        },
        modalities: PricingModalities {
            input: document.modalities.input.clone(),
            output: document.modalities.output.clone(),
        },
        provenance: PricingProvenance {
            source: metadata.source.clone(),
            etag: metadata.etag.clone(),
            fetched_at: metadata.fetched_at,
        },
        created_at: metadata.fetched_at,
        updated_at: metadata.fetched_at,
    })
}

fn pricing_record_matches(existing: &ModelPricingRecord, desired: &ModelPricingRecord) -> bool {
    existing.display_name == desired.display_name
        && existing.input_cost_per_million_tokens == desired.input_cost_per_million_tokens
        && existing.output_cost_per_million_tokens == desired.output_cost_per_million_tokens
        && existing.cache_read_cost_per_million_tokens == desired.cache_read_cost_per_million_tokens
        && existing.cache_write_cost_per_million_tokens
            == desired.cache_write_cost_per_million_tokens
        && existing.input_audio_cost_per_million_tokens
            == desired.input_audio_cost_per_million_tokens
        && existing.output_audio_cost_per_million_tokens
            == desired.output_audio_cost_per_million_tokens
        && existing.release_date == desired.release_date
        && existing.last_updated == desired.last_updated
        && existing.limits == desired.limits
        && existing.modalities == desired.modalities
}

fn parse_money(value: Option<&str>) -> Result<Option<Money4>, GatewayError> {
    value
        .map(|raw| {
            Money4::from_decimal_str(raw).map_err(|error| {
                GatewayError::Internal(format!(
                    "invalid pricing catalog money value `{raw}`: {error}"
                ))
            })
        })
        .transpose()
}

fn project_models_dev_snapshot(
    body: &str,
    source: &str,
    etag: Option<String>,
    fetched_at: OffsetDateTime,
) -> Result<PricingCatalogSnapshot, GatewayError> {
    let providers = serde_json::from_str::<BTreeMap<String, ModelsDevProviderDocument>>(body)
        .map_err(|error| {
            GatewayError::Internal(format!("failed parsing models.dev response: {error}"))
        })?;

    let mut projected_providers = BTreeMap::new();
    for supported_provider_id in SUPPORTED_PRICING_PROVIDER_IDS {
        let Some(provider) = providers.get(supported_provider_id) else {
            continue;
        };

        let mut projected_models = BTreeMap::new();
        for (fallback_key, model) in &provider.models {
            let model_id = if model.id.trim().is_empty() {
                fallback_key.clone()
            } else {
                model.id.clone()
            };
            projected_models.insert(
                model_id.clone(),
                PricingCatalogModelDocument {
                    id: model_id,
                    display_name: model.name.clone(),
                    release_date: model.release_date.clone(),
                    last_updated: model.last_updated.clone(),
                    cost: PricingCatalogCostDocument {
                        input: project_models_dev_cost(model.cost.input.as_ref())?,
                        output: project_models_dev_cost(model.cost.output.as_ref())?,
                        cache_read: project_models_dev_cost(model.cost.cache_read.as_ref())?,
                        cache_write: project_models_dev_cost(model.cost.cache_write.as_ref())?,
                        input_audio: project_models_dev_cost(model.cost.input_audio.as_ref())?,
                        output_audio: project_models_dev_cost(model.cost.output_audio.as_ref())?,
                    },
                    limit: PricingCatalogLimitDocument {
                        context: model.limit.context,
                        input: model.limit.input,
                        output: model.limit.output,
                    },
                    modalities: PricingCatalogModalitiesDocument {
                        input: model.modalities.input.clone(),
                        output: model.modalities.output.clone(),
                    },
                },
            );
        }

        projected_providers.insert(
            supported_provider_id.to_string(),
            PricingCatalogProviderDocument {
                display_name: provider.name.clone(),
                models: projected_models,
            },
        );
    }

    Ok(PricingCatalogSnapshot {
        metadata: PricingCatalogSnapshotMetadata {
            source: source.to_string(),
            etag,
            fetched_at,
        },
        document: PricingCatalogDocument {
            providers: projected_providers,
        },
    })
}

fn project_models_dev_cost(value: Option<&Number>) -> Result<Option<String>, GatewayError> {
    value
        .map(|number| {
            let raw = number.to_string();
            Money4::from_decimal_str(&raw)
                .map(|money| money.format_4dp())
                .map_err(|error| {
                    GatewayError::Internal(format!(
                        "failed normalizing models.dev cost `{raw}`: {error}"
                    ))
                })
        })
        .transpose()
}

fn load_vendored_fallback_snapshot() -> PricingCatalogSnapshot {
    let snapshot = serde_json::from_str::<PricingCatalogSnapshotFile>(VENDORED_FALLBACK_JSON)
        .expect("vendored pricing catalog fallback should deserialize");
    PricingCatalogSnapshot {
        metadata: snapshot.metadata,
        document: PricingCatalogDocument {
            providers: snapshot.providers,
        },
    }
}

#[derive(Debug, Clone)]
struct PricingCatalogSnapshot {
    metadata: PricingCatalogSnapshotMetadata,
    document: PricingCatalogDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogSnapshotFile {
    pub metadata: PricingCatalogSnapshotMetadata,
    pub providers: BTreeMap<String, PricingCatalogProviderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogSnapshotMetadata {
    pub source: String,
    pub etag: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub fetched_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PricingCatalogDocument {
    providers: BTreeMap<String, PricingCatalogProviderDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogProviderDocument {
    pub display_name: String,
    pub models: BTreeMap<String, PricingCatalogModelDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCatalogModelDocument {
    pub id: String,
    pub display_name: String,
    pub release_date: String,
    pub last_updated: String,
    pub cost: PricingCatalogCostDocument,
    pub limit: PricingCatalogLimitDocument,
    pub modalities: PricingCatalogModalitiesDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingCatalogCostDocument {
    pub input: Option<String>,
    pub output: Option<String>,
    pub cache_read: Option<String>,
    pub cache_write: Option<String>,
    pub input_audio: Option<String>,
    pub output_audio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingCatalogLimitDocument {
    pub context: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PricingCatalogModalitiesDocument {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelsDevProviderDocument {
    name: String,
    #[serde(default)]
    models: BTreeMap<String, ModelsDevModelDocument>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelsDevModelDocument {
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    last_updated: String,
    #[serde(default)]
    cost: ModelsDevCostDocument,
    #[serde(default)]
    limit: ModelsDevLimitDocument,
    #[serde(default)]
    modalities: ModelsDevModalitiesDocument,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsDevCostDocument {
    input: Option<Number>,
    output: Option<Number>,
    cache_read: Option<Number>,
    cache_write: Option<Number>,
    input_audio: Option<Number>,
    output_audio: Option<Number>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsDevLimitDocument {
    context: Option<i64>,
    input: Option<i64>,
    output: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModelsDevModalitiesDocument {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use async_trait::async_trait;
    use axum::{
        Router,
        extract::State,
        http::{
            HeaderMap, HeaderValue, StatusCode,
            header::{ETAG, IF_NONE_MATCH},
        },
        response::IntoResponse,
        routing::get,
    };
    use gateway_core::{
        ModelPricingRecord, ModelRoute, Money4, PricingCatalogCacheRecord,
        PricingCatalogRepository, PricingResolution, PricingUnpricedReason, ProviderCapabilities,
        ProviderConnection, StoreError,
    };
    use serde_json::{json, to_string_pretty};
    use time::OffsetDateTime;
    use tokio::net::TcpListener;
    use uuid::Uuid;

    use super::{
        PRICING_CATALOG_CACHE_KEY, PricingCatalog, PricingCatalogCostDocument,
        PricingCatalogDocument, PricingCatalogLimitDocument, PricingCatalogModalitiesDocument,
        PricingCatalogModelDocument, PricingCatalogProviderDocument, PricingCatalogSnapshot,
        PricingCatalogSnapshotMetadata, REMOTE_SOURCE, VENDORED_SOURCE,
    };

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        cache: Arc<Mutex<Option<PricingCatalogCacheRecord>>>,
        pricing_rows: Arc<Mutex<Vec<ModelPricingRecord>>>,
    }

    #[async_trait]
    impl PricingCatalogRepository for InMemoryRepo {
        async fn get_pricing_catalog_cache(
            &self,
            _catalog_key: &str,
        ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
            Ok(self.cache.lock().expect("cache lock").clone())
        }

        async fn upsert_pricing_catalog_cache(
            &self,
            cache: &PricingCatalogCacheRecord,
        ) -> Result<(), StoreError> {
            *self.cache.lock().expect("cache lock") = Some(cache.clone());
            Ok(())
        }

        async fn touch_pricing_catalog_cache_fetched_at(
            &self,
            catalog_key: &str,
            fetched_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            let mut guard = self.cache.lock().expect("cache lock");
            if let Some(cache) = guard.as_mut()
                && cache.catalog_key == catalog_key
            {
                cache.fetched_at = fetched_at;
            }
            Ok(())
        }

        async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError> {
            Ok(self
                .pricing_rows
                .lock()
                .expect("pricing rows lock")
                .iter()
                .filter(|row| row.effective_end_at.is_none())
                .cloned()
                .collect())
        }

        async fn insert_model_pricing(
            &self,
            record: &ModelPricingRecord,
        ) -> Result<(), StoreError> {
            self.pricing_rows
                .lock()
                .expect("pricing rows lock")
                .push(record.clone());
            Ok(())
        }

        async fn close_model_pricing(
            &self,
            model_pricing_id: Uuid,
            effective_end_at: OffsetDateTime,
            updated_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            let mut rows = self.pricing_rows.lock().expect("pricing rows lock");
            let Some(row) = rows
                .iter_mut()
                .find(|row| row.model_pricing_id == model_pricing_id)
            else {
                return Err(StoreError::NotFound(
                    "model pricing row missing".to_string(),
                ));
            };
            row.effective_end_at = Some(effective_end_at);
            row.updated_at = updated_at;
            Ok(())
        }

        async fn resolve_model_pricing_at(
            &self,
            pricing_provider_id: &str,
            pricing_model_id: &str,
            occurred_at: OffsetDateTime,
        ) -> Result<Option<ModelPricingRecord>, StoreError> {
            Ok(self
                .pricing_rows
                .lock()
                .expect("pricing rows lock")
                .iter()
                .filter(|row| {
                    row.pricing_provider_id == pricing_provider_id
                        && row.pricing_model_id == pricing_model_id
                        && row.effective_start_at <= occurred_at
                        && row.effective_end_at.is_none_or(|end| end > occurred_at)
                })
                .max_by_key(|row| row.effective_start_at)
                .cloned())
        }
    }

    fn test_time() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp")
    }

    fn openai_provider(pricing_provider_id: &str) -> ProviderConnection {
        ProviderConnection {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "pricing_provider_id": pricing_provider_id
            }),
            secrets: None,
        }
    }

    fn vertex_provider(location: &str) -> ProviderConnection {
        ProviderConnection {
            provider_key: "vertex-prod".to_string(),
            provider_type: "gcp_vertex".to_string(),
            config: json!({
                "project_id": "proj-123",
                "location": location,
                "api_host": "aiplatform.googleapis.com"
            }),
            secrets: None,
        }
    }

    fn route(provider_key: &str, upstream_model: &str) -> ModelRoute {
        ModelRoute {
            id: Uuid::new_v4(),
            model_id: Uuid::new_v4(),
            provider_key: provider_key.to_string(),
            upstream_model: upstream_model.to_string(),
            priority: 10,
            weight: 1.0,
            enabled: true,
            extra_headers: serde_json::Map::new(),
            extra_body: serde_json::Map::new(),
            capabilities: ProviderCapabilities::all_enabled(),
        }
    }

    fn fallback_snapshot() -> PricingCatalogSnapshot {
        PricingCatalogSnapshot {
            metadata: PricingCatalogSnapshotMetadata {
                source: VENDORED_SOURCE.to_string(),
                etag: None,
                fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
            },
            document: PricingCatalogDocument {
                providers: BTreeMap::from([
                    (
                        "openai".to_string(),
                        PricingCatalogProviderDocument {
                            display_name: "OpenAI".to_string(),
                            models: BTreeMap::from([(
                                "gpt-5".to_string(),
                                PricingCatalogModelDocument {
                                    id: "gpt-5".to_string(),
                                    display_name: "GPT-5".to_string(),
                                    release_date: "2025-08-07".to_string(),
                                    last_updated: "2025-08-07".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("1.2500".to_string()),
                                        output: Some("10.0000".to_string()),
                                        cache_read: Some("0.1250".to_string()),
                                        cache_write: None,
                                        input_audio: None,
                                        output_audio: None,
                                    },
                                    limit: PricingCatalogLimitDocument {
                                        context: Some(400_000),
                                        input: Some(272_000),
                                        output: Some(128_000),
                                    },
                                    modalities: PricingCatalogModalitiesDocument {
                                        input: vec!["text".to_string(), "image".to_string()],
                                        output: vec!["text".to_string()],
                                    },
                                },
                            )]),
                        },
                    ),
                    (
                        "google-vertex".to_string(),
                        PricingCatalogProviderDocument {
                            display_name: "Vertex".to_string(),
                            models: BTreeMap::from([(
                                "gemini-2.5-flash".to_string(),
                                PricingCatalogModelDocument {
                                    id: "gemini-2.5-flash".to_string(),
                                    display_name: "Gemini 2.5 Flash".to_string(),
                                    release_date: "2025-06-17".to_string(),
                                    last_updated: "2025-06-17".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("0.3000".to_string()),
                                        output: Some("2.5000".to_string()),
                                        cache_read: Some("0.0750".to_string()),
                                        cache_write: Some("0.3830".to_string()),
                                        input_audio: None,
                                        output_audio: None,
                                    },
                                    limit: PricingCatalogLimitDocument {
                                        context: Some(1_048_576),
                                        input: None,
                                        output: Some(65_536),
                                    },
                                    modalities: PricingCatalogModalitiesDocument {
                                        input: vec![
                                            "text".to_string(),
                                            "image".to_string(),
                                            "audio".to_string(),
                                            "video".to_string(),
                                            "pdf".to_string(),
                                        ],
                                        output: vec!["text".to_string()],
                                    },
                                },
                            )]),
                        },
                    ),
                    (
                        "google-vertex-anthropic".to_string(),
                        PricingCatalogProviderDocument {
                            display_name: "Vertex (Anthropic)".to_string(),
                            models: BTreeMap::from([(
                                "claude-sonnet-4-5@20250929".to_string(),
                                PricingCatalogModelDocument {
                                    id: "claude-sonnet-4-5@20250929".to_string(),
                                    display_name: "Claude Sonnet 4.5".to_string(),
                                    release_date: "2025-09-29".to_string(),
                                    last_updated: "2025-09-29".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("3.0000".to_string()),
                                        output: Some("15.0000".to_string()),
                                        cache_read: Some("0.3000".to_string()),
                                        cache_write: Some("3.7500".to_string()),
                                        input_audio: None,
                                        output_audio: None,
                                    },
                                    limit: PricingCatalogLimitDocument {
                                        context: Some(200_000),
                                        input: None,
                                        output: Some(64_000),
                                    },
                                    modalities: PricingCatalogModalitiesDocument {
                                        input: vec![
                                            "text".to_string(),
                                            "image".to_string(),
                                            "pdf".to_string(),
                                        ],
                                        output: vec!["text".to_string()],
                                    },
                                },
                            )]),
                        },
                    ),
                ]),
            },
        }
    }

    fn empty_catalog(repo: Arc<InMemoryRepo>, source_url: String) -> PricingCatalog<InMemoryRepo> {
        PricingCatalog::with_fallback_snapshot(
            repo,
            source_url,
            PRICING_CATALOG_CACHE_KEY.to_string(),
            Duration::from_secs(0),
            fallback_snapshot(),
        )
    }

    async fn start_server(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn gcp_vertex_maps_supported_publishers() {
        let catalog = empty_catalog(
            Arc::new(InMemoryRepo::default()),
            "http://127.0.0.1:9/api.json".to_string(),
        );

        let google = catalog
            .resolve_for_provider_connection(
                &vertex_provider("global"),
                &route("vertex-prod", "google/gemini-2.5-flash"),
                test_time(),
            )
            .await
            .expect("resolve google");
        let anthropic = catalog
            .resolve_for_provider_connection(
                &vertex_provider("global"),
                &route("vertex-prod", "anthropic/claude-sonnet-4-5@20250929"),
                test_time(),
            )
            .await
            .expect("resolve anthropic");

        match google {
            PricingResolution::Exact { pricing } => {
                assert_eq!(pricing.pricing_provider_id, "google-vertex");
                assert_eq!(pricing.model_id, "gemini-2.5-flash");
            }
            other => panic!("unexpected google resolution: {other:?}"),
        }
        match anthropic {
            PricingResolution::Exact { pricing } => {
                assert_eq!(pricing.pricing_provider_id, "google-vertex-anthropic");
                assert_eq!(pricing.model_id, "claude-sonnet-4-5@20250929");
            }
            other => panic!("unexpected anthropic resolution: {other:?}"),
        }
    }

    #[tokio::test]
    async fn exact_model_lookup_succeeds_and_fails_closed() {
        let catalog = empty_catalog(
            Arc::new(InMemoryRepo::default()),
            "http://127.0.0.1:9/api.json".to_string(),
        );

        let exact = catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time(),
            )
            .await
            .expect("resolve");
        let missing = catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-unknown"),
                test_time(),
            )
            .await
            .expect("resolve missing");

        match exact {
            PricingResolution::Exact { pricing } => {
                assert_eq!(pricing.model_id, "gpt-5");
                assert_eq!(
                    pricing.input_cost_per_million_tokens,
                    Some(Money4::from_decimal_str("1.2500").expect("money"))
                );
            }
            other => panic!("unexpected exact resolution: {other:?}"),
        }
        assert_eq!(
            missing,
            PricingResolution::Unpriced {
                reason: PricingUnpricedReason::ModelNotFound
            }
        );
    }

    #[tokio::test]
    async fn vendored_snapshot_is_used_without_remote_cache() {
        let catalog = empty_catalog(
            Arc::new(InMemoryRepo::default()),
            "http://127.0.0.1:9/api.json".to_string(),
        );

        let resolved = catalog
            .resolve_for_provider_connection(
                &vertex_provider("global"),
                &route("vertex-prod", "google/gemini-2.5-flash"),
                test_time(),
            )
            .await
            .expect("resolve");

        match resolved {
            PricingResolution::Exact { pricing } => {
                assert_eq!(pricing.provenance.source, VENDORED_SOURCE);
            }
            other => panic!("unexpected vendored resolution: {other:?}"),
        }
    }

    #[tokio::test]
    async fn refresh_uses_conditional_etag_and_handles_304() {
        let repo = Arc::new(InMemoryRepo {
            cache: Arc::new(Mutex::new(Some(PricingCatalogCacheRecord {
                catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
                source: REMOTE_SOURCE.to_string(),
                etag: Some("\"catalog-etag\"".to_string()),
                fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
                snapshot_json: to_string_pretty(&fallback_snapshot().document).expect("json"),
            }))),
            pricing_rows: Arc::new(Mutex::new(Vec::new())),
        });
        let state = Arc::new(Mutex::new(None::<String>));
        let app = Router::new()
            .route(
                "/api.json",
                get(
                    |headers: HeaderMap, State(captured): State<Arc<Mutex<Option<String>>>>| async move {
                        *captured.lock().expect("captured lock") = headers
                            .get(IF_NONE_MATCH)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        StatusCode::NOT_MODIFIED.into_response()
                    },
                ),
            )
            .with_state(state.clone());
        let host = start_server(app).await;

        let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
        let before = repo
            .get_pricing_catalog_cache(PRICING_CATALOG_CACHE_KEY)
            .await
            .expect("cache before")
            .expect("cache row");

        catalog.refresh_if_stale().await.expect("304 refresh");

        let after = repo
            .get_pricing_catalog_cache(PRICING_CATALOG_CACHE_KEY)
            .await
            .expect("cache after")
            .expect("cache row");
        assert_eq!(
            state.lock().expect("captured lock").as_deref(),
            Some("\"catalog-etag\"")
        );
        assert_eq!(after.snapshot_json, before.snapshot_json);
        assert!(after.fetched_at > before.fetched_at);
    }

    #[tokio::test]
    async fn refresh_replaces_cached_snapshot_on_200() {
        let repo = Arc::new(InMemoryRepo::default());
        let body = json!({
            "openai": {
                "name": "OpenAI",
                "models": {
                    "gpt-5": {
                        "id": "gpt-5",
                        "name": "GPT-5",
                        "release_date": "2025-08-07",
                        "last_updated": "2025-08-07",
                        "cost": {
                            "input": 1.25,
                            "output": 10.0,
                            "cache_read": 0.125
                        },
                        "limit": {
                            "context": 400000,
                            "input": 272000,
                            "output": 128000
                        },
                        "modalities": {
                            "input": ["text", "image"],
                            "output": ["text"]
                        }
                    }
                }
            }
        });
        let app = Router::new().route(
            "/api.json",
            get(move || {
                let body = body.clone();
                async move {
                    (
                        [(ETAG, HeaderValue::from_static("\"new-etag\""))],
                        axum::Json(body),
                    )
                }
            }),
        );
        let host = start_server(app).await;

        let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
        catalog.refresh_if_stale().await.expect("200 refresh");

        let cache = repo
            .get_pricing_catalog_cache(PRICING_CATALOG_CACHE_KEY)
            .await
            .expect("cache")
            .expect("cache row");
        assert_eq!(cache.etag.as_deref(), Some("\"new-etag\""));
        assert!(cache.snapshot_json.contains("\"gpt-5\""));
    }

    #[tokio::test]
    async fn remote_failure_falls_back_to_store_then_vendored_snapshot() {
        let repo = Arc::new(InMemoryRepo {
            cache: Arc::new(Mutex::new(Some(PricingCatalogCacheRecord {
                catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
                source: REMOTE_SOURCE.to_string(),
                etag: Some("\"cached\"".to_string()),
                fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
                snapshot_json: to_string_pretty(&PricingCatalogDocument {
                    providers: BTreeMap::from([(
                        "openai".to_string(),
                        PricingCatalogProviderDocument {
                            display_name: "OpenAI".to_string(),
                            models: BTreeMap::from([(
                                "gpt-5".to_string(),
                                PricingCatalogModelDocument {
                                    id: "gpt-5".to_string(),
                                    display_name: "GPT-5 Cached".to_string(),
                                    release_date: "2025-08-07".to_string(),
                                    last_updated: "2025-08-08".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("2.0000".to_string()),
                                        output: Some("20.0000".to_string()),
                                        cache_read: None,
                                        cache_write: None,
                                        input_audio: None,
                                        output_audio: None,
                                    },
                                    limit: PricingCatalogLimitDocument::default(),
                                    modalities: PricingCatalogModalitiesDocument::default(),
                                },
                            )]),
                        },
                    )]),
                })
                .expect("json"),
            }))),
            pricing_rows: Arc::new(Mutex::new(Vec::new())),
        });
        let failing_catalog =
            empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
        let cached = failing_catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time(),
            )
            .await
            .expect("cached resolve");

        match cached {
            PricingResolution::Exact { pricing } => {
                assert_eq!(pricing.display_name, "GPT-5 Cached");
                assert_eq!(pricing.provenance.source, REMOTE_SOURCE);
            }
            other => panic!("unexpected cached resolution: {other:?}"),
        }

        let vendored_catalog = empty_catalog(
            Arc::new(InMemoryRepo::default()),
            "http://127.0.0.1:9/api.json".to_string(),
        );
        let vendored = vendored_catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time(),
            )
            .await
            .expect("vendored resolve");

        match vendored {
            PricingResolution::Exact { pricing } => {
                assert_eq!(pricing.display_name, "GPT-5");
                assert_eq!(pricing.provenance.source, VENDORED_SOURCE);
            }
            other => panic!("unexpected vendored resolution: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unsupported_billing_modifiers_resolve_to_unpriced() {
        let repo = Arc::new(InMemoryRepo::default());
        let catalog = empty_catalog(repo, "http://127.0.0.1:9/api.json".to_string());
        let mut service_tier_route = route("openai-prod", "gpt-5");
        service_tier_route
            .extra_body
            .insert("service_tier".to_string(), json!("priority"));

        let service_tier = catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &service_tier_route,
                test_time(),
            )
            .await
            .expect("service tier resolve");
        let regional_vertex = catalog
            .resolve_for_provider_connection(
                &vertex_provider("us-central1"),
                &route("vertex-prod", "anthropic/claude-sonnet-4-5@20250929"),
                test_time(),
            )
            .await
            .expect("regional vertex resolve");

        assert_eq!(
            service_tier,
            PricingResolution::Unpriced {
                reason: PricingUnpricedReason::UnsupportedBillingModifier(
                    "service_tier".to_string(),
                )
            }
        );
        assert_eq!(
            regional_vertex,
            PricingResolution::Unpriced {
                reason: PricingUnpricedReason::UnsupportedVertexLocation("us-central1".to_string(),)
            }
        );
    }

    #[tokio::test]
    async fn unchanged_snapshot_does_not_insert_duplicate_active_pricing_rows() {
        let repo = Arc::new(InMemoryRepo::default());
        let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());

        catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time(),
            )
            .await
            .expect("first resolve");
        catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time() + Duration::from_secs(60),
            )
            .await
            .expect("second resolve");

        let pricing_rows = repo.pricing_rows.lock().expect("pricing rows lock");
        let matching = pricing_rows
            .iter()
            .filter(|row| row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5")
            .count();
        assert_eq!(matching, 1);
    }

    #[tokio::test]
    async fn changed_snapshot_rolls_active_window_forward() {
        let repo = Arc::new(InMemoryRepo::default());
        let initial = fallback_snapshot();
        repo.upsert_pricing_catalog_cache(&PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: initial.metadata.source.clone(),
            etag: initial.metadata.etag.clone(),
            fetched_at: initial.metadata.fetched_at,
            snapshot_json: to_string_pretty(&initial.document).expect("json"),
        })
        .await
        .expect("seed initial snapshot");

        let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
        catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time(),
            )
            .await
            .expect("seed initial pricing row");

        let mut changed = fallback_snapshot();
        changed.metadata = PricingCatalogSnapshotMetadata {
            source: REMOTE_SOURCE.to_string(),
            etag: Some("\"etag-2\"".to_string()),
            fetched_at: test_time() + Duration::from_secs(3600),
        };
        changed
            .document
            .providers
            .get_mut("openai")
            .expect("openai provider")
            .models
            .get_mut("gpt-5")
            .expect("gpt-5 model")
            .cost
            .input = Some("2.0000".to_string());

        repo.upsert_pricing_catalog_cache(&PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: changed.metadata.source.clone(),
            etag: changed.metadata.etag.clone(),
            fetched_at: changed.metadata.fetched_at,
            snapshot_json: to_string_pretty(&changed.document).expect("json"),
        })
        .await
        .expect("seed changed snapshot");

        catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                changed.metadata.fetched_at + Duration::from_secs(1),
            )
            .await
            .expect("resolve changed pricing row");

        let pricing_rows = repo.pricing_rows.lock().expect("pricing rows lock");
        let matching = pricing_rows
            .iter()
            .filter(|row| row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 2);
        assert!(
            matching
                .iter()
                .any(|row| row.effective_end_at == Some(changed.metadata.fetched_at))
        );
        assert!(matching.iter().any(|row| {
            row.effective_start_at == changed.metadata.fetched_at
                && row.input_cost_per_million_tokens == Some(Money4::from_scaled(20_000))
                && row.effective_end_at.is_none()
        }));
    }

    #[tokio::test]
    async fn resolution_uses_persisted_pricing_row_for_occurrence_time() {
        let repo = Arc::new(InMemoryRepo::default());
        let initial = fallback_snapshot();
        repo.upsert_pricing_catalog_cache(&PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: initial.metadata.source.clone(),
            etag: initial.metadata.etag.clone(),
            fetched_at: initial.metadata.fetched_at,
            snapshot_json: to_string_pretty(&initial.document).expect("json"),
        })
        .await
        .expect("seed initial snapshot");

        let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
        catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                test_time(),
            )
            .await
            .expect("initial resolve");

        let mut changed = fallback_snapshot();
        changed.metadata = PricingCatalogSnapshotMetadata {
            source: REMOTE_SOURCE.to_string(),
            etag: Some("\"etag-3\"".to_string()),
            fetched_at: test_time() + Duration::from_secs(7200),
        };
        changed
            .document
            .providers
            .get_mut("openai")
            .expect("openai provider")
            .models
            .get_mut("gpt-5")
            .expect("gpt-5 model")
            .cost
            .input = Some("2.0000".to_string());

        repo.upsert_pricing_catalog_cache(&PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: changed.metadata.source.clone(),
            etag: changed.metadata.etag.clone(),
            fetched_at: changed.metadata.fetched_at,
            snapshot_json: to_string_pretty(&changed.document).expect("json"),
        })
        .await
        .expect("seed changed snapshot");
        catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                changed.metadata.fetched_at + Duration::from_secs(1),
            )
            .await
            .expect("changed resolve");

        let old_resolution = catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                changed.metadata.fetched_at - Duration::from_secs(1),
            )
            .await
            .expect("resolve old pricing window");
        let new_resolution = catalog
            .resolve_for_provider_connection(
                &openai_provider("openai"),
                &route("openai-prod", "gpt-5"),
                changed.metadata.fetched_at + Duration::from_secs(1),
            )
            .await
            .expect("resolve new pricing window");

        match old_resolution {
            PricingResolution::Exact { pricing } => {
                assert_eq!(
                    pricing.input_cost_per_million_tokens,
                    Some(Money4::from_scaled(12_500))
                );
            }
            other => panic!("unexpected old resolution: {other:?}"),
        }
        match new_resolution {
            PricingResolution::Exact { pricing } => {
                assert_eq!(
                    pricing.input_cost_per_million_tokens,
                    Some(Money4::from_scaled(20_000))
                );
                assert_eq!(pricing.effective_start_at, changed.metadata.fetched_at);
            }
            other => panic!("unexpected new resolution: {other:?}"),
        }
    }
}
