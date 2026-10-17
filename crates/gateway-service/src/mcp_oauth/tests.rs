use super::*;
use crate::mcp_upstream_auth::MCP_SERVER_OAUTH_PROVIDER_KEY;
use gateway_core::{ExternalMcpAuthMode, ExternalMcpServerStatus, ExternalMcpTransport};
use serde_json::{Map, json};

#[test]
fn required_scope_check_accepts_superset_and_rejects_missing_scope() {
    let required = vec![
        "https://www.googleapis.com/auth/drive.readonly".to_string(),
        "https://www.googleapis.com/auth/documents.readonly".to_string(),
    ];
    let mut granted = required.clone();
    granted.push("openid".to_string());
    ensure_required_scopes(&required, &granted).expect("scope superset");

    let error = ensure_required_scopes(&required, &granted[1..])
        .expect_err("missing Drive scope must fail");
    assert_eq!(error.error_code(), "invalid_request");
}

#[tokio::test]
async fn token_response_errors_do_not_expose_provider_details() {
    for (status, body, expected_code) in [
        (
            400,
            r#"{"error":"invalid_grant","error_description":"private-refresh-token"}"#,
            "upstream_transport",
        ),
        (
            200,
            r#"{"error":"invalid_grant","access_token":"private-access-token"}"#,
            "upstream_transport",
        ),
        (
            503,
            r#"{"error":"private-provider-detail"}"#,
            "upstream_http_error",
        ),
        (
            400,
            "malformed private-provider-detail",
            "upstream_http_error",
        ),
    ] {
        let status = reqwest::StatusCode::from_u16(status).expect("status");
        let error = parse_token_response(
            status,
            body.as_bytes(),
            "google",
            "resource",
            &[],
            None,
            true,
        )
        .expect_err("provider error must be rejected");
        assert_eq!(error.error_code(), expected_code);
        assert!(!error.to_string().contains("private-"));
        assert!(!error.to_string().contains("invalid_grant"));
    }
}

fn server_client(token_url: &str) -> McpOauthClient {
    McpOauthClient {
        provider_key: MCP_SERVER_OAUTH_PROVIDER_KEY.to_string(),
        client_id: "oceans".to_string(),
        client_secret: None,
        authorization_url: "https://auth.example.com/authorize".to_string(),
        token_url: token_url.to_string(),
        token_endpoint_auth_method: McpOauthTokenEndpointAuthMethod::None,
        redirect_uri: "https://oceans.example.com/api/v1/mcp/oauth/callback".to_string(),
        issuer: Some("https://auth.example.com".to_string()),
        default_scopes: Vec::new(),
        google: false,
        token_endpoint_source: TokenEndpointSource::Upstream,
        // A public MCP server, named by an IP literal so the tests need no DNS.
        scope_url: "https://93.184.215.14/mcp".to_string(),
    }
}

#[tokio::test]
async fn server_token_endpoints_cannot_reach_internal_addresses() {
    let runtime = McpOauthRuntime::new(Some("https://oceans.example.com".into()), Vec::new());
    // A public MCP server, named by an IP literal so the test needs no DNS.
    let config = McpOauthServerConfig {
        provider_key: MCP_SERVER_OAUTH_PROVIDER_KEY.to_string(),
        resource: "https://93.184.215.14/mcp".to_string(),
        scopes: Vec::new(),
        resource_indicator: true,
        server_oauth: None,
        discovery_tool_allowlist: None,
    };
    for token_url in [
        "https://169.254.169.254/latest/meta-data/token",
        "https://10.0.0.5/token",
        "https://127.0.0.1:9/token",
    ] {
        let error = runtime
            .exchange_code(&server_client(token_url), &config, "code", "verifier", &[])
            .await
            .expect_err("internal token endpoint must be refused");
        // A policy refusal, not a flaky upstream: it is not retried and the
        // refresh fallback does not hide it.
        assert!(
            matches!(&error, GatewayError::InvalidRequest(message) if message.contains("not allowed")),
            "{token_url}: {error}"
        );
        assert!(!is_transport_error(&error), "{token_url}");
    }
}

#[tokio::test]
async fn token_requests_are_scoped_by_the_admin_entered_server() {
    let runtime = McpOauthRuntime::new(Some("https://oceans.example.com".into()), Vec::new());
    // The saved resource names a private address, but the server the admin
    // entered is public, so the token request may not go there.
    let config = McpOauthServerConfig {
        provider_key: MCP_SERVER_OAUTH_PROVIDER_KEY.to_string(),
        resource: "https://10.0.0.5/mcp".to_string(),
        scopes: Vec::new(),
        resource_indicator: false,
        server_oauth: None,
        discovery_tool_allowlist: None,
    };
    let error = runtime
        .exchange_code(
            &server_client("https://10.0.0.5/token"),
            &config,
            "code",
            "verifier",
            &[],
        )
        .await
        .expect_err("scope comes from the server URL");
    assert!(error.to_string().contains("not allowed"), "{error}");
}

#[test]
fn team_server_endpoints_are_scoped_by_their_own_url() {
    // A platform admin's internal server: its private address scopes every
    // endpoint it uses.
    let mut server = oauth_server();
    server.server_url = "http://10.0.0.5/mcp".to_string();
    let endpoint = "https://auth.example.com/token";
    assert_eq!(endpoint_scope_url(&server, endpoint), server.server_url);
    // Once a team owns it, an endpoint the team may have edited is scoped by
    // its own host, so it cannot borrow the server's private reach.
    server.ownership = gateway_core::McpServerOwnership::team(Uuid::new_v4(), None);
    assert_eq!(endpoint_scope_url(&server, endpoint), endpoint);
    let runtime = McpOauthRuntime::new(None, Vec::new());
    assert_eq!(
        runtime
            .token_fetchers
            .for_trusted_url(endpoint_scope_url(&server, endpoint))
            .scope(),
        crate::outbound_fetch::OutboundAddressScope::Public
    );
}

#[test]
fn only_unreachable_token_endpoints_count_as_transport_failures() {
    assert!(is_transport_error(&oauth_fetch_error(
        OutboundFetchError::Transport("timed out".into())
    )));
    assert!(is_transport_error(&oauth_fetch_error(
        OutboundFetchError::Unresolved {
            host: "auth.example.com".into()
        }
    )));
    for refused in [
        OutboundFetchError::BlockedAddress {
            host: "10.0.0.5".into(),
        },
        OutboundFetchError::UnsupportedUrl("the URL must use https".into()),
        OutboundFetchError::TooLarge,
    ] {
        assert!(
            !is_transport_error(&oauth_fetch_error(refused.clone())),
            "{refused:?}"
        );
    }
}

#[tokio::test]
async fn token_requests_never_send_secrets_over_plain_http() {
    let runtime = McpOauthRuntime::new(Some("https://oceans.example.com".into()), Vec::new());
    let config = McpOauthServerConfig {
        provider_key: "corp".to_string(),
        resource: "https://93.184.215.14/mcp".to_string(),
        scopes: Vec::new(),
        resource_indicator: false,
        server_oauth: None,
        discovery_tool_allowlist: None,
    };
    // Both a server-level client and a configured provider's client.
    for source in [
        TokenEndpointSource::Upstream,
        TokenEndpointSource::GatewayConfig,
    ] {
        let mut client = server_client("http://auth.example.com/token");
        client.token_endpoint_source = source;
        client.client_secret = Some("secret".to_string());
        client.token_endpoint_auth_method = McpOauthTokenEndpointAuthMethod::ClientSecretPost;
        let error = runtime
            .exchange_code(&client, &config, "code", "verifier", &[])
            .await
            .expect_err("plain http token endpoint must be refused");
        assert!(error.to_string().contains("must use https"), "{error}");
    }
}

#[test]
fn transient_oauth_statuses_are_limited_to_rate_limits_and_server_errors() {
    assert!(is_transient_oauth_status(
        reqwest::StatusCode::TOO_MANY_REQUESTS
    ));
    assert!(is_transient_oauth_status(
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    ));
    assert!(!is_transient_oauth_status(reqwest::StatusCode::BAD_REQUEST));
    assert!(!is_transient_oauth_status(
        reqwest::StatusCode::UNAUTHORIZED
    ));
}

#[test]
fn oauth_bundle_must_match_the_current_server_contract() {
    let server = oauth_server();
    let bundle = McpOauthTokenBundle {
        version: 1,
        access_token: "access".to_string(),
        refresh_token: Some("refresh".to_string()),
        token_type: "Bearer".to_string(),
        provider_key: "google".to_string(),
        resource: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
        granted_scopes: vec!["https://www.googleapis.com/auth/drive.readonly".to_string()],
        issuer: None,
    };

    validate_oauth_bundle_for_server(&bundle, &server).expect("matching contract");

    let mut wrong_resource = bundle.clone();
    wrong_resource.resource = "https://docsmcp.googleapis.com/mcp/v1".to_string();
    assert_eq!(
        validate_oauth_bundle_for_server(&wrong_resource, &server)
            .expect_err("resource mismatch")
            .error_code(),
        "credential_required"
    );

    let mut missing_scope = bundle;
    missing_scope.granted_scopes.clear();
    assert_eq!(
        validate_oauth_bundle_for_server(&missing_scope, &server)
            .expect_err("scope mismatch")
            .error_code(),
        "credential_required"
    );
}

fn oauth_server() -> ExternalMcpServerRecord {
    ExternalMcpServerRecord {
        ownership: Default::default(),
        mcp_server_id: Uuid::new_v4(),
        server_key: "google_drive".to_string(),
        display_name: "Google Drive".to_string(),
        description: None,
        transport: ExternalMcpTransport::StreamableHttp,
        server_url: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
        auth_mode: ExternalMcpAuthMode::OauthObo,
        auth_config: Map::from_iter([
            ("provider_key".to_string(), json!("google")),
            (
                "resource".to_string(),
                json!("https://drivemcp.googleapis.com/mcp/v1"),
            ),
            (
                "scopes".to_string(),
                json!(["https://www.googleapis.com/auth/drive.readonly"]),
            ),
        ]),
        timeout_ms: 30_000,
        status: ExternalMcpServerStatus::Active,
        last_discovery_status: None,
        last_discovery_at: None,
        last_successful_discovery_at: None,
        last_error_summary: None,
        last_tool_count: None,
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
        disabled_at: None,
    }
}
