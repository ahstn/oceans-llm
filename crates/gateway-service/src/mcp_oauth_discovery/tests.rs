use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode, header::WWW_AUTHENTICATE},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::*;

async fn serve(router: impl FnOnce(String) -> Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let origin = format!("http://{}", listener.local_addr().expect("mock address"));
    let router = router(origin.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve mock");
    });
    origin
}

fn http() -> OutboundFetcher {
    OutboundFetcher::new(crate::outbound_fetch::OutboundAddressScope::Loopback)
}

fn json_route(value: Value) -> axum::routing::MethodRouter {
    get(move || {
        let value = value.clone();
        async move { Json(value) }
    })
}

#[test]
fn bearer_challenge_is_read_among_other_challenges() {
    let challenge = parse_bearer_challenge(
        r#"Basic realm="legacy", Bearer error="invalid_token", resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource/mcp", scope="repo read:org""#,
    )
    .expect("bearer challenge");
    assert_eq!(
        challenge.resource_metadata.as_deref(),
        Some("https://mcp.example.com/.well-known/oauth-protected-resource/mcp")
    );
    assert_eq!(challenge.scope.as_deref(), Some("repo read:org"));
    assert_eq!(challenge.error.as_deref(), Some("invalid_token"));

    // Quoted commas and escapes stay inside the value, and the scheme is case-insensitive.
    let challenge = parse_bearer_challenge(r#"bearer scope="a,b \"c\"""#).expect("challenge");
    assert_eq!(challenge.scope.as_deref(), Some(r#"a,b "c""#));

    assert_eq!(
        parse_bearer_challenge("Bearer"),
        Some(BearerChallenge::default())
    );
    assert!(parse_bearer_challenge(r#"Basic realm="x""#).is_none());
}

#[tokio::test]
async fn discovers_endpoints_from_the_challenge_and_rfc8414_metadata() {
    let origin = serve(|origin| {
        let metadata_url = format!("{origin}/.well-known/oauth-protected-resource/mcp");
        Router::new()
            .route(
                "/mcp",
                post(move || {
                    let challenge =
                        format!(r#"Bearer resource_metadata="{metadata_url}", scope="files:read""#);
                    async move {
                        (StatusCode::UNAUTHORIZED, [(WWW_AUTHENTICATE, challenge)]).into_response()
                    }
                }),
            )
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                json_route(json!({
                    "resource": format!("{origin}/mcp"),
                    "authorization_servers": [format!("{origin}/tenant")],
                    "scopes_supported": ["files:read", "files:write"],
                })),
            )
            .route(
                "/.well-known/oauth-authorization-server/tenant",
                json_route(json!({
                    "issuer": format!("{origin}/tenant"),
                    "authorization_endpoint": format!("{origin}/tenant/authorize"),
                    "token_endpoint": format!("{origin}/tenant/token"),
                    "registration_endpoint": format!("{origin}/tenant/register"),
                    "response_types_supported": ["code"],
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": ["client_secret_post", "none"],
                })),
            )
    })
    .await;

    let discovery = discover_mcp_oauth(&http(), &format!("{origin}/mcp"))
        .await
        .expect("discovery");

    assert_eq!(discovery.resource, format!("{origin}/mcp"));
    assert_eq!(
        discovery.resource_metadata_url.as_deref(),
        Some(format!("{origin}/.well-known/oauth-protected-resource/mcp").as_str())
    );
    assert_eq!(discovery.issuer, format!("{origin}/tenant"));
    assert_eq!(discovery.token_endpoint, format!("{origin}/tenant/token"));
    assert_eq!(discovery.recommended_client_registration, "dynamic");
    // The challenge's scope wins over the resource's full scope list.
    assert_eq!(discovery.default_scopes, vec!["files:read"]);
    assert_eq!(
        discovery.scopes_supported,
        vec!["files:read", "files:write"]
    );
    assert_eq!(
        discovery.token_endpoint_auth_method,
        McpOauthTokenEndpointAuthMethod::ClientSecretPost
    );
    assert!(discovery.resource_indicator);
    assert!(discovery.warnings.is_empty(), "{:?}", discovery.warnings);

    let config = discovery.suggested_auth_config();
    assert_eq!(config["client_registration"], "dynamic");
    assert_eq!(
        config["registration_endpoint"],
        format!("{origin}/tenant/register")
    );
    assert_eq!(config["token_endpoint_auth_method"], "client_secret_post");
    assert_eq!(config["scopes"], json!(["files:read"]));
    crate::mcp_upstream_auth::validate_mcp_auth_config(
        gateway_core::ExternalMcpAuthMode::OauthObo,
        &config,
    )
    .expect("suggested config is valid");
}

#[tokio::test]
async fn falls_back_to_well_known_resource_metadata_and_oidc_discovery() {
    let origin = serve(|origin| {
        Router::new()
            // No challenge: the server answers unauthenticated requests.
            .route("/mcp", post(|| async { StatusCode::OK }))
            .route(
                "/.well-known/oauth-protected-resource",
                json_route(json!({
                    "resource": format!("{origin}/mcp"),
                    "authorization_servers": [origin.clone()],
                    "scopes_supported": ["openid", "tickets"],
                })),
            )
            .route(
                "/.well-known/openid-configuration",
                json_route(json!({
                    "issuer": format!("{origin}/"),
                    "authorization_endpoint": format!("{origin}/authorize"),
                    "token_endpoint": format!("{origin}/token"),
                    "code_challenge_methods_supported": ["S256"],
                    "client_id_metadata_document_supported": true,
                    "authorization_response_iss_parameter_supported": true,
                })),
            )
    })
    .await;

    let discovery = discover_mcp_oauth(&http(), &format!("{origin}/mcp"))
        .await
        .expect("discovery");

    assert_eq!(
        discovery.authorization_server_metadata_url,
        format!("{origin}/.well-known/openid-configuration")
    );
    assert_eq!(discovery.default_scopes, vec!["openid", "tickets"]);
    // Without a registration endpoint, a metadata document is the next choice.
    assert_eq!(
        discovery.recommended_client_registration,
        "metadata_document"
    );
    assert!(discovery.client_id_metadata_document_supported);
    // The issuer is the metadata's own string, trailing `/` included, since
    // callbacks compare `iss` with it exactly.
    assert_eq!(discovery.issuer, format!("{origin}/"));
    // RFC 8414 defaults to client_secret_basic when methods are not listed.
    assert_eq!(
        discovery.token_endpoint_auth_methods_supported,
        vec!["client_secret_basic"]
    );
    let config = discovery.suggested_auth_config();
    assert!(!config.contains_key("registration_endpoint"));
    assert!(!config.contains_key("token_endpoint_auth_method"));
    // RFC 9207 support carries into the config, so callbacks require `iss`.
    assert_eq!(
        config["authorization_response_iss_parameter_supported"],
        json!(true)
    );
    assert!(
        crate::mcp_upstream_auth::mcp_oauth_server_config(&config)
            .expect("suggested config parses")
            .server_oauth
            .expect("server OAuth")
            .authorization_response_iss_parameter_supported
    );
}

#[tokio::test]
async fn servers_without_registration_need_an_admin_client_id() {
    let origin = serve(|origin| {
        Router::new()
            .route("/mcp", post(|| async { StatusCode::UNAUTHORIZED }))
            // Pre-RFC 9728 server: the metadata lives on the server's own origin.
            .route(
                "/.well-known/oauth-authorization-server",
                json_route(json!({
                    "issuer": origin,
                    "authorization_endpoint": format!("{origin}/authorize"),
                    "token_endpoint": format!("{origin}/token"),
                })),
            )
    })
    .await;

    let discovery = discover_mcp_oauth(&http(), &format!("{origin}/mcp"))
        .await
        .expect("discovery");

    assert_eq!(discovery.recommended_client_registration, "manual");
    assert!(!discovery.resource_indicator);
    assert_eq!(discovery.resource, format!("{origin}/mcp"));
    assert_eq!(discovery.warnings.len(), 3, "{:?}", discovery.warnings);
    let config = discovery.suggested_auth_config();
    assert_eq!(config["client_registration"], "manual");
    assert_eq!(config["client_id"], "");
    assert_eq!(config["resource_indicator"], false);
}

#[tokio::test]
async fn rejects_metadata_for_another_issuer_or_without_s256() {
    for (issuer_suffix, methods, expected) in [
        ("/other", json!(["S256"]), "does not match issuer"),
        ("", json!(["plain"]), "S256"),
    ] {
        let origin = serve(move |origin| {
            Router::new()
                .route("/mcp", post(|| async { StatusCode::UNAUTHORIZED }))
                .route(
                    "/.well-known/oauth-protected-resource/mcp",
                    json_route(json!({
                        "resource": format!("{origin}/mcp"),
                        "authorization_servers": [origin.clone()],
                    })),
                )
                .route(
                    "/.well-known/oauth-authorization-server",
                    json_route(json!({
                        "issuer": format!("{origin}{issuer_suffix}"),
                        "authorization_endpoint": format!("{origin}/authorize"),
                        "token_endpoint": format!("{origin}/token"),
                        "code_challenge_methods_supported": methods,
                    })),
                )
        })
        .await;
        let error = discover_mcp_oauth(&http(), &format!("{origin}/mcp"))
            .await
            .expect_err("metadata must be rejected");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn remote_servers_cannot_point_discovery_at_loopback() {
    use crate::outbound_fetch::OutboundAddressScope;

    let url = |raw: &str| Url::parse(raw).expect("url");
    // A public server's fetcher refuses loopback and plain http.
    let remote = OutboundFetcher::new(OutboundAddressScope::Public);
    for (raw, reason) in [
        ("http://127.0.0.1:9000/meta", "https"),
        ("https://127.0.0.1:9000/meta", "not allowed"),
        ("https://[::ffff:127.0.0.1]/meta", "not allowed"),
        ("http://mcp.example.com/x", "https"),
    ] {
        let error = ensure_fetchable(&remote, &url(raw), "resource_metadata")
            .expect_err("refused")
            .to_string();
        assert!(
            error.contains("resource_metadata is not allowed"),
            "{raw}: {error}"
        );
        assert!(error.contains(reason), "{raw}: {error}");
    }
    // A loopback server, for local development, may use loopback over http.
    let local = OutboundFetcher::new(OutboundAddressScope::Loopback);
    ensure_fetchable(
        &local,
        &url("http://127.0.0.1:9000/meta"),
        "resource_metadata",
    )
    .expect("local development");
}

#[tokio::test]
async fn a_publicly_scoped_discovery_cannot_reach_internal_addresses() {
    // A public server's metadata naming an internal https host is refused at
    // the address, not just the scheme.
    let public = OutboundFetcher::new(crate::outbound_fetch::OutboundAddressScope::Public);
    for server in [
        "https://127.0.0.1:9/mcp",
        "https://10.0.0.5/mcp",
        "https://169.254.169.254/mcp",
    ] {
        let error = discover_mcp_oauth(&public, server)
            .await
            .expect_err("internal metadata must be refused");
        assert!(
            error.to_string().contains("address that is not allowed"),
            "{server}: {error}"
        );
    }
    let error = register_oauth_client(
        &public,
        "https://192.168.1.10/register",
        "https://oceans.example.com/api/v1/mcp/oauth/callback",
        McpOauthTokenEndpointAuthMethod::None,
        &[],
    )
    .await
    .expect_err("internal registration endpoint must be refused");
    assert!(error.to_string().contains("not allowed"), "{error}");
}

#[test]
fn prefers_confidential_token_endpoint_auth() {
    let methods = |values: &[&str]| values.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        choose_token_endpoint_auth_method(&methods(&["none", "client_secret_post"])),
        McpOauthTokenEndpointAuthMethod::ClientSecretPost
    );
    assert_eq!(
        choose_token_endpoint_auth_method(&methods(&["none"])),
        McpOauthTokenEndpointAuthMethod::None
    );
    assert_eq!(
        choose_token_endpoint_auth_method(&methods(&["private_key_jwt"])),
        McpOauthTokenEndpointAuthMethod::ClientSecretBasic
    );
}

#[tokio::test]
async fn dynamic_registration_issues_a_confidential_client() {
    let seen = Arc::new(Mutex::new(None::<Value>));
    let recorded = seen.clone();
    let origin = serve(move |_| {
        Router::new().route(
            "/register",
            post(move |Json(body): Json<Value>| {
                *recorded.lock().expect("request") = Some(body);
                async move {
                    (
                        StatusCode::CREATED,
                        Json(json!({
                            "client_id": "registered-client",
                            "client_secret": "registered-secret",
                            "client_secret_expires_at": 0,
                            "registration_access_token": "management-token",
                            "token_endpoint_auth_method": "client_secret_basic",
                        })),
                    )
                }
            }),
        )
    })
    .await;

    let client = register_oauth_client(
        &http(),
        &format!("{origin}/register"),
        "https://gateway.example/api/v1/mcp/oauth/callback",
        McpOauthTokenEndpointAuthMethod::ClientSecretBasic,
        &["files:read".to_string()],
    )
    .await
    .expect("registration");

    assert_eq!(client.client_id, "registered-client");
    assert_eq!(client.client_secret.as_deref(), Some("registered-secret"));
    assert_eq!(client.client_secret_expires_at, None);
    assert!(!format!("{client:?}").contains("registered-secret"));
    let request = seen.lock().expect("request").clone().expect("request body");
    assert_eq!(
        request["redirect_uris"],
        json!(["https://gateway.example/api/v1/mcp/oauth/callback"])
    );
    assert_eq!(
        request["grant_types"],
        json!(["authorization_code", "refresh_token"])
    );
    assert_eq!(request["token_endpoint_auth_method"], "client_secret_basic");
    assert_eq!(request["scope"], "files:read");
}

#[tokio::test]
async fn dynamic_registration_falls_back_to_a_public_client_and_reports_rejections() {
    let origin = serve(|_| {
        Router::new()
            .route(
                "/public",
                post(|| async { Json(json!({"client_id": "public-client"})) }),
            )
            .route(
                "/reject",
                post(|_headers: HeaderMap| async {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "invalid_redirect_uri",
                            "error_description": "private-detail",
                        })),
                    )
                }),
            )
            .route(
                "/no-secret",
                post(|| async {
                    Json(json!({
                        "client_id": "confidential",
                        "token_endpoint_auth_method": "client_secret_post",
                    }))
                }),
            )
    })
    .await;
    let register = |path: &'static str| {
        let origin = origin.clone();
        async move {
            register_oauth_client(
                &http(),
                &format!("{origin}{path}"),
                "https://gateway.example/callback",
                McpOauthTokenEndpointAuthMethod::ClientSecretBasic,
                &[],
            )
            .await
        }
    };

    let public = register("/public").await.expect("public client");
    assert_eq!(
        public.token_endpoint_auth_method,
        McpOauthTokenEndpointAuthMethod::None
    );
    assert_eq!(public.client_secret, None);

    let error = register("/reject").await.expect_err("rejected");
    assert!(error.to_string().contains("invalid_redirect_uri"));
    assert!(!error.to_string().contains("private-detail"));

    assert!(register("/no-secret").await.is_err());
}
