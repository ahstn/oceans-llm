use super::support::*;

#[tokio::test]
async fn request_time_resolution_only_queries_effective_pricing() {
    let repo = Arc::new(InMemoryRepo::default());
    let catalog = empty_catalog(repo.clone(), "http://127.0.0.1:9/api.json".to_string());
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing rows");
    reset_pricing_repo_counters(repo.as_ref());

    let resolution = catalog
        .resolve_for_provider_connection(
            &openai_provider("openai"),
            &route("openai-prod", "gpt-5"),
            test_time(),
        )
        .await
        .expect("resolve pricing");

    assert!(matches!(resolution, PricingResolution::Exact { .. }));
    assert_eq!(repo.cache_reads.load(Ordering::Relaxed), 0);
    assert_eq!(repo.active_pricing_reads.load(Ordering::Relaxed), 0);
    assert_eq!(repo.pricing_sync_applications.load(Ordering::Relaxed), 0);
    assert_eq!(repo.pricing_resolutions.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn gcp_vertex_maps_supported_publishers() {
    let catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        "http://127.0.0.1:9/api.json".to_string(),
    );
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

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
            &route("vertex-prod", "anthropic/claude-sonnet-4-6"),
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
            assert_eq!(pricing.model_id, "claude-sonnet-4-6@default");
        }
        other => panic!("unexpected anthropic resolution: {other:?}"),
    }
}

#[tokio::test]
async fn gcp_vertex_embedding_model_resolves_exact_google_vertex_pricing() {
    let catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        "http://127.0.0.1:9/api.json".to_string(),
    );
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

    let resolved = catalog
        .resolve_for_provider_connection(
            &vertex_provider("global"),
            &route("vertex-prod", "google/gemini-embedding-001"),
            test_time(),
        )
        .await
        .expect("resolve vertex embedding pricing");

    match resolved {
        PricingResolution::Exact { pricing } => {
            assert_eq!(pricing.pricing_provider_id, "google-vertex");
            assert_eq!(pricing.model_id, "gemini-embedding-001");
            assert_eq!(
                pricing.input_cost_per_million_tokens,
                Some(Money4::from_decimal_str("0.1500").expect("money"))
            );
            assert_eq!(pricing.output_cost_per_million_tokens, None);
        }
        other => panic!("unexpected embedding pricing resolution: {other:?}"),
    }
}

#[tokio::test]
async fn aws_bedrock_maps_supported_model_ids() {
    let catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        "http://127.0.0.1:9/api.json".to_string(),
    );
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

    let claude = catalog
        .resolve_for_provider_connection(
            &bedrock_provider(),
            &route("bedrock-prod", "us.anthropic.claude-sonnet-4-6-v1:0"),
            test_time(),
        )
        .await
        .expect("resolve claude");
    let gpt_oss = catalog
        .resolve_for_provider_connection(
            &bedrock_provider(),
            &route("bedrock-prod", "gpt-oss-120b"),
            test_time(),
        )
        .await
        .expect("resolve gpt oss");

    match claude {
        PricingResolution::Exact { pricing } => {
            assert_eq!(pricing.pricing_provider_id, "amazon-bedrock");
            assert_eq!(pricing.model_id, "us.anthropic.claude-sonnet-4-6");
            assert_eq!(
                pricing.input_cost_per_million_tokens,
                Some(Money4::from_decimal_str("3.0000").expect("money"))
            );
        }
        other => panic!("unexpected claude resolution: {other:?}"),
    }
    match gpt_oss {
        PricingResolution::Exact { pricing } => {
            assert_eq!(pricing.pricing_provider_id, "amazon-bedrock");
            assert_eq!(pricing.model_id, "openai.gpt-oss-120b-1:0");
        }
        other => panic!("unexpected gpt oss resolution: {other:?}"),
    }
}

#[test]
fn cloud_run_openai_compat_routes_to_configured_pricing_provider() {
    let target = pricing_target_for_route(
        &cloud_run_provider(Some("google-vertex")),
        &route("gemma-cloud-run", "gemini-2.5-flash"),
    );

    match target {
        PricingTarget::Exact {
            pricing_provider_id,
            model_id,
        } => {
            assert_eq!(pricing_provider_id, "google-vertex");
            assert_eq!(model_id, "gemini-2.5-flash");
        }
        other => panic!("unexpected pricing target: {other:?}"),
    }
}

#[test]
fn openai_compat_can_route_to_openrouter_pricing_provider() {
    let target = pricing_target_for_route(
        &openai_provider("openrouter"),
        &route("openrouter", "deepseek/deepseek-v4-pro"),
    );

    match target {
        PricingTarget::Exact {
            pricing_provider_id,
            model_id,
        } => {
            assert_eq!(pricing_provider_id, "openrouter");
            assert_eq!(model_id, "deepseek/deepseek-v4-pro");
        }
        other => panic!("unexpected pricing target: {other:?}"),
    }
}

#[test]
fn cloud_run_openai_compat_without_pricing_provider_is_unpriced() {
    let target = pricing_target_for_route(
        &cloud_run_provider(None),
        &route("gemma-cloud-run", "gemini-2.5-flash"),
    );

    match target {
        PricingTarget::Unpriced(PricingUnpricedReason::ProviderPricingSourceMissing) => {}
        other => panic!("unexpected pricing target: {other:?}"),
    }
}

#[test]
fn cloud_run_openai_compat_with_unsupported_pricing_provider_is_unpriced() {
    let target = pricing_target_for_route(
        &cloud_run_provider(Some("local-gemma")),
        &route("gemma-cloud-run", "gemini-2.5-flash"),
    );

    match target {
        PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedPricingProviderId(
            provider_id,
        )) => assert_eq!(provider_id, "local-gemma"),
        other => panic!("unexpected pricing target: {other:?}"),
    }
}

#[test]
fn bedrock_default_version_normalization_is_conservative() {
    assert_eq!(
        normalize_bedrock_pricing_model_id("us.anthropic.claude-sonnet-4-6-v1:0"),
        "us.anthropic.claude-sonnet-4-6"
    );
    assert_eq!(
        normalize_bedrock_pricing_model_id("us.anthropic.claude-sonnet-4-6-v2:0"),
        "us.anthropic.claude-sonnet-4-6-v2:0"
    );
}

#[test]
fn vertex_anthropic_default_version_normalization_tracks_default_catalog_ids() {
    assert_eq!(
        normalize_vertex_pricing_model_id("google-vertex-anthropic", "claude-sonnet-4-6"),
        "claude-sonnet-4-6@default"
    );
    assert_eq!(
        normalize_vertex_pricing_model_id("google-vertex-anthropic", "claude-opus-4-8"),
        "claude-opus-4-8@default"
    );
    assert_eq!(
        normalize_vertex_pricing_model_id("google-vertex-anthropic", "claude-sonnet-5"),
        "claude-sonnet-5@default"
    );
    assert_eq!(
        normalize_vertex_pricing_model_id("google-vertex-anthropic", "claude-sonnet-4-5"),
        "claude-sonnet-4-5"
    );
    assert_eq!(
        normalize_vertex_pricing_model_id("google-vertex-anthropic", "claude-opus-4-8@default"),
        "claude-opus-4-8@default"
    );
    assert_eq!(
        normalize_vertex_pricing_model_id("google-vertex", "claude-opus-4-8"),
        "claude-opus-4-8"
    );
}

#[test]
fn models_dev_money_normalization_rounds_extra_precision() {
    let cost = Number::from_f64(0.00875).expect("number");

    assert_eq!(
        normalize_models_dev_money(&cost).expect("normalized cost"),
        "0.0088"
    );
}

#[tokio::test]
async fn exact_model_lookup_succeeds_and_fails_closed() {
    let catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        "http://127.0.0.1:9/api.json".to_string(),
    );
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

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
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

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
async fn vendored_snapshot_prices_openrouter_routes() {
    let catalog = empty_catalog(
        Arc::new(InMemoryRepo::default()),
        "http://127.0.0.1:9/api.json".to_string(),
    );
    catalog
        .refresh_if_stale_and_sync()
        .await
        .expect("initialize pricing catalog");

    let resolved = catalog
        .resolve_for_provider_connection(
            &openai_provider("openrouter"),
            &route("openrouter", "deepseek/deepseek-v4-flash"),
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("resolve openrouter pricing");

    match resolved {
        PricingResolution::Exact { pricing } => {
            assert_eq!(pricing.pricing_provider_id, "openrouter");
            assert_eq!(pricing.model_id, "deepseek/deepseek-v4-flash");
            assert_eq!(
                pricing.input_cost_per_million_tokens,
                Some(Money4::from_decimal_str("0.0900").expect("money"))
            );
        }
        other => panic!("unexpected openrouter pricing resolution: {other:?}"),
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
            reason: PricingUnpricedReason::UnsupportedBillingModifier("service_tier".to_string(),)
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
async fn resolution_uses_persisted_pricing_row_for_occurrence_time() {
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
