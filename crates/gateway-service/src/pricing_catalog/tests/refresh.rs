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

#[test]
fn catalog_generation_advances_when_refreshes_share_a_wall_clock_second() {
    let current = fallback_snapshot();
    let same_second = current.metadata.fetched_at;

    let next = next_catalog_generation_at(Some(current.metadata.fetched_at), same_second);

    assert_eq!(
        next,
        current.metadata.fetched_at + time::Duration::seconds(1)
    );
}

#[tokio::test]
async fn concurrent_refreshes_with_different_documents_converge() {
    let catalog_body = |input_cost: f64| {
        json!({
            "openai": {
                "name": "OpenAI",
                "models": {
                    "gpt-5": {
                        "id": "gpt-5",
                        "name": "GPT-5",
                        "release_date": "2025-01-01",
                        "last_updated": "2026-07-10",
                        "cost": {"input": input_cost, "output": 10.0},
                        "limit": {"context": 128000, "output": 32000},
                        "modalities": {"input": ["text"], "output": ["text"]}
                    }
                }
            }
        })
        .to_string()
    };
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let bodies = Arc::new(Mutex::new(vec![catalog_body(2.0), catalog_body(3.0)]));
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

    let (first, second) = tokio::join!(
        catalog.refresh_now_and_sync(),
        catalog.refresh_now_and_sync()
    );

    first.expect("first refresh");
    second.expect("second refresh");
    let latest = catalog
        .load_snapshot_from_store_or_fallback()
        .await
        .expect("latest snapshot");
    let active = repo
        .list_active_model_pricing()
        .await
        .expect("active pricing");
    assert!(snapshot_is_already_synced(&active, &latest));
    let latest_model = latest
        .document
        .providers
        .get("openai")
        .and_then(|provider| provider.models.get("gpt-5"))
        .expect("latest GPT-5 pricing");
    let expected = build_model_pricing_record(&latest.metadata, "openai", "gpt-5", latest_model)
        .expect("build expected GPT-5 pricing");
    let active_gpt_5 = active
        .iter()
        .find(|row| row.pricing_provider_id == "openai" && row.pricing_model_id == "gpt-5")
        .expect("active GPT-5 pricing");
    assert_eq!(
        active_gpt_5.input_cost_per_million_tokens,
        expected.input_cost_per_million_tokens
    );
}

#[tokio::test]
async fn concurrent_304_does_not_block_a_200_refresh() {
    let repo = Arc::new(InMemoryRepo {
        cache: Arc::new(Mutex::new(Some(PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: REMOTE_SOURCE.to_string(),
            etag: Some("\"old-etag\"".to_string()),
            fetched_at: OffsetDateTime::from_unix_timestamp(1).expect("timestamp"),
            snapshot_json: to_string_pretty(&fallback_snapshot().document).expect("json"),
        }))),
        ..Default::default()
    });
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

    let (conditional, forced) =
        tokio::join!(catalog.refresh_if_stale(), catalog.refresh_now_and_sync());

    conditional.expect("conditional refresh");
    forced.expect("forced refresh");
    let cache = repo
        .get_pricing_catalog_cache(PRICING_CATALOG_CACHE_KEY)
        .await
        .expect("load cache")
        .expect("cache row");
    assert_eq!(cache.etag.as_deref(), Some("\"new-etag\""));
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
    assert_eq!(after, before);
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
async fn refresh_repairs_an_invalid_cached_snapshot() {
    let corrupt_fetched_at = OffsetDateTime::from_unix_timestamp(1).expect("timestamp");
    let repo = Arc::new(InMemoryRepo {
        cache: Arc::new(Mutex::new(Some(PricingCatalogCacheRecord {
            catalog_key: PRICING_CATALOG_CACHE_KEY.to_string(),
            source: REMOTE_SOURCE.to_string(),
            etag: Some("\"corrupt-etag\"".to_string()),
            fetched_at: corrupt_fetched_at,
            snapshot_json: "not valid json".to_string(),
        }))),
        ..Default::default()
    });
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

    catalog
        .refresh_if_stale()
        .await
        .expect("repair corrupt cache row");

    let repaired = repo
        .get_pricing_catalog_cache(PRICING_CATALOG_CACHE_KEY)
        .await
        .expect("load repaired cache")
        .expect("repaired cache row");
    assert!(repaired.fetched_at > corrupt_fetched_at);
    serde_json::from_str::<PricingCatalogDocument>(&repaired.snapshot_json)
        .expect("repaired snapshot JSON");
}

#[tokio::test]
async fn refresh_rejects_an_unexplained_cache_write_miss() {
    let repo = Arc::new(InMemoryRepo {
        cache_write_rejections_remaining: Arc::new(AtomicUsize::new(1)),
        ..Default::default()
    });
    let app = Router::new().route(
        "/api.json",
        get(|| async { axum::Json(minimal_catalog_body()) }),
    );
    let host = start_server(app).await;
    let catalog = empty_catalog(repo, format!("{host}/api.json"));

    let result = catalog.refresh_now_and_sync().await;

    assert!(matches!(
        result,
        Err(GatewayError::Store(StoreError::PricingSyncConflict))
    ));
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
