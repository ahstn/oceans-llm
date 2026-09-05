use super::*;

#[test]
fn accepts_alias_backed_model_config() {
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
  - id: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: fast
    alias_of: fast-v2
"#,
    );

    GatewayConfig::from_path(&config_path).expect("config should parse");
}

#[test]
fn parses_model_allowlist_normalizes_refs_and_preserves_omitted_policy() {
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
  - id: unrestricted
    routes:
      - provider: openai-prod
        upstream_model: gpt-4o-mini
  - id: restricted
    allowlist:
      users:
        - " Alice@Example.COM "
        - "alice@example.com"
        - "Zoe@Example.com"
      teams:
        - " platform "
        - research
        - platform
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let unrestricted = models
        .iter()
        .find(|model| model.model_key == "unrestricted")
        .expect("unrestricted model");
    let restricted = models
        .iter()
        .find(|model| model.model_key == "restricted")
        .expect("restricted model");

    assert_eq!(unrestricted.allowlist, None);
    let allowlist = restricted.allowlist.as_ref().expect("restricted allowlist");
    assert_eq!(
        allowlist.users,
        vec![
            "alice@example.com".to_string(),
            "zoe@example.com".to_string()
        ]
    );
    assert_eq!(
        allowlist.teams,
        vec!["platform".to_string(), "research".to_string()]
    );
}

#[test]
fn rejects_unknown_model_allowlist_keys() {
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
  - id: restricted
    allowlist:
      users:
        - alice@example.com
      team:
        - platform
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("unknown field `team`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_explicit_empty_model_allowlists() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    for allowlist_yaml in [
        "allowlist: {}",
        "allowlist:\n      users: []\n      teams: []",
    ] {
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
  - id: restricted
    {allowlist_yaml}
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#
            ),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text
                .contains("model `restricted` allowlist must include at least one user or team"),
            "unexpected error: {error_text}"
        );
    }
}

#[test]
fn rejects_model_with_alias_and_routes() {
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
    alias_of: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot define both alias_of and routes"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_model_without_alias_or_routes() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("must define either alias_of or at least one route"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_alias_to_unknown_model() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
    alias_of: missing
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("aliases unknown model `missing`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_self_alias() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
    alias_of: fast
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot alias itself"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_alias_cycles() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
    alias_of: fast-v2
  - id: fast-v2
    alias_of: fast
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("model alias cycle detected"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn loads_model_reasoning_effort_policy_into_seed_models() {
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
  - id: reasoning
    max_reasoning_effort: medium
    routes:
      - provider: openai
        upstream_model: gpt-5
        extra_body:
          reasoning_effort: low
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should load");
    assert_eq!(
        config.models[0].max_reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(
        config.seed_models().expect("seed models")[0].max_reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
}

#[test]
fn rejects_route_extra_body_above_model_reasoning_effort_policy() {
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
  - id: reasoning
    max_reasoning_effort: low
    routes:
      - provider: openai
        upstream_model: gpt-5
        extra_body:
          reasoning:
            effort: high
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("extra_body violates max_reasoning_effort"),
        "unexpected error: {error_text}"
    );
    assert!(
        error_text.contains("reasoning effort `high` exceeds the model maximum `low`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_target_route_above_alias_reasoning_effort_policy() {
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
  - id: reasoning-safe
    alias_of: reasoning
    max_reasoning_effort: medium
  - id: reasoning
    max_reasoning_effort: high
    routes:
      - provider: openai
        upstream_model: gpt-5
        extra_body:
          reasoning_effort: high
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
            error_text.contains(
                "model `reasoning-safe` effective route `gpt-5` extra_body violates max_reasoning_effort"
            ),
            "unexpected error: {error_text}"
        );
    assert!(
        error_text.contains("reasoning effort `high` exceeds the model maximum `medium`"),
        "unexpected error: {error_text}"
    );
}
