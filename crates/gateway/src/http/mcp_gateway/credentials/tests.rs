use std::{sync::Arc, time::Instant};

use axum::{
    body::{Bytes, to_bytes},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey,
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpTransport, GatewayError, GlobalRole, McpAccessRepository,
    McpRegistryRepository, McpToolGrantSubjectKind, McpToolGrantTargetKind, NewApiKeyRecord,
    NewExternalMcpServerRecord, UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord, UserStatus,
};
use gateway_service::{McpOauthProvider, McpOauthRuntime};
use gateway_store::GatewayStore;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::credential_guidance;
use crate::http::{mcp_gateway::dispatch_mcp_request, state::AppState};

const CONNECT_URL: &str = "https://gateway.example/admin/account/connections?connect=drive";
const CLIENT_SECRET: &str = "provider-client-secret";
const KEY_PUBLIC_ID: &str = "drive-caller";

fn configure_oauth(state: &mut AppState, public_base_url: Option<&str>) {
    state.mcp_oauth_runtime = Arc::new(McpOauthRuntime::new(
        public_base_url.map(str::to_string),
        vec![McpOauthProvider {
            key: "google".to_string(),
            provider_type: gateway_service::McpOauthProviderType::Google,
            client_id: "provider-client".to_string(),
            client_secret: Some(CLIENT_SECRET.to_string()),
            authorization_url: "https://accounts.example/authorize".to_string(),
            token_url: "https://accounts.example/token".to_string(),
            token_endpoint_auth_method:
                gateway_service::McpOauthTokenEndpointAuthMethod::ClientSecretPost,
            scopes: Vec::new(),
        }],
    ));
}

fn oauth_config() -> Map<String, Value> {
    serde_json::from_value(json!({
        "provider_key": "google",
        "resource": "https://drive.example/mcp",
        "scopes": ["https://www.googleapis.com/auth/drive.readonly"],
    }))
    .expect("auth config")
}

/// Seeds a user API key with a grant on the one tool of server `drive`. The
/// server URL is never contacted because credential resolution fails first.
async fn seed_drive(
    state: &AppState,
    auth_mode: ExternalMcpAuthMode,
    auth_config: Map<String, Value>,
) -> (AuthenticatedApiKey, ExternalMcpServerRecord) {
    let store = &state.store;
    let now = OffsetDateTime::now_utc();
    let user = store
        .create_identity_user(
            "Drive caller",
            "drive@example.test",
            "drive@example.test",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let key = store
        .create_api_key(&NewApiKeyRecord {
            name: "Drive caller".to_string(),
            public_id: KEY_PUBLIC_ID.to_string(),
            secret_hash: "unused-secret-hash".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            access: Default::default(),
            created_at: now,
        })
        .await
        .expect("API key");
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "drive".to_string(),
            display_name: "Drive".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://drive.example/mcp".to_string(),
            auth_mode,
            auth_config,
            timeout_ms: 2_000,
            created_at: now,
            approve_tool_changes: false,
        })
        .await
        .expect("server");
    let tools = store
        .record_external_mcp_discovery_success(
            &ExternalMcpDiscoveryRunRecord {
                discovery_run_id: Uuid::new_v4(),
                mcp_server_id: server.mcp_server_id,
                status: ExternalMcpDiscoveryStatus::Success,
                started_at: now,
                finished_at: now,
                discovered_tool_count: 1,
                active_tool_count: 1,
                schema_set_hash: None,
                error_summary: None,
                details: Map::new(),
            },
            &[UpsertExternalMcpToolRecord {
                mcp_server_id: server.mcp_server_id,
                upstream_name: "search".to_string(),
                display_name: "Search".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
                metadata: gateway_core::McpToolMetadata::default(),
                schema_hash: "search-v1".to_string(),
            }],
        )
        .await
        .expect("tool discovery");
    store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: McpToolGrantSubjectKind::ApiKey,
            subject_id: key.id,
            target_kind: McpToolGrantTargetKind::Tool,
            target_id: tools[0].mcp_tool_id,
            updated_at: now,
        })
        .await
        .expect("tool grant");
    let auth = AuthenticatedApiKey {
        id: key.id,
        public_id: key.public_id,
        name: key.name,
        model_grant_mode: key.model_grant_mode,
        owner_kind: key.owner_kind,
        owner_user_id: key.owner_user_id,
        owner_team_id: key.owner_team_id,
        owner_service_account_id: key.owner_service_account_id,
        access: Default::default(),
        oauth: None,
    };
    (auth, server)
}

async fn send(state: &AppState, auth: &AuthenticatedApiKey, message: Value) -> (StatusCode, Value) {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let response = dispatch_mcp_request(
        state,
        &crate::http::request_policy::RequestPolicyContext::default(),
        auth,
        "drive",
        &Method::POST,
        &headers,
        Bytes::from(message.to_string()),
        Instant::now(),
    )
    .await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65_536).await.expect("body");
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains(CLIENT_SECRET), "response leaked a secret");
    assert!(!text.contains(KEY_PUBLIC_ID), "response leaked the API key");
    (status, serde_json::from_slice(&bytes).expect("JSON body"))
}

fn required() -> GatewayError {
    GatewayError::McpCredentialRequired {
        server_key: "drive".to_string(),
    }
}

#[tokio::test]
async fn direct_tools_call_returns_a_tool_error_with_the_connect_url() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_oauth(&mut state, Some("https://gateway.example"));
    let (auth, _) = seed_drive(&state, ExternalMcpAuthMode::OauthObo, oauth_config()).await;

    let (status, body) = send(
        &state,
        &auth,
        json!({"jsonrpc": "2.0", "id": "call-1", "method": "tools/call", "params": {"name": "search", "arguments": {}}}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], "call-1");
    let result = &body["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(
        result["content"][0]["text"],
        format!(
            "MCP credential is required for server `drive`. Connect your account at {CONNECT_URL}, then retry the call."
        )
    );
    assert_eq!(
        result["structuredContent"],
        json!({"server_key": "drive", "connect_url": CONNECT_URL, "error_code": "credential_required"})
    );
}

#[tokio::test]
async fn direct_non_tool_requests_return_the_connect_url_in_the_error() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_oauth(&mut state, Some("https://gateway.example"));
    let (auth, _) = seed_drive(&state, ExternalMcpAuthMode::OauthObo, oauth_config()).await;

    for message in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}),
    ] {
        let (status, body) = send(&state, &auth, message).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        let error = &body["error"];
        assert_eq!(error["code"], "credential_required");
        assert_eq!(error["type"], "authentication_error");
        assert_eq!(error["connect_url"], CONNECT_URL);
        assert!(error["message"].as_str().unwrap().ends_with(&format!(
            "Connect your account at {CONNECT_URL}, then retry the call."
        )));
    }
}

#[tokio::test]
async fn admin_managed_credentials_name_an_administrator_instead_of_a_url() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_oauth(&mut state, Some("https://gateway.example"));
    let (auth, _) = seed_drive(&state, ExternalMcpAuthMode::UserPassthrough, Map::new()).await;

    let (status, body) = send(
        &state,
        &auth,
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "search", "arguments": {}}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["result"]["content"][0]["text"],
        "MCP credential is required for server `drive`. Contact an Oceans administrator to set up a credential for this server."
    );
    assert!(
        body["result"]["structuredContent"]
            .get("connect_url")
            .is_none()
    );

    let (status, body) = send(
        &state,
        &auth,
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/list"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"].get("connect_url").is_none());
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Contact an Oceans administrator")
    );
}

#[tokio::test]
async fn expired_credentials_ask_the_user_to_reconnect() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_oauth(&mut state, Some("https://gateway.example"));
    let (auth, server) = seed_drive(&state, ExternalMcpAuthMode::OauthObo, oauth_config()).await;
    let expired = GatewayError::McpCredentialExpired {
        server_key: "drive".to_string(),
    };

    let guidance = credential_guidance(&state, &auth, &server, &expired).expect("guidance");

    assert_eq!(guidance.connect_url.as_deref(), Some(CONNECT_URL));
    assert_eq!(
        guidance.message,
        format!(
            "MCP credential for server `drive` is expired. Reconnect your account at {CONNECT_URL}, then retry the call."
        )
    );
}

#[tokio::test]
async fn connect_url_needs_a_user_key_and_a_reachable_connection_page() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_oauth(&mut state, Some("https://gateway.example"));
    let (auth, server) = seed_drive(&state, ExternalMcpAuthMode::OauthObo, oauth_config()).await;

    // A service account cannot use a person's connection, so an admin must bind one.
    let service_account = AuthenticatedApiKey {
        owner_kind: ApiKeyOwnerKind::ServiceAccount,
        owner_user_id: None,
        owner_service_account_id: Some(Uuid::new_v4()),
        ..auth.clone()
    };
    let guidance =
        credential_guidance(&state, &service_account, &server, &required()).expect("guidance");
    assert_eq!(guidance.connect_url, None);
    assert!(
        guidance
            .message
            .ends_with("Contact an Oceans administrator to set up a credential for this server.")
    );

    // Without a public base URL there is no page to link to.
    configure_oauth(&mut state, None);
    let guidance = credential_guidance(&state, &auth, &server, &required()).expect("guidance");
    assert_eq!(guidance.connect_url, None);

    // Other errors carry no credential guidance.
    assert!(
        credential_guidance(
            &state,
            &auth,
            &server,
            &GatewayError::InvalidRequest("bad".to_string())
        )
        .is_none()
    );
}
