use gateway_core::ContextPricingTier;

use super::support::*;

fn tier(above_input_tokens: i64, input: &str, output: &str) -> PricingCatalogContextTier {
    PricingCatalogContextTier {
        above_input_tokens,
        input: Some(input.to_string()),
        output: Some(output.to_string()),
        cache_read: None,
        cache_write: None,
    }
}

#[tokio::test]
async fn pricing_reconciliation_reloads_and_retries_after_a_store_conflict() {
    let repo = Arc::new(InMemoryRepo::default());
    repo.pricing_sync_conflicts_remaining
        .store(1, Ordering::Relaxed);
    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());

    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("retry pricing sync");

    assert_eq!(repo.pricing_sync_applications.load(Ordering::Relaxed), 2);
    assert!(!repo.pricing_rows.lock().expect("pricing rows").is_empty());
}

#[tokio::test]
async fn snapshot_replaced_in_the_cache_is_a_sync_conflict() {
    let repo = Arc::new(InMemoryRepo::default());
    let stale = models_dev_at(test_time(), "2025-01-01");
    seed_catalog_snapshot(repo.as_ref(), &stale).await;
    seed_catalog_snapshot(
        repo.as_ref(),
        &models_dev_at(test_time() + time::Duration::minutes(1), "2025-02-02"),
    )
    .await;

    let error = reconcile_model_pricing(repo.as_ref(), &merged(stale))
        .await
        .expect_err("a snapshot the cache replaced must not be applied");

    assert!(matches!(
        error,
        GatewayError::Store(StoreError::PricingSyncConflict)
    ));
    assert!(repo.pricing_rows.lock().expect("pricing rows").is_empty());
}

#[tokio::test]
async fn newer_source_combination_of_the_same_generation_replaces_the_applied_one() {
    let repo = Arc::new(InMemoryRepo::default());
    let generation = test_time();
    let mut openrouter = empty_source_snapshot(CatalogSourceId::OpenRouter);
    openrouter.metadata.fetched_at = generation;
    seed_source_snapshot(repo.as_ref(), CatalogSourceId::OpenRouter, &openrouter).await;
    // models.dev prices GPT-5 in both combinations, from snapshots older than the
    // OpenRouter one, so both combinations share its generation.
    let older = models_dev_at(generation - time::Duration::minutes(2), "2025-01-01");
    let newer = models_dev_at(generation - time::Duration::minutes(1), "2025-02-02");
    let stale = merged_with_openrouter(older.clone(), openrouter.clone());
    let latest = merged_with_openrouter(newer.clone(), openrouter);
    assert_eq!(stale.generation_at(), latest.generation_at());

    seed_catalog_snapshot(repo.as_ref(), &older).await;
    reconcile_model_pricing(repo.as_ref(), &stale)
        .await
        .expect("apply the first combination");
    seed_catalog_snapshot(repo.as_ref(), &newer).await;
    reconcile_model_pricing(repo.as_ref(), &latest)
        .await
        .expect("the latest combination replaces rows of its own generation");
    let error = reconcile_model_pricing(repo.as_ref(), &stale)
        .await
        .expect_err("the replaced combination must not be applied again");
    assert!(matches!(
        error,
        GatewayError::Store(StoreError::PricingSyncConflict)
    ));
    let applications = repo.sync_applications();
    reconcile_model_pricing(repo.as_ref(), &latest)
        .await
        .expect("the latest combination stays reconciled");
    assert_eq!(repo.sync_applications(), applications);

    let active = repo.active_row("openai", "gpt-5");
    assert_eq!(active.last_updated, "2025-02-02");
    assert_eq!(active.provenance.fetched_at, newer.metadata.fetched_at);
    assert_eq!(
        active.effective_start_at,
        generation + time::Duration::seconds(1)
    );
    let rows = repo.pricing_rows.lock().expect("pricing rows");
    let gpt_5_rows = rows
        .iter()
        .filter(|row| row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5")
        .collect::<Vec<_>>();
    assert_eq!(gpt_5_rows.len(), 2);
    assert!(gpt_5_rows.iter().any(|row| {
        row.effective_start_at == generation
            && row.effective_end_at == Some(active.effective_start_at)
    }));
}

#[tokio::test]
async fn unparseable_cache_row_does_not_block_its_vendored_fallback() {
    let repo = Arc::new(InMemoryRepo::with_cache_rows([PricingCatalogCacheRecord {
        catalog_key: OPENROUTER_CACHE_KEY.to_string(),
        source: "openrouter_api".to_string(),
        etag: None,
        fetched_at: test_time(),
        snapshot_json: "not a catalog document".to_string(),
    }]));
    // Every source is unreachable, so the unparseable row stays and OpenRouter keeps its
    // vendored snapshot.
    let catalog = empty_catalog(repo.clone(), UNREACHABLE_URL.to_string());

    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("reconcile the vendored fallback");

    assert!(!repo.pricing_rows.lock().expect("pricing rows").is_empty());
}

/// A remote models.dev snapshot of the fallback providers, fetched at `fetched_at`, whose
/// GPT-5 was last updated on `gpt_5_last_updated`.
fn models_dev_at(
    fetched_at: OffsetDateTime,
    gpt_5_last_updated: &str,
) -> PricingCatalogSnapshotFile {
    let mut snapshot = fallback_snapshot();
    snapshot.metadata = PricingCatalogSnapshotMetadata {
        source: MODELS_DEV_REMOTE_SOURCE.to_string(),
        etag: None,
        fetched_at,
        checksum: None,
    };
    snapshot
        .providers
        .get_mut("openai")
        .expect("openai provider")
        .models
        .get_mut("gpt-5")
        .expect("gpt-5 model")
        .last_updated = gpt_5_last_updated.to_string();
    snapshot
}

/// The merged snapshot of `models_dev` and `openrouter` beside the empty LiteLLM one.
fn merged_with_openrouter(
    models_dev: PricingCatalogSnapshotFile,
    openrouter: PricingCatalogSnapshotFile,
) -> PricingCatalogSnapshot {
    PricingCatalogSnapshot::from_sources(BTreeMap::from([
        (CatalogSourceId::ModelsDev, models_dev),
        (CatalogSourceId::OpenRouter, openrouter),
        (
            CatalogSourceId::LiteLlm,
            empty_source_snapshot(CatalogSourceId::LiteLlm),
        ),
    ]))
}

#[tokio::test]
async fn mixed_current_and_older_active_rows_finish_reconciling() {
    let repo = Arc::new(InMemoryRepo::default());
    let snapshot = merged(fallback_snapshot());
    reconcile_model_pricing(repo.as_ref(), &snapshot)
        .await
        .expect("seed pricing rows");

    repo.pricing_rows
        .lock()
        .expect("pricing rows")
        .first_mut()
        .expect("active pricing row")
        .provenance
        .fetched_at -= time::Duration::seconds(1);

    reconcile_model_pricing(repo.as_ref(), &snapshot)
        .await
        .expect("finish partially applied snapshot");

    let rows = repo.pricing_rows.lock().expect("pricing rows");
    let active_rows = rows
        .iter()
        .filter(|row| row.effective_end_at.is_none())
        .collect::<Vec<_>>();
    assert!(!active_rows.is_empty());
    assert!(
        active_rows
            .iter()
            .all(|row| row.provenance.fetched_at == snapshot.generation_at())
    );
}

#[tokio::test]
async fn forced_refresh_syncs_pricing_rows() {
    let repo = Arc::new(InMemoryRepo::default());
    let body = json!({
        "google-vertex-anthropic": {
            "name": "Google Vertex AI Anthropic",
            "models": {
                "claude-opus-4-8@default": {
                    "id": "claude-opus-4-8@default",
                    "name": "Claude Opus 4.8",
                    "release_date": "2026-07-01",
                    "last_updated": "2026-07-02",
                    "cost": {
                        "input": 5.0,
                        "output": 25.0
                    },
                    "limit": {
                        "context": 200000,
                        "input": 200000,
                        "output": 32000
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
                    [(ETAG, HeaderValue::from_static("\"forced-etag\""))],
                    axum::Json(body),
                )
            }
        }),
    );
    let host = start_server(app).await;

    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
    catalog
        .refresh_now_and_sync_tracked(&AuditProgress::default())
        .await
        .expect("forced refresh");

    let rows = repo.pricing_rows.lock().expect("pricing rows lock");
    let row = rows
        .iter()
        .find(|row| {
            row.pricing_provider_id == "google-vertex-anthropic"
                && row.pricing_model_id == "claude-opus-4-8@default"
        })
        .expect("pricing row");
    assert_eq!(
        row.input_cost_per_million_tokens,
        Some(Money4::from_decimal_str("5.0000").expect("money"))
    );
    assert_eq!(row.provenance.etag.as_deref(), Some("\"forced-etag\""));
}

#[tokio::test]
async fn unchanged_snapshot_does_not_insert_duplicate_active_pricing_rows() {
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

    catalog
        .resolve_for_provider_connection(
            &openai_provider("openai"),
            &route("openai-prod", "gpt-5"),
            test_time(),
        )
        .await
        .expect("first resolve");
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("reconcile unchanged pricing catalog");
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
    seed_catalog_snapshot(repo.as_ref(), &initial).await;

    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");
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
        source: MODELS_DEV_REMOTE_SOURCE.to_string(),
        etag: Some("\"etag-2\"".to_string()),
        fetched_at: test_time() + Duration::from_secs(3600),
        checksum: None,
    };
    changed
        .providers
        .get_mut("openai")
        .expect("openai provider")
        .models
        .get_mut("gpt-5")
        .expect("gpt-5 model")
        .cost
        .input = Some("2.0000".to_string());

    seed_catalog_snapshot(repo.as_ref(), &changed).await;
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("reconcile changed pricing catalog");

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
async fn unchanged_snapshot_refresh_updates_active_row_provenance() {
    let repo = Arc::new(InMemoryRepo::default());
    let initial = fallback_snapshot();
    seed_catalog_snapshot(repo.as_ref(), &initial).await;

    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");
    catalog
        .resolve_for_provider_connection(
            &openai_provider("openai"),
            &route("openai-prod", "gpt-5"),
            test_time(),
        )
        .await
        .expect("seed initial pricing row");

    let mut refreshed = fallback_snapshot();
    refreshed.metadata = PricingCatalogSnapshotMetadata {
        source: MODELS_DEV_REMOTE_SOURCE.to_string(),
        etag: Some("\"etag-refreshed\"".to_string()),
        fetched_at: test_time() + Duration::from_secs(3600),
        checksum: None,
    };
    seed_catalog_snapshot(repo.as_ref(), &refreshed).await;
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("reconcile refreshed pricing catalog");

    catalog
        .resolve_for_provider_connection(
            &openai_provider("openai"),
            &route("openai-prod", "gpt-5"),
            refreshed.metadata.fetched_at + Duration::from_secs(1),
        )
        .await
        .expect("resolve refreshed pricing row");

    let pricing_rows = repo.pricing_rows.lock().expect("pricing rows lock");
    let matching = pricing_rows
        .iter()
        .filter(|row| row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5")
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].provenance.source, refreshed.metadata.source);
    assert_eq!(matching[0].provenance.etag, refreshed.metadata.etag);
    assert_eq!(
        matching[0].provenance.fetched_at,
        refreshed.metadata.fetched_at
    );
}

#[tokio::test]
async fn refreshed_snapshot_closes_removed_active_pricing_rows() {
    let repo = Arc::new(InMemoryRepo::default());
    let initial = fallback_snapshot();
    seed_catalog_snapshot(repo.as_ref(), &initial).await;

    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");
    catalog
        .resolve_for_provider_connection(
            &openai_provider("openai"),
            &route("openai-prod", "gpt-5"),
            test_time(),
        )
        .await
        .expect("seed initial pricing row");

    let mut removed = fallback_snapshot();
    removed.metadata = PricingCatalogSnapshotMetadata {
        source: MODELS_DEV_REMOTE_SOURCE.to_string(),
        etag: Some("\"etag-removed\"".to_string()),
        fetched_at: test_time() + Duration::from_secs(3600),
        checksum: None,
    };
    removed
        .providers
        .get_mut("openai")
        .expect("openai provider")
        .models
        .remove("gpt-5")
        .expect("gpt-5 model");
    seed_catalog_snapshot(repo.as_ref(), &removed).await;
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("reconcile removed pricing catalog");

    let resolved = catalog
        .resolve_for_provider_connection(
            &openai_provider("openai"),
            &route("openai-prod", "gpt-5"),
            removed.metadata.fetched_at + Duration::from_secs(1),
        )
        .await
        .expect("resolve removed pricing row");

    assert_eq!(
        resolved,
        PricingResolution::Unpriced {
            reason: PricingUnpricedReason::ModelNotFound,
        }
    );
    let pricing_rows = repo.pricing_rows.lock().expect("pricing rows lock");
    let removed_row = pricing_rows
        .iter()
        .find(|row| row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5")
        .expect("removed pricing row");
    assert_eq!(
        removed_row.effective_end_at,
        Some(removed.metadata.fetched_at)
    );
}

#[test]
fn pricing_record_carries_context_tiers_and_image_rate() {
    let snapshot = fallback_snapshot();
    let mut document = snapshot.providers["openai"].models["gpt-5"].clone();
    document.cost.output_image = Some("40.0000".to_string());
    document.cost.tiers = vec![
        PricingCatalogContextTier {
            cache_read: Some("0.5000".to_string()),
            ..tier(200_000, "2.5000", "15.0000")
        },
        tier(272_000, "5.0000", "22.5000"),
    ];

    let record = build_model_pricing_record(&snapshot.metadata, "openai", "gpt-5", &document)
        .expect("pricing record");

    assert_eq!(
        record.output_image_usd_per_million,
        Some(Money4::from_scaled(400_000))
    );
    assert_eq!(
        record
            .context_tiers
            .iter()
            .map(|tier| (tier.above_input_tokens, tier.input, tier.cache_read))
            .collect::<Vec<_>>(),
        vec![
            (
                200_000,
                Some(Money4::from_scaled(25_000)),
                Some(Money4::from_scaled(5_000))
            ),
            (272_000, Some(Money4::from_scaled(50_000)), None),
        ]
    );
}

#[test]
fn pricing_record_carries_service_tier_schedules_with_unset_rates_left_unset() {
    let snapshot = fallback_snapshot();
    let mut document = snapshot.providers["openai"].models["gpt-5"].clone();
    document.cost.cache_read = Some("0.1250".to_string());
    document.cost.service_tiers = BTreeMap::from([
        (
            ServiceTier::Flex,
            PricingCatalogServiceTierCost {
                input: Some("0.6250".to_string()),
                output: Some("5.0000".to_string()),
                ..PricingCatalogServiceTierCost::default()
            },
        ),
        (
            ServiceTier::Priority,
            PricingCatalogServiceTierCost {
                input: Some("2.5000".to_string()),
                output: Some("20.0000".to_string()),
                cache_read: Some("0.2500".to_string()),
                cache_write: None,
                context_tiers: vec![tier(272_000, "5.0000", "30.0000")],
            },
        ),
    ]);

    let record = build_model_pricing_record(&snapshot.metadata, "openai", "gpt-5", &document)
        .expect("pricing record");

    let money = |scaled| Some(Money4::from_scaled(scaled));
    assert_eq!(
        record.service_tiers,
        vec![
            ServiceTierPricing {
                service_tier: ServiceTier::Flex,
                input: money(6_250),
                output: money(50_000),
                cache_read: None,
                cache_write: None,
                context_tiers: Vec::new(),
            },
            ServiceTierPricing {
                service_tier: ServiceTier::Priority,
                input: money(25_000),
                output: money(200_000),
                cache_read: money(2_500),
                cache_write: None,
                context_tiers: vec![ContextPricingTier {
                    above_input_tokens: 272_000,
                    input: money(50_000),
                    output: money(300_000),
                    cache_read: None,
                    cache_write: None,
                }],
            },
        ],
        "flex has no cache read rate of its own though the base has one"
    );
}

#[tokio::test]
async fn models_no_source_attributes_data_to_are_not_priced() {
    let repo = Arc::new(InMemoryRepo::default());
    let mut snapshot = fallback_snapshot();
    let gpt_5 = snapshot
        .providers
        .get_mut("openai")
        .expect("openai provider")
        .models
        .get_mut("gpt-5")
        .expect("gpt-5 model");
    gpt_5.cost = PricingCatalogCostDocument::default();
    gpt_5.limit = PricingCatalogLimitDocument::default();
    gpt_5.modalities = PricingCatalogModalitiesDocument::default();

    reconcile_model_pricing(repo.as_ref(), &merged(snapshot))
        .await
        .expect("reconcile snapshot");

    let rows = repo.pricing_rows.lock().expect("pricing rows");
    assert!(
        rows.iter()
            .all(|row| !(row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5"))
    );
    assert!(rows.iter().any(|row| row.pricing_provider_id == "opencode"));
}
