//! Scoped API keys through the real router: each scope boundary in both
//! directions, MCP server lists on the proxy and the aggregate endpoint,
//! unscoped keys, and editing scopes after creation.
//! See `docs/adr/2026-10-17-scoped-api-keys.md`.

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header::COOKIE},
    response::IntoResponse,
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, ExternalMcpAuthMode,
    ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus, ExternalMcpTransport, GlobalRole,
    McpAccessRepository, McpRegistryRepository, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    NewApiKeyRecord, NewExternalMcpServerRecord, UpsertExternalMcpToolRecord,
    UpsertMcpToolGrantRecord, UserStatus, hash_gateway_key_secret,
};
use gateway_store::GatewayStore;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::{build_router, state::AppState, test_support::app_state};

const API_KEYS: &str = "/api/v1/admin/api-keys";

struct Upstream(tokio::task::JoinHandle<()>);

impl Drop for Upstream {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// An upstream MCP server with one `search` tool.
async fn spawn_upstream() -> (String, Upstream) {
    let app = Router::new().route(
        "/mcp",
        axum::routing::post(|Json(message): Json<Value>| async move {
            let id = message["id"].clone();
            let result = match message["method"].as_str().unwrap_or_default() {
                "tools/list" => {
                    json!({"tools": [{"name": "search", "inputSchema": {"type": "object"}}]})
                }
                "tools/call" => json!({"content": [{"type": "text", "text": "found it"}]}),
                _ => json!({}),
            };
            Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("upstream");
    });
    (url, Upstream(task))
}

struct Harness {
    state: AppState,
    router: Router,
    user_id: Uuid,
    admin: HeaderMap,
    _upstream: Upstream,
    _directory: tempfile::TempDir,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Harness {
    /// A platform admin who is granted `search` on both `notes` and `wiki`.
    async fn new() -> Self {
        let (directory, state) = app_state().await;
        let (upstream_url, upstream) = spawn_upstream().await;
        let (user_id, admin) = signed_in_admin(&state).await;
        for server_key in ["notes", "wiki"] {
            seed_granted_server(&state, server_key, &upstream_url, user_id).await;
        }
        Self {
            router: build_router(state.clone(), admin_ui::AdminUiConfig::default()),
            state,
            user_id,
            admin,
            _upstream: upstream,
            _directory: directory,
        }
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn admin(&self, method: Method, uri: &str, body: Option<Value>) -> Reply {
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in &self.admin {
            request = request.header(name, value);
        }
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        };
        self.send(request.expect("request")).await
    }

    /// Creates a key for the admin through the API; returns its id and raw key.
    async fn create_key(&self, scopes: Value, mcp_server_keys: Value) -> (String, String) {
        let reply = self
            .admin(
                Method::POST,
                API_KEYS,
                Some(json!({
                    "name": "scoped",
                    "owner_kind": "user",
                    "owner_user_id": self.user_id.to_string(),
                    "owner_team_id": null,
                    "owner_service_account_id": null,
                    "model_grant_mode": "all",
                    "model_keys": [],
                    "scopes": scopes,
                    "mcp_server_keys": mcp_server_keys,
                })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        let data = &reply.body["data"];
        (
            data["api_key"]["id"].as_str().unwrap().to_string(),
            data["raw_key"].as_str().unwrap().to_string(),
        )
    }

    async fn set_scopes(&self, api_key_id: &str, body: Value) -> Reply {
        self.admin(
            Method::PUT,
            &format!("{API_KEYS}/{api_key_id}/scopes"),
            Some(body),
        )
        .await
    }

    async fn get(&self, path: &str, key: &str) -> Reply {
        self.send(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn chat(&self, key: &str) -> Reply {
        self.send(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"model": "fast", "messages": [{"role": "user", "content": "hi"}]})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
    }

    async fn mcp(&self, path: &str, key: &str, session: Option<&str>, body: Value) -> Reply {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("authorization", format!("Bearer {key}"))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(session) = session {
            request = request
                .header("mcp-session-id", session)
                .header("mcp-protocol-version", "2025-11-25");
        }
        self.send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn tools_list(&self, server_key: &str, key: &str) -> Reply {
        self.mcp(
            &format!("/mcp/{server_key}"),
            key,
            None,
            rpc("tools/list", json!({})),
        )
        .await
    }

    /// Opens an aggregate session; returns the reply and the session id, if any.
    async fn open_aggregate(&self, key: &str) -> (Reply, Option<String>) {
        let reply = self
            .mcp(
                "/mcp",
                key,
                None,
                rpc(
                    "initialize",
                    json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
                ),
            )
            .await;
        let session = reply
            .headers
            .get("mcp-session-id")
            .map(|value| value.to_str().unwrap().to_string());
        if let Some(session) = &session {
            let initialized = self
                .mcp(
                    "/mcp",
                    key,
                    Some(session),
                    json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                )
                .await;
            assert_eq!(initialized.status, StatusCode::ACCEPTED);
        }
        (reply, session)
    }

    async fn aggregate_tool(
        &self,
        key: &str,
        session: &str,
        name: &str,
        arguments: Value,
    ) -> Reply {
        self.mcp(
            "/mcp",
            key,
            Some(session),
            rpc("tools/call", json!({"name": name, "arguments": arguments})),
        )
        .await
    }
}

fn rpc(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})
}

async fn signed_in_admin(state: &AppState) -> (Uuid, HeaderMap) {
    let email = format!("{}@example.test", Uuid::new_v4());
    let user = state
        .store
        .create_identity_user(
            "Scope tester",
            &email,
            &email,
            GlobalRole::PlatformAdmin,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("create user");
    let session_id = Uuid::new_v4();
    let token = format!("{session_id}.scope-test-token");
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = OffsetDateTime::now_utc();
    state
        .store
        .create_user_session(
            session_id,
            user.user_id,
            &token_hash,
            now + Duration::hours(1),
            now,
        )
        .await
        .expect("create session");
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("ogw_session={token}")).unwrap(),
    );
    (user.user_id, headers)
}

/// Registers `server_key` with a discovered `search` tool granted to `user_id`.
async fn seed_granted_server(state: &AppState, server_key: &str, url: &str, user_id: Uuid) {
    let store = &state.store;
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: server_key.to_string(),
            display_name: server_key.to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: url.to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
            approve_tool_changes: false,
        })
        .await
        .expect("server");
    let tools = store
        .record_external_mcp_discovery_success(
            &ExternalMcpDiscoveryRunRecord {
                expected_server_status: None,
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
                display_name: "search".to_string(),
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
            subject_kind: McpToolGrantSubjectKind::User,
            subject_id: user_id,
            target_kind: McpToolGrantTargetKind::Tool,
            target_id: tools[0].mcp_tool_id,
            updated_at: now,
        })
        .await
        .expect("grant");
}

fn assert_openai_insufficient_scope(reply: &Reply) {
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
    assert_eq!(
        reply.body["error"]["code"], "insufficient_scope",
        "{}",
        reply.body
    );
    assert!(reply.body["error"]["message"].is_string());
}

/// The JSON-RPC error an MCP route sends; `required` is the missing scope,
/// or `None` for a server outside the key's list.
fn assert_jsonrpc_insufficient_scope(reply: &Reply, required: Option<&str>) {
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
    assert_eq!(reply.body["jsonrpc"], "2.0", "{}", reply.body);
    assert_eq!(reply.body["id"], Value::Null);
    assert_eq!(reply.body["error"]["code"], -32001);
    assert_eq!(reply.body["error"]["data"]["error"], "insufficient_scope");
    assert_eq!(
        reply.body["error"]["data"]["required_scope"],
        required.map_or(Value::Null, |scope| json!(scope)),
        "{}",
        reply.body
    );
}

fn search_addresses(reply: &Reply) -> Vec<String> {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["result"]["structuredContent"]["items"]
        .as_array()
        .unwrap_or_else(|| panic!("no items in {}", reply.body))
        .iter()
        .map(|item| item["address"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn mcp_proxy_keys_cannot_call_models_and_inference_keys_cannot_call_mcp() {
    let harness = Harness::new().await;
    let (_, proxy_key) = harness.create_key(json!(["mcp:proxy"]), Value::Null).await;
    let (_, inference_key) = harness.create_key(json!(["inference"]), Value::Null).await;

    // `mcp:proxy` reaches a server, but no model route and not the aggregate.
    let listed = harness.tools_list("notes", &proxy_key).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["result"]["tools"][0]["name"], "search");
    assert_openai_insufficient_scope(&harness.chat(&proxy_key).await);
    assert_openai_insufficient_scope(&harness.get("/v1/models", &proxy_key).await);
    let (aggregate, session) = harness.open_aggregate(&proxy_key).await;
    assert_jsonrpc_insufficient_scope(&aggregate, Some("mcp:aggregate"));
    assert!(session.is_none());

    // `inference` reaches model routes, but no MCP route.
    let models = harness.get("/v1/models", &inference_key).await;
    assert_eq!(models.status, StatusCode::OK, "{}", models.body);
    let chat = harness.chat(&inference_key).await;
    assert_ne!(
        chat.body["error"]["code"], "insufficient_scope",
        "{}",
        chat.body
    );
    assert_jsonrpc_insufficient_scope(
        &harness.tools_list("notes", &inference_key).await,
        Some("mcp:proxy"),
    );
    let (aggregate, _) = harness.open_aggregate(&inference_key).await;
    assert_jsonrpc_insufficient_scope(&aggregate, Some("mcp:aggregate"));

    // Anthropic-shaped routes refuse with their own envelope.
    let messages = harness
        .send(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("x-api-key", &proxy_key)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"model": "fast", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(messages.status, StatusCode::FORBIDDEN, "{}", messages.body);
    assert_eq!(messages.body["type"], "error", "{}", messages.body);
}

#[tokio::test]
async fn aggregate_and_proxy_scopes_are_separate() {
    let harness = Harness::new().await;
    let (_, aggregate_key) = harness
        .create_key(json!(["mcp:aggregate"]), Value::Null)
        .await;

    let (opened, session) = harness.open_aggregate(&aggregate_key).await;
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.body);
    let session = session.expect("session");
    let found = harness
        .aggregate_tool(
            &aggregate_key,
            &session,
            "search_tools",
            json!({"query": ""}),
        )
        .await;
    assert_eq!(
        search_addresses(&found),
        ["mcp://notes/tools/search", "mcp://wiki/tools/search"]
    );
    assert_jsonrpc_insufficient_scope(
        &harness.tools_list("notes", &aggregate_key).await,
        Some("mcp:proxy"),
    );
    assert_openai_insufficient_scope(&harness.chat(&aggregate_key).await);
}

#[tokio::test]
async fn scoped_keys_cannot_call_full_access_routes() {
    let harness = Harness::new().await;
    let (_, scoped) = harness
        .create_key(
            json!(["inference", "mcp:aggregate", "mcp:proxy"]),
            Value::Null,
        )
        .await;
    let (_, unscoped) = harness.create_key(Value::Null, Value::Null).await;

    // Every scope is still not full access.
    let refused = harness
        .get("/api/v1/me/provider-credentials", &scoped)
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(refused.body["error"]["code"], "insufficient_scope");
    let allowed = harness
        .get("/api/v1/me/provider-credentials", &unscoped)
        .await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.body);
}

#[tokio::test]
async fn server_lists_limit_the_proxy_without_revealing_other_servers() {
    let harness = Harness::new().await;
    let (_, notes_only) = harness
        .create_key(json!(["mcp:proxy"]), json!(["notes"]))
        .await;

    let listed = harness.tools_list("notes", &notes_only).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    // The owner holds a grant on `wiki`, but the key does not list it.
    let refused = harness.tools_list("wiki", &notes_only).await;
    assert_jsonrpc_insufficient_scope(&refused, None);
    // A server that does not exist gets the same answer.
    let missing = harness.tools_list("missing", &notes_only).await;
    assert_eq!(missing.status, refused.status);
    assert_eq!(missing.body, refused.body);
}

#[tokio::test]
async fn request_telemetry_counts_only_listed_servers() {
    let harness = Harness::new().await;
    let (_, notes_only) = harness
        .create_key(json!(["inference", "mcp:aggregate"]), json!(["notes"]))
        .await;
    let auth = harness
        .state
        .service
        .authenticate_bearer_token_for_scope(&notes_only, "inference")
        .await
        .unwrap();
    let resolution = gateway_service::McpAccess::new(harness.state.store.clone())
        .effective_tools_for_api_key(&auth, None, gateway_core::McpToolProjection::Full)
        .await
        .unwrap();
    assert_eq!(
        resolution.referenced_server_count, 2,
        "the owner is granted both"
    );

    let restricted =
        crate::http::handlers::restrict_to_listed_servers(&harness.state, &auth, resolution)
            .await
            .unwrap();
    let notes = harness
        .state
        .store
        .get_external_mcp_server_by_key("notes")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restricted.referenced_server_count, 1);
    assert_eq!(restricted.allowed_tools.len(), 1);
    assert_eq!(
        restricted.allowed_tools[0].mcp_server_id,
        notes.mcp_server_id
    );
    assert_eq!(
        restricted.filtered_tool_count,
        restricted.exposed_tool_count - 1
    );
}

#[tokio::test]
async fn multi_server_lists_drop_tools_on_unlisted_servers() {
    let harness = Harness::new().await;
    // The owner is also granted a tool on a third server the key does not list.
    seed_granted_server(
        &harness.state,
        "extra",
        "http://127.0.0.1:9/mcp",
        harness.user_id,
    )
    .await;
    let (_, two_servers) = harness
        .create_key(json!(["mcp:aggregate"]), json!(["notes", "wiki"]))
        .await;

    let (_, session) = harness.open_aggregate(&two_servers).await;
    let found = harness
        .aggregate_tool(
            &two_servers,
            &session.unwrap(),
            "search_tools",
            json!({"query": ""}),
        )
        .await;
    assert_eq!(
        search_addresses(&found),
        ["mcp://notes/tools/search", "mcp://wiki/tools/search"]
    );
    assert_eq!(found.body["result"]["structuredContent"]["total"], 2);
}

#[tokio::test]
async fn server_lists_limit_aggregate_search_describe_and_call() {
    let harness = Harness::new().await;
    let (_, notes_only) = harness
        .create_key(json!(["mcp:aggregate"]), json!(["notes"]))
        .await;
    let (_, everywhere) = harness
        .create_key(json!(["mcp:aggregate"]), Value::Null)
        .await;

    let (_, session) = harness.open_aggregate(&notes_only).await;
    let session = session.expect("session");
    let found = harness
        .aggregate_tool(&notes_only, &session, "search_tools", json!({"query": ""}))
        .await;
    assert_eq!(search_addresses(&found), ["mcp://notes/tools/search"]);
    let filtered = harness
        .aggregate_tool(
            &notes_only,
            &session,
            "search_tools",
            json!({"query": "", "server_key": "wiki"}),
        )
        .await;
    assert!(search_addresses(&filtered).is_empty());

    let described = harness
        .aggregate_tool(
            &notes_only,
            &session,
            "describe_tool",
            json!({"address": "mcp://notes/tools/search"}),
        )
        .await;
    assert_eq!(described.status, StatusCode::OK, "{}", described.body);
    for name in ["describe_tool", "call_tool"] {
        let refused = harness
            .aggregate_tool(
                &notes_only,
                &session,
                name,
                json!({"address": "mcp://wiki/tools/search"}),
            )
            .await;
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{name}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["message"], "MCP tool address is not granted",
            "{name}"
        );
    }
    let called = harness
        .aggregate_tool(
            &notes_only,
            &session,
            "call_tool",
            json!({"address": "mcp://notes/tools/search", "arguments": {}}),
        )
        .await;
    assert_eq!(called.status, StatusCode::OK, "{}", called.body);
    assert!(called.body.get("error").is_none(), "{}", called.body);

    // The same owner's unlisted key still sees both servers.
    let (_, session) = harness.open_aggregate(&everywhere).await;
    let found = harness
        .aggregate_tool(
            &everywhere,
            &session.unwrap(),
            "search_tools",
            json!({"query": ""}),
        )
        .await;
    assert_eq!(search_addresses(&found).len(), 2);
}

#[tokio::test]
async fn keys_without_scopes_keep_full_access() {
    let harness = Harness::new().await;
    // A row written the way keys were before scopes existed: both columns NULL.
    harness
        .state
        .store
        .create_api_key(&NewApiKeyRecord {
            name: "Existing".to_string(),
            public_id: "existingkey".to_string(),
            secret_hash: hash_gateway_key_secret("existing-secret").unwrap(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(harness.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            access: Default::default(),
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    let key = "gwk_existingkey.existing-secret";

    assert_eq!(harness.get("/v1/models", key).await.status, StatusCode::OK);
    assert_eq!(harness.tools_list("wiki", key).await.status, StatusCode::OK);
    let (opened, _) = harness.open_aggregate(key).await;
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.body);
    let credentials = harness.get("/api/v1/me/provider-credentials", key).await;
    assert_eq!(credentials.status, StatusCode::OK, "{}", credentials.body);

    let listing = harness.admin(Method::GET, API_KEYS, None).await;
    let existing = listing.body["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Existing")
        .expect("listed")
        .clone();
    assert_eq!(existing["scopes"], Value::Null);
    assert_eq!(existing["mcp_server_keys"], Value::Null);
    assert_eq!(
        listing.body["data"]["available_scopes"],
        json!(["inference", "mcp:aggregate", "mcp:proxy"])
    );
    assert_eq!(
        listing.body["data"]["mcp_servers"],
        json!([
            {"key": "notes", "display_name": "notes"},
            {"key": "wiki", "display_name": "wiki"},
        ])
    );
}

#[tokio::test]
async fn scopes_can_be_narrowed_widened_and_restored_after_creation() {
    let harness = Harness::new().await;
    let (api_key_id, key) = harness.create_key(json!(["inference"]), Value::Null).await;
    assert_jsonrpc_insufficient_scope(&harness.tools_list("notes", &key).await, Some("mcp:proxy"));

    // Swap to `mcp:proxy` limited to `notes`; it applies on the next request.
    let updated = harness
        .set_scopes(
            &api_key_id,
            json!({"scopes": ["mcp:proxy"], "mcp_server_keys": ["notes"]}),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(
        updated.body["data"]["api_key"]["scopes"],
        json!(["mcp:proxy"])
    );
    assert_eq!(
        updated.body["data"]["api_key"]["mcp_server_keys"],
        json!(["notes"])
    );
    assert_eq!(
        harness.tools_list("notes", &key).await.status,
        StatusCode::OK
    );
    assert_jsonrpc_insufficient_scope(&harness.tools_list("wiki", &key).await, None);
    assert_openai_insufficient_scope(&harness.get("/v1/models", &key).await);

    // `null` restores full access and every server.
    let restored = harness
        .set_scopes(
            &api_key_id,
            json!({"scopes": null, "mcp_server_keys": null}),
        )
        .await;
    assert_eq!(restored.status, StatusCode::OK, "{}", restored.body);
    assert_eq!(restored.body["data"]["api_key"]["scopes"], Value::Null);
    assert_eq!(harness.get("/v1/models", &key).await.status, StatusCode::OK);
    assert_eq!(
        harness.tools_list("wiki", &key).await.status,
        StatusCode::OK
    );

    // Each change is audited with what it replaced; a resubmit records nothing.
    let resubmitted = harness
        .set_scopes(
            &api_key_id,
            json!({"scopes": null, "mcp_server_keys": null}),
        )
        .await;
    assert_eq!(resubmitted.status, StatusCode::OK);
    let entries = harness
        .admin(
            Method::GET,
            "/api/v1/admin/audit-logs?action=api_key.scopes_updated&limit=50",
            None,
        )
        .await;
    let items = entries.body["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "{}", entries.body);
    let newest = harness
        .admin(
            Method::GET,
            &format!(
                "/api/v1/admin/audit-logs/{}",
                items[0]["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(
        newest.body["data"]["before"],
        json!({"scopes": ["mcp:proxy"], "mcp_server_keys": ["notes"]})
    );
    assert_eq!(
        newest.body["data"]["after"],
        json!({"scopes": null, "mcp_server_keys": null})
    );
    assert_eq!(newest.body["data"]["target_id"], api_key_id);
}

#[tokio::test]
async fn invalid_scope_edits_are_rejected() {
    let harness = Harness::new().await;
    let (api_key_id, _) = harness.create_key(json!(["mcp:proxy"]), Value::Null).await;

    for body in [
        json!({"scopes": ["observability:read"], "mcp_server_keys": null}),
        json!({"scopes": [], "mcp_server_keys": null}),
        json!({"scopes": ["mcp:proxy"], "mcp_server_keys": ["unregistered"]}),
        json!({"scopes": ["inference"], "mcp_server_keys": ["notes"]}),
    ] {
        let reply = harness.set_scopes(&api_key_id, body.clone()).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            reply.body
        );
    }
    // Leaving a field out never widens a key by accident.
    let reply = harness
        .set_scopes(&api_key_id, json!({"scopes": ["mcp:proxy"]}))
        .await;
    assert!(reply.status.is_client_error(), "{}", reply.body);

    let revoked = harness
        .admin(
            Method::POST,
            &format!("{API_KEYS}/{api_key_id}/revoke"),
            None,
        )
        .await;
    assert_eq!(revoked.status, StatusCode::OK);
    let reply = harness
        .set_scopes(
            &api_key_id,
            json!({"scopes": null, "mcp_server_keys": null}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
}

#[tokio::test]
async fn store_replaces_access_only_when_it_changes() {
    let harness = Harness::new().await;
    let (api_key_id, _) = harness.create_key(json!(["inference"]), Value::Null).await;
    let api_key_id = Uuid::parse_str(&api_key_id).unwrap();
    let store = &harness.state.store;
    let proxy = gateway_core::ApiKeyAccess::new(
        Some(vec!["mcp:proxy".to_string()]),
        Some(vec!["notes".to_string()]),
    )
    .unwrap();

    let replaced = store
        .replace_api_key_access(api_key_id, &proxy)
        .await
        .unwrap();
    assert_eq!(
        replaced.and_then(|previous| previous.scopes),
        Some(vec!["inference".to_string()])
    );
    assert_eq!(
        store
            .replace_api_key_access(api_key_id, &proxy)
            .await
            .unwrap(),
        None
    );
    let stored = store.get_api_key_by_id(api_key_id).await.unwrap().unwrap();
    assert_eq!(stored.access, proxy);
}
