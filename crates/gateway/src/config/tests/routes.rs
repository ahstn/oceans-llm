use super::*;

#[test]
fn parses_route_openai_compatibility_config_into_seed_models() {
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
          openai_compat:
            supports_store: false
            max_tokens_field: max_tokens
            developer_role: system
            reasoning_effort: reasoning_object
            supports_stream_usage: true
            empty_tools: preserve_with_tool_history
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let profile = models[0].routes[0]
        .compatibility
        .openai_compat
        .as_ref()
        .expect("openai compat profile");

    assert!(!profile.supports_store);
    assert_eq!(
        profile.max_tokens_field,
        OpenAiCompatMaxTokensField::MaxTokens
    );
    assert_eq!(profile.developer_role, OpenAiCompatDeveloperRole::System);
    assert_eq!(
        profile.reasoning_effort,
        OpenAiCompatReasoningEffort::ReasoningObject
    );
    assert!(profile.supports_stream_usage);
    assert_eq!(
        profile.empty_tools,
        OpenAiCompatEmptyTools::PreserveWithToolHistory
    );
}

#[test]
fn rejects_runtime_anthropic_invoke_with_stream_capability() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
models:
  - id: claude
    routes:
      - provider: bedrock
        upstream_model: anthropic.claude-sonnet-4-5-20250929-v1:0
        capabilities:
          responses: false
          stream: true
          json_schema: false
        compatibility:
          aws_bedrock:
            api_style: runtime_anthropic_invoke
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot enable stream capability"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_invalid_vertex_upstream_model_route_format() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: vertex
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: adc
models:
  - id: fast
    routes:
      - provider: vertex
        upstream_model: gemini-2.0-flash
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}")
            .contains("gcp_vertex routes require upstream_model in <publisher>/<model_id> format"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn accepts_openai_compat_with_supported_pricing_provider() {
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
"#,
    );

    GatewayConfig::from_path(&config_path).expect("config should parse");
}

#[test]
fn rejects_openai_compat_without_pricing_provider_id() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("pricing_provider_id cannot be empty"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_openai_compat_with_unsupported_pricing_provider_id() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: azure
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("pricing_provider_id `azure` is not supported"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn parses_route_capability_metadata_into_seed_models() {
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
        upstream_model: gpt-5
        capabilities:
          stream: false
          tools: false
          vision: false
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let seeded = config.seed_models().expect("seed models");

    let route = &seeded[0].routes[0];
    assert_eq!(
        route.capabilities,
        gateway_core::PartialCapabilities {
            stream: Some(false),
            tools: Some(false),
            vision: Some(false),
            ..Default::default()
        },
        "only explicit settings are seeded"
    );
    let resolved = route.capabilities.resolve();
    assert!(resolved.chat_completions);
    assert!(resolved.embeddings);
    assert!(!resolved.images, "images is opt-in");
    assert!(resolved.json_schema);
    assert!(resolved.developer_role);
}

#[test]
fn parses_route_context_and_exact_pricing_override() {
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
  - id: contracted
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
        context_window_tokens: 128000
        pricing_override:
          input_usd_per_million_tokens: "0"
          output_usd_per_million_tokens: "1.2345"
          cache_write_usd_per_million_tokens: "0.5000"
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let seeded = config.seed_models().expect("seed models");
    let route = &seeded[0].routes[0];
    let pricing = route.pricing_override.as_ref().expect("pricing override");

    assert_eq!(route.context_window_tokens, Some(128_000));
    assert_eq!(pricing.input_cost_per_million_tokens, Money4::ZERO);
    assert_eq!(
        pricing.output_cost_per_million_tokens,
        Money4::from_scaled(12_345)
    );
    assert_eq!(pricing.cache_read_cost_per_million_tokens, None);
    assert_eq!(
        pricing.cache_write_cost_per_million_tokens,
        Some(Money4::from_scaled(5_000))
    );
    assert_eq!(pricing.image_output_cost_per_million_tokens, None);
}

#[test]
fn parses_route_image_output_pricing_override() {
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
  - id: image-model
    routes:
      - provider: openrouter
        upstream_model: google/gemini-3.1-flash-lite-image
        pricing_override:
          input_usd_per_million_tokens: "0.2500"
          output_usd_per_million_tokens: "1.5000"
          image_output_usd_per_million_tokens: "30"
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let seeded = config.seed_models().expect("seed models");
    let pricing = seeded[0].routes[0]
        .pricing_override
        .as_ref()
        .expect("pricing override");

    assert_eq!(
        pricing.output_cost_per_million_tokens,
        Money4::from_scaled(15_000)
    );
    assert_eq!(
        pricing.image_output_cost_per_million_tokens,
        Some(Money4::from_scaled(300_000))
    );
}

#[test]
fn rejects_negative_or_numeric_image_output_pricing_override() {
    for (image_rate, expected_error) in [
        (
            r#""-30.0000""#,
            "image_output_usd_per_million_tokens cannot be negative",
        ),
        ("30", "expected a quoted decimal string"),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!(
                r#"
models:
  - id: contracted
    routes:
      - provider: private
        upstream_model: upstream
        pricing_override:
          input_usd_per_million_tokens: "0.2500"
          output_usd_per_million_tokens: "1.5000"
          image_output_usd_per_million_tokens: {image_rate}
"#
            ),
        );

        let error = GatewayConfig::from_path(&config_path)
            .expect_err("invalid image output rate must be rejected");
        assert!(
            format!("{error:#}").contains(expected_error),
            "unexpected error for `{image_rate}`: {error:#}"
        );
    }
}

#[test]
fn rejects_numeric_route_pricing_values() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
models:
  - id: contracted
    routes:
      - provider: private
        upstream_model: upstream
        pricing_override:
          input_usd_per_million_tokens: 1.25
          output_usd_per_million_tokens: "5.0000"
"#,
    );

    let error = GatewayConfig::from_path(&config_path)
        .expect_err("floating-point YAML pricing must be rejected");
    assert!(
        format!("{error:#}").contains("expected a quoted decimal string"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn rejects_non_positive_route_context_override() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
models:
  - id: contracted
    routes:
      - provider: private
        upstream_model: upstream
        context_window_tokens: 0
"#,
    );

    let error =
        GatewayConfig::from_path(&config_path).expect_err("non-positive context must be rejected");
    assert!(
        format!("{error:#}").contains("context_window_tokens must be positive"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn rejects_negative_route_pricing_override() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
models:
  - id: contracted
    routes:
      - provider: private
        upstream_model: upstream
        pricing_override:
          input_usd_per_million_tokens: "-1.0000"
          output_usd_per_million_tokens: "5.0000"
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("negative rate must be rejected");
    assert!(
        format!("{error:#}").contains("input_usd_per_million_tokens cannot be negative"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn rejects_malformed_and_overflowing_route_pricing_overrides() {
    for (input_rate, expected_error) in [
        ("1.23456", "invalid fractional part"),
        ("not-a-rate", "invalid integer part"),
        ("922337203685478", "overflowed"),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!(
                r#"
models:
  - id: contracted
    routes:
      - provider: private
        upstream_model: upstream
        pricing_override:
          input_usd_per_million_tokens: "{input_rate}"
          output_usd_per_million_tokens: "5.0000"
"#
            ),
        );

        let error = GatewayConfig::from_path(&config_path)
            .expect_err("invalid route pricing must be rejected");
        assert!(
            format!("{error:#}").contains(expected_error),
            "unexpected error for `{input_rate}`: {error:#}"
        );
    }
}

#[test]
fn rejects_routes_for_unknown_providers() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
models:
  - id: fast
    routes:
      - provider: missing
        upstream_model: upstream
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("unknown provider must fail");
    assert!(
        format!("{error:#}").contains("model `fast` route references unknown provider `missing`"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn rejects_misspelled_model_reasoning_limit() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast
    max_reasoning_efort: high
    routes:
      - provider: openai
        upstream_model: upstream
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("policy typo must fail");
    assert!(
        format!("{error:#}").contains("unknown field `max_reasoning_efort`"),
        "unexpected error: {error:#}"
    );
}

const OPENAI_DECISIONS_CAPABILITIES: &str = r#"
        capabilities:
          chat_completions: false
          responses: false
          stream: false
          embeddings: false
          decisions: true
          tools: false
          vision: false
          json_schema: false
          developer_role: false"#;

#[test]
fn parses_openai_decisions_route_with_input_only_pricing() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        &format!(
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: luna
    routes:
      - provider: openai-prod
        upstream_model: gpt-6-luna{OPENAI_DECISIONS_CAPABILITIES}
        compatibility:
          openai_compat:
            decisions_api: openai
        pricing_override:
          input_usd_per_million_tokens: "0.5000"
          output_usd_per_million_tokens: "0"
          cache_read_usd_per_million_tokens: "0.0500"
"#
        ),
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let route = &models[0].routes[0];
    assert_eq!(
        route
            .compatibility
            .openai_compat
            .as_ref()
            .and_then(|profile| profile.decisions_api),
        Some(OpenAiCompatDecisionsApi::OpenAi)
    );
    assert!(route.compatibility.selects_decisions_api());
    assert_eq!(route.capabilities.decisions, Some(true));
    assert!(route.pricing_override.is_some());
}

#[test]
fn infers_decisions_capabilities_for_openai_decisions_api() {
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
  - id: luna
    routes:
      - provider: openai-prod
        upstream_model: gpt-6-luna
        compatibility:
          openai_compat:
            decisions_api: openai
"#,
    );

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
}

#[test]
fn rejects_explicit_capabilities_that_contradict_openai_decisions_api() {
    for (capability, expected) in [
        (
            "decisions: false",
            "uses compatibility.openai_compat.decisions_api but disables capabilities.decisions",
        ),
        (
            "chat_completions: true",
            "enables capabilities.decisions and `chat_completions`",
        ),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!(
                r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: luna
    routes:
      - provider: openai-prod
        upstream_model: gpt-6-luna
        capabilities:
          {capability}
        compatibility:
          openai_compat:
            decisions_api: openai
"#
            ),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        assert!(
            format!("{error:#}").contains(expected),
            "unexpected error for {capability}: {error:#}"
        );
    }
}

#[test]
fn rejects_openai_decisions_api_on_non_openai_compat_providers() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        &format!(
            r#"
providers:
  - id: typesafe
    type: typesafe
    pricing_provider_id: openai
models:
  - id: jev
    routes:
      - provider: typesafe
        upstream_model: jev{OPENAI_DECISIONS_CAPABILITIES}
        compatibility:
          openai_compat:
            decisions_api: openai
"#
        ),
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("decisions_api but requires an openai_compat provider"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn parses_images_capability_on_openai_compatible_routes() {
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
  - id: cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://service.example.com/v1
    pricing_provider_id: google-vertex
    auth:
      mode: adc
models:
  - id: gpt-image
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-image-1
        capabilities:
          images: true
      - provider: cloud-run
        upstream_model: google/imagen
        capabilities:
          images: true
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let images = models[0]
        .routes
        .iter()
        .map(|route| (route.provider_key.as_str(), route.capabilities.images))
        .collect::<Vec<_>>();
    assert_eq!(
        images,
        [("openrouter", Some(true)), ("cloud-run", Some(true))]
    );
}

#[test]
fn rejects_images_capability_on_providers_without_the_images_api() {
    for (provider, upstream_model) in [
        (
            "  - id: drawer\n    type: anthropic_compat\n    base_url: https://opencode.ai/zen\n    pricing_provider_id: opencode\n    auth:\n      kind: x_api_key\n      token: literal.test_opencode_key\n",
            "claude-fable-5-1",
        ),
        (
            "  - id: drawer\n    type: gcp_vertex\n    project_id: test-proj\n    auth:\n      mode: adc\n",
            "google/imagen-4.0-generate-001",
        ),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!(
                "providers:\n{provider}models:\n  - id: image\n    routes:\n      - provider: drawer\n        upstream_model: {upstream_model}\n        capabilities:\n          images: true\n"
            ),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        assert!(
            format!("{error:#}").contains(
                "model `image` route for provider `drawer` enables capabilities.images but image generation requires an openai_compat or gcp_cloud_run_openai_compat provider"
            ),
            "unexpected error for {upstream_model}: {error:#}"
        );
    }
}

#[test]
fn rejects_images_capability_on_decisions_routes() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        &format!(
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: luna
    routes:
      - provider: openai-prod
        upstream_model: gpt-6-luna{OPENAI_DECISIONS_CAPABILITIES}
          images: true
        compatibility:
          openai_compat:
            decisions_api: openai
"#
        ),
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}").contains(
            "enables capabilities.decisions and `images`; decisions routes must disable all other capabilities"
        ),
        "unexpected error: {error:#}"
    );
}
