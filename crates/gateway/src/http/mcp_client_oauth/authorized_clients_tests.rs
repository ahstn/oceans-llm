//! Listing, disconnecting, and cleaning up authorized MCP clients.

use axum::{
    body::Body,
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, COOKIE},
    },
};
use gateway_core::{AuthMode, GatewayOauthRepository, GlobalRole, UserStatus};
use gateway_service::McpClientOauthService;
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::flow_tests::Harness;
use super::tests::send;

/// Creates an active user with a session and returns its cookie.
async fn session_cookie(harness: &Harness, email: &str, role: GlobalRole) -> String {
    let user = harness
        .state
        .store
        .create_identity_user(
            email,
            email,
            email,
            role,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let session_id = Uuid::new_v4();
    let token = format!("{session_id}.test-signature");
    let now = OffsetDateTime::now_utc();
    harness
        .state
        .store
        .create_user_session(
            session_id,
            user.user_id,
            &format!("{:x}", Sha256::digest(token.as_bytes())),
            now + Duration::days(1),
            now,
        )
        .await
        .expect("session");
    format!("ogw_session={token}")
}

async fn request(
    harness: &Harness,
    method: &str,
    uri: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(COOKIE, cookie);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    send(&harness.router, builder.body(body).unwrap()).await
}

async fn initialize_status(harness: &Harness, access_token: &str) -> StatusCode {
    let (status, _) = send(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    status
}

#[tokio::test]
async fn users_list_and_disconnect_their_authorized_clients() {
    let harness = Harness::new().await;
    let tokens = harness.issue_tokens(&[]).await;
    let access = tokens["access_token"].as_str().unwrap();
    assert_eq!(initialize_status(&harness, access).await, StatusCode::OK);

    let (status, body) = request(
        &harness,
        "GET",
        "/api/v1/me/authorized-mcp-clients",
        &harness.cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], true);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["client_name"], "Test Editor");
    assert_eq!(items[0]["client_id"], "editor");
    assert_eq!(items[0]["resource_kind"], "aggregate");
    assert_eq!(items[0]["scopes"][0]["scope"], "mcp:aggregate");
    let grant_id = items[0]["grant_id"].as_str().unwrap().to_string();

    // Another user can neither see nor revoke the grant.
    let other = session_cookie(&harness, "other@example.com", GlobalRole::User).await;
    let (_, body) = request(
        &harness,
        "GET",
        "/api/v1/me/authorized-mcp-clients",
        &other,
        None,
    )
    .await;
    assert_eq!(body["items"], json!([]));
    let uri = format!("/api/v1/me/authorized-mcp-clients/{grant_id}");
    let (status, _) = request(&harness, "DELETE", &uri, &other, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(initialize_status(&harness, access).await, StatusCode::OK);

    // Disconnecting stops the client's tokens and removes it from the list.
    let (status, body) = request(&harness, "DELETE", &uri, &harness.cookie, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revoked_count"], 1);
    assert_eq!(
        initialize_status(&harness, access).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        harness.grant_revoked_reason(access).await.as_deref(),
        Some("user_revoked")
    );
    let (_, body) = request(
        &harness,
        "GET",
        "/api/v1/me/authorized-mcp-clients",
        &harness.cookie,
        None,
    )
    .await;
    assert_eq!(body["items"], json!([]));
    let (status, _) = request(&harness, "DELETE", &uri, &harness.cookie, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn listing_requires_a_session_and_reports_a_disabled_feature() {
    let harness = Harness::new().await;
    let (status, _) = send(
        &harness.router,
        Request::builder()
            .uri("/api/v1/me/authorized-mcp-clients")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let mut state = harness.state.clone();
    state.mcp_client_oauth = None;
    let router = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    let (status, body) = send(
        &router,
        Request::builder()
            .uri("/api/v1/me/authorized-mcp-clients")
            .header(COOKIE, &harness.cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"enabled": false, "items": []}));
}

#[tokio::test]
async fn platform_admins_list_and_revoke_a_users_clients() {
    let harness = Harness::new().await;
    let first = harness.issue_tokens(&[]).await;
    let second = harness.issue_tokens(&[]).await;
    let admin = session_cookie(&harness, "admin@example.com", GlobalRole::PlatformAdmin).await;
    let list_uri = format!(
        "/api/v1/admin/mcp/authorized-clients?user_id={}",
        harness.user_id
    );

    let (status, _) = request(&harness, "GET", &list_uri, &harness.cookie, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = request(&harness, "GET", &list_uri, &admin, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // One grant, then the rest.
    let grant_id = items[0]["grant_id"].as_str().unwrap();
    let revoke = |grant_id: Option<&str>| json!({"user_id": harness.user_id, "grant_id": grant_id});
    let (status, body) = request(
        &harness,
        "POST",
        "/api/v1/admin/mcp/authorized-clients/revoke",
        &admin,
        Some(revoke(Some(grant_id))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revoked_count"], 1);
    let (_, body) = request(
        &harness,
        "POST",
        "/api/v1/admin/mcp/authorized-clients/revoke",
        &admin,
        Some(revoke(None)),
    )
    .await;
    assert_eq!(body["revoked_count"], 1);
    for tokens in [&first, &second] {
        let access = tokens["access_token"].as_str().unwrap();
        assert_eq!(
            initialize_status(&harness, access).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            harness.grant_revoked_reason(access).await.as_deref(),
            Some("admin_revoked")
        );
    }
    let (status, _) = request(
        &harness,
        "POST",
        "/api/v1/admin/mcp/authorized-clients/revoke",
        &harness.cookie,
        Some(revoke(None)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn cleanup_removes_registered_clients_that_never_got_a_grant() {
    let harness = Harness::new().await;
    harness.issue_tokens(&[]).await;
    let (status, _, registered) = harness
        .register(json!({
            "client_name": "Abandoned",
            "redirect_uris": ["http://127.0.0.1/callback"],
            "token_endpoint_auth_method": "none"
        }))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let abandoned = registered["client_id"].as_str().unwrap();
    let service = McpClientOauthService::new(
        harness.state.store.clone(),
        harness.state.mcp_client_oauth.clone().unwrap(),
    );

    // Within the retention window nothing is removed.
    let counts = service
        .purge_stale_records(OffsetDateTime::now_utc())
        .await
        .unwrap();
    assert_eq!(counts.clients, 0);

    let later = OffsetDateTime::now_utc() + Duration::days(8);
    let counts = service.purge_stale_records(later).await.unwrap();
    assert_eq!(counts.clients, 1);
    assert!(counts.tokens >= 1, "the expired access token");
    let store = &harness.state.store;
    assert!(
        store
            .get_gateway_oauth_client(abandoned)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_gateway_oauth_client("editor")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn grants_from_a_previous_issuer_are_not_listed() {
    let harness = Harness::new().await;
    harness.issue_tokens(&[]).await;
    let mut state = harness.state.clone();
    state.mcp_client_oauth = Some(std::sync::Arc::new(
        gateway_service::McpClientOauthSettings::new(
            "https://new-origin.example.com",
            3600,
            86400,
            true,
        )
        .unwrap(),
    ));
    let router = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    let (status, body) = send(
        &router,
        Request::builder()
            .uri("/api/v1/me/authorized-mcp-clients")
            .header(COOKIE, &harness.cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"enabled": true, "items": []}));
}

#[tokio::test]
async fn revocation_while_oauth_is_disabled_survives_re_enabling() {
    let harness = Harness::new().await;
    let first = harness.issue_tokens(&[]).await;
    let second = harness.issue_tokens(&[]).await;
    let grant_id = harness
        .state
        .store
        .list_live_gateway_oauth_grants_for_user(harness.user_id, OffsetDateTime::now_utc())
        .await
        .unwrap()
        .into_iter()
        .map(|grant| grant.grant_id)
        .next()
        .unwrap();
    let admin = session_cookie(&harness, "admin@example.com", GlobalRole::PlatformAdmin).await;

    // Turn the feature off; revocation must still be recorded.
    let mut disabled = harness.state.clone();
    disabled.mcp_client_oauth = None;
    let disabled_router = crate::http::build_router(disabled, admin_ui::AdminUiConfig::default());
    let (status, body) = send(
        &disabled_router,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/me/authorized-mcp-clients/{grant_id}"))
            .header(COOKIE, &harness.cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &disabled_router,
        Request::builder()
            .method("POST")
            .uri("/api/v1/admin/mcp/authorized-clients/revoke")
            .header(COOKIE, &admin)
            .header("content-type", "application/json")
            .body(Body::from(json!({"user_id": harness.user_id}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["revoked_count"], 1);

    // Back on, neither client's token works.
    for tokens in [&first, &second] {
        assert_eq!(
            initialize_status(&harness, tokens["access_token"].as_str().unwrap()).await,
            StatusCode::UNAUTHORIZED
        );
    }
}
