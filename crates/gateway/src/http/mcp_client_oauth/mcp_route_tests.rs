//! End to end: discovery, registration, the code flow, MCP calls with the
//! access token, refresh, and revocation, against a local upstream MCP server.

use axum::{
    Json,
    body::Body,
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    response::IntoResponse,
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, ExternalMcpAuthMode,
    ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus, ExternalMcpTransport,
    McpAccessRepository, McpRegistryRepository, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpToolInvocationQuery, McpToolInvocationRepository, NewApiKeyRecord,
    NewExternalMcpServerRecord, UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord,
    hash_gateway_key_secret,
};
use gateway_store::GatewayStore;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use super::flow_tests::{CODE_VERIFIER, Harness, REDIRECT_URI, code_challenge};
use super::tests::{ISSUER, send, send_with_headers};

const API_KEY: &str = "gwk_editorkey.editor-secret";

struct Upstream {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// An upstream MCP server with `search` and `purge` tools.
async fn spawn_upstream() -> (String, Upstream) {
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(|Json(message): Json<Value>| async move {
            let id = message["id"].clone();
            match message["method"].as_str().unwrap_or_default() {
                "tools/list" => Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"tools": [
                        {"name": "search", "inputSchema": {"type": "object"}},
                        {"name": "purge", "inputSchema": {"type": "object"}}
                    ]},
                }))
                .into_response(),
                "tools/call" => Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": "found it"}]},
                }))
                .into_response(),
                _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})).into_response(),
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("upstream");
    });
    (url, Upstream { task })
}

/// Seeds server `notes` with `search` granted to the user and `purge`
/// granted only to the user's API key.
async fn seed_notes(harness: &Harness, server_url: &str) {
    let store = &harness.state.store;
    let now = OffsetDateTime::now_utc();
    let key = store
        .create_api_key(&NewApiKeyRecord {
            name: "Editor key".to_string(),
            public_id: "editorkey".to_string(),
            secret_hash: hash_gateway_key_secret("editor-secret").unwrap(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(harness.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            access: Default::default(),
            created_at: now,
        })
        .await
        .expect("API key");
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            ownership: Default::default(),
            server_key: "notes".to_string(),
            display_name: "Notes".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: server_url.to_string(),
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
                expected_status_revision: None,
                discovery_run_id: Uuid::new_v4(),
                mcp_server_id: server.mcp_server_id,
                status: ExternalMcpDiscoveryStatus::Success,
                started_at: now,
                finished_at: now,
                discovered_tool_count: 2,
                active_tool_count: 2,
                schema_set_hash: None,
                error_summary: None,
                details: Map::new(),
                expected_server: None,
            },
            &["search", "purge"].map(|name| UpsertExternalMcpToolRecord {
                mcp_server_id: server.mcp_server_id,
                upstream_name: name.to_string(),
                display_name: name.to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
                metadata: gateway_core::McpToolMetadata::default(),
                schema_hash: format!("{name}-v1"),
            }),
        )
        .await
        .expect("tool discovery");
    let tool = |name: &str| {
        tools
            .iter()
            .find(|tool| tool.upstream_name == name)
            .expect("discovered tool")
    };
    for (subject_kind, subject_id, tool) in [
        (
            McpToolGrantSubjectKind::User,
            harness.user_id,
            tool("search"),
        ),
        (McpToolGrantSubjectKind::ApiKey, key.id, tool("purge")),
    ] {
        store
            .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
                subject_kind,
                subject_id,
                target_kind: McpToolGrantTargetKind::Tool,
                target_id: tool.mcp_tool_id,
                updated_at: now,
                expected_server: None,
                expected_subject_team: None,
            })
            .await
            .expect("grant");
    }
}

fn rpc(method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params}).to_string()
}

impl Harness {
    async fn mcp(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: String,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(bearer) = bearer {
            request = request.header(AUTHORIZATION, format!("Bearer {bearer}"));
        }
        send_with_headers(&self.router, request.body(Body::from(body)).unwrap()).await
    }

    async fn get_json(&self, url: &str) -> Value {
        let path = url.strip_prefix(ISSUER).expect("issuer URL");
        let (status, body) = send(
            &self.router,
            Request::builder().uri(path).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{url}: {body}");
        body
    }

    /// Runs consent and the code exchange for a registered client.
    async fn authorize_client(
        &self,
        client_id: &str,
        resource: &str,
        scope: Option<&str>,
    ) -> Value {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("state", "client-state")
            .append_pair("code_challenge", &code_challenge(CODE_VERIFIER))
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", resource);
        if let Some(scope) = scope {
            query.append_pair("scope", scope);
        }
        let request_id = self.start(&query.finish()).await;
        let code = self.approve(&request_id).await;
        let (status, _, token) = self
            .token(
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", client_id),
                    ("code", &code),
                    ("redirect_uri", REDIRECT_URI),
                    ("code_verifier", CODE_VERIFIER),
                    ("resource", resource),
                ],
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{token}");
        token
    }

    async fn register_mcp_client(&self) -> String {
        let (status, _, body) = self
            .register(json!({
                "client_name": "Spec client",
                "redirect_uris": ["http://127.0.0.1/callback"],
                "grant_types": ["authorization_code", "refresh_token"],
                "token_endpoint_auth_method": "none"
            }))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["client_id"].as_str().unwrap().to_string()
    }
}

fn tool_names(body: &Value) -> Vec<&str> {
    body["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tools in {body}"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

fn challenge_param<'a>(header: &'a str, name: &str) -> &'a str {
    let start = header.find(&format!("{name}=\"")).expect("parameter") + name.len() + 2;
    let end = start + header[start..].find('"').unwrap();
    &header[start..end]
}

#[tokio::test]
async fn spec_client_discovers_registers_signs_in_and_calls_tools() {
    let harness = Harness::new().await;
    let (upstream_url, _upstream) = spawn_upstream().await;
    seed_notes(&harness, &upstream_url).await;
    let list = rpc("tools/list", json!({}));

    // 1. Unauthenticated call: the challenge names the metadata and scope.
    let (status, headers, _) = harness.mcp("/mcp/notes", None, list.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let challenge = headers[WWW_AUTHENTICATE].to_str().unwrap();
    assert_eq!(challenge_param(challenge, "scope"), "mcp:proxy");
    let resource_metadata = harness
        .get_json(challenge_param(challenge, "resource_metadata"))
        .await;
    let resource = resource_metadata["resource"].as_str().unwrap();
    assert_eq!(resource, "https://oceans.example.com/mcp/notes");
    let issuer = resource_metadata["authorization_servers"][0]
        .as_str()
        .unwrap();
    let metadata = harness
        .get_json(&format!("{issuer}/.well-known/oauth-authorization-server"))
        .await;
    assert_eq!(metadata["issuer"], issuer);

    // 2. Registration, consent, and token exchange.
    assert_eq!(
        Url::parse(metadata["registration_endpoint"].as_str().unwrap())
            .unwrap()
            .path(),
        "/oauth/register"
    );
    let client_id = harness.register_mcp_client().await;
    let tokens = harness.authorize_client(&client_id, resource, None).await;
    assert_eq!(tokens["scope"], "mcp:proxy");
    let access = tokens["access_token"].as_str().unwrap();

    // 3. Tool calls resolve the user's grants, not the user's key grants.
    let (status, _, body) = harness.mcp("/mcp/notes", Some(access), list.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(tool_names(&body), ["search"]);
    let call = rpc("tools/call", json!({"name": "search", "arguments": {}}));
    let (status, _, body) = harness.mcp("/mcp/notes", Some(access), call).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["content"][0]["text"], "found it");
    let denied = rpc("tools/call", json!({"name": "purge", "arguments": {}}));
    let (status, _, _) = harness.mcp("/mcp/notes", Some(access), denied).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let invocations = harness
        .state
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 20,
            ..McpToolInvocationQuery::default()
        })
        .await
        .unwrap();
    let search = invocations
        .items
        .iter()
        .find(|item| item.tool_display_key == "search" && item.mcp_method == "tools/call")
        .expect("search invocation");
    assert_eq!(search.user_id, Some(harness.user_id));
    assert_eq!(search.api_key_id, None);
    assert_eq!(search.metadata["auth_method"], "oauth");
    assert_eq!(search.metadata["oauth_client_id"], client_id.as_str());

    // 4. The token is bound to /mcp/notes and refused everywhere else.
    let (status, headers, _) = harness.mcp("/mcp", Some(access), list.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers[WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .starts_with(r#"Bearer error="invalid_token""#)
    );
    let (status, body) = send(
        &harness.router,
        Request::builder()
            .uri("/v1/models")
            .header(AUTHORIZATION, format!("Bearer {access}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "oauth_token_not_accepted", "{body}");

    // 5. Refresh, then revocation ends access.
    let (status, _, refreshed) = harness
        .token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &client_id),
                ("refresh_token", tokens["refresh_token"].as_str().unwrap()),
                ("resource", resource),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");
    let access = refreshed["access_token"].as_str().unwrap();
    let (status, _, _) = harness.mcp("/mcp/notes", Some(access), list.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        harness
            .revoke(&[
                ("client_id", &client_id),
                ("token", refreshed["refresh_token"].as_str().unwrap()),
            ])
            .await,
        StatusCode::OK
    );
    let (status, _, _) = harness.mcp("/mcp/notes", Some(access), list.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 6. The user's API key keeps working and keeps its own key grants.
    let (status, _, body) = harness.mcp("/mcp/notes", Some(API_KEY), list).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(tool_names(&body), ["search", "purge"]);
}

#[tokio::test]
async fn aggregate_tokens_need_the_aggregate_scope_and_own_their_sessions() {
    let harness = Harness::new().await;
    let (upstream_url, _upstream) = spawn_upstream().await;
    seed_notes(&harness, &upstream_url).await;
    let client_id = harness.register_mcp_client().await;
    let initialize = rpc(
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );

    // A proxy-only token for the whole gateway reaches servers but not /mcp.
    let proxy = harness
        .authorize_client(
            &client_id,
            "https://oceans.example.com/mcp",
            Some("mcp:proxy"),
        )
        .await;
    let proxy_access = proxy["access_token"].as_str().unwrap();
    let (status, headers, _) = harness
        .mcp("/mcp", Some(proxy_access), initialize.clone())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        headers[WWW_AUTHENTICATE],
        r#"Bearer error="insufficient_scope", scope="mcp:aggregate", resource_metadata="https://oceans.example.com/.well-known/oauth-protected-resource/mcp""#
    );
    let (status, _, _) = harness
        .mcp(
            "/mcp/notes",
            Some(proxy_access),
            rpc("tools/list", json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // An aggregate token opens a session that belongs to its grant.
    let aggregate = harness
        .authorize_client(&client_id, "https://oceans.example.com/mcp", None)
        .await;
    let access = aggregate["access_token"].as_str().unwrap();
    let (status, headers, body) = harness.mcp("/mcp", Some(access), initialize).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let session = headers["mcp-session-id"].to_str().unwrap().to_string();

    let in_session = |bearer: &str, body: String| {
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .header(AUTHORIZATION, format!("Bearer {bearer}"))
            .body(Body::from(body))
            .unwrap()
    };
    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
    let (status, _) = send(&harness.router, in_session(access, initialized)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let list = |bearer: &str| in_session(bearer, rpc("tools/list", json!({})));
    let (status, body) = send(&harness.router, list(access)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        tool_names(&body),
        ["search_tools", "describe_tool", "call_tool"]
    );
    let (status, _) = send(&harness.router, list(API_KEY)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn signing_out_ends_mcp_access_and_tokens_need_the_authorization_header() {
    let harness = Harness::new().await;
    let (upstream_url, _upstream) = spawn_upstream().await;
    seed_notes(&harness, &upstream_url).await;
    let client_id = harness.register_mcp_client().await;
    let tokens = harness
        .authorize_client(&client_id, "https://oceans.example.com/mcp/notes", None)
        .await;
    let access = tokens["access_token"].as_str().unwrap();

    let (status, _) = send(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/mcp/notes")
            .header("content-type", "application/json")
            .header("x-oceans-api-key", access)
            .body(Body::from(rpc("tools/list", json!({}))))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    harness
        .state
        .store
        .revoke_user_session(harness.session_id, OffsetDateTime::now_utc())
        .await
        .unwrap();
    let (status, _, _) = harness
        .mcp("/mcp/notes", Some(access), rpc("tools/list", json!({})))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oauth_tokens_are_refused_when_the_feature_is_disabled() {
    let harness = Harness::new().await;
    let (upstream_url, _upstream) = spawn_upstream().await;
    seed_notes(&harness, &upstream_url).await;
    let client_id = harness.register_mcp_client().await;
    let tokens = harness
        .authorize_client(&client_id, "https://oceans.example.com/mcp/notes", None)
        .await;
    let mut state = harness.state.clone();
    state.mcp_client_oauth = None;
    let router = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    let (status, _, _) = send_with_headers(
        &router,
        Request::builder()
            .method("POST")
            .uri("/mcp/notes")
            .header("content-type", "application/json")
            .header(
                AUTHORIZATION,
                format!("Bearer {}", tokens["access_token"].as_str().unwrap()),
            )
            .body(Body::from(rpc("tools/list", json!({}))))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oauth_tokens_are_refused_on_non_mcp_routes() {
    let harness = Harness::new().await;
    let client_id = harness.register_mcp_client().await;
    let tokens = harness
        .authorize_client(&client_id, "https://oceans.example.com/mcp", None)
        .await;
    let access = tokens["access_token"].as_str().unwrap();
    for (method, path) in [
        ("GET", "/v1/models"),
        ("GET", "/v1/model-metadata"),
        ("POST", "/v1/chat/completions"),
        ("POST", "/v1/responses"),
        ("POST", "/v1/messages"),
        ("POST", "/v1/embeddings"),
        ("GET", "/api/v1/batches"),
    ] {
        let (status, body) = send(
            &harness.router,
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header(AUTHORIZATION, format!("Bearer {access}"))
                .body(Body::from(r#"{"model":"fast","messages":[],"input":"x"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}: {body}");
    }
}
