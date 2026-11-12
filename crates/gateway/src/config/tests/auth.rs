use super::environment::TestEnvironment;
use super::*;
use serial_test::serial;

#[test]
#[serial]
fn production_config_requires_bootstrap_password_change() {
    let _environment =
        TestEnvironment::capture(&["POSTGRES_URL", "GATEWAY_BOOTSTRAP_ADMIN_PASSWORD"]);
    let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gateway.prod.yaml");
    unsafe {
        env::set_var(
            "POSTGRES_URL",
            "postgres://postgres:postgres@localhost/test",
        );
        env::set_var(
            "GATEWAY_BOOTSTRAP_ADMIN_PASSWORD",
            "test-only-strong-bootstrap-password",
        );
    }

    let config = GatewayConfig::from_path(&config_path).expect("prod config should parse");

    assert!(config.auth.bootstrap_admin.enabled);
    assert_eq!(config.auth.bootstrap_admin.email, "admin@local");
    assert!(config.auth.bootstrap_admin.require_password_change);
}

#[test]
fn built_in_defaults_do_not_enable_bootstrap_admin() {
    let config = GatewayConfig::default();

    assert!(!config.auth.bootstrap_admin.enabled);
    assert!(config.auth.bootstrap_admin.password.is_empty());
    assert!(config.auth.bootstrap_admin.require_password_change);
}

#[test]
fn omitted_bootstrap_password_change_defaults_to_required() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
server:
  bind: 127.0.0.1:8080
auth:
  bootstrap_admin:
    enabled: true
    password: literal.admin
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");

    assert!(config.auth.bootstrap_admin.require_password_change);
}

#[test]
fn rejects_literal_bootstrap_secret_on_non_loopback_listener() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
server:
  bind: 0.0.0.0:8080
auth:
  bootstrap_admin:
    enabled: true
    password: literal.this-is-not-an-external-secret
    require_password_change: true
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}").contains("must use an env.* or file.* secret reference"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn rejects_weak_external_bootstrap_secret_on_non_loopback_listener() {
    let tmp = tempdir().expect("tempdir");
    let secret_path = tmp.path().join("bootstrap-password");
    std::fs::write(&secret_path, "admin\n").expect("write password");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        &format!(
            r#"
server:
  bind: 0.0.0.0:8080
auth:
  bootstrap_admin:
    enabled: true
    password: file.{}
    require_password_change: true
"#,
            secret_path.display()
        ),
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}").contains("must contain at least 16 characters"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn requires_password_rotation_for_non_loopback_bootstrap_admin() {
    let tmp = tempdir().expect("tempdir");
    let secret_path = tmp.path().join("bootstrap-password");
    std::fs::write(&secret_path, "test-only-strong-bootstrap-password\n").expect("write password");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        &format!(
            r#"
server:
  bind: 0.0.0.0:8080
auth:
  bootstrap_admin:
    enabled: true
    password: file.{}
    require_password_change: false
"#,
            secret_path.display()
        ),
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}").contains("require_password_change must be true"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn accepts_strong_external_bootstrap_secret_on_non_loopback_listener() {
    let tmp = tempdir().expect("tempdir");
    let secret_path = tmp.path().join("bootstrap-password");
    std::fs::write(&secret_path, "test-only-strong-bootstrap-password\n").expect("write password");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        &format!(
            r#"
server:
  bind: 0.0.0.0:8080
auth:
  bootstrap_admin:
    enabled: true
    password: file.{}
    require_password_change: true
"#,
            secret_path.display()
        ),
    );

    GatewayConfig::from_path(&config_path).expect("config should parse");
}

#[test]
fn disabled_bootstrap_admin_does_not_require_a_secret() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
server:
  bind: 0.0.0.0:8080
auth:
  bootstrap_admin:
    enabled: false
"#,
    );

    GatewayConfig::from_path(&config_path).expect("disabled bootstrap should parse");
}

#[test]
fn loopback_listener_allows_explicit_development_bootstrap_secret() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
server:
  bind: 127.0.0.1:8080
auth:
  bootstrap_admin:
    enabled: true
    password: literal.admin
    require_password_change: false
"#,
    );

    GatewayConfig::from_path(&config_path).expect("loopback development config should parse");
}

#[test]
fn rejects_duplicate_oidc_provider_keys() {
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
      - key: " okta "
        label: Okta Duplicate
        issuer_url: https://id2.example.com
        client_id: oceans
        client_secret: literal.secret
"#,
    );

    GatewayConfig::from_path(&config_path).expect_err("config should fail");
}

#[test]
fn rejects_oidc_jit_unknown_team() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oidc:
    providers:
      - key: authentik
        label: Authentik
        issuer_url: https://id.example.com
        client_id: oceans
        client_secret: literal.secret
        jit:
          enabled: true
          membership:
            team: missing
            role: admin
"#,
    );

    GatewayConfig::from_path(&config_path).expect_err("config should fail");
}

#[test]
#[serial]
fn resolves_enabled_oauth_client_id_env_reference() {
    let _environment = TestEnvironment::capture(&["OCEANS_TEST_GITHUB_CLIENT_ID"]);
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    unsafe {
        env::set_var("OCEANS_TEST_GITHUB_CLIENT_ID", "github-client-id");
    }

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    public_base_url: literal.https://gateway.example.com
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: env.OCEANS_TEST_GITHUB_CLIENT_ID
        client_secret: literal.secret
        scopes: [read:user, user:email]
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let oauth_providers = config.seed_oauth_providers().expect("seed oauth providers");
    assert_eq!(oauth_providers[0].client_id, "github-client-id");
    assert!(oauth_providers[0].sso_email_verification_enabled);
}

#[test]
fn rejects_enabled_oauth_provider_without_public_base_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: github-client-id
        client_secret: literal.secret
        scopes: [read:user, user:email]
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("auth.oauth.public_base_url is required"),
        "unexpected error: {error_text}"
    );
}

#[test]
#[serial]
fn disabled_oauth_provider_allows_unset_secret_references() {
    let _environment = TestEnvironment::capture(&[
        "OCEANS_TEST_MISSING_GITHUB_CLIENT_ID",
        "OCEANS_TEST_MISSING_GITHUB_CLIENT_SECRET",
    ]);
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    unsafe {
        env::remove_var("OCEANS_TEST_MISSING_GITHUB_CLIENT_ID");
        env::remove_var("OCEANS_TEST_MISSING_GITHUB_CLIENT_SECRET");
    }

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: env.OCEANS_TEST_MISSING_GITHUB_CLIENT_ID
        client_secret: env.OCEANS_TEST_MISSING_GITHUB_CLIENT_SECRET
        scopes: [read:user, user:email]
        enabled: false
"#,
    );

    GatewayConfig::from_path(&config_path).expect("disabled provider should parse");
}

#[test]
fn rejects_github_oauth_provider_without_email_scope() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    public_base_url: literal.https://gateway.example.com
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: github-client-id
        client_secret: literal.secret
        scopes: [read:user]
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("must include `user:email`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn parses_github_oauth_allowed_email_domains_into_seed() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    public_base_url: literal.https://gateway.example.com
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: github-client-id
        client_secret: literal.secret
        scopes: [read:user, user:email]
        allowed_email_domains:
          - Test.com
          - Engineering.Example.com.
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let oauth_providers = config.seed_oauth_providers().expect("seed oauth providers");
    assert_eq!(
        oauth_providers[0].allowed_email_domains,
        vec!["test.com", "engineering.example.com"]
    );
}

#[test]
fn parses_github_oauth_sso_email_verification_escape_hatch() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    public_base_url: literal.https://gateway.example.com
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: github-client-id
        client_secret: literal.secret
        scopes: [read:user, user:email]
        sso_email_verification_enabled: false
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let oauth_providers = config.seed_oauth_providers().expect("seed oauth providers");
    assert!(!oauth_providers[0].sso_email_verification_enabled);
}

#[test]
fn rejects_github_oauth_duplicate_allowed_email_domains() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    public_base_url: literal.https://gateway.example.com
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: github-client-id
        client_secret: literal.secret
        scopes: [read:user, user:email]
        allowed_email_domains:
          - Test.com
          - test.com
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("contains duplicate domain `test.com`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_github_oauth_invalid_allowed_email_domain() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  oauth:
    public_base_url: literal.https://gateway.example.com
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: github-client-id
        client_secret: literal.secret
        scopes: [read:user, user:email]
        allowed_email_domains:
          - alice@test.com
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("must be a domain name"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_user_email_matching_configured_bootstrap_admin_email() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
auth:
  bootstrap_admin:
    enabled: true
    email: "ops-admin@example.com"
    password: "literal.secret"
users:
  - name: Ops Admin
    email: " ops-admin@example.com "
    auth_mode: password
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("user email `ops-admin@example.com` is reserved for bootstrap admin"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn seeds_oidc_oauth_and_bootstrap_auth_config() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
auth:
  bootstrap_admin:
    password: literal.bootstrap-secret
  oidc:
    public_base_url: literal.https://gateway.example.com/
    providers:
      - key: workforce
        label: Workforce
        issuer_url: https://id.example.com
        client_id: workforce-client
        client_secret: literal.oidc-secret
        jit:
          enabled: true
          membership:
            team: platform
            role: member
  oauth:
    public_base_url: literal.https://gateway.example.com/
    providers:
      - key: github
        label: GitHub
        client_id: literal.github-client
        client_secret: literal.github-secret
        allowed_email_domains:
          - Example.COM
        jit:
          enabled: true
          membership:
            team: platform
            role: admin
teams:
  - id: platform
    name: Platform
"#,
    );

    let config = GatewayConfig::from_path(&config_path)?;
    assert_eq!(
        config.auth.bootstrap_admin.resolved_password()?,
        "bootstrap-secret"
    );
    assert_eq!(
        config.auth.oidc.resolved_public_base_url()?.as_deref(),
        Some("https://gateway.example.com")
    );
    assert_eq!(
        config.auth.oauth.resolved_public_base_url()?.as_deref(),
        Some("https://gateway.example.com")
    );

    let oidc = config.seed_oidc_providers()?;
    assert_eq!(oidc.len(), 1);
    assert_eq!(oidc[0].provider_key, "workforce");
    assert_eq!(
        oidc[0]
            .jit
            .membership
            .as_ref()
            .map(|value| value.team_key.as_str()),
        Some("platform")
    );

    let oauth = config.seed_oauth_providers()?;
    assert_eq!(oauth.len(), 1);
    assert_eq!(oauth[0].client_id, "github-client");
    assert_eq!(oauth[0].allowed_email_domains, ["example.com"]);
    assert_eq!(
        oauth[0]
            .jit
            .membership
            .as_ref()
            .map(|value| value.team_key.as_str()),
        Some("platform")
    );

    Ok(())
}
