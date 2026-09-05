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

    GatewayConfig::from_path(&config_path).expect_err("config should fail");
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
    assert!(route.capabilities.chat_completions);
    assert!(!route.capabilities.stream);
    assert!(route.capabilities.embeddings);
    assert!(!route.capabilities.tools);
    assert!(!route.capabilities.vision);
    assert!(route.capabilities.json_schema);
    assert!(route.capabilities.developer_role);
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

    GatewayConfig::from_path(&config_path)
        .expect_err("floating-point YAML pricing must be rejected");
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
