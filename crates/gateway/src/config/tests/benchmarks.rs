use super::*;

#[test]
fn loads_server_api_key_and_explicit_model_binding() {
    let directory = tempdir().expect("config directory");
    let config_path = directory.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
benchmark_catalog:
  artificial_analysis:
    api_key: literal.test-key
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: scored-model
    artificial_analysis_model_id: 36f73aaf-d38a-4b56-a2b3-d04d17186910
    routes:
      - provider: openai
        upstream_model: upstream-model
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config");
    assert_eq!(
        config.artificial_analysis_api_key().expect("api key"),
        Some("test-key".to_string())
    );
    let bindings = config.model_benchmark_bindings();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].source, "artificial_analysis");
    assert_eq!(
        bindings[0].source_model_id,
        "36f73aaf-d38a-4b56-a2b3-d04d17186910"
    );
}

#[test]
fn rejects_non_uuid_artificial_analysis_model_id() {
    let directory = tempdir().expect("config directory");
    let config_path = directory.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: scored-model
    artificial_analysis_model_id: guessed-name
    routes:
      - provider: openai
        upstream_model: upstream-model
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}").contains("artificial_analysis_model_id must be a UUID"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn rejects_api_key_reference_that_resolves_empty() {
    let directory = tempdir().expect("config directory");
    let config_path = directory.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
benchmark_catalog:
  artificial_analysis:
    api_key: literal.
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config");
    let error = config
        .artificial_analysis_api_key()
        .expect_err("empty resolved key should fail");
    assert!(
        format!("{error:#}").contains("resolved to an empty value"),
        "unexpected error: {error:#}"
    );
}
