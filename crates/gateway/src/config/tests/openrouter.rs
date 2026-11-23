use super::*;

#[test]
fn parses_route_openrouter_policy_config_into_seed_models() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openrouter:
            provider:
              zdr: true
              only: [openai, anthropic]
              ignore: [deepinfra]
              order: [openai, anthropic]
              preferred_max_latency:
                p90: 2.5
              max_price:
                prompt: 1.0
                completion: 2.0
                request: 0.01
                image: 0.05
              sort:
                by: throughput
                partition: none
              data_collection: deny
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let routing = &models[0].routes[0]
        .compatibility
        .openrouter
        .as_ref()
        .expect("openrouter policy")
        .provider;

    assert_eq!(routing.zdr, Some(true));
    assert_eq!(routing.only, vec!["openai", "anthropic"]);
    assert_eq!(routing.ignore, vec!["deepinfra"]);
    assert_eq!(routing.order, vec!["openai", "anthropic"]);
    match routing
        .preferred_max_latency
        .as_ref()
        .expect("latency preference")
    {
        OpenRouterPercentilePreference::Percentiles(percentiles) => {
            assert_eq!(percentiles.p90, Some(2.5));
        }
        OpenRouterPercentilePreference::Number(value) => {
            panic!("expected percentile latency, got {value}");
        }
    }
    let max_price = routing.max_price.as_ref().expect("max price");
    assert_eq!(max_price.prompt, Some(1.0));
    assert_eq!(max_price.completion, Some(2.0));
    assert_eq!(max_price.request, Some(0.01));
    assert_eq!(max_price.image, Some(0.05));
    assert_eq!(
        routing.sort,
        Some(OpenRouterSort::Options(OpenRouterSortOptions {
            by: OpenRouterSortBy::Throughput,
            partition: Some(OpenRouterSortPartition::None),
        }))
    );
    assert_eq!(
        routing.data_collection,
        Some(OpenRouterDataCollection::Deny)
    );
}

fn openrouter_policy_config(provider_policy: &str) -> String {
    format!(
        r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openrouter:
            provider:
{provider_policy}
"#
    )
}

#[test]
fn accepts_sort_or_data_collection_as_the_only_openrouter_policy_field() {
    for (policy, expected_sort, expected_data_collection) in [
        (
            "              sort: price",
            Some(OpenRouterSort::By(OpenRouterSortBy::Price)),
            None,
        ),
        (
            "              sort:\n                by: latency",
            Some(OpenRouterSort::Options(OpenRouterSortOptions {
                by: OpenRouterSortBy::Latency,
                partition: None,
            })),
            None,
        ),
        (
            "              data_collection: allow",
            None,
            Some(OpenRouterDataCollection::Allow),
        ),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(&config_path, &openrouter_policy_config(policy));

        let config = GatewayConfig::from_path(&config_path).expect("config should parse");
        let models = config.seed_models().expect("seed models");
        let routing = &models[0].routes[0]
            .compatibility
            .openrouter
            .as_ref()
            .expect("openrouter policy")
            .provider;
        assert_eq!(routing.sort, expected_sort, "{policy}");
        assert_eq!(
            routing.data_collection, expected_data_collection,
            "{policy}"
        );
    }
}

#[test]
fn rejects_invalid_openrouter_sort_and_data_collection_values() {
    for (policy, expected) in [
        (
            "              sort: cheapest",
            "expected `price`, `throughput`, `latency`",
        ),
        (
            "              sort:\n                partition: model",
            "expected `price`, `throughput`, `latency`",
        ),
        (
            "              sort:\n                by: price\n                partition: provider",
            "expected `price`, `throughput`, `latency`",
        ),
        (
            "              data_collection: maybe",
            "unknown variant `maybe`",
        ),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(&config_path, &openrouter_policy_config(policy));

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(error_text.contains(expected), "{policy}: {error_text}");
    }
}

#[test]
fn rejects_openrouter_policy_on_non_openrouter_openai_compat_provider() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openai-prod
        upstream_model: gpt-4o-mini
        compatibility:
          openrouter:
            provider:
              zdr: true
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("provider base_url is not an OpenRouter endpoint"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_openrouter_policy_on_openrouter_lookalike_provider_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter-lookalike
    type: openai_compat
    base_url: https://openrouter.ai.evil.example/api/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openrouter-lookalike
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openrouter:
            provider:
              zdr: true
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("provider base_url is not an OpenRouter endpoint"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_openrouter_policy_on_non_https_provider_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter-insecure
    type: openai_compat
    base_url: http://openrouter.ai/api/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openrouter-insecure
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openrouter:
            provider:
              zdr: true
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("provider base_url is not an OpenRouter endpoint"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_unknown_openrouter_policy_fields() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openrouter:
            provider:
              zdr: true
              allow_fallbacks: false
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("unknown field `allow_fallbacks`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_openrouter_policy_with_raw_extra_body_provider() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        extra_body:
          provider:
            zdr: false
        compatibility:
          openrouter:
            provider:
              zdr: true
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text
            .contains("cannot set both compatibility.openrouter.provider and extra_body.provider"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn accepts_decisions_api_with_empty_provider_policy() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
models:
  - id: jev
    routes:
      - provider: openrouter
        upstream_model: typesafe/jev-1.13
        capabilities:
          chat_completions: false
          responses: false
          stream: false
          embeddings: false
          decisions: true
          tools: false
          vision: false
          json_schema: false
          developer_role: false
        compatibility:
          openrouter:
            api: decisions
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let compatibility = models[0].routes[0]
        .compatibility
        .openrouter
        .as_ref()
        .expect("openrouter compatibility");

    assert_eq!(
        compatibility.api,
        gateway_core::OpenRouterRouteApi::Decisions
    );
    assert!(compatibility.provider.is_empty());
    assert_eq!(models[0].routes[0].capabilities.decisions, Some(true));
}

#[test]
fn rejects_auxiliary_capabilities_on_decisions_routes() {
    for enabled_capability in ["tools", "vision", "json_schema", "developer_role"] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        let capability = |name| name == enabled_capability;
        write_config(
            &config_path,
            &format!(
                r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
models:
  - id: jev
    routes:
      - provider: openrouter
        upstream_model: typesafe/jev-1.13
        capabilities:
          chat_completions: false
          responses: false
          stream: false
          embeddings: false
          decisions: true
          tools: {}
          vision: {}
          json_schema: {}
          developer_role: {}
        compatibility:
          openrouter:
            api: decisions
"#,
                capability("tools"),
                capability("vision"),
                capability("json_schema"),
                capability("developer_role"),
            ),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains(&format!(
                "enables capabilities.decisions and `{enabled_capability}`"
            )),
            "unexpected error: {error_text}"
        );
    }
}

#[test]
fn infers_decisions_capabilities_for_openrouter_decisions_api() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    let yaml = r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
models:
  - id: jev
    routes:
      - provider: openrouter
        upstream_model: typesafe/jev-1.13
        compatibility:
          openrouter:
            api: decisions
"#;
    write_config(&config_path, yaml);
    let config = GatewayConfig::from_path(&config_path).expect("omitted capabilities are inferred");
    let models = config.seed_models().expect("seed models");
    let route = &models[0].routes[0];
    assert_eq!(
        route.capabilities,
        gateway_core::PartialCapabilities::default()
    );
    assert_eq!(
        gateway_core::infer_compatibility_capabilities(&route.compatibility, "openai_compat")
            .resolve(),
        gateway_core::ProviderCapabilities {
            decisions: true,
            ..gateway_core::ProviderCapabilities::none()
        }
    );

    write_config(
        &config_path,
        &format!("{yaml}        capabilities:\n          decisions: false\n"),
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains(
            "uses compatibility.openrouter.api `decisions` but disables capabilities.decisions"
        ),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_decisions_capability_on_non_decisions_routes() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
models:
  - id: jev
    routes:
      - provider: openrouter
        upstream_model: typesafe/jev-1.13
        capabilities:
          chat_completions: false
          responses: false
          stream: false
          embeddings: false
          decisions: true
          tools: false
          vision: false
          json_schema: false
          developer_role: false
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("decisions requires a typesafe provider"),
        "unexpected error: {error_text}"
    );
}
