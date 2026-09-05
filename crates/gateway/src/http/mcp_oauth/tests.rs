use super::*;

#[test]
fn connection_redirect_is_limited_to_the_self_service_page() {
    assert_eq!(
        normalize_connection_redirect(Some("/admin/account/connections?from=drive")),
        "/admin/account/connections?from=drive"
    );
    assert_eq!(
        normalize_connection_redirect(Some("https://attacker.example")),
        DEFAULT_CONNECTION_REDIRECT
    );
    assert_eq!(
        normalize_connection_redirect(Some("//attacker.example")),
        DEFAULT_CONNECTION_REDIRECT
    );
    assert_eq!(
        normalize_connection_redirect(Some("/admin/account/connections-extra")),
        DEFAULT_CONNECTION_REDIRECT
    );
    assert_eq!(
        normalize_connection_redirect(Some("/admin/account/connections/child")),
        DEFAULT_CONNECTION_REDIRECT
    );
    assert_eq!(
        normalize_connection_redirect(Some("/admin/account/connections#fragment")),
        DEFAULT_CONNECTION_REDIRECT
    );
}

#[test]
fn connection_success_redirect_preserves_existing_query() {
    let response = connection_success_redirect("/admin/account/connections?from=drive");
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some("/admin/account/connections?from=drive&oauth=connected")
    );
}

#[test]
fn oauth_callback_requires_the_initiating_browser_session() {
    let initiator = Uuid::new_v4();
    assert!(callback_session_matches(Some(initiator), initiator));
    assert!(!callback_session_matches(Some(Uuid::new_v4()), initiator));
    assert!(!callback_session_matches(None, initiator));
}

#[test]
fn oauth_callback_rejects_expired_state_before_exchange() {
    let now = OffsetDateTime::now_utc();
    let user_id = Uuid::new_v4();
    let transaction = McpOauthStateRecord {
        state_hash: "state-hash".to_string(),
        user_id,
        mcp_server_id: Uuid::new_v4(),
        provider_key: "google".to_string(),
        pkce_verifier: "verifier".to_string(),
        redirect_to: DEFAULT_CONNECTION_REDIRECT.to_string(),
        resource: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
        scopes: vec!["https://www.googleapis.com/auth/drive.readonly".to_string()],
        expires_at: now - Duration::seconds(1),
        consumed_at: Some(now),
        created_at: now - Duration::minutes(MCP_OAUTH_STATE_TTL_MINUTES),
    };

    assert_eq!(
        callback_transaction_error(&transaction, "google", user_id, now),
        Some("state_expired")
    );
}

#[test]
fn refreshable_oauth_metadata_keeps_expired_access_tokens_connected() {
    let metadata = serde_json::Map::from_iter([
        ("oauth_bundle_version".to_string(), serde_json::json!(1)),
        (
            "oauth_provider_key".to_string(),
            serde_json::json!("google"),
        ),
        (
            "resource".to_string(),
            serde_json::json!("https://drivemcp.googleapis.com/mcp/v1"),
        ),
    ]);
    assert!(has_refreshable_oauth_metadata(&metadata));

    let legacy_metadata = serde_json::Map::new();
    assert!(!has_refreshable_oauth_metadata(&legacy_metadata));
}

#[tokio::test]
async fn oauth_start_persists_only_hashed_state_and_bound_pkce_transaction() {
    use gateway_core::{
        AuthMode, ExternalMcpTransport, GlobalRole, NewExternalMcpServerRecord, UserStatus,
    };
    use gateway_service::{McpOauthProvider, McpOauthRuntime};
    use std::sync::Arc;

    let (_directory, mut state) = crate::http::test_support::app_state().await;
    state.mcp_oauth_runtime = Arc::new(McpOauthRuntime::new(
        Some("https://gateway.example".to_string()),
        vec![McpOauthProvider {
            key: "google".to_string(),
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            authorization_url: "https://accounts.example/authorize".to_string(),
            token_url: "https://accounts.example/token".to_string(),
        }],
    ));
    let now = OffsetDateTime::now_utc();
    let user = state
        .store
        .create_identity_user(
            "Test User",
            "test@example.com",
            "test@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("session user");
    let session_id = Uuid::new_v4();
    let session_token = format!("{session_id}.test-signature");
    // Browser-session tokens use a hex digest; MCP OAuth state uses base64url.
    let session_hash = format!("{:x}", Sha256::digest(session_token.as_bytes()));
    state
        .store
        .create_user_session(
            session_id,
            user.user_id,
            &session_hash,
            now + Duration::hours(1),
            now,
        )
        .await
        .expect("browser session");
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        format!("ogw_session={session_token}")
            .parse()
            .expect("cookie"),
    );
    let server = state
        .store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "google_drive".to_string(),
            display_name: "Google Drive".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
            auth_mode: ExternalMcpAuthMode::OauthObo,
            auth_config: serde_json::from_value(serde_json::json!({
                "provider_key": "google",
                "resource": "https://drivemcp.googleapis.com/mcp/v1",
                "scopes": ["https://www.googleapis.com/auth/drive.readonly"]
            }))
            .expect("auth config"),
            timeout_ms: 30_000,
            created_at: now,
        })
        .await
        .expect("OAuth server");
    let Json(response) = start_mcp_oauth_connection(
        State(state.clone()),
        headers,
        Path(server.mcp_server_id),
        Json(McpOauthStartRequest {
            redirect_to: Some("/admin/account/connections?from=drive".to_string()),
        }),
    )
    .await
    .unwrap_or_else(|error| panic!("start authorization: {}", error.0));
    let url = url::Url::parse(&response.authorization_url).expect("authorization URL");
    let query = url.query_pairs().collect::<HashMap<_, _>>();
    let raw_state = query.get("state").expect("CSRF state");
    assert!(
        state
            .store
            .consume_mcp_oauth_state(raw_state, now)
            .await
            .expect("raw state lookup")
            .is_none(),
        "raw state must never be stored"
    );
    let expected_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(raw_state.as_bytes()));
    let transaction = state
        .store
        .consume_mcp_oauth_state(&expected_hash, now)
        .await
        .expect("hashed state lookup")
        .expect("persisted transaction");
    assert_eq!(transaction.state_hash, expected_hash);
    assert_eq!(transaction.user_id, user.user_id);
    assert_eq!(transaction.mcp_server_id, server.mcp_server_id);
    assert_eq!(transaction.provider_key, "google");
    assert_eq!(
        transaction.resource,
        "https://drivemcp.googleapis.com/mcp/v1"
    );
    assert_eq!(
        transaction.scopes,
        ["https://www.googleapis.com/auth/drive.readonly"]
    );
    assert_eq!(
        transaction.redirect_to,
        "/admin/account/connections?from=drive"
    );
    assert_eq!(
        transaction.expires_at - transaction.created_at,
        Duration::minutes(10)
    );
    assert_eq!(
        query.get("code_challenge_method").expect("PKCE method"),
        "S256"
    );
    assert_eq!(
        query
            .get("code_challenge")
            .expect("PKCE challenge")
            .as_ref(),
        URL_SAFE_NO_PAD.encode(Sha256::digest(transaction.pkce_verifier.as_bytes()))
    );
    assert!(
        state
            .store
            .consume_mcp_oauth_state(&expected_hash, now)
            .await
            .expect("replay lookup")
            .is_none(),
        "transaction must be single-use"
    );
}
