//! Route tests for `/code-mode-mcp` using the deterministic test executor.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use admin_ui::AdminUiConfig;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode},
    routing::any,
};
use gateway_core::{
    McpToolInvocationQuery, McpToolInvocationRepository, McpUpstreamCredentialOwnerScopeKind,
    McpUpstreamCredentialRepository, parse_gateway_api_key,
};
use gateway_service::{
    CodeExecutor, CodeModeLimits, DeterministicTestExecutor, GatewayService, WeightedRoutePlanner,
    hash_gateway_key_secret,
};
use gateway_store::{AnyStore, LibsqlStore, run_migrations};
use serde_json::{Map, Value, json};
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::{
    build_router,
    state::{AppState, CodeModeState},
};

const RAW_KEY: &str = "gwk_codemode1.code-mode-secret";
const OTHER_RAW_KEY: &str = "gwk_codemode2.other-secret";

struct TestApp {
    app: Router,
    store: Arc<AnyStore>,
    db_path: PathBuf,
}

async fn build_code_mode_test_app(enabled: bool, limits: CodeModeLimits) -> TestApp {
    build_code_mode_test_app_with_executor(enabled, limits, Arc::new(DeterministicTestExecutor))
        .await
}

async fn build_code_mode_test_app_with_executor(
    enabled: bool,
    limits: CodeModeLimits,
    executor: Arc<dyn CodeExecutor>,
) -> TestApp {
    let tmp = tempdir().expect("tempdir");
    let tmp_path = tmp.keep();
    let db_path = tmp_path.join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");

    let store = Arc::new(AnyStore::Libsql(
        LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store"),
    ));
    insert_user_api_key(&db_path, RAW_KEY).await;
    insert_user_api_key(&db_path, OTHER_RAW_KEY).await;

    let service = Arc::new(GatewayService::new(
        store.clone(),
        Arc::new(WeightedRoutePlanner::seeded(11)),
    ));
    let app = build_router(
        AppState {
            service,
            store: store.clone(),
            providers: gateway_core::ProviderRegistry::new(),
            metrics: Arc::new(crate::observability::GatewayMetrics::default()),
            mcp_http_client: reqwest::Client::new(),
            identity_token_secret: Arc::new("local-dev-identity-secret".to_string()),
            oidc_public_base_url: Arc::new(None),
            oauth_public_base_url: Arc::new(None),
            code_mode: CodeModeState {
                enabled,
                executor,
                limits,
            },
        },
        AdminUiConfig::default(),
    );

    TestApp {
        app,
        store,
        db_path,
    }
}

async fn libsql_connection(db_path: &Path) -> libsql::Connection {
    let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
        .build()
        .await
        .expect("libsql db");
    db.connect().expect("libsql connection")
}

async fn insert_user_api_key(db_path: &Path, raw_key: &str) -> Uuid {
    let parsed = parse_gateway_api_key(raw_key).expect("parse key");
    let connection = libsql_connection(db_path).await;
    let user_id = Uuid::new_v4();
    connection
        .execute(
            r#"
            INSERT INTO users (
                user_id, name, email, email_normalized, global_role, auth_mode, status,
                request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, 'user', 'password', 'active', 1, 'all', unixepoch(), unixepoch())
            "#,
            libsql::params![
                user_id.to_string(),
                "Code Mode Test User",
                format!("{}@example.com", user_id.simple()),
                format!("{}@example.com", user_id.simple()),
            ],
        )
        .await
        .expect("insert user");
    connection
        .execute(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status, owner_kind, owner_user_id, created_at
            ) VALUES (?1, ?2, ?3, 'code-mode', 'active', 'user', ?4, unixepoch())
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                parsed.public_id,
                hash_gateway_key_secret(&parsed.secret).expect("hash"),
                user_id.to_string(),
            ],
        )
        .await
        .expect("insert api key");
    user_id
}

async fn insert_mcp_server(
    db_path: &Path,
    server_key: &str,
    server_url: &str,
    auth_mode: &str,
) -> Uuid {
    let connection = libsql_connection(db_path).await;
    let server_id = Uuid::new_v4();
    connection
        .execute(
            r#"
            INSERT INTO external_mcp_servers (
                mcp_server_id, server_key, display_name, description, transport, server_url,
                auth_mode, auth_config_json, timeout_ms, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, NULL, 'streamable_http', ?4, ?5, '{}', 30000, 'active', unixepoch(), unixepoch())
            "#,
            libsql::params![
                server_id.to_string(),
                server_key,
                server_key,
                server_url,
                auth_mode,
            ],
        )
        .await
        .expect("insert mcp server");
    server_id
}

async fn insert_mcp_tool(
    db_path: &Path,
    server_id: Uuid,
    upstream_name: &str,
    schema_hash: &str,
) -> Uuid {
    let connection = libsql_connection(db_path).await;
    let tool_id = Uuid::new_v4();
    connection
        .execute(
            r#"
            INSERT INTO external_mcp_tools (
                mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                input_schema_json, schema_hash, schema_version, is_active,
                first_discovered_at, last_discovered_at
            ) VALUES (?1, ?2, ?3, ?3, 'test tool', '{"type":"object"}', ?4, 1, 1, unixepoch(), unixepoch())
            "#,
            libsql::params![
                tool_id.to_string(),
                server_id.to_string(),
                upstream_name,
                schema_hash,
            ],
        )
        .await
        .expect("insert mcp tool");
    tool_id
}

async fn grant_tool_to_user(db_path: &Path, user_id: Uuid, tool_id: Uuid) {
    let connection = libsql_connection(db_path).await;
    connection
        .execute(
            r#"
            INSERT INTO mcp_tool_grants (
                grant_id, subject_kind, subject_id, target_kind, target_id, is_active,
                created_at, updated_at
            ) VALUES (?1, 'user', ?2, 'tool', ?3, 1, unixepoch(), unixepoch())
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                user_id.to_string(),
                tool_id.to_string(),
            ],
        )
        .await
        .expect("insert grant");
}

fn rpc_request(raw_key: &str, session: Option<&str>, body: Value) -> Request<Body> {
    rpc_request_at("/code-mode-mcp", raw_key, session, body)
}

fn rpc_request_at(uri: &str, raw_key: &str, session: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {raw_key}"))
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder.header("mcp-session-id", session);
    }
    builder.body(Body::from(body.to_string())).expect("request")
}

async fn json_body(response: Response<Body>) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&body).expect("json body")
}

async fn establish_session(app: &Router, raw_key: &str) -> String {
    establish_session_at(app, "/code-mode-mcp", raw_key).await
}

async fn establish_session_at(app: &Router, uri: &str, raw_key: &str) -> String {
    let response = app
        .clone()
        .oneshot(rpc_request_at(
            uri,
            raw_key,
            None,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}
            }),
        ))
        .await
        .expect("initialize response");
    assert_eq!(response.status(), StatusCode::OK);
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("session header")
        .to_string();
    let notified = app
        .clone()
        .oneshot(rpc_request_at(
            uri,
            raw_key,
            Some(&session),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ))
        .await
        .expect("initialized notification");
    assert_eq!(notified.status(), StatusCode::ACCEPTED);
    session
}

/// Sends a `tools/call`. JSON scripts (deterministic executor) are
/// serialized; raw strings (real-backend JavaScript) pass through as-is.
async fn call_tool(
    app: &Router,
    raw_key: &str,
    session: &str,
    tool: &str,
    code: Value,
) -> Response<Body> {
    let code = match code {
        Value::String(code) => code,
        script => script.to_string(),
    };
    app.clone()
        .oneshot(rpc_request(
            raw_key,
            Some(session),
            json!({
                "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": {"name": tool, "arguments": {"code": code}}
            }),
        ))
        .await
        .expect("tools/call response")
}

async fn list_code_mode_invocations(
    store: &AnyStore,
    tool_display_key: &str,
) -> Vec<gateway_core::McpToolInvocationRecord> {
    store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 50,
            server_display_key: Some("code-mode".to_string()),
            tool_display_key: Some(tool_display_key.to_string()),
            ..Default::default()
        })
        .await
        .expect("list invocations")
        .items
}

/// Mock upstream MCP server replying to initialize, notifications, and
/// tools/call over Streamable HTTP.
async fn spawn_mock_upstream() -> String {
    let upstream = Router::new().route(
        "/mcp",
        any(|body: String| async move {
            let request: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let method = request["method"].as_str().unwrap_or_default();
            let id = request["id"].clone();
            let payload = match method {
                "initialize" => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "mock", "version": "1"}
                    }
                }),
                "notifications/initialized" => {
                    return Response::builder()
                        .status(StatusCode::ACCEPTED)
                        .body(Body::empty())
                        .expect("accepted");
                }
                "tools/call" => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {
                        "content": [{"type": "text", "text": "issue created"}],
                        "isError": false
                    }
                }),
                _ => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            };
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .expect("response")
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });
    format!("http://{addr}/mcp")
}

#[tokio::test]
async fn code_mode_route_returns_404_when_disabled() {
    let test = build_code_mode_test_app(false, CodeModeLimits::default()).await;
    for method in ["POST", "GET", "DELETE"] {
        let response = test
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/code-mode-mcp")
                    .header("authorization", format!("Bearer {RAW_KEY}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "method {method}");
    }
}

#[tokio::test]
async fn code_mode_route_rejects_get_and_query_strings_when_enabled() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;

    let get_response = test
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/code-mode-mcp")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(get_response.status(), StatusCode::METHOD_NOT_ALLOWED);

    let query_response = test
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/code-mode-mcp?token=x")
                .header("authorization", format!("Bearer {RAW_KEY}"))
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(query_response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn code_mode_route_requires_authentication() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let response = test
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/code-mode-mcp")
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn code_mode_sessions_are_bound_to_the_authenticated_key() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;

    let cross_principal = test
        .app
        .clone()
        .oneshot(rpc_request(
            OTHER_RAW_KEY,
            Some(&session),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ))
        .await
        .expect("response");
    assert_eq!(cross_principal.status(), StatusCode::NOT_FOUND);

    let same_principal = test
        .app
        .clone()
        .oneshot(rpc_request(
            RAW_KEY,
            Some(&session),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ))
        .await
        .expect("response");
    assert_eq!(same_principal.status(), StatusCode::OK);
}

/// Sessions are bound to the surface they were initialized against: an
/// aggregate `/mcp` session must not validate at `/code-mode-mcp` (and vice
/// versa). The mismatch is indistinguishable from "not found".
#[tokio::test]
async fn sessions_are_not_shared_between_mcp_and_code_mode_surfaces() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;

    let aggregate_session = establish_session_at(&test.app, "/mcp", RAW_KEY).await;
    let cross_surface = test
        .app
        .clone()
        .oneshot(rpc_request_at(
            "/code-mode-mcp",
            RAW_KEY,
            Some(&aggregate_session),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ))
        .await
        .expect("response");
    assert_eq!(cross_surface.status(), StatusCode::NOT_FOUND);

    let code_mode_session = establish_session_at(&test.app, "/code-mode-mcp", RAW_KEY).await;
    let reverse = test
        .app
        .clone()
        .oneshot(rpc_request_at(
            "/mcp",
            RAW_KEY,
            Some(&code_mode_session),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ))
        .await
        .expect("response");
    assert_eq!(reverse.status(), StatusCode::NOT_FOUND);

    // Each session still works at its own surface.
    let same_surface = test
        .app
        .clone()
        .oneshot(rpc_request_at(
            "/code-mode-mcp",
            RAW_KEY,
            Some(&code_mode_session),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        ))
        .await
        .expect("response");
    assert_eq!(same_surface.status(), StatusCode::OK);
}

#[tokio::test]
async fn code_mode_tools_list_exposes_exactly_explore_and_execute() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = test
        .app
        .clone()
        .oneshot(rpc_request(
            RAW_KEY,
            Some(&session),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    let names = payload["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["explore", "execute"]);
}

#[tokio::test]
async fn code_mode_rejects_unknown_tools() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = test
        .app
        .clone()
        .oneshot(rpc_request(
            RAW_KEY,
            Some(&session),
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "call_tool", "arguments": {}}
            }),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn code_mode_missing_code_is_invalid_request_with_parent_log() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = test
        .app
        .clone()
        .oneshot(rpc_request(
            RAW_KEY,
            Some(&session),
            json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "explore", "arguments": {}}
            }),
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let rows = list_code_mode_invocations(&test.store, "explore").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status,
        gateway_core::McpToolInvocationStatus::InvalidRequest
    );
    assert_eq!(rows[0].error_code.as_deref(), Some("invalid_request"));
}

#[tokio::test]
async fn explore_only_sees_granted_tools_and_denies_ungranted_describe() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let user_id = insert_user_api_key(&test.db_path, "gwk_codemode3.grantee").await;
    let granted_server =
        insert_mcp_server(&test.db_path, "github", "https://granted.test/mcp", "none").await;
    let granted_tool = insert_mcp_tool(
        &test.db_path,
        granted_server,
        "issues_create",
        "sha256:granted",
    )
    .await;
    let hidden_server =
        insert_mcp_server(&test.db_path, "secrets", "https://hidden.test/mcp", "none").await;
    insert_mcp_tool(&test.db_path, hidden_server, "vault_read", "sha256:hidden").await;
    grant_tool_to_user(&test.db_path, user_id, granted_tool).await;

    let session = establish_session(&test.app, "gwk_codemode3.grantee").await;
    let response = call_tool(
        &test.app,
        "gwk_codemode3.grantee",
        &session,
        "explore",
        json!({
            "calls": [
                {"name": "searchTools", "args": {}},
                {"name": "describeTool", "args": {"address": "mcp://secrets/tools/vault_read"}}
            ]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    let envelopes = payload["result"]["structuredContent"]["result"]
        .as_array()
        .expect("envelopes");

    let search = &envelopes[0]["result"];
    assert_eq!(search["total"], json!(1));
    let items = search["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["server"]["server_key"], json!("github"));
    let rendered = serde_json::to_string(&payload).expect("rendered");
    assert!(!rendered.contains("vault_read"));
    assert!(!rendered.contains("sha256:hidden"));

    let denied = envelopes[1]["error"].as_str().expect("describe error");
    assert!(denied.contains("not granted"));
    assert!(!denied.contains("vault"));
}

#[tokio::test]
async fn explore_cannot_call_tools_and_logs_policy_denied() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = call_tool(
        &test.app,
        RAW_KEY,
        &session,
        "explore",
        json!({
            "fail_on_error": true,
            "calls": [{"name": "callTool", "args": {"address": "mcp://github/tools/x"}}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(true));
    assert!(
        payload["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("not available in this capability profile")
    );

    let rows = list_code_mode_invocations(&test.store, "explore").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status,
        gateway_core::McpToolInvocationStatus::PolicyDenied
    );
    assert_eq!(rows[0].error_code.as_deref(), Some("capability_denied"));
    assert_eq!(
        rows[0].policy_result,
        gateway_core::McpToolPolicyResult::Denied
    );
}

/// W7 attribution: a capability denial that the code catches, followed by
/// an unrelated failure, must be audited as `gateway_error`, not
/// `policy_denied`.
#[tokio::test]
async fn caught_capability_denial_with_unrelated_failure_is_not_policy_denied() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = call_tool(
        &test.app,
        RAW_KEY,
        &session,
        "explore",
        json!({
            // The denied callTool is "caught" (fail_on_error is false); the
            // execution then fails for an unrelated reason.
            "calls": [{"name": "callTool", "args": {"address": "mcp://github/tools/x"}}],
            "fail": "ReferenceError: unrelated is not defined"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(true));

    let rows = list_code_mode_invocations(&test.store, "explore").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status,
        gateway_core::McpToolInvocationStatus::GatewayError
    );
    assert_eq!(rows[0].error_code.as_deref(), Some("code_execution_error"));
    assert_eq!(
        rows[0].policy_result,
        gateway_core::McpToolPolicyResult::Allowed
    );
}

#[tokio::test]
async fn execute_runs_nested_calls_and_links_invocations() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let upstream_url = spawn_mock_upstream().await;
    let user_id = insert_user_api_key(&test.db_path, "gwk_codemode4.exec").await;
    let server_id = insert_mcp_server(&test.db_path, "github", &upstream_url, "none").await;
    let tool_id = insert_mcp_tool(&test.db_path, server_id, "issues_create", "sha256:ok").await;
    grant_tool_to_user(&test.db_path, user_id, tool_id).await;

    let session = establish_session(&test.app, "gwk_codemode4.exec").await;
    let response = call_tool(
        &test.app,
        "gwk_codemode4.exec",
        &session,
        "execute",
        json!({
            "calls": [{
                "name": "callTool",
                "args": {
                    "address": "mcp://github/tools/issues_create",
                    "arguments": {"title": "Bug", "token": "super-secret-value"},
                    "schema_hash": "sha256:ok"
                }
            }]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(false));
    let envelopes = payload["result"]["structuredContent"]["result"]
        .as_array()
        .expect("envelopes");
    assert_eq!(
        envelopes[0]["result"]["content"][0]["text"],
        json!("issue created")
    );

    let parents = list_code_mode_invocations(&test.store, "execute").await;
    assert_eq!(parents.len(), 1);
    assert_eq!(
        parents[0].status,
        gateway_core::McpToolInvocationStatus::Success
    );
    assert_eq!(parents[0].parent_invocation_id, None);

    let children = test
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 50,
            parent_invocation_id: Some(parents[0].mcp_tool_invocation_id),
            ..Default::default()
        })
        .await
        .expect("list children")
        .items;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].server_display_key, "github");
    assert_eq!(children[0].tool_display_key, "issues_create");
    assert_eq!(
        children[0].status,
        gateway_core::McpToolInvocationStatus::Success
    );

    // Nested payloads are captured with redaction applied.
    let detail = test
        .store
        .get_mcp_tool_invocation_detail(children[0].mcp_tool_invocation_id)
        .await
        .expect("child detail");
    let arguments = detail.payload.expect("payload").arguments_json;
    assert_eq!(arguments["token"], json!("[REDACTED]"));
    assert_eq!(arguments["title"], json!("Bug"));

    // Documented parent-payload behavior: the submitted `code` string is
    // stored as-submitted. Key-based redaction runs over the parent payload
    // like every other invocation payload, but it cannot scrub secrets
    // embedded inside the opaque code string — callers must not put secrets
    // in code, and operators can disable capture via the payload policy.
    let parent_detail = test
        .store
        .get_mcp_tool_invocation_detail(parents[0].mcp_tool_invocation_id)
        .await
        .expect("parent detail");
    assert!(parent_detail.invocation.arguments_payload_redacted);
    assert!(!parent_detail.invocation.arguments_payload_truncated);
    let parent_code = parent_detail.payload.expect("parent payload").arguments_json["code"]
        .as_str()
        .expect("code string")
        .to_string();
    assert!(parent_code.contains("super-secret-value"));
}

#[tokio::test]
async fn execute_denies_calls_to_ungranted_tools_without_leaking() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    insert_user_api_key(&test.db_path, "gwk_codemode7.deny").await;
    let server_id =
        insert_mcp_server(&test.db_path, "github", "https://unused.test/mcp", "none").await;
    insert_mcp_tool(&test.db_path, server_id, "issues_create", "sha256:ok").await;
    // No grant for this user.

    let session = establish_session(&test.app, "gwk_codemode7.deny").await;
    let response = call_tool(
        &test.app,
        "gwk_codemode7.deny",
        &session,
        "execute",
        json!({
            "calls": [{
                "name": "callTool",
                "args": {"address": "mcp://github/tools/issues_create"}
            }]
        }),
    )
    .await;
    let payload = json_body(response).await;
    let denied = payload["result"]["structuredContent"]["result"][0]["error"]
        .as_str()
        .expect("denied error");
    assert!(denied.contains("not granted"));
    // Schema and other tool details must not leak through the error.
    assert!(!denied.contains("sha256:ok"));
}

#[tokio::test]
async fn execute_disambiguates_duplicate_upstream_tool_names_by_server_key() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let upstream_url = spawn_mock_upstream().await;
    let user_id = insert_user_api_key(&test.db_path, "gwk_codemode8.dupe").await;
    let granted_server = insert_mcp_server(&test.db_path, "github", &upstream_url, "none").await;
    let granted_tool = insert_mcp_tool(
        &test.db_path,
        granted_server,
        "create_issue",
        "sha256:github",
    )
    .await;
    let other_server = insert_mcp_server(&test.db_path, "gitlab", &upstream_url, "none").await;
    insert_mcp_tool(&test.db_path, other_server, "create_issue", "sha256:gitlab").await;
    grant_tool_to_user(&test.db_path, user_id, granted_tool).await;

    let session = establish_session(&test.app, "gwk_codemode8.dupe").await;
    let response = call_tool(
        &test.app,
        "gwk_codemode8.dupe",
        &session,
        "execute",
        json!({
            "calls": [
                {"name": "callTool", "args": {"address": "mcp://github/tools/create_issue"}},
                {"name": "callTool", "args": {"address": "mcp://gitlab/tools/create_issue"}}
            ]
        }),
    )
    .await;
    let payload = json_body(response).await;
    let envelopes = payload["result"]["structuredContent"]["result"]
        .as_array()
        .expect("envelopes");
    // Granted server resolves; same tool name on the ungranted server is denied.
    assert_eq!(
        envelopes[0]["result"]["content"][0]["text"],
        json!("issue created")
    );
    assert!(
        envelopes[1]["error"]
            .as_str()
            .expect("denied")
            .contains("not granted")
    );

    let parents = list_code_mode_invocations(&test.store, "execute").await;
    let children = test
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 10,
            parent_invocation_id: Some(parents[0].mcp_tool_invocation_id),
            ..Default::default()
        })
        .await
        .expect("children")
        .items;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].server_display_key, "github");
}

#[tokio::test]
async fn execute_schema_hash_mismatch_surfaces_tool_schema_changed() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let user_id = insert_user_api_key(&test.db_path, "gwk_codemode5.hash").await;
    let server_id =
        insert_mcp_server(&test.db_path, "github", "https://unused.test/mcp", "none").await;
    let tool_id = insert_mcp_tool(&test.db_path, server_id, "issues_create", "sha256:new").await;
    grant_tool_to_user(&test.db_path, user_id, tool_id).await;

    let session = establish_session(&test.app, "gwk_codemode5.hash").await;
    let response = call_tool(
        &test.app,
        "gwk_codemode5.hash",
        &session,
        "execute",
        json!({
            "calls": [{
                "name": "callTool",
                "args": {
                    "address": "mcp://github/tools/issues_create",
                    "schema_hash": "sha256:stale"
                }
            }]
        }),
    )
    .await;
    let payload = json_body(response).await;
    let envelope = &payload["result"]["structuredContent"]["result"][0]["result"];
    assert_eq!(envelope["isError"], json!(true));
    assert_eq!(
        envelope["structuredContent"]["error_code"],
        json!("tool_schema_changed")
    );
    assert_eq!(
        envelope["structuredContent"]["actual_schema_hash"],
        json!("sha256:new")
    );
}

#[tokio::test]
async fn execute_missing_and_expired_credentials_pass_through_envelopes() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let user_id = insert_user_api_key(&test.db_path, "gwk_codemode6.cred").await;
    let server_id = insert_mcp_server(
        &test.db_path,
        "github",
        "https://credentialed.test/mcp",
        "user_passthrough",
    )
    .await;
    let tool_id = insert_mcp_tool(&test.db_path, server_id, "issues_create", "sha256:ok").await;
    grant_tool_to_user(&test.db_path, user_id, tool_id).await;

    let session = establish_session(&test.app, "gwk_codemode6.cred").await;
    let call = json!({
        "calls": [{
            "name": "callTool",
            "args": {"address": "mcp://github/tools/issues_create"}
        }]
    });

    let missing = json_body(
        call_tool(
            &test.app,
            "gwk_codemode6.cred",
            &session,
            "execute",
            call.clone(),
        )
        .await,
    )
    .await;
    let missing_envelope = &missing["result"]["structuredContent"]["result"][0]["result"];
    assert_eq!(
        missing_envelope["structuredContent"]["error_code"],
        json!("credential_required")
    );

    test.store
        .upsert_mcp_upstream_credential_binding(
            &gateway_core::UpsertMcpUpstreamCredentialBindingRecord {
                credential_binding_id: None,
                mcp_server_id: server_id,
                owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::User,
                owner_scope_key: format!("mcp_credential:v1:user:{user_id}"),
                owner_user_id: Some(user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                material_kind: gateway_core::McpUpstreamCredentialMaterialKind::BearerToken,
                header_name: None,
                storage_kind: gateway_core::McpUpstreamSecretStorageKind::SecretRef,
                secret_ciphertext: None,
                secret_nonce: None,
                secret_key_id: None,
                secret_ref: Some("env/OCEANS_MCP_CREDENTIAL_CODE_MODE_TEST".to_string()),
                expires_at: Some(OffsetDateTime::now_utc() - Duration::hours(1)),
                metadata: Map::new(),
                updated_at: OffsetDateTime::now_utc(),
            },
        )
        .await
        .expect("insert expired binding");

    let expired =
        json_body(call_tool(&test.app, "gwk_codemode6.cred", &session, "execute", call).await)
            .await;
    let expired_envelope = &expired["result"]["structuredContent"]["result"][0]["result"];
    assert_eq!(
        expired_envelope["structuredContent"]["error_code"],
        json!("credential_expired")
    );

    // Credential failures are logged as unauthorized nested invocations
    // linked to their execute parents.
    let parents = list_code_mode_invocations(&test.store, "execute").await;
    assert_eq!(parents.len(), 2);
    for parent in &parents {
        let children = test
            .store
            .list_mcp_tool_invocations(&McpToolInvocationQuery {
                page: 1,
                page_size: 10,
                parent_invocation_id: Some(parent.mcp_tool_invocation_id),
                ..Default::default()
            })
            .await
            .expect("children")
            .items;
        assert_eq!(children.len(), 1);
        assert_eq!(
            children[0].status,
            gateway_core::McpToolInvocationStatus::Unauthorized
        );
    }
}

#[tokio::test]
async fn host_call_limit_is_enforced() {
    let limits = CodeModeLimits {
        max_host_calls: 1,
        ..CodeModeLimits::default()
    };
    let test = build_code_mode_test_app(true, limits).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = call_tool(
        &test.app,
        RAW_KEY,
        &session,
        "explore",
        json!({
            "calls": [
                {"name": "searchTools", "args": {}},
                {"name": "searchTools", "args": {}}
            ]
        }),
    )
    .await;
    let payload = json_body(response).await;
    let envelopes = payload["result"]["structuredContent"]["result"]
        .as_array()
        .expect("envelopes");
    assert!(envelopes[0].get("result").is_some());
    assert!(
        envelopes[1]["error"]
            .as_str()
            .expect("limit error")
            .contains("host call limit exceeded")
    );
}

#[tokio::test]
async fn oversized_output_is_truncated_with_marker_and_log_caps_apply() {
    let limits = CodeModeLimits {
        max_output_bytes: 64,
        max_log_lines: 1,
        max_log_bytes: 8,
        ..CodeModeLimits::default()
    };
    let test = build_code_mode_test_app(true, limits).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = call_tool(
        &test.app,
        RAW_KEY,
        &session,
        "explore",
        json!({
            "result": {"blob": "x".repeat(512)},
            "logs": ["0123456789abcdef", "second line never kept"]
        }),
    )
    .await;
    let payload = json_body(response).await;
    let structured = &payload["result"]["structuredContent"];
    assert_eq!(structured["truncated"], json!(true));
    assert_eq!(structured["logs"], json!(["01234567"]));
    assert!(
        payload["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("--- TRUNCATED ---")
    );
}

#[tokio::test]
async fn guest_errors_are_tool_errors_with_gateway_error_log() {
    let test = build_code_mode_test_app(true, CodeModeLimits::default()).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = call_tool(
        &test.app,
        RAW_KEY,
        &session,
        "execute",
        json!({"fail": "ReferenceError: nope is not defined"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(true));
    assert!(
        payload["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .starts_with("Error: ReferenceError")
    );

    let rows = list_code_mode_invocations(&test.store, "execute").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status,
        gateway_core::McpToolInvocationStatus::GatewayError
    );
    assert_eq!(rows[0].error_code.as_deref(), Some("code_execution_error"));
}

/// End-to-end coverage of the real wasmtime_quickjs backend through the
/// route: explore code filters describe results in-sandbox and returns a
/// small projection (the token-saving pattern), and execute drives a real
/// nested upstream tool call with parent/child invocation linkage.
#[tokio::test]
async fn wasmtime_backend_runs_explore_and_execute_end_to_end() {
    let executor = Arc::new(
        gateway_code_mode_wasmtime::WasmtimeQuickjsExecutor::new(&CodeModeLimits::default())
            .expect("embedded guest must validate"),
    );
    let test =
        build_code_mode_test_app_with_executor(true, CodeModeLimits::default(), executor).await;
    let upstream_url = spawn_mock_upstream().await;
    let user_id = insert_user_api_key(&test.db_path, "gwk_codemode9.wasm").await;
    let server_id = insert_mcp_server(&test.db_path, "github", &upstream_url, "none").await;
    let issues_tool = insert_mcp_tool(&test.db_path, server_id, "issues_create", "sha256:ok").await;
    let search_tool = insert_mcp_tool(&test.db_path, server_id, "code_search", "sha256:cs").await;
    grant_tool_to_user(&test.db_path, user_id, issues_tool).await;
    grant_tool_to_user(&test.db_path, user_id, search_tool).await;

    let session = establish_session(&test.app, "gwk_codemode9.wasm").await;

    // Explore: filter the catalog in-sandbox, return a small projection.
    let explore_code = r#"
        const { items } = await oceans.searchTools({});
        console.log("found", items.length, "tools");
        const issueTools = [];
        for (const item of items) {
            const detail = await oceans.describeTool({ address: item.address });
            if (detail.tool.upstream_name.includes("issues")) {
                issueTools.push({ address: detail.address, schema_hash: detail.tool.schema_hash });
            }
        }
        return issueTools;
    "#;
    let response = call_tool(
        &test.app,
        "gwk_codemode9.wasm",
        &session,
        "explore",
        Value::String(explore_code.to_string()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(false));
    let structured = &payload["result"]["structuredContent"];
    assert_eq!(
        structured["result"],
        json!([{"address": "mcp://github/tools/issues_create", "schema_hash": "sha256:ok"}])
    );
    assert_eq!(structured["logs"], json!(["found 2 tools"]));

    // Execute: catchable denied error first, then a real upstream call.
    let execute_code = r#"
        let caught = null;
        try {
            await oceans.callTool({ address: "mcp://github/tools/not_granted" });
        } catch (error) {
            caught = error.message;
        }
        const result = await oceans.callTool({
            address: "mcp://github/tools/issues_create",
            arguments: { title: "Bug" },
            schema_hash: "sha256:ok",
        });
        return { caught, text: result.content[0].text };
    "#;
    let response = call_tool(
        &test.app,
        "gwk_codemode9.wasm",
        &session,
        "execute",
        Value::String(execute_code.to_string()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(false));
    let result = &payload["result"]["structuredContent"]["result"];
    assert_eq!(result["text"], json!("issue created"));
    assert!(
        result["caught"]
            .as_str()
            .expect("caught message")
            .contains("not granted")
    );

    // Parent/child invocation linkage with the real backend.
    let parents = list_code_mode_invocations(&test.store, "execute").await;
    assert_eq!(parents.len(), 1);
    let children = test
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 50,
            parent_invocation_id: Some(parents[0].mcp_tool_invocation_id),
            ..Default::default()
        })
        .await
        .expect("list children")
        .items;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].tool_display_key, "issues_create");
}

/// Real-backend timeout: hostile guest code is preempted and logged with the
/// Milestone 1 timeout status mapping.
#[tokio::test]
async fn wasmtime_backend_times_out_hostile_code_with_timeout_status() {
    let limits = CodeModeLimits {
        execution_timeout_ms: 300,
        ..CodeModeLimits::default()
    };
    let executor = Arc::new(
        gateway_code_mode_wasmtime::WasmtimeQuickjsExecutor::new(&limits)
            .expect("embedded guest must validate"),
    );
    let test = build_code_mode_test_app_with_executor(true, limits, executor).await;
    let session = establish_session(&test.app, RAW_KEY).await;
    let response = call_tool(
        &test.app,
        RAW_KEY,
        &session,
        "explore",
        Value::String("while (true) {}".to_string()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["result"]["isError"], json!(true));
    assert_eq!(
        payload["result"]["structuredContent"]["error_code"],
        json!("timeout")
    );

    let rows = list_code_mode_invocations(&test.store, "explore").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].status,
        gateway_core::McpToolInvocationStatus::Timeout
    );
}
