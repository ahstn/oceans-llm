use super::*;

#[test]
fn parses_openai_and_openrouter_batch_provider_config() {
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
    batch:
      dialect: open_ai
  - id: openrouter-prod
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
    batch:
      dialect: open_router
      base_url: https://openrouter.ai/api/beta
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let providers = config
        .openai_compatible_provider_configs()
        .expect("runtime provider configs");
    assert_eq!(providers[0].batch.dialect, OpenAiBatchDialect::OpenAi);
    assert_eq!(providers[0].batch.base_url, None);
    assert_eq!(providers[1].batch.dialect, OpenAiBatchDialect::OpenRouter);
    assert_eq!(
        providers[1].batch.base_url.as_deref(),
        Some("https://openrouter.ai/api/beta")
    );
}

#[test]
fn rejects_blank_openai_batch_base_url() {
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
    batch:
      dialect: open_ai
      base_url: " "
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(format!("{error:#}").contains("batch.base_url cannot be empty"));
}

#[test]
fn rejects_invalid_openai_batch_base_url() {
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
    batch:
      dialect: open_ai
      base_url: not-a-url
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(format!("{error:#}").contains("batch.base_url is invalid"));
}

#[test]
fn rejects_openrouter_batch_without_base_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
providers:
  - id: openrouter-prod
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
    batch:
      dialect: open_router
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(format!("{error:#}").contains("OpenRouter batch mode requires batch.base_url"));
}

#[test]
fn parses_vertex_batch_provider_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: vertex-prod
    type: gcp_vertex
    project_id: vertex-project
    location: europe-west4
    auth:
      mode: adc
    batch:
      bigquery_project_id: billing-project
      dataset: batch_jobs_eu
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let providers = config
        .vertex_provider_configs()
        .expect("runtime provider configs");
    let batch = providers[0].batch.as_ref().expect("batch config");
    assert_eq!(batch.bigquery_project_id, "billing-project");
    assert_eq!(batch.dataset, "batch_jobs_eu");
}

#[test]
fn rejects_blank_vertex_batch_dataset() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
providers:
  - id: vertex-prod
    type: gcp_vertex
    project_id: vertex-project
    location: europe-west4
    auth:
      mode: adc
    batch:
      dataset: " "
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(format!("{error:#}").contains("batch.dataset cannot be empty"));
}
