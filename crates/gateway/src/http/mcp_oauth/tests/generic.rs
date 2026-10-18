//! The authorization code flow and refresh for upstream servers whose
//! authorization server is configured on the server row, run against a mock
//! authorization server.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Form, Json, Router,
    extract::State as MockState,
    http::{HeaderMap as MockHeaders, StatusCode},
    routing::post,
};
use base64::engine::general_purpose::STANDARD;
use gateway_core::{
    ExternalMcpServerRecord, ExternalMcpTransport, McpUpstreamCredentialOwnerScopeKind,
    NewExternalMcpServerRecord,
};
use gateway_service::McpOauthRuntime;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::*;

const GATEWAY: &str = "https://gateway.example";
const ENCRYPTION_KEY_ENV: &str = "OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY";

#[derive(Default)]
struct MockAuthorizationServer {
    /// Whether token responses include a refresh token and lifetime.
    refreshable: bool,
    registrations: Vec<Value>,
    token_requests: Vec<(Option<String>, HashMap<String, String>)>,
}

type SharedMock = Arc<Mutex<MockAuthorizationServer>>;

async fn start_mock(refreshable: bool) -> (String, SharedMock) {
    let mock = Arc::new(Mutex::new(MockAuthorizationServer {
        refreshable,
        ..MockAuthorizationServer::default()
    }));
    let router = Router::new()
        .route("/register", post(register))
        .route("/token", post(token))
        .with_state(mock.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let origin = format!("http://{}", listener.local_addr().expect("mock address"));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve mock");
    });
    (origin, mock)
}

async fn register(
    MockState(mock): MockState<SharedMock>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    mock.lock().expect("mock").registrations.push(body);
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": "dcr-client",
            "client_secret": "dcr-secret",
            "client_secret_expires_at": 0,
            "token_endpoint_auth_method": "client_secret_basic",
        })),
    )
}

async fn token(
    MockState(mock): MockState<SharedMock>,
    headers: MockHeaders,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let mut mock = mock.lock().expect("mock");
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let refresh = form.get("grant_type").map(String::as_str) == Some("refresh_token");
    mock.token_requests.push((authorization, form));
    let count = mock.token_requests.len();
    if mock.refreshable {
        Json(json!({
            "access_token": format!("access-{count}"),
            "refresh_token": if refresh { format!("refresh-{count}") } else { "refresh-1".to_string() },
            "expires_in": 3600,
            "token_type": "Bearer",
            "scope": "files:read",
        }))
    } else {
        Json(json!({"access_token": "long-lived", "token_type": "bearer"}))
    }
}

fn configure(state: &mut AppState) {
    // Every test in this binary uses the same key, so setting it is idempotent.
    unsafe {
        std::env::set_var(ENCRYPTION_KEY_ENV, STANDARD.encode([7_u8; 32]));
    }
    state.mcp_oauth_runtime = Arc::new(McpOauthRuntime::new(Some(GATEWAY.to_string()), Vec::new()));
}

async fn server(state: &AppState, origin: &str, registration: Value) -> ExternalMcpServerRecord {
    let mut auth_config = json!({
        "resource": format!("{origin}/mcp"),
        "issuer": origin,
        "authorization_endpoint": format!("{origin}/authorize"),
        "token_endpoint": format!("{origin}/token"),
        "registration_endpoint": format!("{origin}/register"),
        "scopes": ["files:read"],
    });
    auth_config
        .as_object_mut()
        .expect("object")
        .extend(registration.as_object().expect("registration").clone());
    state
        .store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            ownership: Default::default(),
            server_key: format!("files_{}", &Uuid::new_v4().simple().to_string()[..8]),
            display_name: "Files".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: format!("{origin}/mcp"),
            auth_mode: ExternalMcpAuthMode::OauthObo,
            auth_config: serde_json::from_value(auth_config).expect("auth config"),
            timeout_ms: 30_000,
            created_at: OffsetDateTime::now_utc(),
            approve_tool_changes: false,
        })
        .await
        .expect("server")
}

async fn start(
    state: &AppState,
    headers: &HeaderMap,
    server: &ExternalMcpServerRecord,
) -> HashMap<String, String> {
    let Json(response) = start_mcp_oauth_connection(
        State(state.clone()),
        headers.clone(),
        Path(server.mcp_server_id),
        Json(McpOauthStartRequest { redirect_to: None }),
    )
    .await
    .unwrap_or_else(|error| panic!("start authorization: {}", error.0));
    url::Url::parse(&response.authorization_url)
        .expect("authorization URL")
        .query_pairs()
        .into_owned()
        .collect()
}

async fn callback(state: &AppState, headers: &HeaderMap, raw_state: &str) -> String {
    let response = mcp_oauth_server_callback(
        State(state.clone()),
        headers.clone(),
        crate::http::audit::AuditRequestMeta::default(),
        Query(McpOauthCallbackQuery {
            code: Some("auth-code".to_string()),
            state: Some(raw_state.to_string()),
            error: None,
        }),
    )
    .await
    .unwrap_or_else(|error| panic!("callback: {}", error.0));
    response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("redirect")
        .to_string()
}

async fn user_binding(
    state: &AppState,
    server: &ExternalMcpServerRecord,
    user_id: Uuid,
) -> gateway_core::McpUpstreamCredentialBindingRecord {
    let owner_scope_key = credential_owner_scope_key(
        McpUpstreamCredentialOwnerScopeKind::User,
        Some(user_id),
        None,
        None,
    )
    .expect("scope key");
    state
        .store
        .get_active_mcp_upstream_credential_binding(server.mcp_server_id, &owner_scope_key)
        .await
        .expect("binding lookup")
        .expect("binding")
}

#[tokio::test]
async fn dynamic_registration_code_flow_and_refresh() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure(&mut state);
    let (origin, mock) = start_mock(true).await;
    let server = server(&state, &origin, json!({"client_registration": "dynamic"})).await;
    let (user, headers) = oauth_session(&state).await;

    // Starting the flow registers the gateway once and uses the issued client.
    let query = start(&state, &headers, &server).await;
    let callback_url = format!("{GATEWAY}/api/v1/mcp/oauth/callback");
    assert_eq!(query["client_id"], "dcr-client");
    assert_eq!(query["redirect_uri"], callback_url);
    assert_eq!(query["scope"], "files:read");
    assert_eq!(query["resource"], format!("{origin}/mcp"));
    assert_eq!(query["code_challenge_method"], "S256");
    assert!(!query.contains_key("access_type"), "Google-only parameter");
    {
        let mock = mock.lock().expect("mock");
        assert_eq!(mock.registrations.len(), 1);
        assert_eq!(
            mock.registrations[0]["redirect_uris"],
            json!([callback_url])
        );
    }
    // The registered secret is stored encrypted.
    let stored = state
        .store
        .get_mcp_oauth_client(server.mcp_server_id)
        .await
        .expect("client lookup")
        .expect("registered client");
    assert_eq!(stored.client_id, "dcr-client");
    let ciphertext = stored.secret_ciphertext.expect("encrypted secret");
    assert!(!ciphertext.contains("dcr-secret"));

    // A second start reuses the stored client.
    let query = start(&state, &headers, &server).await;
    assert_eq!(mock.lock().expect("mock").registrations.len(), 1);

    let location = callback(&state, &headers, &query["state"]).await;
    assert_eq!(location, "/admin/account/connections?oauth=connected");
    let basic = format!("Basic {}", STANDARD.encode("dcr-client:dcr-secret"));
    {
        let mock = mock.lock().expect("mock");
        let (authorization, form) = mock.token_requests.last().expect("code exchange");
        assert_eq!(authorization.as_deref(), Some(basic.as_str()));
        assert_eq!(form["grant_type"], "authorization_code");
        assert_eq!(form["code"], "auth-code");
        assert_eq!(form["redirect_uri"], callback_url);
        assert_eq!(form["resource"], format!("{origin}/mcp"));
        assert!(!form.contains_key("client_secret"));
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())),
            query["code_challenge"]
        );
    }

    let connections = list_mcp_oauth_connections(State(state.clone()), headers.clone())
        .await
        .unwrap_or_else(|error| panic!("connections: {}", error.0));
    let body = axum::body::to_bytes(connections.into_body(), usize::MAX)
        .await
        .expect("body");
    let connections: Vec<Value> = serde_json::from_slice(&body).expect("connections JSON");
    let connection = connections
        .iter()
        .find(|item| item["server_key"] == server.server_key)
        .expect("generic connection is listed");
    assert_eq!(connection["status"], "connected");
    assert_eq!(connection["provider_key"], "mcp_server");
    assert_eq!(connection["provider_label"], "127.0.0.1");
    assert_eq!(connection["availability_error"], Value::Null);

    // Refresh runs through the same lease and compare-and-swap path as Google.
    let binding = user_binding(&state, &server, user.user_id).await;
    let grant = gateway_service::McpOauthTokenGrant {
        bundle: gateway_service::McpOauthTokenBundle {
            version: 1,
            access_token: "access-near-expiry".to_string(),
            refresh_token: Some("refresh-1".to_string()),
            token_type: "Bearer".to_string(),
            provider_key: MCP_SERVER_OAUTH_PROVIDER_KEY.to_string(),
            resource: format!("{origin}/mcp"),
            granted_scopes: vec!["files:read".to_string()],
            issuer: Some(origin.clone()),
        },
        expires_at: Some(OffsetDateTime::now_utc() + Duration::seconds(60)),
    };
    McpCredentialService::new(state.store.clone())
        .upsert_oauth_user_binding(server.mcp_server_id, user.user_id, grant, None)
        .await
        .expect("near-expiry binding");
    let near_expiry = user_binding(&state, &server, user.user_id).await;
    assert_eq!(
        near_expiry.credential_binding_id,
        binding.credential_binding_id
    );
    let refreshed = state
        .mcp_oauth_runtime
        .refresh_binding(state.store.as_ref(), near_expiry, &server)
        .await
        .expect("refresh");
    assert!(
        refreshed.expires_at.expect("expiry") > OffsetDateTime::now_utc() + Duration::minutes(30)
    );
    let mock = mock.lock().expect("mock");
    let (authorization, form) = mock.token_requests.last().expect("refresh request");
    assert_eq!(authorization.as_deref(), Some(basic.as_str()));
    assert_eq!(form["grant_type"], "refresh_token");
    assert_eq!(form["refresh_token"], "refresh-1");
    assert_eq!(form["resource"], format!("{origin}/mcp"));
    assert_eq!(mock.registrations.len(), 1, "refresh never re-registers");
}

#[tokio::test]
async fn metadata_document_clients_connect_as_public_clients() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure(&mut state);
    let (origin, mock) = start_mock(false).await;
    let server = server(
        &state,
        &origin,
        json!({"client_registration": "metadata_document", "resource_indicator": false}),
    )
    .await;
    let (user, headers) = oauth_session(&state).await;
    let document_url = format!("{GATEWAY}/api/v1/mcp/oauth/client-metadata.json");

    let response = mcp_oauth_client_metadata_document(State(state.clone()))
        .await
        .unwrap_or_else(|error| panic!("metadata document: {}", error.0));
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let document: Value = serde_json::from_slice(&body).expect("document JSON");
    assert_eq!(document["client_id"], document_url);
    assert_eq!(
        document["redirect_uris"],
        json!([format!("{GATEWAY}/api/v1/mcp/oauth/callback")])
    );
    assert_eq!(document["token_endpoint_auth_method"], "none");

    let query = start(&state, &headers, &server).await;
    assert_eq!(query["client_id"], document_url);
    assert!(!query.contains_key("resource"));
    let location = callback(&state, &headers, &query["state"]).await;
    assert_eq!(location, "/admin/account/connections?oauth=connected");
    {
        let mock = mock.lock().expect("mock");
        assert!(mock.registrations.is_empty());
        let (authorization, form) = mock.token_requests.last().expect("code exchange");
        assert_eq!(authorization, &None);
        assert_eq!(form["client_id"], document_url);
        assert!(!form.contains_key("client_secret"));
        assert!(!form.contains_key("resource"));
    }

    // A token with no refresh token and no lifetime does not expire.
    let binding = user_binding(&state, &server, user.user_id).await;
    assert_eq!(binding.expires_at, None);
    assert_eq!(binding.metadata["refreshable"], json!(false));
}

#[tokio::test]
async fn generic_servers_get_a_connect_url_and_their_own_callback() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure(&mut state);
    let (origin, _mock) = start_mock(true).await;
    let server = server(&state, &origin, json!({"client_registration": "dynamic"})).await;

    assert_eq!(
        connection_page_url(&state, &server).as_deref(),
        Some(
            format!(
                "{GATEWAY}/admin/account/connections?connect={}",
                server.server_key
            )
            .as_str()
        )
    );
    // Without a public origin the gateway has no callback to offer.
    let mut unconfigured = state.clone();
    unconfigured.mcp_oauth_runtime = Arc::new(McpOauthRuntime::new(None, Vec::new()));
    assert_eq!(connection_page_url(&unconfigured, &server), None);

    // A flow started for a configured provider cannot complete on the shared
    // server-level callback.
    let (user, headers) = oauth_session(&state).await;
    let raw_state = Uuid::new_v4().to_string();
    let now = OffsetDateTime::now_utc();
    state
        .store
        .create_mcp_oauth_state(&McpOauthStateRecord {
            state_hash: token_hash(&raw_state),
            user_id: user.user_id,
            mcp_server_id: server.mcp_server_id,
            provider_key: "google".to_string(),
            pkce_verifier: "verifier".to_string(),
            redirect_to: DEFAULT_CONNECTION_REDIRECT.to_string(),
            resource: server.server_url.clone(),
            scopes: Vec::new(),
            expires_at: now + Duration::minutes(10),
            consumed_at: None,
            created_at: now,
        })
        .await
        .expect("state");
    assert_eq!(
        callback(&state, &headers, &raw_state).await,
        "/admin/account/connections?oauth_error=state_invalid"
    );
}

#[tokio::test]
async fn manual_clients_use_the_admin_secret_and_configured_auth_method() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure(&mut state);
    unsafe {
        std::env::set_var("OCEANS_MCP_DISCOVERY_GENERIC_MANUAL_SECRET", "admin-secret");
    }
    let (origin, mock) = start_mock(true).await;
    let server = server(
        &state,
        &origin,
        json!({
            "client_registration": "manual",
            "client_id": "admin-client",
            "client_secret_ref": "env/OCEANS_MCP_DISCOVERY_GENERIC_MANUAL_SECRET",
            "token_endpoint_auth_method": "client_secret_post",
        }),
    )
    .await;
    let (_user, headers) = oauth_session(&state).await;

    let query = start(&state, &headers, &server).await;
    assert_eq!(query["client_id"], "admin-client");
    let location = callback(&state, &headers, &query["state"]).await;
    assert_eq!(location, "/admin/account/connections?oauth=connected");

    let mock = mock.lock().expect("mock");
    assert!(
        mock.registrations.is_empty(),
        "manual clients never register"
    );
    let (authorization, form) = mock.token_requests.last().expect("code exchange");
    assert_eq!(authorization, &None);
    assert_eq!(form["client_id"], "admin-client");
    assert_eq!(form["client_secret"], "admin-secret");
}
