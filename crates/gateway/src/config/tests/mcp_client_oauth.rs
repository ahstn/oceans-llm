use super::*;

fn load(yaml: &str) -> anyhow::Result<GatewayConfig> {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(&config_path, yaml);
    GatewayConfig::from_path(&config_path)
}

#[test]
fn mcp_client_oauth_is_disabled_by_default() {
    let config = load("mcp: {}\n").expect("valid config");
    assert!(
        config
            .mcp_client_oauth_settings()
            .expect("settings")
            .is_none()
    );
}

#[test]
fn mcp_client_oauth_uses_its_own_public_base_url_and_lifetimes() {
    let config = load(
        r#"
mcp:
  client_oauth:
    enabled: true
    public_base_url: https://oceans.example.com/
    access_token_ttl_seconds: 900
    refresh_token_ttl_seconds: 86400
    dynamic_client_registration: false
"#,
    )
    .expect("valid config");
    let settings = config
        .mcp_client_oauth_settings()
        .expect("settings")
        .expect("enabled");
    assert_eq!(settings.issuer(), "https://oceans.example.com");
    assert_eq!(settings.access_token_ttl().whole_seconds(), 900);
    assert_eq!(settings.refresh_token_ttl().whole_seconds(), 86400);
    assert!(!settings.dynamic_client_registration());
}

#[test]
fn mcp_client_oauth_falls_back_to_other_public_base_urls_in_order() {
    let config = load(
        r#"
auth:
  oidc:
    public_base_url: literal.https://sso.example.com
  oauth:
    public_base_url: literal.https://github-sso.example.com
mcp:
  oauth:
    public_base_url: https://upstream.example.com
  client_oauth:
    enabled: true
"#,
    )
    .expect("valid config");
    let settings = config.mcp_client_oauth_settings().unwrap().unwrap();
    assert_eq!(settings.issuer(), "https://upstream.example.com");
    assert!(settings.dynamic_client_registration());

    let config = load(
        r#"
auth:
  oidc:
    public_base_url: literal.https://sso.example.com
  oauth:
    public_base_url: literal.https://github-sso.example.com
mcp:
  client_oauth:
    enabled: true
"#,
    )
    .expect("valid config");
    let settings = config.mcp_client_oauth_settings().unwrap().unwrap();
    assert_eq!(settings.issuer(), "https://sso.example.com");

    let config = load(
        r#"
auth:
  oauth:
    public_base_url: literal.https://github-sso.example.com
mcp:
  client_oauth:
    enabled: true
"#,
    )
    .expect("valid config");
    let settings = config.mcp_client_oauth_settings().unwrap().unwrap();
    assert_eq!(settings.issuer(), "https://github-sso.example.com");
}

#[test]
fn mcp_client_oauth_accepts_loopback_http_for_local_stacks() {
    let config = load(
        r#"
mcp:
  client_oauth:
    enabled: true
    public_base_url: http://127.0.0.1:38090
"#,
    )
    .expect("valid config");
    assert_eq!(
        config
            .mcp_client_oauth_settings()
            .unwrap()
            .unwrap()
            .issuer(),
        "http://127.0.0.1:38090"
    );
}

#[test]
fn mcp_client_oauth_rejects_missing_or_insecure_issuers() {
    let error = load("mcp:\n  client_oauth:\n    enabled: true\n").unwrap_err();
    assert!(
        format!("{error:#}").contains("mcp.client_oauth.public_base_url is required"),
        "{error:#}"
    );

    let error = load(
        r#"
mcp:
  client_oauth:
    enabled: true
    public_base_url: http://oceans.example.com
"#,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("https"), "{error:#}");

    let error = load(
        r#"
auth:
  oidc:
    public_base_url: literal.https://sso.example.com/tenant
mcp:
  client_oauth:
    enabled: true
"#,
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("auth.oidc.public_base_url cannot be used"),
        "{error:#}"
    );
}

#[test]
fn mcp_client_oauth_rejects_out_of_range_lifetimes() {
    let error = load(
        r#"
mcp:
  client_oauth:
    enabled: true
    public_base_url: https://oceans.example.com
    access_token_ttl_seconds: 172800
"#,
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("access_token_ttl_seconds"),
        "{error:#}"
    );
}

#[test]
fn mcp_client_oauth_resolves_env_references() {
    // SAFETY: test-local variable name; no other test reads it.
    unsafe {
        env::set_var(
            "OCEANS_TEST_MCP_CLIENT_OAUTH_ISSUER",
            "https://env.example.com",
        )
    };
    let config = load(
        r#"
mcp:
  client_oauth:
    enabled: true
    public_base_url: env.OCEANS_TEST_MCP_CLIENT_OAUTH_ISSUER
"#,
    )
    .expect("valid config");
    assert_eq!(
        config
            .mcp_client_oauth_settings()
            .unwrap()
            .unwrap()
            .issuer(),
        "https://env.example.com"
    );
}
