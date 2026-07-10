use super::support::*;

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
        ..Default::default()
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
async fn forced_refresh_ignores_cached_etag() {
    let repo = Arc::new(InMemoryRepo {
        cache: Arc::new(Mutex::new(Some(PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: REMOTE_SOURCE.to_string(),
            etag: Some("\"cached-etag\"".to_string()),
            fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
            snapshot_json: to_string_pretty(&fallback_snapshot().document).expect("json"),
        }))),
        pricing_rows: Arc::new(Mutex::new(Vec::new())),
        ..Default::default()
    });
    let state = Arc::new(Mutex::new(None::<String>));
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
                        "output": 10.0
                    },
                    "limit": {},
                    "modalities": {}
                }
            }
        }
    });
    let app = Router::new()
        .route(
            "/api.json",
            get(
                move |headers: HeaderMap, State(captured): State<Arc<Mutex<Option<String>>>>| {
                    let body = body.clone();
                    async move {
                        *captured.lock().expect("captured lock") = headers
                            .get(IF_NONE_MATCH)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        (
                            [(ETAG, HeaderValue::from_static("\"new-etag\""))],
                            axum::Json(body),
                        )
                    }
                },
            ),
        )
        .with_state(state.clone());
    let host = start_server(app).await;

    let catalog = empty_catalog(repo, format!("{host}/api.json"));
    catalog
        .refresh_now_and_sync()
        .await
        .expect("forced refresh");

    assert_eq!(state.lock().expect("captured lock").as_deref(), None);
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
        ..Default::default()
    });
    let failing_catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
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
            assert_eq!(pricing.provenance.source, REMOTE_SOURCE);
        }
        other => panic!("unexpected cached resolution: {other:?}"),
    }

    let vendored_catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        "http://127.0.0.1:9/api.json".to_string(),
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
            assert_eq!(pricing.provenance.source, VENDORED_SOURCE);
        }
        other => panic!("unexpected vendored resolution: {other:?}"),
    }
}
