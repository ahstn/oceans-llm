use super::*;
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, AuthMode, ExternalMcpAuthMode,
    ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus, ExternalMcpTransport, GlobalRole,
    McpAccessRepository, McpRegistryRepository, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpToolInvocationQuery, McpToolInvocationRecord, McpToolInvocationRepository,
    McpToolInvocationStatus, McpToolPolicyResult, NewApiKeyRecord, NewExternalMcpServerRecord,
    UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord, UserStatus,
};
use gateway_guardrails::{
    ContentTransformation, FailureDisposition, GuardPhase, GuardrailEngine, ManagedCheckConfig,
    ManagedCheckKind, ManagedOutcome, ManagedService, PolicyMode, ReasonCode,
    test_utils::StubManagedEvaluator,
};
use gateway_store::GatewayStore;
use serde_json::Map;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn signed_session_token_is_visible_ascii_and_parseable() {
    let session_id = Uuid::new_v4();
    let token = signed_session_token("secret", session_id);
    assert!(token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)));
    let mut headers = HeaderMap::new();
    headers.insert(
        MCP_SESSION_ID_HEADER,
        HeaderValue::from_str(&token).unwrap(),
    );
    let (parsed_id, parsed_hash) = session_identity(&headers).expect("session identity");
    assert_eq!(parsed_id, session_id);
    assert_eq!(parsed_hash, token_hash(&token));
}

#[tokio::test]
async fn aggregate_tools_list_serializes_only_discovery_tools() {
    let response = list_builtin_tools(JsonRpcId::String("discovery".to_string()));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers()[MCP_PROTOCOL_VERSION_HEADER],
        DEFAULT_PROTOCOL_VERSION
    );
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC response");
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], "discovery");
    assert!(body.get("error").is_none());
    let tools = body["result"]["tools"].as_array().expect("tool list");
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["search_tools", "describe_tool", "call_tool"]
    );
    assert_eq!(tools[2]["inputSchema"]["required"], json!(["address"]));
}

#[tokio::test]
async fn preparation_failure_records_authorized_invocation() {
    // This persisted gateway-auth server has no secret reference. Authorization succeeds,
    // then upstream preparation fails before an HTTP client or request can be created.
    assert_failed_invocation(
        "https://example.test/mcp",
        ExternalMcpAuthMode::GatewayBearerToken,
        McpToolInvocationStatus::GatewayError,
        "invalid_request",
    )
    .await;
}

#[tokio::test]
async fn client_construction_failure_records_authorized_invocation() {
    // Registry rows can outlive validation changes; a malformed stored URL must still
    // produce an invocation record when construction rejects it.
    assert_failed_invocation(
        "not an absolute URL",
        ExternalMcpAuthMode::None,
        McpToolInvocationStatus::UpstreamError,
        "upstream_transport",
    )
    .await;
}

#[tokio::test]
async fn upstream_success_preserves_result_and_records_one_invocation() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let tool_result = json!({
        "content": [{"type": "text", "text": "Repository found"}],
        "structuredContent": {"repository": "oceans"},
        "isError": false,
    });
    let (url, requests, server) = spawn_tools_call_upstream("result", tool_result.clone()).await;
    let auth = seed_authorized_tool(&state, &url, ExternalMcpAuthMode::None).await;
    let response = call_catalog_tool(
        &state,
        &auth,
        JsonRpcId::Number(23),
        CallMcpToolInput {
            address: "mcp://repository/tools/search".to_string(),
            arguments: json!({"query": "oceans"}),
            schema_hash: Some("search-v1".to_string()),
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC response");
    assert_eq!(
        body,
        json!({"jsonrpc": "2.0", "id": 23, "result": tool_result})
    );
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    let page = state
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 10,
            ..Default::default()
        })
        .await
        .expect("invocation log");
    assert_eq!(page.total, 1);
    let invocation = &page.items[0];
    assert_eq!(invocation.status, McpToolInvocationStatus::Success);
    assert_eq!(invocation.policy_result, McpToolPolicyResult::Allowed);
    assert_eq!(invocation.request_id, "23");
    assert_eq!(invocation.error_code, None);
    let detail = state
        .store
        .get_mcp_tool_invocation_detail(invocation.mcp_tool_invocation_id)
        .await
        .expect("invocation detail");
    let payload = detail.payload.expect("captured payload");
    assert_eq!(payload.arguments_json, json!({"query": "oceans"}));
    assert_eq!(payload.result_json, tool_result);
    server.abort();
}

#[tokio::test]
async fn allowed_json_rpc_error_preserves_the_existing_tool_error_response() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_result_guardrail(
        &mut state,
        ManagedOutcome::Allow {
            reason_code: ReasonCode::new("test.allowed").unwrap(),
            metadata: Default::default(),
        },
    );
    let (body, invocation) = call_with_upstream_reply(
        &state,
        "error",
        json!({
            "code": -32000, "message": "private-message", "data": {"detail": "private-data"},
        }),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error_code"],
        "upstream_transport"
    );
    assert_eq!(
        body["result"]["content"][0]["text"],
        "upstream provider transport failure: MCP JSON-RPC error: JsonRpcErrorObject { code: -32000, message: \"private-message\", data: Some(Object {\"detail\": String(\"private-data\")}) }"
    );
    assert_eq!(invocation.status, McpToolInvocationStatus::UpstreamError);
    assert_eq!(invocation.policy_result, McpToolPolicyResult::Allowed);
    assert_eq!(invocation.error_code.as_deref(), Some("upstream_transport"));
    assert_eq!(
        invocation.metadata["guardrail_result"]["guardrail_decision"],
        "allowed"
    );
}

#[tokio::test]
async fn denied_json_rpc_error_hides_message_and_data_and_records_policy_denial() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_result_guardrail(
        &mut state,
        ManagedOutcome::Intervention {
            reason_code: ReasonCode::new("test.private_error").unwrap(),
            metadata: Default::default(),
        },
    );
    let (body, invocation) = call_with_upstream_reply(
        &state,
        "error",
        json!({
            "code": -32000, "message": "private-message", "data": {"detail": "private-data"},
        }),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error_code"],
        "guardrail_policy_denied"
    );
    assert_eq!(
        body["result"]["content"][0]["text"],
        "MCP result denied by guardrail policy"
    );
    assert!(!body.to_string().contains("private-"));
    assert_eq!(invocation.status, McpToolInvocationStatus::PolicyDenied);
    assert_eq!(invocation.policy_result, McpToolPolicyResult::Denied);
    assert_eq!(
        invocation.error_code.as_deref(),
        Some("guardrail_policy_denied")
    );
    assert_eq!(
        invocation.metadata["guardrail_result"]["guardrail_decision"],
        "denied"
    );
}

#[tokio::test]
async fn transformed_json_rpc_error_emits_only_sanitized_message_and_data() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    configure_result_guardrail(
        &mut state,
        ManagedOutcome::Transformed {
            transformation: ContentTransformation::new(
                json!({
                    "code": -32000,
                    "message": "sanitized-message",
                    "data": {"detail": "sanitized-data"},
                })
                .to_string(),
            ),
            reason_code: ReasonCode::new("test.redacted").unwrap(),
            metadata: Default::default(),
        },
    );
    let (body, invocation) = call_with_upstream_reply(
        &state,
        "error",
        json!({
            "code": -32000, "message": "private-message", "data": {"detail": "private-data"},
        }),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error_code"],
        "upstream_transport"
    );
    let output = body.to_string();
    assert!(!output.contains("private-"));
    assert!(output.contains("sanitized-message"));
    assert!(output.contains("sanitized-data"));
    assert_eq!(invocation.status, McpToolInvocationStatus::UpstreamError);
    assert_eq!(invocation.policy_result, McpToolPolicyResult::Allowed);
    assert_eq!(invocation.error_code.as_deref(), Some("upstream_transport"));
    assert_eq!(
        invocation.metadata["guardrail_result"]["guardrail_decision"],
        "transformed"
    );
}

#[tokio::test]
async fn malformed_upstream_result_does_not_expose_decoder_details() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let (body, invocation) = call_with_upstream_reply(
        &state,
        "result",
        json!({
            "content": [], "isError": "private-decoder-secret",
        }),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error_code"],
        "upstream_transport"
    );
    assert_eq!(
        body["result"]["content"][0]["text"],
        "upstream provider transport failure: invalid MCP response"
    );
    assert!(!body.to_string().contains("private-decoder-secret"));
    assert_eq!(invocation.status, McpToolInvocationStatus::UpstreamError);
    assert_eq!(invocation.policy_result, McpToolPolicyResult::Allowed);
    assert_eq!(invocation.error_code.as_deref(), Some("upstream_transport"));
}

fn configure_result_guardrail(state: &mut AppState, outcome: ManagedOutcome) {
    let config = Arc::make_mut(&mut state.guardrail_config);
    config.default.enabled = true;
    config.default.mode = PolicyMode::Deny;
    config.default.managed_checks = vec!["result-policy".to_string()];
    config.managed_checks.insert(
        "result-policy".to_string(),
        ManagedCheckConfig {
            kind: ManagedCheckKind::GoogleModelArmor,
            phases: BTreeSet::from([GuardPhase::McpResult]),
            timeout_ms: 1_000,
            failure_disposition: FailureDisposition::FailClosed,
            max_content_bytes: 4_096,
            bedrock: None,
            model_armor: None,
        },
    );
    state.guardrail_engine = Arc::new(GuardrailEngine::new(
        Vec::new(),
        BTreeMap::from([(
            "result-policy".to_string(),
            Arc::new(StubManagedEvaluator::new(
                "result-policy",
                ManagedService::GoogleModelArmor,
                [Ok(outcome)],
            )) as Arc<dyn gateway_guardrails::ManagedEvaluator>,
        )]),
    ));
}

async fn call_with_upstream_reply(
    state: &AppState,
    response_field: &'static str,
    payload: Value,
) -> (Value, McpToolInvocationRecord) {
    let (url, requests, server) = spawn_tools_call_upstream(response_field, payload).await;
    let auth = seed_authorized_tool(state, &url, ExternalMcpAuthMode::None).await;
    let response = call_catalog_tool(
        state,
        &auth,
        JsonRpcId::Number(23),
        CallMcpToolInput {
            address: "mcp://repository/tools/search".to_string(),
            arguments: json!({"query": "oceans"}),
            schema_hash: Some("search-v1".to_string()),
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC response");
    assert_eq!(body["id"], 23);
    assert_eq!(body["result"]["isError"], true);
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    let page = state
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 10,
            ..Default::default()
        })
        .await
        .expect("invocation log");
    assert_eq!(page.total, 1);
    server.abort();
    (body, page.items.into_iter().next().expect("invocation"))
}

async fn spawn_tools_call_upstream(
    response_field: &'static str,
    payload: Value,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let request_count = requests.clone();
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |Json(message): Json<Value>| {
            let requests = request_count.clone();
            let payload = payload.clone();
            async move {
                requests.fetch_add(1, Ordering::SeqCst);
                match message["method"].as_str().expect("MCP method") {
                    "initialize" => Json(json!({
                        "jsonrpc": "2.0", "id": message["id"],
                        "result": initialize_result("test-upstream", "1"),
                    }))
                    .into_response(),
                    "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
                    "tools/call" => {
                        assert_eq!(message["params"]["name"], "search");
                        assert_eq!(message["params"]["arguments"], json!({"query": "oceans"}));
                        let mut response = json!({"jsonrpc": "2.0", "id": message["id"]});
                        response[response_field] = payload;
                        Json(response).into_response()
                    }
                    method => panic!("unexpected MCP method: {method}"),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("upstream");
    });
    (url, requests, server)
}

async fn assert_failed_invocation(
    server_url: &str,
    auth_mode: ExternalMcpAuthMode,
    expected_status: McpToolInvocationStatus,
    expected_error_code: &str,
) {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let auth = seed_authorized_tool(&state, server_url, auth_mode).await;
    let response = call_catalog_tool(
        &state,
        &auth,
        JsonRpcId::String("failed-call".to_string()),
        CallMcpToolInput {
            address: "mcp://repository/tools/search".to_string(),
            arguments: json!({"query": "private-call-argument"}),
            schema_hash: None,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC response");
    assert_eq!(body["id"], "failed-call");
    assert_eq!(body["result"]["isError"], true);
    assert_eq!(
        body["result"]["structuredContent"]["error_code"],
        expected_error_code
    );
    assert!(!String::from_utf8_lossy(&bytes).contains("private-call-argument"));

    let page = state
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 10,
            ..Default::default()
        })
        .await
        .expect("invocation log");
    assert_eq!(
        page.total, 1,
        "each authorized failure must be logged exactly once"
    );
    let invocation = &page.items[0];
    assert_eq!(invocation.status, expected_status);
    assert_eq!(invocation.policy_result, McpToolPolicyResult::Allowed);
    assert_eq!(invocation.error_code.as_deref(), Some(expected_error_code));
    assert_eq!(invocation.api_key_id, Some(auth.id));
    assert_eq!(invocation.user_id, auth.owner_user_id);
    assert_eq!(invocation.server_display_key, "repository");
    assert_eq!(invocation.tool_display_key, "search");
    assert_eq!(invocation.metadata["mcp_route"], "aggregate");
    assert!(invocation.metadata.contains_key("guardrail_call"));
}

async fn seed_authorized_tool(
    state: &AppState,
    server_url: &str,
    auth_mode: ExternalMcpAuthMode,
) -> AuthenticatedApiKey {
    let store = &state.store;
    let now = OffsetDateTime::now_utc();
    let user = store
        .create_identity_user(
            "Tool caller",
            "caller@example.test",
            "caller@example.test",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let key = store
        .create_api_key(&NewApiKeyRecord {
            name: "Tool caller".to_string(),
            public_id: "caller".to_string(),
            secret_hash: "unused-secret-hash".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            created_at: now,
        })
        .await
        .expect("API key");
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "repository".to_string(),
            display_name: "Repository".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: server_url.to_string(),
            auth_mode,
            auth_config: Map::new(),
            timeout_ms: 2_000,
            created_at: now,
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
    AuthenticatedApiKey {
        id: key.id,
        public_id: key.public_id,
        name: key.name,
        model_grant_mode: key.model_grant_mode,
        owner_kind: key.owner_kind,
        owner_user_id: key.owner_user_id,
        owner_team_id: key.owner_team_id,
        owner_service_account_id: key.owner_service_account_id,
    }
}
