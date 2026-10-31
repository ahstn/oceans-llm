//! Refreshing and reconciling the three catalog sources together.

use super::support::*;

/// Request counts per source, in [`CatalogSourceId::ALL`] order.
type SourceHits = Arc<[AtomicUsize; 3]>;

fn models_dev_body() -> Value {
    json!({
        "openai": {
            "name": "OpenAI",
            "models": {
                "gpt-5": {
                    "id": "gpt-5",
                    "name": "GPT-5",
                    "release_date": "2025-08-07",
                    "last_updated": "2025-08-07",
                    "cost": {"input": 1.25, "output": 10.0},
                    "limit": {"context": 400000, "output": 128000},
                    "modalities": {"input": ["text"], "output": ["text"]}
                }
            }
        },
        "openrouter": {
            "name": "OpenRouter",
            "models": {
                "deepseek/deepseek-v4-flash": {
                    "id": "deepseek/deepseek-v4-flash",
                    "name": "DeepSeek V4 Flash",
                    "release_date": "2026-07-01",
                    "last_updated": "2026-07-01",
                    "cost": {"input": 0.5, "output": 1.0},
                    "limit": {"context": 128000, "output": 8192},
                    "modalities": {"input": ["text"], "output": ["text"]}
                }
            }
        }
    })
}

fn openrouter_body(prompt: &str, completion: &str) -> Value {
    json!({"data": [{
        "id": "deepseek/deepseek-v4-flash",
        "name": "DeepSeek: DeepSeek V4 Flash",
        "context_length": 128000,
        "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
        "pricing": {"prompt": prompt, "completion": completion},
        "top_provider": {"max_completion_tokens": 8192}
    }]})
}

fn litellm_body() -> Value {
    json!({
        "amazon.nova-lite-v1:0": {
            "litellm_provider": "bedrock_converse",
            "mode": "chat",
            "input_cost_per_token": 6e-08,
            "output_cost_per_token": 2.4e-07,
            "max_input_tokens": 300000,
            "max_output_tokens": 10000
        }
    })
}

fn project(source: CatalogSourceId, body: &Value) -> Providers {
    (source.descriptor().project)(body.to_string().as_bytes()).expect("project source body")
}

/// Serve each source's body at its [`SOURCE_PATHS`] path, or a 500 for `None`. Returns
/// the host, the request counts and the number of conditional requests.
async fn serve_sources(bodies: [Option<Value>; 3]) -> (String, SourceHits, Arc<AtomicUsize>) {
    let hits: SourceHits = Arc::new(Default::default());
    let conditional = Arc::new(AtomicUsize::new(0));
    let mut app = Router::new();
    for (index, (path, body)) in SOURCE_PATHS.into_iter().zip(bodies).enumerate() {
        let (hits, conditional) = (hits.clone(), conditional.clone());
        app = app.route(
            path,
            get(move |headers: HeaderMap| async move {
                hits[index].fetch_add(1, Ordering::Relaxed);
                if headers.contains_key(IF_NONE_MATCH) {
                    conditional.fetch_add(1, Ordering::Relaxed);
                }
                match body {
                    Some(body) => axum::Json(body).into_response(),
                    None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }),
        );
    }
    (start_server(app).await, hits, conditional)
}

fn hit_counts(hits: &SourceHits) -> [usize; 3] {
    hits.each_ref().map(|hits| hits.load(Ordering::Relaxed))
}

#[tokio::test]
async fn a_failing_source_keeps_its_snapshot_while_the_others_update_in_one_reconciliation() {
    let cached_openrouter = project(
        CatalogSourceId::OpenRouter,
        &openrouter_body("0.00000009", "0.00000018"),
    );
    let repo = Arc::new(InMemoryRepo::with_cache_rows([cache_row(
        CatalogSourceId::OpenRouter,
        None,
        test_time(),
        &cached_openrouter,
    )]));
    let (host, hits, _) =
        serve_sources([Some(models_dev_body()), None, Some(litellm_body())]).await;
    let catalog = catalog_with(repo.clone(), Duration::ZERO, source_urls(&host));
    let progress = AuditProgress::default();

    let sources = catalog
        .refresh_now_and_sync_tracked(&progress)
        .await
        .expect("a failed source does not fail the refresh");

    assert_eq!(
        sources
            .iter()
            .map(|refresh| (refresh.source, refresh.outcome))
            .collect::<Vec<_>>(),
        vec![
            (
                CatalogSourceId::ModelsDev,
                PricingCatalogRefreshOutcome::Updated
            ),
            (
                CatalogSourceId::OpenRouter,
                PricingCatalogRefreshOutcome::Failed(PricingCatalogRefreshError::UnexpectedStatus)
            ),
            (
                CatalogSourceId::LiteLlm,
                PricingCatalogRefreshOutcome::Updated
            ),
        ]
    );
    assert_eq!(
        sources[1].fetched_at,
        test_time(),
        "kept its cached snapshot"
    );
    assert!(progress.has_landed());
    assert_eq!(hit_counts(&hits), [1, 1, 1]);
    assert_eq!(repo.sync_applications(), 1, "one reconciliation");

    let generation = sources
        .iter()
        .map(|refresh| refresh.fetched_at)
        .max()
        .expect("sources");
    assert!(
        repo.pricing_rows
            .lock()
            .expect("pricing rows")
            .iter()
            .all(|row| row.effective_start_at == generation),
        "every row takes effect at the merged generation"
    );

    // OpenRouter's own price wins for its provider, even from its kept snapshot.
    let openrouter = repo.active_row("openrouter", "deepseek/deepseek-v4-flash");
    assert_eq!(
        openrouter.input_cost_per_million_tokens,
        Some(Money4::from_scaled(900))
    );
    assert_eq!(openrouter.provenance.source, "openrouter_api");
    assert_eq!(openrouter.provenance.etag, None);
    assert_eq!(openrouter.provenance.fetched_at, test_time());

    let openai = repo.active_row("openai", "gpt-5");
    assert_eq!(openai.provenance.source, MODELS_DEV_REMOTE_SOURCE);
    assert_eq!(openai.provenance.fetched_at, sources[0].fetched_at);

    let bedrock = repo.active_row("amazon-bedrock", "amazon.nova-lite-v1:0");
    assert_eq!(
        bedrock.input_cost_per_million_tokens,
        Some(Money4::from_scaled(600))
    );
    assert_eq!(bedrock.provenance.source, "litellm_github");
    assert_eq!(bedrock.provenance.fetched_at, sources[2].fetched_at);
}

#[tokio::test]
async fn unchanged_sources_skip_reconciliation_and_keep_the_shared_snapshot() {
    let (host, hits, _) = serve_sources([
        Some(models_dev_body()),
        Some(openrouter_body("0.00000009", "0.00000018")),
        Some(litellm_body()),
    ])
    .await;
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = catalog_with(repo.clone(), Duration::ZERO, source_urls(&host));
    catalog
        .refresh_now_and_sync_tracked(&AuditProgress::default())
        .await
        .expect("initial refresh");
    let before = catalog.metadata_snapshot().await.expect("snapshot");
    let applications = repo.sync_applications();
    let progress = AuditProgress::default();

    let sources = catalog
        .refresh_now_and_sync_tracked(&progress)
        .await
        .expect("unchanged refresh");

    assert!(
        sources
            .iter()
            .all(|refresh| refresh.outcome == PricingCatalogRefreshOutcome::Unchanged),
        "{sources:?}"
    );
    assert_eq!(hit_counts(&hits), [2, 2, 2]);
    assert!(!progress.has_landed());
    assert_eq!(repo.sync_applications(), applications);
    let after = catalog.metadata_snapshot().await.expect("snapshot");
    assert!(Arc::ptr_eq(&before, &after));
}

#[tokio::test]
async fn forced_refresh_requests_every_source_without_cached_etags() {
    let repo = Arc::new(InMemoryRepo::with_cache_rows([
        cache_row(
            CatalogSourceId::ModelsDev,
            Some("\"models-dev-etag\""),
            test_time(),
            &fallback_snapshot().providers,
        ),
        cache_row(
            CatalogSourceId::LiteLlm,
            Some("\"litellm-etag\""),
            test_time(),
            &Providers::new(),
        ),
    ]));
    let (host, hits, conditional) = serve_sources([
        Some(models_dev_body()),
        Some(openrouter_body("0.00000009", "0.00000018")),
        Some(litellm_body()),
    ])
    .await;
    let catalog = catalog_with(repo, Duration::from_secs(3600), source_urls(&host));

    let sources = catalog
        .refresh_now_and_sync_tracked(&AuditProgress::default())
        .await
        .expect("forced refresh");

    assert_eq!(
        sources
            .iter()
            .map(|refresh| refresh.source)
            .collect::<Vec<_>>(),
        CatalogSourceId::ALL.to_vec()
    );
    assert_eq!(hit_counts(&hits), [1, 1, 1]);
    assert_eq!(conditional.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn refresh_if_stale_requests_only_stale_sources() {
    let repo = Arc::new(InMemoryRepo::with_cache_rows([
        cache_row(
            CatalogSourceId::ModelsDev,
            None,
            OffsetDateTime::now_utc(),
            &fallback_snapshot().providers,
        ),
        cache_row(
            CatalogSourceId::LiteLlm,
            None,
            test_time(),
            &Providers::new(),
        ),
    ]));
    let (host, hits, _) = serve_sources([
        Some(models_dev_body()),
        Some(openrouter_body("0.00000009", "0.00000018")),
        Some(litellm_body()),
    ])
    .await;
    let catalog = catalog_with(repo.clone(), Duration::from_secs(3600), source_urls(&host));

    let outcomes = catalog.refresh_sources(RefreshMode::IfStale).await;

    assert_eq!(
        outcomes,
        vec![
            (
                CatalogSourceId::OpenRouter,
                PricingCatalogRefreshOutcome::Updated
            ),
            (
                CatalogSourceId::LiteLlm,
                PricingCatalogRefreshOutcome::Updated
            ),
        ]
    );
    assert_eq!(hit_counts(&hits), [0, 1, 1]);
    assert!(repo.cache_row(OPENROUTER_CACHE_KEY).is_some());
    assert!(
        repo.cache_row(LITELLM_CACHE_KEY)
            .expect("litellm row")
            .fetched_at
            > test_time()
    );
}
