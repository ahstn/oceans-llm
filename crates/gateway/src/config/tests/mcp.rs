use super::*;

#[test]
fn parses_google_mcp_oauth_runtime_with_literal_public_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
mcp:
  oauth:
    public_base_url: https://gateway.example.com/
    providers:
      - key: google
        client_id: literal.google-client-id
        client_secret: literal.google-client-secret
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("valid MCP OAuth config");
    let runtime = config.mcp.oauth.runtime().expect("MCP OAuth runtime");
    assert_eq!(
        runtime.callback_url("google").expect("callback URL"),
        "https://gateway.example.com/api/v1/mcp/oauth/google/callback"
    );
    assert_eq!(
        runtime
            .provider("google")
            .expect("Google provider")
            .token_url,
        "https://oauth2.googleapis.com/token"
    );
}

#[test]
fn mcp_oauth_public_base_url_must_be_an_https_origin() {
    let config = McpOauthConfig {
        public_base_url: Some("https://gateway.example.com:8443/".to_string()),
        providers: Vec::new(),
    };
    assert_eq!(
        config
            .resolved_public_base_url()
            .expect("valid public origin")
            .as_deref(),
        Some("https://gateway.example.com:8443")
    );

    for invalid in [
        "https://user@gateway.example.com",
        "https://gateway.example.com/path",
        "https://gateway.example.com?tenant=x",
        "https://gateway.example.com#fragment",
    ] {
        let config = McpOauthConfig {
            public_base_url: Some(invalid.to_string()),
            providers: Vec::new(),
        };
        assert!(
            config.resolved_public_base_url().is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn google_mcp_oauth_endpoints_are_pinned() {
    let provider = McpOauthProviderConfig {
        key: "google".to_string(),
        provider_type: "google".to_string(),
        client_id: "literal.google-client-id".to_string(),
        client_secret: "literal.google-client-secret".to_string(),
        authorization_url: default_google_authorization_url(),
        token_url: default_google_token_url(),
    };
    let config = McpOauthConfig {
        public_base_url: Some("https://gateway.example.com".to_string()),
        providers: vec![provider.clone()],
    };
    config.validate().expect("official Google endpoints");

    for invalid in [
        "https://oauth2.googleapis.com.evil.example/token",
        "https://user@oauth2.googleapis.com/token",
        "https://oauth2.googleapis.com:8443/token",
        "https://oauth2.googleapis.com/other",
        "https://oauth2.googleapis.com/token?tenant=x",
        "https://oauth2.googleapis.com/token#fragment",
    ] {
        let mut provider = provider.clone();
        provider.token_url = invalid.to_string();
        let config = McpOauthConfig {
            public_base_url: Some("https://gateway.example.com".to_string()),
            providers: vec![provider],
        };
        assert!(config.validate().is_err(), "accepted {invalid}");
    }
}
