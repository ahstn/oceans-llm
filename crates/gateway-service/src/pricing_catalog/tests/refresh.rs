use super::support::*;

type ConcurrentRefreshState = (Arc<tokio::sync::Barrier>, Arc<Mutex<Vec<String>>>);

fn minimal_catalog_body() -> Value {
    json!({
        "openai": {
            "name": "OpenAI",
            "models": {}
        }
    })
}

fn gpt_5_body(input_cost: f64) -> Value {
    json!({
        "openai": {
            "name": "OpenAI",
            "models": {
                "gpt-5": {
                    "id": "gpt-5",
                    "name": "GPT-5",
                    "release_date": "2025-08-07",
                    "last_updated": "2025-08-07",
                    "cost": {"input": input_cost, "output": 10.0},
                    "limit": {"context": 400000, "output": 128000},
                    "modalities": {"input": ["text"], "output": ["text"]}
                }
            }
        }
    })
}

fn epoch_second() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1).expect("timestamp")
}

fn models_dev_outcome(
    outcomes: &[(CatalogSourceId, PricingCatalogRefreshOutcome)],
) -> PricingCatalogRefreshOutcome {
    outcomes
        .iter()
        .find(|(source, _)| *source == CatalogSourceId::ModelsDev)
        .map(|(_, outcome)| *outcome)
        .expect("models.dev was refreshed")
}

#[test]
fn vendored_snapshot_json_ends_with_one_newline() {
    let json = snapshot_to_pretty_json(&fallback_snapshot()).expect("snapshot serializes");

    assert!(json.ends_with("}\n"), "expected a single trailing newline");
}

#[test]
fn vendored_sync_is_unchanged_only_for_the_same_projected_providers() {
    // The file on disk predates checksums and carries different metadata.
    let mut existing = fallback_snapshot();
    let fetched = projected_snapshot(
        existing.providers.clone(),
        MODELS_DEV_VENDORED_SOURCE,
        Some("\"new-etag\"".to_string()),
        test_time(),
    )
    .expect("project snapshot")
    .snapshot;
    let existing_json = snapshot_to_pretty_json(&existing).expect("existing json");

    assert!(vendored_snapshot_is_unchanged(&existing_json, &fetched));

    existing.providers.remove("opencode");
    let changed_json = snapshot_to_pretty_json(&existing).expect("changed json");

    assert!(!vendored_snapshot_is_unchanged(&changed_json, &fetched));
}

#[test]
fn catalog_generation_advances_when_refreshes_share_a_wall_clock_second() {
    let current = fallback_snapshot().metadata.fetched_at;

    let next = next_catalog_generation_at(Some(current), current);

    assert_eq!(next, current + time::Duration::seconds(1));
}

#[tokio::test]
async fn concurrent_refreshes_with_different_documents_converge() {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let bodies = Arc::new(Mutex::new(vec![
        gpt_5_body(2.0).to_string(),
        gpt_5_body(3.0).to_string(),
    ]));
    let app = Router::new()
        .route(
            "/api.json",
            get(
                |State((barrier, bodies)): State<ConcurrentRefreshState>| async move {
                    barrier.wait().await;
                    let body = bodies.lock().expect("catalog bodies").pop().expect("body");
                    (StatusCode::OK, body)
                },
            ),
        )
        .with_state((barrier, bodies));
    let host = start_server(app).await;
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
    let (first_progress, second_progress) = (AuditProgress::default(), AuditProgress::default());

    let (first, second) = tokio::join!(
        catalog.refresh_now_and_sync_tracked(&first_progress),
        catalog.refresh_now_and_sync_tracked(&second_progress)
    );

    first.expect("first refresh");
    second.expect("second refresh");
    let latest = catalog.reload_snapshot().await.expect("latest snapshot");
    let applications = repo.sync_applications();
    reconcile_model_pricing(repo.as_ref(), &latest)
        .await
        .expect("reconcile latest snapshot");
    assert_eq!(
        repo.sync_applications(),
        applications,
        "the latest snapshot is already reconciled"
    );
    let expected = build_model_pricing_record(
        &latest.sources[&CatalogSourceId::ModelsDev],
        "openai",
        "gpt-5",
        latest.model("openai", "gpt-5").expect("latest GPT-5"),
    )
    .expect("build expected GPT-5 pricing");
    assert_eq!(
        repo.active_row("openai", "gpt-5")
            .input_cost_per_million_tokens,
        expected.input_cost_per_million_tokens
    );
}

#[tokio::test]
async fn concurrent_304_does_not_block_a_200_refresh() {
    let repo = Arc::new(InMemoryRepo::with_cache_rows([cache_row(
        CatalogSourceId::ModelsDev,
        Some("\"old-etag\""),
        epoch_second(),
        &fallback_snapshot().providers,
    )]));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let app = Router::new().route(
        "/api.json",
        get(move |headers: HeaderMap| {
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                if headers.contains_key(IF_NONE_MATCH) {
                    StatusCode::NOT_MODIFIED.into_response()
                } else {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    (
                        [(ETAG, HeaderValue::from_static("\"new-etag\""))],
                        axum::Json(minimal_catalog_body()),
                    )
                        .into_response()
                }
            }
        }),
    );
    let host = start_server(app).await;
    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
    let progress = AuditProgress::default();

    let (conditional, forced) = tokio::join!(
        catalog.refresh_sources(RefreshMode::IfStale),
        catalog.refresh_now_and_sync_tracked(&progress)
    );

    assert_eq!(
        models_dev_outcome(&conditional),
        PricingCatalogRefreshOutcome::Unchanged
    );
    forced.expect("forced refresh");
    let cache = repo.cache_row(MODELS_DEV_CACHE_KEY).expect("cache row");
    assert_eq!(cache.etag.as_deref(), Some("\"new-etag\""));
}

#[tokio::test]
async fn refresh_uses_conditional_etag_and_handles_304() {
    let repo = Arc::new(InMemoryRepo::with_cache_rows([cache_row(
        CatalogSourceId::ModelsDev,
        Some("\"catalog-etag\""),
        epoch_second(),
        &fallback_snapshot().providers,
    )]));
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
    let before = repo.cache_row(MODELS_DEV_CACHE_KEY).expect("cache row");

    let outcomes = catalog.refresh_sources(RefreshMode::IfStale).await;

    assert_eq!(
        models_dev_outcome(&outcomes),
        PricingCatalogRefreshOutcome::Unchanged
    );
    assert_eq!(
        state.lock().expect("captured lock").as_deref(),
        Some("\"catalog-etag\"")
    );
    assert_eq!(repo.cache_row(MODELS_DEV_CACHE_KEY), Some(before));
}

#[tokio::test]
async fn not_modified_is_not_rechecked_within_the_refresh_interval() {
    let repo = Arc::new(InMemoryRepo::with_cache_rows([cache_row(
        CatalogSourceId::ModelsDev,
        Some("\"catalog-etag\""),
        epoch_second(),
        &fallback_snapshot().providers,
    )]));
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api.json",
            get(|State(hits): State<Arc<AtomicUsize>>| async move {
                hits.fetch_add(1, Ordering::Relaxed);
                StatusCode::NOT_MODIFIED
            }),
        )
        .with_state(hits.clone());
    let host = start_server(app).await;
    let catalog = catalog_with(
        repo,
        Duration::from_secs(3600),
        [
            format!("{host}/api.json"),
            UNREACHABLE_URL.to_string(),
            UNREACHABLE_URL.to_string(),
        ],
    );

    let first = catalog.refresh_sources(RefreshMode::IfStale).await;
    let second = catalog.refresh_sources(RefreshMode::IfStale).await;

    assert_eq!(
        models_dev_outcome(&first),
        PricingCatalogRefreshOutcome::Unchanged
    );
    assert!(
        second
            .iter()
            .all(|(source, _)| *source != CatalogSourceId::ModelsDev),
        "a checked source is skipped until the interval passes"
    );
    assert_eq!(hits.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn identical_projection_on_200_keeps_the_snapshot_and_pricing_generation() {
    let body = gpt_5_body(1.25);
    let stored = project_models_dev(&body, Some("\"old-etag\""), test_time());
    let repo = Arc::new(InMemoryRepo::default());
    seed_catalog_snapshot(&repo, &stored).await;
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/api.json",
            get(move |State(hits): State<Arc<AtomicUsize>>| {
                let body = body.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    (
                        [(ETAG, HeaderValue::from_static("\"new-etag\""))],
                        axum::Json(body),
                    )
                }
            }),
        )
        .with_state(hits.clone());
    let host = start_server(app).await;
    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
    catalog
        .sync_latest_snapshot_with_retry()
        .await
        .expect("sync stored snapshot");
    let cache_before = repo.cache_row(MODELS_DEV_CACHE_KEY).expect("cache row");
    let rows_before =
        serde_json::to_value(repo.list_active_model_pricing().await.unwrap()).unwrap();

    let sources = catalog
        .refresh_now_and_sync_tracked(&AuditProgress::default())
        .await
        .expect("refresh unchanged catalog");

    assert_eq!(sources[0].source, CatalogSourceId::ModelsDev);
    assert_eq!(sources[0].outcome, PricingCatalogRefreshOutcome::Unchanged);
    assert_eq!(sources[0].fetched_at, test_time());
    assert_eq!(hits.load(Ordering::Relaxed), 1);
    assert_eq!(repo.cache_row(MODELS_DEV_CACHE_KEY), Some(cache_before));
    assert_eq!(
        serde_json::to_value(repo.list_active_model_pricing().await.unwrap()).unwrap(),
        rows_before
    );
}

#[tokio::test]
async fn refresh_replaces_cached_snapshot_on_200() {
    let repo = Arc::new(InMemoryRepo::default());
    let body = gpt_5_body(1.25);
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

    let outcomes = catalog.refresh_sources(RefreshMode::IfStale).await;

    assert_eq!(
        models_dev_outcome(&outcomes),
        PricingCatalogRefreshOutcome::Updated
    );
    let cache = repo.cache_row(MODELS_DEV_CACHE_KEY).expect("cache row");
    assert_eq!(cache.source, MODELS_DEV_REMOTE_SOURCE);
    assert_eq!(cache.etag.as_deref(), Some("\"new-etag\""));
    assert!(cache.snapshot_json.contains("\"gpt-5\""));
}

#[tokio::test]
async fn refresh_repairs_an_invalid_cached_snapshot() {
    let corrupt_fetched_at = epoch_second();
    let repo = Arc::new(InMemoryRepo::with_cache_rows([PricingCatalogCacheRecord {
        catalog_key: MODELS_DEV_CACHE_KEY.to_string(),
        source: MODELS_DEV_REMOTE_SOURCE.to_string(),
        etag: Some("\"corrupt-etag\"".to_string()),
        fetched_at: corrupt_fetched_at,
        snapshot_json: "not valid json".to_string(),
    }]));
    let body = minimal_catalog_body();
    let app = Router::new().route(
        "/api.json",
        get(move |headers: HeaderMap| {
            let body = body.clone();
            async move {
                if headers.contains_key(IF_NONE_MATCH) {
                    StatusCode::NOT_MODIFIED.into_response()
                } else {
                    axum::Json(body).into_response()
                }
            }
        }),
    );
    let host = start_server(app).await;
    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));

    let outcomes = catalog.refresh_sources(RefreshMode::IfStale).await;

    assert_eq!(
        models_dev_outcome(&outcomes),
        PricingCatalogRefreshOutcome::Updated
    );
    let repaired = repo
        .cache_row(MODELS_DEV_CACHE_KEY)
        .expect("repaired cache row");
    assert!(repaired.fetched_at > corrupt_fetched_at);
    serde_json::from_str::<PricingCatalogDocument>(&repaired.snapshot_json)
        .expect("repaired snapshot JSON");
}

#[tokio::test]
async fn an_unexplained_cache_write_miss_fails_only_that_source() {
    let repo = Arc::new(InMemoryRepo {
        cache_write_rejections_remaining: Arc::new(AtomicUsize::new(1)),
        ..Default::default()
    });
    let app = Router::new().route(
        "/api.json",
        get(|| async { axum::Json(minimal_catalog_body()) }),
    );
    let host = start_server(app).await;
    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));

    let sources = catalog
        .refresh_now_and_sync_tracked(&AuditProgress::default())
        .await
        .expect("reconcile the snapshots in use");

    assert_eq!(
        sources[0].outcome,
        PricingCatalogRefreshOutcome::Failed(PricingCatalogRefreshError::StoreFailed)
    );
    assert_eq!(
        sources[0].fetched_at,
        fallback_snapshot().metadata.fetched_at
    );
    assert_eq!(repo.cache_row(MODELS_DEV_CACHE_KEY), None);
}

#[tokio::test]
async fn remote_failure_falls_back_to_store_then_vendored_snapshot() {
    let cached_providers = BTreeMap::from([(
        "openai".to_string(),
        PricingCatalogProviderDocument {
            display_name: "OpenAI".to_string(),
            models: BTreeMap::from([(
                "gpt-5".to_string(),
                PricingCatalogModelDocument {
                    metadata: Default::default(),
                    id: "gpt-5".to_string(),
                    display_name: "GPT-5 Cached".to_string(),
                    release_date: "2025-08-07".to_string(),
                    last_updated: "2025-08-08".to_string(),
                    cost: PricingCatalogCostDocument {
                        conditions: Default::default(),
                        input: Some("2.0000".to_string()),
                        output: Some("20.0000".to_string()),
                        cache_read: None,
                        cache_write: None,
                        input_audio: None,
                        output_audio: None,
                        output_image: None,
                        tiers: Vec::new(),
                    },
                    limit: PricingCatalogLimitDocument::default(),
                    modalities: PricingCatalogModalitiesDocument::default(),
                },
            )]),
        },
    )]);
    let repo = Arc::new(InMemoryRepo::with_cache_rows([cache_row(
        CatalogSourceId::ModelsDev,
        Some("\"cached\""),
        epoch_second(),
        &cached_providers,
    )]));
    let failing_catalog = empty_catalog(repo.clone(), UNREACHABLE_URL.to_string());
    failing_catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize cached pricing catalog");
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
            assert_eq!(pricing.provenance.source, MODELS_DEV_REMOTE_SOURCE);
        }
        other => panic!("unexpected cached resolution: {other:?}"),
    }

    let vendored_catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        UNREACHABLE_URL.to_string(),
    );
    vendored_catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize vendored pricing catalog");
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
            assert_eq!(pricing.provenance.source, MODELS_DEV_VENDORED_SOURCE);
        }
        other => panic!("unexpected vendored resolution: {other:?}"),
    }
}

#[tokio::test]
async fn metadata_reuses_parsed_snapshot_and_observes_refresh() {
    let app = Router::new().route(
        "/api.json",
        get(|| async { (StatusCode::OK, minimal_catalog_body().to_string()) }),
    );
    let host = start_server(app).await;
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = empty_catalog(repo.clone(), format!("{host}/api.json"));
    let first = catalog.metadata_snapshot().await.unwrap();
    let reads = repo.cache_reads.load(Ordering::Relaxed);
    let second = catalog.metadata_snapshot().await.unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(repo.cache_reads.load(Ordering::Relaxed), reads);
    catalog
        .refresh_now_and_sync_tracked(&AuditProgress::default())
        .await
        .unwrap();
    let refreshed = catalog.metadata_snapshot().await.unwrap();
    assert!(!Arc::ptr_eq(&first, &refreshed));
    assert_eq!(
        refreshed.sources[&CatalogSourceId::ModelsDev].source,
        MODELS_DEV_REMOTE_SOURCE
    );
    let reads = repo.cache_reads.load(Ordering::Relaxed);
    catalog.metadata_snapshot().await.unwrap();
    assert_eq!(repo.cache_reads.load(Ordering::Relaxed), reads);
}
