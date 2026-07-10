use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use gateway_core::{
    GatewayError, ModelPricingProvenanceUpdate, ModelPricingRecord, ModelPricingSyncChanges,
    ModelRoute, Money4, PricingCatalogCacheRecord, PricingCatalogRepository, PricingLimits,
    PricingModalities, PricingProvenance, PricingResolution, PricingUnpricedReason,
    ProviderConnection, ResolvedModelPricing, StoreError,
};
use reqwest::{
    Client, StatusCode,
    header::{ETAG, IF_NONE_MATCH},
};
use serde::{Deserialize, Serialize};
use serde_json::Number;
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

pub const DEFAULT_PRICING_CATALOG_SOURCE_URL: &str = "https://models.dev/api.json";
pub const PRICING_CATALOG_CACHE_KEY: &str = "models_dev_supported_v2";
pub const DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const DEFAULT_PRICING_CATALOG_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const REMOTE_SOURCE: &str = "models_dev_api";
const VENDORED_SOURCE: &str = "vendored_models_dev";
const VENDORED_FALLBACK_JSON: &str = include_str!("../data/pricing_catalog_fallback.json");
const MAX_PRICING_SYNC_ATTEMPTS: usize = 3;

mod target;

pub(crate) use target::exact_pricing_target_for_route;
use target::{PricingTarget, pricing_target_for_route};
pub use target::{SUPPORTED_PRICING_PROVIDER_IDS, is_supported_pricing_provider_id};
#[cfg(test)]
use target::{normalize_bedrock_pricing_model_id, normalize_vertex_pricing_model_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshMode {
    Conditional,
    Unconditional,
}

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
            client: pricing_catalog_http_client(),
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
            client: pricing_catalog_http_client(),
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

        self.refresh_remote_snapshot(current, RefreshMode::Conditional)
            .await
    }

    pub async fn refresh_now_and_sync(&self) -> Result<(), GatewayError> {
        self.refresh_remote_snapshot(None, RefreshMode::Unconditional)
            .await?;
        self.sync_latest_snapshot_with_retry().await
    }

    async fn refresh_remote_snapshot(
        &self,
        current: Option<PricingCatalogSnapshot>,
        mode: RefreshMode,
    ) -> Result<(), GatewayError> {
        let now = OffsetDateTime::now_utc();
        let mut request = self.client.get(&self.source_url);
        if mode == RefreshMode::Conditional
            && let Some(etag) = current
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

    pub async fn refresh_if_stale_and_sync(&self) -> Result<(), GatewayError> {
        if let Err(error) = self.refresh_if_stale().await {
            warn!(
                catalog_key = %self.catalog_key,
                source_url = %self.source_url,
                error = %error,
                "pricing catalog refresh failed; falling back to cached snapshot"
            );
        }

        self.sync_latest_snapshot_with_retry().await
    }

    async fn sync_latest_snapshot_with_retry(&self) -> Result<(), GatewayError> {
        for attempt in 1..=MAX_PRICING_SYNC_ATTEMPTS {
            let snapshot = self.load_snapshot_from_store_or_fallback().await?;
            match self.sync_model_pricing_snapshot(&snapshot).await {
                Err(GatewayError::Store(StoreError::PricingSyncConflict))
                    if attempt < MAX_PRICING_SYNC_ATTEMPTS =>
                {
                    warn!(
                        catalog_key = %self.catalog_key,
                        attempt,
                        "pricing catalog changed during reconciliation; retrying latest snapshot"
                    );
                }
                result => return result,
            }
        }

        unreachable!("pricing sync attempts always return on their final iteration")
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
        if active_rows
            .iter()
            .any(|row| row.provenance.fetched_at > snapshot.metadata.fetched_at)
        {
            return Ok(());
        }
        if snapshot_is_already_synced(&active_rows, snapshot) {
            return Ok(());
        }

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

        let mut snapshot_keys = BTreeSet::new();
        let mut changes = ModelPricingSyncChanges::default();
        for (pricing_provider_id, provider_document) in &snapshot.document.providers {
            for (pricing_model_id, model_document) in &provider_document.models {
                let desired = build_model_pricing_record(
                    &snapshot.metadata,
                    pricing_provider_id,
                    pricing_model_id,
                    model_document,
                )?;
                let key = (pricing_provider_id.clone(), pricing_model_id.clone());
                snapshot_keys.insert(key.clone());

                match active_by_target.get(&key) {
                    Some(existing) if pricing_record_matches(existing, &desired) => {
                        if existing.provenance != desired.provenance {
                            changes
                                .update_provenance
                                .push(ModelPricingProvenanceUpdate {
                                    model_pricing_id: existing.model_pricing_id,
                                    provenance: desired.provenance,
                                    updated_at: snapshot.metadata.fetched_at,
                                });
                        }
                    }
                    Some(existing) => {
                        changes
                            .close_model_pricing_ids
                            .push(existing.model_pricing_id);
                        changes.insert_model_pricing.push(desired);
                    }
                    None => {
                        changes.insert_model_pricing.push(desired);
                    }
                }
            }
        }

        for (key, existing) in &active_by_target {
            if !snapshot_keys.contains(key) {
                changes
                    .close_model_pricing_ids
                    .push(existing.model_pricing_id);
            }
        }

        if changes.close_model_pricing_ids.is_empty()
            && changes.update_provenance.is_empty()
            && changes.insert_model_pricing.is_empty()
        {
            return Ok(());
        }

        self.repo
            .apply_model_pricing_sync(&changes, snapshot.metadata.fetched_at)
            .await?;
        Ok(())
    }
}

pub async fn fetch_vendored_snapshot(
    source_url: &str,
) -> anyhow::Result<PricingCatalogSnapshotFile> {
    let client = pricing_catalog_http_client();
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

fn pricing_catalog_http_client() -> Client {
    Client::builder()
        .timeout(DEFAULT_PRICING_CATALOG_REQUEST_TIMEOUT)
        .build()
        .expect("pricing catalog HTTP client configuration is valid")
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

fn snapshot_is_already_synced(
    active_rows: &[ModelPricingRecord],
    snapshot: &PricingCatalogSnapshot,
) -> bool {
    let snapshot_keys = snapshot
        .document
        .providers
        .iter()
        .flat_map(|(provider_id, provider)| {
            provider
                .models
                .keys()
                .map(move |model_id| (provider_id.clone(), model_id.clone()))
        })
        .collect::<BTreeSet<_>>();

    !snapshot_keys.is_empty()
        && active_rows.len() == snapshot_keys.len()
        && active_rows.iter().all(|row| {
            row.provenance.source == snapshot.metadata.source
                && row.provenance.etag == snapshot.metadata.etag
                && row.provenance.fetched_at == snapshot.metadata.fetched_at
                && snapshot_keys.contains(&(
                    row.pricing_provider_id.clone(),
                    row.pricing_model_id.clone(),
                ))
        })
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
    value.map(normalize_models_dev_money).transpose()
}

fn normalize_models_dev_money(number: &Number) -> Result<String, GatewayError> {
    let raw = number.to_string();
    if let Ok(money) = Money4::from_decimal_str(&raw) {
        return Ok(money.format_4dp());
    }

    let value = number.as_f64().ok_or_else(|| {
        GatewayError::Internal(format!(
            "failed normalizing models.dev cost `{raw}`: not finite"
        ))
    })?;
    let scaled = (value * Money4::SCALE as f64).round();
    if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(GatewayError::Internal(format!(
            "failed normalizing models.dev cost `{raw}`: rounded value overflowed"
        )));
    }

    Ok(Money4::from_scaled(scaled as i64).format_4dp())
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
mod tests;
