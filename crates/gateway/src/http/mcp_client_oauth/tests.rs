use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::WWW_AUTHENTICATE},
};
use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpTransport, McpRegistryRepository, NewExternalMcpServerRecord,
};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use tower::ServiceExt;

use super::*;
use crate::http::state::AppState;

pub(super) const ISSUER: &str = "https://oceans.example.com";

pub(super) fn oauth_settings() -> McpClientOauthSettings {
    McpClientOauthSettings::new(ISSUER, 3600, 86400, true).expect("settings")
}

pub(super) async fn oauth_state() -> (tempfile::TempDir, AppState) {
    oauth_state_with(oauth_settings()).await
}

pub(super) async fn oauth_state_with(
    settings: McpClientOauthSettings,
) -> (tempfile::TempDir, AppState) {
    let (directory, mut state) = crate::http::test_support::app_state().await;
    state.mcp_client_oauth = Some(Arc::new(settings));
    (directory, state)
}

pub(super) fn router(state: &AppState) -> Router {
    crate::http::build_router(state.clone(), admin_ui::AdminUiConfig::default())
}

pub(super) async fn seed_server(state: &AppState, server_key: &str, server_url: &str) {
    state
        .store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: server_key.to_string(),
            display_name: "Repository".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: server_url.to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .expect("server");
}

pub(super) async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let (status, _, body) = send_with_headers(router, request).await;
    (status, body)
}

pub(super) async fn send_with_headers(
    router: &Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, headers, body)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn mcp_post(uri: &str, authorization: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(authorization) = authorization {
        builder = builder.header("authorization", authorization);
    }
    builder
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        ))
        .unwrap()
}

#[tokio::test]
async fn aggregate_protected_resource_metadata_names_issuer_and_scope() {
    let (_directory, state) = oauth_state().await;
    let (status, body) = send(
        &router(&state),
        get("/.well-known/oauth-protected-resource/mcp"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resource"], "https://oceans.example.com/mcp");
    assert_eq!(
        body["authorization_servers"],
        serde_json::json!(["https://oceans.example.com"])
    );
    assert_eq!(
        body["scopes_supported"],
        serde_json::json!(["mcp:aggregate", "mcp:proxy"])
    );
    assert_eq!(
        body["bearer_methods_supported"],
        serde_json::json!(["header"])
    );
}

#[tokio::test]
async fn server_protected_resource_metadata_requires_an_active_server() {
    let (_directory, state) = oauth_state().await;
    seed_server(&state, "repository", "http://127.0.0.1:9/mcp").await;
    let router = router(&state);

    let (status, body) = send(
        &router,
        get("/.well-known/oauth-protected-resource/mcp/repository"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["resource"],
        "https://oceans.example.com/mcp/repository"
    );
    assert_eq!(body["scopes_supported"], serde_json::json!(["mcp:proxy"]));
    assert_eq!(body["resource_name"], "Repository");

    let (status, _) = send(
        &router,
        get("/.well-known/oauth-protected-resource/mcp/missing"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn authorization_server_metadata_advertises_pkce_and_iss() {
    let (_directory, state) = oauth_state().await;
    let (status, body) = send(
        &router(&state),
        get("/.well-known/oauth-authorization-server"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["issuer"], ISSUER);
    assert_eq!(
        body["authorization_endpoint"],
        "https://oceans.example.com/oauth/authorize"
    );
    assert_eq!(
        body["token_endpoint"],
        "https://oceans.example.com/oauth/token"
    );
    assert_eq!(
        body["code_challenge_methods_supported"],
        serde_json::json!(["S256"])
    );
    assert_eq!(body["authorization_response_iss_parameter_supported"], true);
    assert_eq!(
        body["scopes_supported"],
        serde_json::json!(["mcp:aggregate", "mcp:proxy"])
    );
}

#[tokio::test]
async fn metadata_is_not_served_when_oauth_is_disabled() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let router = router(&state);
    for uri in [
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-authorization-server",
    ] {
        let (status, _) = send(&router, get(uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn unauthenticated_mcp_requests_get_a_resource_metadata_challenge() {
    let (_directory, state) = oauth_state().await;
    let router = router(&state);

    let (status, headers, _) = send_with_headers(&router, mcp_post("/mcp", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers[WWW_AUTHENTICATE],
        r#"Bearer resource_metadata="https://oceans.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp:aggregate""#
    );

    let (status, headers, _) = send_with_headers(
        &router,
        mcp_post("/mcp/repository", Some("Bearer gwk_missing.secret")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers[WWW_AUTHENTICATE],
        r#"Bearer error="invalid_token", resource_metadata="https://oceans.example.com/.well-known/oauth-protected-resource/mcp/repository", scope="mcp:proxy""#
    );
}

#[tokio::test]
async fn mcp_challenge_stays_plain_when_oauth_is_disabled() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let (status, headers, _) = send_with_headers(&router(&state), mcp_post("/mcp", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers[WWW_AUTHENTICATE], "Bearer");
}

#[test]
fn insufficient_scope_challenge_names_the_missing_scope() {
    let challenge = bearer_challenge(
        &oauth_settings(),
        &McpOauthResource::Aggregate,
        &GatewayError::Auth(AuthError::OauthInsufficientScope {
            required: "mcp:aggregate".to_string(),
        }),
    )
    .expect("challenge");
    assert_eq!(
        challenge,
        r#"Bearer error="insufficient_scope", scope="mcp:aggregate", resource_metadata="https://oceans.example.com/.well-known/oauth-protected-resource/mcp""#
    );
    assert!(
        bearer_challenge(
            &oauth_settings(),
            &McpOauthResource::Aggregate,
            &GatewayError::Auth(AuthError::InsufficientPrivileges),
        )
        .is_none()
    );
}
