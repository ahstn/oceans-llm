use super::environment::TestEnvironment;
use super::*;
use serial_test::serial;

#[test]
fn parses_declarative_teams_and_users_into_seed_inputs() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oidc:
    providers:
      - key: okta
        label: Okta
        issuer_url: https://id.example.com
        client_id: oceans
        client_secret: literal.secret
teams:
  - id: " platform "
    name: Platform
    tags:
      - key: cost-center
        value: platform
users:
  - name: Member
    email: " Member@Example.com "
    auth_mode: oidc
    global_role: platform_admin
    request_logging_enabled: false
    tags:
      - key: workload
        value: admin
    oidc_provider_key: " okta "
    membership:
      team: " platform "
      role: admin
    budget:
      cadence: weekly
      amount_usd: "75.0000"
      hard_limit: false
      timezone: Europe/London
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let teams = config.seed_teams().expect("seed teams");
    let oidc_providers = config.seed_oidc_providers().expect("seed oidc providers");
    let users = config.seed_users().expect("seed users");

    assert_eq!(oidc_providers.len(), 1);
    assert_eq!(oidc_providers[0].provider_key, "okta");
    assert_eq!(oidc_providers[0].scopes, ["openid", "email", "profile"]);
    assert!(!oidc_providers[0].jit.enabled);

    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0].team_key, "platform");
    assert_eq!(teams[0].team_name, "Platform");
    let team_tags = teams[0].tags.as_ref().expect("team tags");
    assert_eq!(team_tags[0].key, "cost-center");
    assert_eq!(team_tags[0].value, "platform");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].email_normalized, "member@example.com");
    assert_eq!(users[0].auth_mode, AuthMode::Oidc);
    assert_eq!(users[0].global_role, GlobalRole::PlatformAdmin);
    assert!(!users[0].request_logging_enabled);
    let user_tags = users[0].tags.as_ref().expect("user tags");
    assert_eq!(user_tags[0].key, "workload");
    assert_eq!(user_tags[0].value, "admin");
    assert_eq!(users[0].oidc_provider_key.as_deref(), Some("okta"));
    let membership = users[0].membership.as_ref().expect("membership");
    assert_eq!(membership.team_key, "platform");
    assert_eq!(membership.role, MembershipRole::Admin);
    let user_budget = users[0].budget.as_ref().expect("user budget");
    assert_eq!(user_budget.cadence, BudgetCadence::Weekly);
    assert_eq!(user_budget.amount_usd, Money4::from_scaled(750_000));
    assert!(!user_budget.hard_limit);
    assert_eq!(user_budget.timezone, "Europe/London");
}

#[test]
#[serial]
fn accepts_multiple_managed_keys_for_same_service_account() {
    let _environment = TestEnvironment::capture(&[
        "OCEANS_TEST_SEED_API_KEY_ONE",
        "OCEANS_TEST_SEED_API_KEY_TWO",
        "OCEANS_API_KEY_SECRET_ENCRYPTION_KEY",
    ]);
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    unsafe {
        env::set_var("OCEANS_TEST_SEED_API_KEY_ONE", "gwk_abcd1234.secret-one");
        env::set_var("OCEANS_TEST_SEED_API_KEY_TWO", "gwk_wxyz9876.secret-two");
        env::set_var(
            "OCEANS_API_KEY_SECRET_ENCRYPTION_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        );
    }

    write_config(
        &config_path,
        r#"
teams:
  - id: platform
    name: Platform
service_accounts:
  - id: ci-indexer
    name: CI Indexer
    team: platform
    budget:
      cadence: daily
      amount_usd: "25.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: primary
        value: env.OCEANS_TEST_SEED_API_KEY_ONE
      - id: fallback
        value: env.OCEANS_TEST_SEED_API_KEY_TWO
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let service_accounts = config
        .seed_service_accounts()
        .expect("seed service accounts");
    assert_eq!(service_accounts.len(), 1);
    assert_eq!(service_accounts[0].managed_api_keys.len(), 2);
    assert_eq!(
        service_accounts[0].managed_api_keys[0].config_key,
        "primary"
    );
    assert_eq!(
        service_accounts[0].managed_api_keys[1].config_key,
        "fallback"
    );
}

#[test]
#[serial]
fn parses_generated_service_account_key_without_encryption_key() {
    let _environment = TestEnvironment::capture(&["OCEANS_API_KEY_SECRET_ENCRYPTION_KEY"]);
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    unsafe {
        env::remove_var("OCEANS_API_KEY_SECRET_ENCRYPTION_KEY");
    }

    write_config(
        &config_path,
        r#"
teams:
  - id: platform
    name: Platform
service_accounts:
  - id: ci-indexer
    name: CI Indexer
    team: platform
    budget:
      cadence: daily
      amount_usd: "25.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: primary
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let service_accounts = config
        .seed_service_accounts()
        .expect("seed service accounts");
    let managed_key = &service_accounts[0].managed_api_keys[0];
    assert_eq!(managed_key.source, ManagedApiKeySource::Generated);
    assert_eq!(managed_key.public_id, None);
    assert_eq!(managed_key.secret_hash, None);
    assert!(managed_key.secret_material.is_none());
}

#[test]
fn rejects_duplicate_service_account_ids_after_normalization() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
teams:
  - id: platform
    name: Platform
service_accounts:
  - id: ci-indexer
    team: platform
    budget:
      cadence: daily
      amount_usd: "25.0000"
  - id: " ci-indexer "
    team: platform
    budget:
      cadence: daily
      amount_usd: "25.0000"
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("duplicate service account id `ci-indexer`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_duplicate_declarative_team_keys_after_normalization() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
teams:
  - id: platform
    name: Platform
  - id: " platform "
    name: Duplicate
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("duplicate team id `platform`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_legacy_auth_seed_api_keys_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  seed_api_keys: []
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("seed_api_keys"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_invalid_declarative_user_memberships() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
teams:
  - id: platform
    name: Platform
users:
  - name: Member
    email: member@example.com
    auth_mode: password
    membership:
      team: platform
      role: owner
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot seed membership role `owner`"),
        "unexpected error: {error_text}"
    );
}
#[test]
#[serial]
fn parses_service_accounts_with_managed_key_and_budget() {
    let _environment = TestEnvironment::capture(&[
        "OCEANS_TEST_SEED_API_KEY",
        "OCEANS_API_KEY_SECRET_ENCRYPTION_KEY",
    ]);
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    unsafe {
        env::set_var("OCEANS_TEST_SEED_API_KEY", "gwk_abcd1234.secret-value");
        env::set_var(
            "OCEANS_API_KEY_SECRET_ENCRYPTION_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        );
    }

    write_config(
        &config_path,
        r#"
teams:
  - id: " platform "
    name: Platform
service_accounts:
  - id: " ci-indexer "
    name: CI Indexer
    team: " platform "
    tags:
      - key: workload
        value: ci-indexer
    budget:
      cadence: daily
      amount_usd: "25.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: primary
        name: CI Indexer Primary
        value: env.OCEANS_TEST_SEED_API_KEY
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let service_accounts = config
        .seed_service_accounts()
        .expect("seed service accounts");
    assert_eq!(service_accounts.len(), 1);
    assert_eq!(service_accounts[0].service_account_key, "ci-indexer");
    assert_eq!(service_accounts[0].service_account_name, "CI Indexer");
    assert_eq!(service_accounts[0].team_key, "platform");
    let service_account_tags = service_accounts[0]
        .tags
        .as_ref()
        .expect("service account tags");
    assert_eq!(service_account_tags[0].key, "workload");
    assert_eq!(service_account_tags[0].value, "ci-indexer");
    assert_eq!(
        service_accounts[0].budget.amount_usd,
        Money4::from_scaled(250_000)
    );
    assert_eq!(service_accounts[0].managed_api_keys.len(), 1);
    let managed_key = &service_accounts[0].managed_api_keys[0];
    assert_eq!(managed_key.config_key, "primary");
    assert_eq!(managed_key.name, "CI Indexer Primary");
    assert_eq!(managed_key.public_id.as_deref(), Some("abcd1234"));
    assert!(managed_key.secret_hash.is_some());
    assert!(managed_key.secret_material.is_some());
}
