pub(super) use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(super) use async_trait::async_trait;
pub(super) use axum::{
    Router,
    extract::State,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ETAG, IF_NONE_MATCH},
    },
    response::IntoResponse,
    routing::get,
};
pub(super) use gateway_core::{
    GatewayError, ModelPricingRecord, ModelPricingSyncChanges, ModelRoute, Money4,
    PricingCatalogCacheRecord, PricingCatalogRepository, PricingResolution, PricingUnpricedReason,
    ProviderCapabilities, ProviderConnection, StoreError,
};
pub(super) use serde_json::{Value, json};
pub(super) use time::OffsetDateTime;
pub(super) use tokio::net::TcpListener;
pub(super) use uuid::Uuid;

pub(super) use super::super::{
    PricingCatalog, PricingCatalogContextTier, PricingCatalogCostDocument, PricingCatalogDocument,
    PricingCatalogLimitDocument, PricingCatalogModalitiesDocument, PricingCatalogModelDocument,
    PricingCatalogProviderDocument, PricingCatalogRefreshError, PricingCatalogRefreshOutcome,
    PricingCatalogSnapshot, PricingCatalogSnapshotFile, PricingCatalogSnapshotMetadata,
    PricingTarget, build_model_pricing_record, canonical_document_json, catalog_model_id_fallback,
    normalize_bedrock_pricing_model_id, pricing_target_for_route, projected_snapshot,
    reconcile::reconcile_model_pricing,
    refresh::{CatalogSource, RefreshMode, next_catalog_generation_at},
    snapshot_to_pretty_json,
    sources::{CatalogSourceId, Providers, models_dev},
    vendored_snapshot_is_unchanged,
};
pub(super) use crate::admin_audit::AuditProgress;

/// models.dev's cache key and provenance sources.
pub(super) const MODELS_DEV_CACHE_KEY: &str = "models_dev_supported_v4";
pub(super) const MODELS_DEV_REMOTE_SOURCE: &str = "models_dev_api";
pub(super) const MODELS_DEV_VENDORED_SOURCE: &str = "vendored_models_dev";
pub(super) const OPENROUTER_CACHE_KEY: &str = "openrouter_v1";
pub(super) const LITELLM_CACHE_KEY: &str = "litellm_v1";
/// A URL that refuses connections, so a refresh of it fails at once.
pub(super) const UNREACHABLE_URL: &str = "http://127.0.0.1:9/api.json";
/// The paths [`source_urls`] serves each source from, in [`CatalogSourceId::ALL`] order.
pub(super) const SOURCE_PATHS: [&str; 3] = ["/models-dev", "/openrouter", "/litellm"];

#[derive(Clone, Default)]
pub(super) struct InMemoryRepo {
    /// Cache rows keyed by `catalog_key`.
    pub(super) cache: Arc<Mutex<BTreeMap<String, PricingCatalogCacheRecord>>>,
    pub(super) pricing_rows: Arc<Mutex<Vec<ModelPricingRecord>>>,
    pub(super) cache_reads: Arc<AtomicUsize>,
    pub(super) active_pricing_reads: Arc<AtomicUsize>,
    pub(super) pricing_sync_applications: Arc<AtomicUsize>,
    pub(super) pricing_resolutions: Arc<AtomicUsize>,
    pub(super) pricing_sync_conflicts_remaining: Arc<AtomicUsize>,
    pub(super) cache_write_rejections_remaining: Arc<AtomicUsize>,
}

impl InMemoryRepo {
    pub(super) fn with_cache_rows(
        rows: impl IntoIterator<Item = PricingCatalogCacheRecord>,
    ) -> Self {
        Self {
            cache: Arc::new(Mutex::new(
                rows.into_iter()
                    .map(|row| (row.catalog_key.clone(), row))
                    .collect(),
            )),
            ..Self::default()
        }
    }

    pub(super) fn cache_row(&self, catalog_key: &str) -> Option<PricingCatalogCacheRecord> {
        self.cache
            .lock()
            .expect("cache lock")
            .get(catalog_key)
            .cloned()
    }

    pub(super) fn sync_applications(&self) -> usize {
        self.pricing_sync_applications.load(Ordering::Relaxed)
    }

    /// The active row of `provider_id` / `model_id`.
    pub(super) fn active_row(&self, provider_id: &str, model_id: &str) -> ModelPricingRecord {
        self.pricing_rows
            .lock()
            .expect("pricing rows lock")
            .iter()
            .find(|row| {
                row.effective_end_at.is_none()
                    && row.pricing_provider_id == provider_id
                    && row.pricing_model_id == model_id
            })
            .cloned()
            .unwrap_or_else(|| panic!("active pricing row for {provider_id}/{model_id}"))
    }
}

#[async_trait]
impl PricingCatalogRepository for InMemoryRepo {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
        self.cache_reads.fetch_add(1, Ordering::Relaxed);
        Ok(self.cache_row(catalog_key))
    }

    async fn compare_and_swap_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
        expected_fetched_at: Option<OffsetDateTime>,
    ) -> Result<bool, StoreError> {
        if self
            .cache_write_rejections_remaining
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Ok(false);
        }

        let mut stored = self.cache.lock().expect("cache lock");
        if stored
            .get(&cache.catalog_key)
            .map(|current| current.fetched_at)
            != expected_fetched_at
            || expected_fetched_at.is_some_and(|expected| cache.fetched_at <= expected)
        {
            return Ok(false);
        }
        stored.insert(cache.catalog_key.clone(), cache.clone());
        Ok(true)
    }

    async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError> {
        self.active_pricing_reads.fetch_add(1, Ordering::Relaxed);
        Ok(self
            .pricing_rows
            .lock()
            .expect("pricing rows lock")
            .iter()
            .filter(|row| row.effective_end_at.is_none())
            .cloned()
            .collect())
    }

    async fn insert_model_pricing(&self, record: &ModelPricingRecord) -> Result<(), StoreError> {
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

    async fn apply_model_pricing_sync(
        &self,
        changes: &ModelPricingSyncChanges,
        effective_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.pricing_sync_applications
            .fetch_add(1, Ordering::Relaxed);
        if self
            .pricing_sync_conflicts_remaining
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(StoreError::PricingSyncConflict);
        }

        let mut rows = self.pricing_rows.lock().expect("pricing rows lock");
        if rows
            .iter()
            .any(|row| row.effective_end_at.is_none() && effective_at < row.provenance.fetched_at)
        {
            return Err(StoreError::PricingSyncConflict);
        }
        let closing = changes
            .close_model_pricing_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for expected_id in closing.iter().copied().chain(
            changes
                .update_provenance
                .iter()
                .map(|update| update.model_pricing_id),
        ) {
            let Some(active) = rows
                .iter()
                .find(|row| row.model_pricing_id == expected_id && row.effective_end_at.is_none())
            else {
                return Err(StoreError::PricingSyncConflict);
            };
            if effective_at <= active.provenance.fetched_at {
                return Err(StoreError::PricingSyncConflict);
            }
        }
        for inserted in &changes.insert_model_pricing {
            if rows.iter().any(|row| {
                row.effective_end_at.is_none()
                    && row.pricing_provider_id == inserted.pricing_provider_id
                    && row.pricing_model_id == inserted.pricing_model_id
                    && !closing.contains(&row.model_pricing_id)
            }) {
                return Err(StoreError::PricingSyncConflict);
            }
        }
        for model_pricing_id in &changes.close_model_pricing_ids {
            let Some(row) = rows
                .iter_mut()
                .find(|row| row.model_pricing_id == *model_pricing_id)
            else {
                return Err(StoreError::NotFound(format!(
                    "model pricing row `{model_pricing_id}`"
                )));
            };
            row.effective_end_at = Some(effective_at);
            row.updated_at = effective_at;
        }
        for update in &changes.update_provenance {
            let Some(row) = rows
                .iter_mut()
                .find(|row| row.model_pricing_id == update.model_pricing_id)
            else {
                return Err(StoreError::NotFound(format!(
                    "model pricing row `{}`",
                    update.model_pricing_id
                )));
            };
            row.provenance = update.provenance.clone();
            row.updated_at = update.updated_at;
        }
        rows.extend(changes.insert_model_pricing.iter().cloned());
        Ok(())
    }

    async fn resolve_model_pricing_at(
        &self,
        pricing_provider_id: &str,
        pricing_model_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<Option<ModelPricingRecord>, StoreError> {
        self.pricing_resolutions.fetch_add(1, Ordering::Relaxed);
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

pub(super) fn test_time() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp")
}

pub(super) fn openai_provider(pricing_provider_id: &str) -> ProviderConnection {
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
pub(super) fn anthropic_compat_provider(pricing_provider_id: &str) -> ProviderConnection {
    ProviderConnection {
        provider_key: "opencode-zen".to_string(),
        provider_type: "anthropic_compat".to_string(),
        config: json!({
            "base_url": "https://opencode.ai/zen",
            "pricing_provider_id": pricing_provider_id
        }),
        secrets: None,
    }
}

pub(super) fn cloud_run_provider(pricing_provider_id: Option<&str>) -> ProviderConnection {
    let mut config = serde_json::Map::from_iter([(
        "base_url".to_string(),
        json!("https://gemma-service.run.app/v1"),
    )]);
    if let Some(pricing_provider_id) = pricing_provider_id {
        config.insert(
            "pricing_provider_id".to_string(),
            json!(pricing_provider_id),
        );
    }

    ProviderConnection {
        provider_key: "gemma-cloud-run".to_string(),
        provider_type: "gcp_cloud_run_openai_compat".to_string(),
        config: Value::Object(config),
        secrets: None,
    }
}

pub(super) fn vertex_provider(location: &str) -> ProviderConnection {
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

pub(super) fn bedrock_provider() -> ProviderConnection {
    ProviderConnection {
        provider_key: "bedrock-prod".to_string(),
        provider_type: "aws_bedrock".to_string(),
        config: json!({
            "region": "us-east-1",
            "endpoint_url": "https://bedrock-runtime.us-east-1.amazonaws.com"
        }),
        secrets: None,
    }
}

pub(super) fn route(provider_key: &str, upstream_model: &str) -> ModelRoute {
    ModelRoute {
        id: Uuid::new_v4(),
        model_id: Uuid::new_v4(),
        provider_key: provider_key.to_string(),
        upstream_model: upstream_model.to_string(),
        priority: 10,
        weight: 1.0,
        enabled: true,
        context_window_tokens: None,
        pricing_override: None,
        extra_headers: serde_json::Map::new(),
        extra_body: serde_json::Map::new(),
        capabilities: ProviderCapabilities::all_enabled().into(),
        compatibility: Default::default(),
    }
}

/// The models.dev fallback snapshot the test catalogs use.
pub(super) fn fallback_snapshot() -> PricingCatalogSnapshotFile {
    PricingCatalogSnapshotFile {
        metadata: PricingCatalogSnapshotMetadata {
            source: MODELS_DEV_VENDORED_SOURCE.to_string(),
            etag: None,
            fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
            checksum: None,
        },
        providers: fallback_providers(),
    }
}

fn fallback_providers() -> Providers {
    BTreeMap::from([
        (
            "amazon-bedrock".to_string(),
            PricingCatalogProviderDocument {
                display_name: "Amazon Bedrock".to_string(),
                models: BTreeMap::from([
                    (
                        "us.anthropic.claude-sonnet-4-6".to_string(),
                        PricingCatalogModelDocument {
                            metadata: Default::default(),
                            id: "us.anthropic.claude-sonnet-4-6".to_string(),
                            display_name: "Claude Sonnet 4.6 (US)".to_string(),
                            release_date: "2026-02-17".to_string(),
                            last_updated: "2026-03-13".to_string(),
                            cost: PricingCatalogCostDocument {
                                conditions: Default::default(),
                                input: Some("3.0000".to_string()),
                                output: Some("15.0000".to_string()),
                                cache_read: Some("0.3000".to_string()),
                                cache_write: Some("3.7500".to_string()),
                                input_audio: None,
                                output_audio: None,
                                output_image: None,
                                tiers: Vec::new(),
                            },
                            limit: PricingCatalogLimitDocument {
                                context: Some(1_000_000),
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
                    ),
                    (
                        "openai.gpt-oss-120b-1:0".to_string(),
                        PricingCatalogModelDocument {
                            metadata: Default::default(),
                            id: "openai.gpt-oss-120b-1:0".to_string(),
                            display_name: "gpt-oss-120b".to_string(),
                            release_date: "2024-12-01".to_string(),
                            last_updated: "2024-12-01".to_string(),
                            cost: PricingCatalogCostDocument {
                                conditions: Default::default(),
                                input: Some("0.1500".to_string()),
                                output: Some("0.6000".to_string()),
                                cache_read: None,
                                cache_write: None,
                                input_audio: None,
                                output_audio: None,
                                output_image: None,
                                tiers: Vec::new(),
                            },
                            limit: PricingCatalogLimitDocument {
                                context: Some(128_000),
                                input: None,
                                output: Some(4_096),
                            },
                            modalities: PricingCatalogModalitiesDocument {
                                input: vec!["text".to_string()],
                                output: vec!["text".to_string()],
                            },
                        },
                    ),
                ]),
            },
        ),
        (
            "openai".to_string(),
            PricingCatalogProviderDocument {
                display_name: "OpenAI".to_string(),
                models: BTreeMap::from([(
                    "gpt-5".to_string(),
                    PricingCatalogModelDocument {
                        metadata: Default::default(),
                        id: "gpt-5".to_string(),
                        display_name: "GPT-5".to_string(),
                        release_date: "2025-08-07".to_string(),
                        last_updated: "2025-08-07".to_string(),
                        cost: PricingCatalogCostDocument {
                            conditions: Default::default(),
                            input: Some("1.2500".to_string()),
                            output: Some("10.0000".to_string()),
                            cache_read: Some("0.1250".to_string()),
                            cache_write: None,
                            input_audio: None,
                            output_audio: None,
                            output_image: None,
                            tiers: Vec::new(),
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
            "openrouter".to_string(),
            PricingCatalogProviderDocument {
                display_name: "OpenRouter".to_string(),
                models: BTreeMap::from([(
                    "deepseek/deepseek-v4-flash".to_string(),
                    PricingCatalogModelDocument {
                        metadata: Default::default(),
                        id: "deepseek/deepseek-v4-flash".to_string(),
                        display_name: "DeepSeek V4 Flash".to_string(),
                        release_date: "2026-07-01".to_string(),
                        last_updated: "2026-07-01".to_string(),
                        cost: PricingCatalogCostDocument {
                            conditions: Default::default(),
                            input: Some("0.0900".to_string()),
                            output: Some("0.1800".to_string()),
                            cache_read: Some("0.0180".to_string()),
                            cache_write: None,
                            input_audio: None,
                            output_audio: None,
                            output_image: None,
                            tiers: Vec::new(),
                        },
                        limit: PricingCatalogLimitDocument {
                            context: Some(128_000),
                            input: None,
                            output: Some(8_192),
                        },
                        modalities: PricingCatalogModalitiesDocument {
                            input: vec!["text".to_string()],
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
                models: BTreeMap::from([
                    (
                        "gemini-2.5-flash".to_string(),
                        PricingCatalogModelDocument {
                            metadata: Default::default(),
                            id: "gemini-2.5-flash".to_string(),
                            display_name: "Gemini 2.5 Flash".to_string(),
                            release_date: "2025-06-17".to_string(),
                            last_updated: "2025-06-17".to_string(),
                            cost: PricingCatalogCostDocument {
                                conditions: Default::default(),
                                input: Some("0.3000".to_string()),
                                output: Some("2.5000".to_string()),
                                cache_read: Some("0.0750".to_string()),
                                cache_write: Some("0.3830".to_string()),
                                input_audio: None,
                                output_audio: None,
                                output_image: None,
                                tiers: Vec::new(),
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
                    ),
                    (
                        "gemini-embedding-001".to_string(),
                        PricingCatalogModelDocument {
                            metadata: Default::default(),
                            id: "gemini-embedding-001".to_string(),
                            display_name: "Gemini Embedding".to_string(),
                            release_date: "2025-05-20".to_string(),
                            last_updated: "2025-05-20".to_string(),
                            cost: PricingCatalogCostDocument {
                                conditions: Default::default(),
                                input: Some("0.1500".to_string()),
                                output: None,
                                cache_read: None,
                                cache_write: None,
                                input_audio: None,
                                output_audio: None,
                                output_image: None,
                                tiers: Vec::new(),
                            },
                            limit: PricingCatalogLimitDocument {
                                context: Some(2_048),
                                input: None,
                                output: None,
                            },
                            modalities: PricingCatalogModalitiesDocument {
                                input: vec!["text".to_string()],
                                output: vec!["embedding".to_string()],
                            },
                        },
                    ),
                ]),
            },
        ),
        (
            "google-vertex-anthropic".to_string(),
            PricingCatalogProviderDocument {
                display_name: "Vertex (Anthropic)".to_string(),
                models: BTreeMap::from([(
                    "claude-sonnet-4-6@default".to_string(),
                    PricingCatalogModelDocument {
                        metadata: Default::default(),
                        id: "claude-sonnet-4-6@default".to_string(),
                        display_name: "Claude Sonnet 4.6".to_string(),
                        release_date: "2026-02-17".to_string(),
                        last_updated: "2026-03-13".to_string(),
                        cost: PricingCatalogCostDocument {
                            conditions: Default::default(),
                            input: Some("3.0000".to_string()),
                            output: Some("15.0000".to_string()),
                            cache_read: Some("0.3000".to_string()),
                            cache_write: Some("3.7500".to_string()),
                            input_audio: None,
                            output_audio: None,
                            output_image: None,
                            tiers: Vec::new(),
                        },
                        limit: PricingCatalogLimitDocument {
                            context: Some(200_000),
                            input: None,
                            output: Some(64_000),
                        },
                        modalities: PricingCatalogModalitiesDocument {
                            input: vec!["text".to_string(), "image".to_string(), "pdf".to_string()],
                            output: vec!["text".to_string()],
                        },
                    },
                )]),
            },
        ),
        (
            "opencode".to_string(),
            PricingCatalogProviderDocument {
                display_name: "OpenCode".to_string(),
                models: BTreeMap::from([
                    (
                        "claude-fable-5-1".to_string(),
                        PricingCatalogModelDocument {
                            metadata: Default::default(),
                            id: "claude-fable-5-1".to_string(),
                            display_name: "Claude Fable 5.1".to_string(),
                            release_date: "2026-09-01".to_string(),
                            last_updated: "2026-09-01".to_string(),
                            cost: PricingCatalogCostDocument {
                                conditions: Default::default(),
                                input: Some("10.0000".to_string()),
                                output: Some("50.0000".to_string()),
                                cache_read: Some("0.2500".to_string()),
                                cache_write: Some("12.5000".to_string()),
                                input_audio: None,
                                output_audio: None,
                                output_image: None,
                                tiers: Vec::new(),
                            },
                            limit: PricingCatalogLimitDocument {
                                context: Some(1_000_000),
                                input: None,
                                output: Some(128_000),
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
                    ),
                    (
                        "claude-sonnet-5".to_string(),
                        PricingCatalogModelDocument {
                            metadata: Default::default(),
                            id: "claude-sonnet-5".to_string(),
                            display_name: "Claude Sonnet 5".to_string(),
                            release_date: "2026-05-15".to_string(),
                            last_updated: "2026-05-15".to_string(),
                            cost: PricingCatalogCostDocument {
                                conditions: Default::default(),
                                input: Some("3.0000".to_string()),
                                output: Some("15.0000".to_string()),
                                cache_read: Some("0.3000".to_string()),
                                cache_write: Some("3.7500".to_string()),
                                input_audio: None,
                                output_audio: None,
                                output_image: None,
                                tiers: Vec::new(),
                            },
                            limit: PricingCatalogLimitDocument {
                                context: Some(1_000_000),
                                input: None,
                                output: Some(128_000),
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
                    ),
                ]),
            },
        ),
    ])
}

/// An empty vendored snapshot for `source`, fetched at the epoch's first second.
pub(super) fn empty_source_snapshot(source: CatalogSourceId) -> PricingCatalogSnapshotFile {
    PricingCatalogSnapshotFile {
        metadata: PricingCatalogSnapshotMetadata {
            source: source.descriptor().vendored_source.to_string(),
            etag: None,
            fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
            checksum: None,
        },
        providers: Providers::new(),
    }
}

/// A catalog fetching each source from `urls` (in [`CatalogSourceId::ALL`] order), with
/// [`fallback_snapshot`] for models.dev and empty fallbacks for the others.
pub(super) fn catalog_with(
    repo: Arc<InMemoryRepo>,
    refresh_interval: Duration,
    urls: [String; 3],
) -> PricingCatalog<InMemoryRepo> {
    let [models_dev, openrouter, litellm] = urls;
    PricingCatalog::with_sources(
        repo,
        refresh_interval,
        [
            CatalogSource::new(
                CatalogSourceId::ModelsDev.descriptor(),
                models_dev,
                fallback_snapshot(),
            ),
            CatalogSource::new(
                CatalogSourceId::OpenRouter.descriptor(),
                openrouter,
                empty_source_snapshot(CatalogSourceId::OpenRouter),
            ),
            CatalogSource::new(
                CatalogSourceId::LiteLlm.descriptor(),
                litellm,
                empty_source_snapshot(CatalogSourceId::LiteLlm),
            ),
        ],
    )
}

/// A catalog refreshing models.dev from `models_dev_url` on every check, whose other
/// sources are unreachable.
pub(super) fn empty_catalog(
    repo: Arc<InMemoryRepo>,
    models_dev_url: String,
) -> PricingCatalog<InMemoryRepo> {
    catalog_with(
        repo,
        Duration::ZERO,
        [
            models_dev_url,
            UNREACHABLE_URL.to_string(),
            UNREACHABLE_URL.to_string(),
        ],
    )
}

/// The merged snapshot of `models_dev` beside empty OpenRouter and LiteLLM snapshots.
pub(super) fn merged(models_dev: PricingCatalogSnapshotFile) -> PricingCatalogSnapshot {
    PricingCatalogSnapshot::from_sources(BTreeMap::from([
        (CatalogSourceId::ModelsDev, models_dev),
        (
            CatalogSourceId::OpenRouter,
            empty_source_snapshot(CatalogSourceId::OpenRouter),
        ),
        (
            CatalogSourceId::LiteLlm,
            empty_source_snapshot(CatalogSourceId::LiteLlm),
        ),
    ]))
}

/// A cache row for `source` holding `providers` as its canonical document JSON.
pub(super) fn cache_row(
    source: CatalogSourceId,
    etag: Option<&str>,
    fetched_at: OffsetDateTime,
    providers: &Providers,
) -> PricingCatalogCacheRecord {
    let descriptor = source.descriptor();
    PricingCatalogCacheRecord {
        catalog_key: descriptor.cache_key.to_string(),
        source: descriptor.remote_source.to_string(),
        etag: etag.map(str::to_string),
        fetched_at,
        snapshot_json: canonical_document_json(providers).expect("document json"),
    }
}

pub(super) async fn seed_catalog_snapshot(
    repo: &InMemoryRepo,
    snapshot: &PricingCatalogSnapshotFile,
) {
    seed_source_snapshot(repo, CatalogSourceId::ModelsDev, snapshot).await;
}

pub(super) async fn seed_source_snapshot(
    repo: &InMemoryRepo,
    source: CatalogSourceId,
    snapshot: &PricingCatalogSnapshotFile,
) {
    let catalog_key = source.descriptor().cache_key;
    let expected_fetched_at = repo
        .get_pricing_catalog_cache(catalog_key)
        .await
        .expect("load catalog snapshot")
        .map(|cache| cache.fetched_at);
    let stored = repo
        .compare_and_swap_pricing_catalog_cache(
            &PricingCatalogCacheRecord {
                catalog_key: catalog_key.to_string(),
                source: snapshot.metadata.source.clone(),
                etag: snapshot.metadata.etag.clone(),
                fetched_at: snapshot.metadata.fetched_at,
                snapshot_json: canonical_document_json(&snapshot.providers).expect("json"),
            },
            expected_fetched_at,
        )
        .await
        .expect("seed catalog snapshot");
    assert!(
        stored,
        "catalog snapshot should replace the expected generation"
    );
}

pub(super) async fn start_server(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    format!("http://{addr}")
}

/// Each source's URL on `host`, in [`CatalogSourceId::ALL`] order.
pub(super) fn source_urls(host: &str) -> [String; 3] {
    SOURCE_PATHS.map(|path| format!("{host}{path}"))
}

/// A remote models.dev snapshot of the raw `body`.
pub(super) fn project_models_dev(
    body: &Value,
    etag: Option<&str>,
    fetched_at: OffsetDateTime,
) -> PricingCatalogSnapshotFile {
    let providers =
        models_dev::project(body.to_string().as_bytes()).expect("project models.dev body");
    projected_snapshot(
        providers,
        MODELS_DEV_REMOTE_SOURCE,
        etag.map(str::to_string),
        fetched_at,
    )
    .expect("projected snapshot")
    .snapshot
}

pub(super) fn reset_pricing_repo_counters(repo: &InMemoryRepo) {
    repo.cache_reads.store(0, Ordering::Relaxed);
    repo.active_pricing_reads.store(0, Ordering::Relaxed);
    repo.pricing_sync_applications.store(0, Ordering::Relaxed);
    repo.pricing_resolutions.store(0, Ordering::Relaxed);
}
