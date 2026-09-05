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
