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
pub(super) use serde_json::{Number, Value, json, to_string_pretty};
pub(super) use time::OffsetDateTime;
pub(super) use tokio::net::TcpListener;
pub(super) use uuid::Uuid;

pub(super) use super::super::{
    PRICING_CATALOG_CACHE_KEY, PricingCatalog, PricingCatalogCostDocument, PricingCatalogDocument,
    PricingCatalogLimitDocument, PricingCatalogModalitiesDocument, PricingCatalogModelDocument,
    PricingCatalogProviderDocument, PricingCatalogSnapshot, PricingCatalogSnapshotMetadata,
    PricingTarget, REMOTE_SOURCE, VENDORED_SOURCE, build_model_pricing_record,
    next_catalog_generation_at, normalize_bedrock_pricing_model_id, normalize_models_dev_money,
    normalize_vertex_pricing_model_id, pricing_target_for_route, snapshot_is_already_synced,
};

#[derive(Clone, Default)]
pub(super) struct InMemoryRepo {
    pub(super) cache: Arc<Mutex<Option<PricingCatalogCacheRecord>>>,
    pub(super) pricing_rows: Arc<Mutex<Vec<ModelPricingRecord>>>,
    pub(super) cache_reads: Arc<AtomicUsize>,
    pub(super) active_pricing_reads: Arc<AtomicUsize>,
    pub(super) pricing_sync_applications: Arc<AtomicUsize>,
    pub(super) pricing_resolutions: Arc<AtomicUsize>,
    pub(super) pricing_sync_conflicts_remaining: Arc<AtomicUsize>,
    pub(super) cache_write_rejections_remaining: Arc<AtomicUsize>,
}

#[async_trait]
impl PricingCatalogRepository for InMemoryRepo {
    async fn get_pricing_catalog_cache(
        &self,
        _catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
        self.cache_reads.fetch_add(1, Ordering::Relaxed);
        Ok(self.cache.lock().expect("cache lock").clone())
    }

    async fn compare_and_swap_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
        expected_fetched_at: Option<OffsetDateTime>,
    ) -> Result<bool, StoreError> {
        if self
            .cache_write_rejections_remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Ok(false);
        }

        let mut stored = self.cache.lock().expect("cache lock");
        if stored.as_ref().map(|current| current.fetched_at) != expected_fetched_at
            || expected_fetched_at.is_some_and(|expected| cache.fetched_at <= expected)
        {
            return Ok(false);
        }
        *stored = Some(cache.clone());
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
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
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
        extra_headers: serde_json::Map::new(),
        extra_body: serde_json::Map::new(),
        capabilities: ProviderCapabilities::all_enabled(),
        compatibility: Default::default(),
    }
}

pub(super) fn fallback_snapshot() -> PricingCatalogSnapshot {
    PricingCatalogSnapshot {
        metadata: PricingCatalogSnapshotMetadata {
            source: VENDORED_SOURCE.to_string(),
            etag: None,
            fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
        },
        document: PricingCatalogDocument {
            providers: BTreeMap::from([
                (
                    "amazon-bedrock".to_string(),
                    PricingCatalogProviderDocument {
                        display_name: "Amazon Bedrock".to_string(),
                        models: BTreeMap::from([
                            (
                                "us.anthropic.claude-sonnet-4-6".to_string(),
                                PricingCatalogModelDocument {
                                    id: "us.anthropic.claude-sonnet-4-6".to_string(),
                                    display_name: "Claude Sonnet 4.6 (US)".to_string(),
                                    release_date: "2026-02-17".to_string(),
                                    last_updated: "2026-03-13".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("3.0000".to_string()),
                                        output: Some("15.0000".to_string()),
                                        cache_read: Some("0.3000".to_string()),
                                        cache_write: Some("3.7500".to_string()),
                                        input_audio: None,
                                        output_audio: None,
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
                                    id: "openai.gpt-oss-120b-1:0".to_string(),
                                    display_name: "gpt-oss-120b".to_string(),
                                    release_date: "2024-12-01".to_string(),
                                    last_updated: "2024-12-01".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("0.1500".to_string()),
                                        output: Some("0.6000".to_string()),
                                        cache_read: None,
                                        cache_write: None,
                                        input_audio: None,
                                        output_audio: None,
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
                    "openrouter".to_string(),
                    PricingCatalogProviderDocument {
                        display_name: "OpenRouter".to_string(),
                        models: BTreeMap::from([(
                            "deepseek/deepseek-v4-flash".to_string(),
                            PricingCatalogModelDocument {
                                id: "deepseek/deepseek-v4-flash".to_string(),
                                display_name: "DeepSeek V4 Flash".to_string(),
                                release_date: "2026-07-01".to_string(),
                                last_updated: "2026-07-01".to_string(),
                                cost: PricingCatalogCostDocument {
                                    input: Some("0.0900".to_string()),
                                    output: Some("0.1800".to_string()),
                                    cache_read: Some("0.0180".to_string()),
                                    cache_write: None,
                                    input_audio: None,
                                    output_audio: None,
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
                            ),
                            (
                                "gemini-embedding-001".to_string(),
                                PricingCatalogModelDocument {
                                    id: "gemini-embedding-001".to_string(),
                                    display_name: "Gemini Embedding".to_string(),
                                    release_date: "2025-05-20".to_string(),
                                    last_updated: "2025-05-20".to_string(),
                                    cost: PricingCatalogCostDocument {
                                        input: Some("0.1500".to_string()),
                                        output: None,
                                        cache_read: None,
                                        cache_write: None,
                                        input_audio: None,
                                        output_audio: None,
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
                                id: "claude-sonnet-4-6@default".to_string(),
                                display_name: "Claude Sonnet 4.6".to_string(),
                                release_date: "2026-02-17".to_string(),
                                last_updated: "2026-03-13".to_string(),
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

pub(super) fn empty_catalog(
    repo: Arc<InMemoryRepo>,
    source_url: String,
) -> PricingCatalog<InMemoryRepo> {
    PricingCatalog::with_fallback_snapshot(
        repo,
        source_url,
        PRICING_CATALOG_CACHE_KEY.to_string(),
        Duration::from_secs(0),
        fallback_snapshot(),
    )
}

pub(super) async fn seed_catalog_snapshot(repo: &InMemoryRepo, snapshot: &PricingCatalogSnapshot) {
    let expected_fetched_at = repo
        .get_pricing_catalog_cache(PRICING_CATALOG_CACHE_KEY)
        .await
        .expect("load catalog snapshot")
        .map(|cache| cache.fetched_at);
    let stored = repo
        .compare_and_swap_pricing_catalog_cache(
            &PricingCatalogCacheRecord {
                catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
                source: snapshot.metadata.source.clone(),
                etag: snapshot.metadata.etag.clone(),
                fetched_at: snapshot.metadata.fetched_at,
                snapshot_json: to_string_pretty(&snapshot.document).expect("json"),
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

pub(super) fn reset_pricing_repo_counters(repo: &InMemoryRepo) {
    repo.cache_reads.store(0, Ordering::Relaxed);
    repo.active_pricing_reads.store(0, Ordering::Relaxed);
    repo.pricing_sync_applications.store(0, Ordering::Relaxed);
    repo.pricing_resolutions.store(0, Ordering::Relaxed);
}
