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

fn search_tool_metadata() -> gateway_core::McpToolMetadata {
    gateway_core::McpToolMetadata {
        title: Some("Search repository".to_string()),
        annotations: Some(Map::from_iter([
            ("readOnlyHint".to_string(), json!(true)),
            ("openWorldHint".to_string(), json!(false)),
        ])),
        output_schema: Some(json!({"type": "object", "properties": {"hits": {"type": "array"}}})),
    }
}

async fn builtin_tool_result(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    name: &str,
    arguments: Value,
) -> Value {
    let response = call_builtin_tool(
        state,
        &crate::http::request_policy::RequestPolicyContext::default(),
        auth,
        JsonRpcId::Number(5),
        name.to_string(),
        arguments,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC response");
    body["result"]["structuredContent"].clone()
}

#[tokio::test]
async fn describe_and_search_return_declared_tool_metadata() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let auth =
        seed_authorized_tool(&state, "http://127.0.0.1:9/mcp", ExternalMcpAuthMode::None).await;
    let expected = search_tool_metadata();

    let described = builtin_tool_result(
        &state,
        &auth,
        "describe_tool",
        json!({"address": "mcp://repository/tools/search"}),
    )
    .await;
    let tool = &described["tool"];
    assert_eq!(tool["title"], "Search repository");
    assert_eq!(tool["annotations"], json!(expected.annotations));
    assert_eq!(tool["output_schema"], expected.output_schema.unwrap());

    let searched =
        builtin_tool_result(&state, &auth, "search_tools", json!({"query": "search"})).await;
    let summary = &searched["items"][0]["tool"];
    assert_eq!(summary["title"], "Search repository");
    assert_eq!(summary["annotations"]["readOnlyHint"], true);
    assert!(summary.get("output_schema").is_none());
}

#[tokio::test]
async fn tag_rules_govern_aggregate_search_describe_and_call() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let auth =
        seed_authorized_tool(&state, "http://127.0.0.1:9/mcp", ExternalMcpAuthMode::None).await;
    let server = state
        .store
        .get_external_mcp_server_by_key("repository")
        .await
        .expect("server lookup")
        .expect("server");
    let search = state
        .store
        .get_active_mcp_tool_by_name(server.mcp_server_id, "search")
        .await
        .expect("tool lookup")
        .expect("search tool");
    // Swap the explicit grant for an include rule on the user.
    state
        .store
        .revoke_mcp_tool_grant(
            McpToolGrantSubjectKind::ApiKey,
            auth.id,
            McpToolGrantTargetKind::Tool,
            search.mcp_tool_id,
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("revoke grant");
    let rule = |mcp_server_id, tag: &str, effect| gateway_core::McpToolTagRuleKey {
        subject_kind: McpToolGrantSubjectKind::User,
        subject_id: auth.owner_user_id.expect("owner user"),
        mcp_server_id,
        tag: tag.to_string(),
        effect,
    };
    state
        .store
        .upsert_mcp_tool_tag_rule(
            &rule(
                Some(server.mcp_server_id),
                "read_only",
                gateway_core::McpTagRuleEffect::Include,
            ),
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("include rule");
    state
        .store
        .set_mcp_tool_annotation_overrides(
            search.mcp_tool_id,
            &gateway_core::McpAnnotationOverrides {
                open_world: Some(true),
                ..Default::default()
            },
        )
        .await
        .expect("override");

    let searched =
        builtin_tool_result(&state, &auth, "search_tools", json!({"query": "search"})).await;
    assert_eq!(searched["total"], 1);
    // Clients see the admin's value, not the upstream's `openWorldHint: false`.
    assert_eq!(
        searched["items"][0]["tool"]["annotations"]["openWorldHint"],
        true
    );
    let described = builtin_tool_result(
        &state,
        &auth,
        "describe_tool",
        json!({"address": "mcp://repository/tools/search"}),
    )
    .await;
    assert_eq!(described["tool"]["annotations"]["openWorldHint"], true);
    assert_eq!(described["tool"]["schema_hash"], "search-v1");

    state
        .store
        .upsert_mcp_tool_tag_rule(
            &rule(None, "open_world", gateway_core::McpTagRuleEffect::Exclude),
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("exclude rule");
    let searched =
        builtin_tool_result(&state, &auth, "search_tools", json!({"query": "search"})).await;
    assert_eq!(searched["total"], 0);
    let response = call_builtin_tool(
        &state,
        &crate::http::request_policy::RequestPolicyContext::default(),
        &auth,
        JsonRpcId::Number(6),
        "call_tool".to_string(),
        json!({"address": "mcp://repository/tools/search", "arguments": {}}),
    )
    .await;
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("not granted"), "{body}");
}

#[tokio::test]
async fn missing_oauth_credential_returns_the_connect_url() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    state.mcp_oauth_runtime = Arc::new(gateway_service::McpOauthRuntime::new(
        Some("https://gateway.example".to_string()),
        vec![gateway_service::McpOauthProvider {
            key: "google".to_string(),
            provider_type: gateway_service::McpOauthProviderType::Google,
            client_id: "provider-client".to_string(),
            client_secret: Some("provider-client-secret".to_string()),
            authorization_url: "https://accounts.example/authorize".to_string(),
            token_url: "https://accounts.example/token".to_string(),
            token_endpoint_auth_method:
                gateway_service::McpOauthTokenEndpointAuthMethod::ClientSecretPost,
            scopes: Vec::new(),
        }],
    ));
    let auth_config = serde_json::from_value(json!({
        "provider_key": "google",
        "resource": "https://drive.example/mcp",
        "scopes": ["https://www.googleapis.com/auth/drive.readonly"],
    }))
    .expect("auth config");
    let auth = seed_authorized_tool_with_config(
        &state,
        "https://drive.example/mcp",
        ExternalMcpAuthMode::OauthObo,
        auth_config,
    )
    .await;
    let connect_url = "https://gateway.example/admin/account/connections?connect=repository";

    let response = call_catalog_tool(
        &state,
        &crate::http::request_policy::RequestPolicyContext::default(),
        &auth,
        JsonRpcId::Number(9),
        CallMcpToolInput {
            address: "mcp://repository/tools/search".to_string(),
            arguments: json!({}),
            schema_hash: None,
        },
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 16_384).await.expect("body");
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("provider-client-secret"));
    assert!(!text.contains(&auth.public_id));
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC response");
    let result = &body["result"];
    assert_eq!(result["isError"], true);
    assert_eq!(
        result["content"][0]["text"],
        format!(
            "MCP credential is required for server `repository`. Connect your account at {connect_url}, then retry the call."
        )
    );
    assert_eq!(
        result["structuredContent"],
        json!({
            "address": "mcp://repository/tools/search",
            "server_key": "repository",
            "connect_url": connect_url,
            "error_code": "credential_required",
        })
    );
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
        &crate::http::request_policy::RequestPolicyContext::default(),
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
async fn invocation_payload_uses_the_configured_secret_redactor() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    let payload_policy = gateway_service::RequestLogPayloadPolicy::default().with_secret_redactor(
        gateway_service::PayloadSecretRedactor::new(|value| replace_string(value, "oceans")),
    );
    state.service = Arc::new(
        gateway_service::GatewayService::new_with_budget_alert_sender_and_payload_policy(
            state.store.clone(),
            Arc::new(gateway_service::WeightedRoutePlanner::default()),
            Arc::new(gateway_service::SinkBudgetAlertSender),
            payload_policy,
        ),
    );
    let (_, invocation) = call_with_upstream_reply(
        &state,
        "result",
        json!({"content": [{"type": "text", "text": "oceans"}], "isError": true}),
    )
    .await;
    let detail = state
        .store
        .get_mcp_tool_invocation_detail(invocation.mcp_tool_invocation_id)
        .await
        .expect("invocation detail");
    let payload = detail.payload.expect("captured payload");
    assert_eq!(payload.arguments_json, json!({"query": "[REDACTED:test]"}));
    assert!(!payload.result_json.to_string().contains("oceans"));
}

fn replace_string(value: &mut Value, secret: &str) {
    match value {
        Value::String(text) if text == secret => *text = "[REDACTED:test]".to_string(),
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| replace_string(item, secret)),
        Value::Object(map) => map
            .values_mut()
            .for_each(|item| replace_string(item, secret)),
        _ => {}
    }
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
        &crate::http::request_policy::RequestPolicyContext::default(),
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
        &crate::http::request_policy::RequestPolicyContext::default(),
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
    seed_authorized_tool_with_config(state, server_url, auth_mode, Map::new()).await
}

async fn seed_authorized_tool_with_config(
    state: &AppState,
    server_url: &str,
    auth_mode: ExternalMcpAuthMode,
    auth_config: Map<String, Value>,
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
            auth_config,
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
                metadata: search_tool_metadata(),
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
        oauth: None,
    }
}

#[tokio::test]
async fn rate_limited_call_tool_returns_a_json_rpc_429_and_is_logged() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    crate::http::test_support::install_rate_limits(
        &mut state,
        vec![gateway_service::ConfiguredRateLimitPolicy {
            name: "user tool calls".to_string(),
            operation: gateway_core::RateLimitOperation::McpToolCall,
            scope_kind: gateway_core::RateLimitScopeKind::User,
            model_key: None,
            mcp_server_key: None,
            metric: gateway_core::RateLimitMetric::Requests,
            period: gateway_core::RateLimitPeriod::Day,
            limit: 1,
        }],
    );
    let (url, requests, server) = spawn_tools_call_upstream(
        "result",
        json!({"content": [{"type": "text", "text": "ok"}]}),
    )
    .await;
    let auth = seed_authorized_tool(&state, &url, ExternalMcpAuthMode::None).await;
    let policy = crate::http::request_policy::RequestPolicyContext::default();
    let call = |id| {
        call_catalog_tool(
            &state,
            &policy,
            &auth,
            JsonRpcId::Number(id),
            CallMcpToolInput {
                address: "mcp://repository/tools/search".to_string(),
                arguments: json!({"query": "oceans"}),
                schema_hash: Some("search-v1".to_string()),
            },
        )
    };

    assert_eq!(call(1).await.status(), StatusCode::OK);
    let upstream_calls = requests.load(Ordering::SeqCst);

    let response = call(2).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.expect("body"))
            .expect("JSON-RPC error");
    assert_eq!(body["id"], 2);
    assert_eq!(body["error"]["code"], -32029);
    assert_eq!(body["error"]["data"]["error_code"], "rate_limited");
    assert!(
        body["error"]["data"]["retry_after_seconds"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("User rate limit exceeded: 1 MCP tool calls per calendar day (UTC)")
    );
    assert_eq!(requests.load(Ordering::SeqCst), upstream_calls);

    let page = state
        .store
        .list_mcp_tool_invocations(&McpToolInvocationQuery {
            page: 1,
            page_size: 10,
            ..Default::default()
        })
        .await
        .expect("invocation log");
    let denied = page
        .items
        .iter()
        .find(|invocation| invocation.error_code.as_deref() == Some("rate_limited"))
        .expect("rate-limited invocation");
    assert_eq!(denied.status, McpToolInvocationStatus::PolicyDenied);
    assert_eq!(denied.policy_result, McpToolPolicyResult::Denied);
    assert_eq!(denied.metadata["mcp_route"], "aggregate");
    assert_eq!(denied.metadata["rate_limit"]["scope_kind"], "user");
    assert_eq!(denied.metadata["rate_limit"]["policy"], "user tool calls");
    server.abort();
}

#[tokio::test]
async fn closed_rate_limit_failures_return_http_503_on_call_tool() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    crate::http::test_support::install_down_rate_limiter(&mut state);
    let (url, requests, server) = spawn_tools_call_upstream(
        "result",
        json!({"content": [{"type": "text", "text": "ok"}]}),
    )
    .await;
    let auth = seed_authorized_tool(&state, &url, ExternalMcpAuthMode::None).await;
    let response = call_catalog_tool(
        &state,
        &crate::http::request_policy::RequestPolicyContext::default(),
        &auth,
        JsonRpcId::Number(7),
        CallMcpToolInput {
            address: "mcp://repository/tools/search".to_string(),
            arguments: json!({"query": "oceans"}),
            schema_hash: Some("search-v1".to_string()),
        },
    )
    .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await.expect("body"))
            .expect("JSON-RPC error");
    assert_eq!(body["id"], 7);
    assert_eq!(body["error"]["code"], -32603);
    assert!(body.get("result").is_none());
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    server.abort();
}
