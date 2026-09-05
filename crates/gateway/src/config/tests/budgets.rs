use super::*;

#[test]
fn parses_human_budget_defaults_into_seed_inputs() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
budgets:
  users:
    default:
      cadence: daily
      amount_usd: "70.0000"
      hard_limit: true
      timezone: UTC
    model_defaults:
      - model: " fable-5 "
        budget:
          cadence: daily
          amount_usd: "40.0000"
          hard_limit: true
          timezone: UTC
models:
  - id: fable-5
    routes:
      - provider: openai
        upstream_model: fable-5
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let defaults = config
        .seed_human_budget_defaults()
        .expect("seed budget defaults");

    let default_budget = defaults.default_user_budget.expect("default user budget");
    assert_eq!(default_budget.cadence, BudgetCadence::Daily);
    assert_eq!(default_budget.amount_usd, Money4::from_scaled(700_000));
    assert!(default_budget.hard_limit);

    assert_eq!(defaults.model_defaults.len(), 1);
    assert_eq!(defaults.model_defaults[0].model_key, "fable-5");
    assert_eq!(
        defaults.model_defaults[0].budget.amount_usd,
        Money4::from_scaled(400_000)
    );
}

#[test]
fn rejects_human_budget_default_for_unknown_model() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
budgets:
  users:
    model_defaults:
      - model: missing
        budget:
          cadence: daily
          amount_usd: "40.0000"
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("budgets.users.model_defaults references unknown model `missing`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_duplicate_human_budget_model_defaults() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
budgets:
  users:
    model_defaults:
      - model: fable-5
        budget:
          cadence: daily
          amount_usd: "40.0000"
      - model: " fable-5 "
        budget:
          cadence: daily
          amount_usd: "30.0000"
models:
  - id: fable-5
    routes:
      - provider: openai
        upstream_model: fable-5
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("duplicate budgets.users.model_defaults model `fable-5`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_unknown_human_budget_field() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
budgets:
  users:
    default:
      cadence: daily
      amount_usd: "70.0000"
      hard_limti: true
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("unknown field `hard_limti`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_zero_human_budget_amount() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
budgets:
  users:
    default:
      cadence: daily
      amount_usd: "0.0000"
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("budgets.users.default amount_usd must be greater than zero"),
        "unexpected error: {error_text}"
    );
}
