use super::support::*;

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
async fn mixed_newer_active_rows_are_reported_as_a_sync_conflict() {
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    let snapshot = fallback_snapshot();
    catalog
        .sync_model_pricing_snapshot(&snapshot)
        .await
        .expect("seed pricing rows");

    repo.pricing_rows
        .lock()
        .expect("pricing rows")
        .first_mut()
        .expect("active pricing row")
        .provenance
        .fetched_at += time::Duration::seconds(1);

    let error = catalog
        .sync_model_pricing_snapshot(&snapshot)
        .await
        .expect_err("mixed catalog generations must not report convergence");

    assert!(matches!(
        error,
        GatewayError::Store(StoreError::PricingSyncConflict)
    ));
}

#[tokio::test]
async fn mixed_current_and_older_active_rows_finish_reconciling() {
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    let snapshot = fallback_snapshot();
    catalog
        .sync_model_pricing_snapshot(&snapshot)
        .await
        .expect("seed pricing rows");

    repo.pricing_rows
        .lock()
        .expect("pricing rows")
        .first_mut()
        .expect("active pricing row")
        .provenance
        .fetched_at -= time::Duration::seconds(1);

    catalog
        .sync_model_pricing_snapshot(&snapshot)
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
            .all(|row| row.provenance.fetched_at == snapshot.metadata.fetched_at)
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
        .refresh_now_and_sync()
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
        source: REMOTE_SOURCE.to_string(),
        etag: Some("\"etag-refreshed\"".to_string()),
        fetched_at: test_time() + Duration::from_secs(3600),
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
        source: REMOTE_SOURCE.to_string(),
        etag: Some("\"etag-removed\"".to_string()),
        fetched_at: test_time() + Duration::from_secs(3600),
    };
    removed
        .document
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
