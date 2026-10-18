use super::*;
use gateway_service::{McpOauthProviderType, McpOauthTokenEndpointAuthMethod};

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
        ..Default::default()
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
            ..Default::default()
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
        client_secret: Some("literal.google-client-secret".to_string()),
        authorization_url: Some(default_google_authorization_url()),
        token_url: Some(default_google_token_url()),
        scopes: Vec::new(),
        token_endpoint_auth_method: None,
    };
    let config = McpOauthConfig {
        public_base_url: Some("https://gateway.example.com".to_string()),
        providers: vec![provider.clone()],
        ..Default::default()
    };
    config.validate().expect("official Google endpoints");

    for (field, invalid_urls) in [
        (
            "token_url",
            [
                "https://oauth2.googleapis.com.evil.example/token",
                "https://user@oauth2.googleapis.com/token",
                "https://oauth2.googleapis.com:8443/token",
                "https://oauth2.googleapis.com/other",
                "https://oauth2.googleapis.com/token?tenant=x",
                "https://oauth2.googleapis.com/token#fragment",
            ],
        ),
        (
            "authorization_url",
            [
                "https://accounts.google.com.evil.example/o/oauth2/v2/auth",
                "https://user@accounts.google.com/o/oauth2/v2/auth",
                "https://accounts.google.com:8443/o/oauth2/v2/auth",
                "https://accounts.google.com/other",
                "https://accounts.google.com/o/oauth2/v2/auth?tenant=x",
                "https://accounts.google.com/o/oauth2/v2/auth#fragment",
            ],
        ),
    ] {
        for invalid in invalid_urls {
            let mut provider = provider.clone();
            let endpoint = match field {
                "token_url" => &mut provider.token_url,
                "authorization_url" => &mut provider.authorization_url,
                _ => unreachable!(),
            };
            *endpoint = Some(invalid.to_string());
            let config = McpOauthConfig {
                public_base_url: Some("https://gateway.example.com".to_string()),
                providers: vec![provider],
                ..Default::default()
            };
            let error = config
                .validate()
                .expect_err("unofficial endpoint must fail");
            assert!(
                error.to_string().contains(&format!("{field} must be")),
                "unexpected rejection for {invalid}: {error:#}"
            );
        }
    }
}

#[test]
fn parses_generic_oauth2_provider_with_explicit_endpoints() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
mcp:
  oauth:
    public_base_url: https://gateway.example.com
    providers:
      - key: linear
        provider_type: oauth2
        client_id: literal.linear-client
        client_secret: literal.linear-secret
        authorization_url: https://linear.example.com/oauth/authorize
        token_url: https://linear.example.com/oauth/token
        scopes: [read, write]
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("valid oauth2 provider");
    let runtime = config.mcp.oauth.runtime().expect("MCP OAuth runtime");
    let provider = runtime.provider("linear").expect("linear provider");
    assert_eq!(provider.provider_type, McpOauthProviderType::Oauth2);
    assert_eq!(provider.token_url, "https://linear.example.com/oauth/token");
    assert_eq!(provider.scopes, vec!["read", "write"]);
    assert_eq!(
        provider.token_endpoint_auth_method,
        McpOauthTokenEndpointAuthMethod::ClientSecretBasic
    );
    assert_eq!(
        runtime.callback_url("linear").expect("callback URL"),
        "https://gateway.example.com/api/v1/mcp/oauth/linear/callback"
    );
}

#[test]
fn generic_oauth2_provider_requires_endpoints_and_consistent_client_auth() {
    let base = McpOauthProviderConfig {
        key: "linear".to_string(),
        provider_type: "oauth2".to_string(),
        client_id: "literal.linear-client".to_string(),
        client_secret: None,
        authorization_url: Some("https://linear.example.com/oauth/authorize".to_string()),
        token_url: Some("https://linear.example.com/oauth/token".to_string()),
        scopes: Vec::new(),
        token_endpoint_auth_method: None,
    };
    let validate = |provider: McpOauthProviderConfig| {
        McpOauthConfig {
            public_base_url: Some("https://gateway.example.com".to_string()),
            providers: vec![provider],
            ..Default::default()
        }
        .validate()
    };

    // A public client without a secret authenticates with `none`.
    validate(base.clone()).expect("public oauth2 client");

    let mut missing_token_url = base.clone();
    missing_token_url.token_url = None;
    assert!(validate(missing_token_url).is_err());

    let mut insecure = base.clone();
    insecure.authorization_url = Some("http://linear.example.com/oauth/authorize".to_string());
    assert!(validate(insecure).is_err());

    let mut basic_without_secret = base.clone();
    basic_without_secret.token_endpoint_auth_method = Some("client_secret_basic".to_string());
    assert!(validate(basic_without_secret).is_err());

    let mut reserved = base.clone();
    reserved.key = "mcp_server".to_string();
    assert!(validate(reserved).is_err());

    let mut unknown = base;
    unknown.provider_type = "saml".to_string();
    assert!(validate(unknown).is_err());
}
