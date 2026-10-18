use super::*;
use axum::http::{HeaderMap, HeaderValue};
use json_rpc::GovernedMcpMethod;

#[test]
fn auth_extractor_accepts_authorization_only() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    assert_eq!(
        extract_mcp_bearer_token(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_accepts_explicit_header_only() {
    let mut headers = HeaderMap::new();
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    assert_eq!(
        extract_mcp_bearer_token(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_accepts_identical_dual_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    assert_eq!(
        extract_mcp_bearer_token(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_rejects_missing_credentials() {
    let headers = HeaderMap::new();
    let error = extract_mcp_bearer_token(&headers).expect_err("missing");
    assert!(matches!(error, AuthError::MissingAuthorizationHeader));
}

#[test]
fn auth_extractor_rejects_malformed_authorization_even_with_explicit_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Basic gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    let error = extract_mcp_bearer_token(&headers).expect_err("malformed");
    assert!(matches!(error, AuthError::InvalidAuthorizationHeader));
}

#[test]
fn auth_extractor_rejects_conflicting_dual_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.other"));
    let error = extract_mcp_bearer_token(&headers).expect_err("conflict");
    assert!(matches!(error, AuthError::ConflictingApiKeyHeaders));
}

mod method_classification {
    use super::*;

    fn classify(http_method: Method, body: Value) -> McpRpcRequest {
        parse_mcp_rpc_request(&http_method, body.to_string().as_bytes()).expect("classified")
    }

    #[test]
    fn lifecycle_methods_pass_through() {
        assert!(matches!(
            classify(
                Method::POST,
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
            ),
            McpRpcRequest::Initialize
        ));
        for body in [
            json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "logging/setLevel", "params": {"level": "info"}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}}),
            json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": 1, "progress": 1}}),
            json!({"jsonrpc": "2.0", "method": "notifications/roots/list_changed"}),
            json!({"jsonrpc": "2.0", "id": "server-1", "result": {"roots": []}}),
        ] {
            assert!(
                matches!(
                    classify(Method::POST, body.clone()),
                    McpRpcRequest::Lifecycle
                ),
                "{body} should be lifecycle"
            );
        }
        assert!(matches!(
            parse_mcp_rpc_request(&Method::DELETE, b"").expect("delete"),
            McpRpcRequest::Lifecycle
        ));
    }

    #[test]
    fn task_augmented_tool_calls_are_rejected() {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "search", "arguments": {}, "task": {"ttl": 60000}},
        });
        assert!(parse_mcp_rpc_request(&Method::POST, body.to_string().as_bytes()).is_err());
    }

    #[test]
    fn get_without_body_opens_the_event_stream() {
        assert!(matches!(
            parse_mcp_rpc_request(&Method::GET, b"").expect("get"),
            McpRpcRequest::EventStream
        ));
    }

    #[test]
    fn get_and_delete_with_a_body_are_rejected() {
        let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}).to_string();
        for method in [Method::GET, Method::DELETE] {
            assert!(
                parse_mcp_rpc_request(&method, ping.as_bytes()).is_err(),
                "{method} with a body should be rejected"
            );
        }
    }

    #[test]
    fn post_without_body_is_rejected() {
        assert!(parse_mcp_rpc_request(&Method::POST, b"").is_err());
    }

    #[test]
    fn resources_prompts_and_completions_are_governed() {
        let McpRpcRequest::Governed { method, target, .. } = classify(
            Method::POST,
            json!({"jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": {"uri": "file:///notes.md"}}),
        ) else {
            panic!("resources/read should be governed");
        };
        assert_eq!(method, GovernedMcpMethod::ResourcesRead);
        assert_eq!(target.as_deref(), Some("file:///notes.md"));

        let McpRpcRequest::Governed { method, target, .. } = classify(
            Method::POST,
            json!({"jsonrpc": "2.0", "id": 2, "method": "prompts/get", "params": {"name": "summarise"}}),
        ) else {
            panic!("prompts/get should be governed");
        };
        assert_eq!(method, GovernedMcpMethod::PromptsGet);
        assert_eq!(target.as_deref(), Some("summarise"));

        for name in [
            "resources/list",
            "resources/templates/list",
            "resources/subscribe",
            "resources/unsubscribe",
            "prompts/list",
            "completion/complete",
        ] {
            assert!(
                matches!(
                    classify(
                        Method::POST,
                        json!({"jsonrpc": "2.0", "id": 3, "method": name})
                    ),
                    McpRpcRequest::Governed { .. }
                ),
                "{name} should be governed"
            );
        }
    }

    #[test]
    fn unknown_methods_and_notification_shaped_requests_are_denied() {
        assert!(matches!(
            classify(
                Method::POST,
                json!({"jsonrpc": "2.0", "id": 1, "method": "vendor/export"})
            ),
            McpRpcRequest::Unknown { .. }
        ));
        // A request that expects a reply is not a notification, whatever its name.
        assert!(matches!(
            classify(
                Method::POST,
                json!({"jsonrpc": "2.0", "id": 2, "method": "notifications/resources/read"})
            ),
            McpRpcRequest::Unknown { .. }
        ));
        // Only the notifications MCP defines for clients are forwarded.
        assert!(matches!(
            classify(
                Method::POST,
                json!({"jsonrpc": "2.0", "method": "notifications/vendor/delete_all"})
            ),
            McpRpcRequest::Unknown { id: None, .. }
        ));
    }

    #[test]
    fn malformed_client_replies_are_rejected() {
        for body in [
            json!({"id": "server-1", "result": {}}),
            json!({"jsonrpc": "1.0", "id": "server-1", "result": {}}),
            json!({"jsonrpc": "2.0", "id": "server-1", "result": {}, "error": {"code": 1}}),
        ] {
            assert!(
                parse_mcp_rpc_request(&Method::POST, body.to_string().as_bytes()).is_err(),
                "{body} should be rejected"
            );
        }
    }

    #[test]
    fn unknown_method_names_are_truncated() {
        let McpRpcRequest::Unknown { method, .. } = classify(
            Method::POST,
            json!({"jsonrpc": "2.0", "id": 1, "method": "x".repeat(1_000)}),
        ) else {
            panic!("expected unknown");
        };
        assert_eq!(method.len(), 256);
    }
}

mod proxy_policy {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{body::to_bytes, http::header::CONTENT_TYPE};
    use gateway_core::{
        AdminApiKeyRepository, ApiKeyModelGrantMode, AuthMode, ExternalMcpAuthMode,
        ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus, ExternalMcpTransport,
        GlobalRole, McpAccessRepository, McpRegistryRepository, McpToolGrantSubjectKind,
        McpToolGrantTargetKind, McpToolInvocationQuery, McpToolInvocationRecord,
        McpToolInvocationRepository, NewApiKeyRecord, NewExternalMcpServerRecord,
        UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord, UserStatus,
    };
    use gateway_guardrails::{
        FailureDisposition, GuardPhase, GuardrailEngine, ManagedCheckConfig, ManagedCheckKind,
        ManagedOutcome, ManagedService, PolicyMode, ReasonCode, test_utils::StubManagedEvaluator,
    };
    use gateway_store::GatewayStore;

    use super::*;
    use crate::http::state::AppState;

    const SECRET_TEXT: &str = "private-resource-text";
    /// `seed_server` key, as `gwk_reader.<secret>`.
    const READER_SECRET: &str = "reader-secret";

    struct Upstream {
        url: String,
        requests: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Upstream {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    /// Answers lifecycle, resource, and prompt requests. `prompts/get` replies
    /// over SSE so the guarded SSE path is exercised.
    async fn spawn_upstream() -> Upstream {
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(move |Json(message): Json<Value>| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let id = message["id"].clone();
                    match message["method"].as_str().unwrap_or_default() {
                        "initialize" => Json(json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": {
                                "protocolVersion": "2025-11-25",
                                "capabilities": {"logging": {}, "tasks": {"list": {}, "cancel": {}, "requests": {"tools": {"call": {}}}}},
                                "serverInfo": {"name": "upstream", "version": "1"},
                            },
                        }))
                        .into_response(),
                        "tools/list" => {
                            let mut search = annotated_search_tool();
                            search["execution"] = json!({"taskSupport": "optional"});
                            Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {"tools": [search, {"name": "purge", "inputSchema": {"type": "object"}}]},
                            }))
                            .into_response()
                        }
                        "resources/read" => Json(json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": {"contents": [{"uri": message["params"]["uri"], "text": SECRET_TEXT}]},
                        }))
                        .into_response(),
                        "prompts/get" => {
                            let event = json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {"messages": [{"role": "user", "content": {"type": "text", "text": SECRET_TEXT}}]},
                            });
                            (
                                [(CONTENT_TYPE, "text/event-stream")],
                                format!("event: message\ndata: {event}\n\n"),
                            )
                                .into_response()
                        }
                        _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})).into_response(),
                    }
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
        Upstream {
            url,
            requests,
            task,
        }
    }

    /// Seeds a user API key and one server with one tool. The key gets a grant on
    /// that tool only when `granted` is set.
    async fn seed_server(state: &AppState, server_url: &str, granted: bool) -> AuthenticatedApiKey {
        let store = &state.store;
        let now = OffsetDateTime::now_utc();
        let user = store
            .create_identity_user(
                "Resource reader",
                "reader@example.test",
                "reader@example.test",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("user");
        let key = store
            .create_api_key(&NewApiKeyRecord {
                name: "Resource reader".to_string(),
                public_id: "reader".to_string(),
                secret_hash: gateway_core::hash_gateway_key_secret(READER_SECRET)
                    .expect("hash reader key"),
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
                    expected_server: None,
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
        if granted {
            store
                .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
                    subject_kind: McpToolGrantSubjectKind::ApiKey,
                    subject_id: key.id,
                    target_kind: McpToolGrantTargetKind::Tool,
                    target_id: tools[0].mcp_tool_id,
                    updated_at: now,
                    expected_server: None,
                    expected_subject_team: None,
                })
                .await
                .expect("tool grant");
        }
        AuthenticatedApiKey {
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
        }
    }

    fn deny_results(state: &mut AppState) {
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
                    [Ok(ManagedOutcome::Intervention {
                        reason_code: ReasonCode::new("test.private_resource").unwrap(),
                        metadata: Default::default(),
                    })],
                )) as Arc<dyn gateway_guardrails::ManagedEvaluator>,
            )]),
        ));
    }

    async fn send(
        state: &AppState,
        auth: &AuthenticatedApiKey,
        message: Value,
    ) -> (StatusCode, String) {
        send_as(state, auth, &Method::POST, message).await
    }

    async fn send_as(
        state: &AppState,
        auth: &AuthenticatedApiKey,
        http_method: &Method,
        message: Value,
    ) -> (StatusCode, String) {
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
            "notes",
            http_method,
            &headers,
            Bytes::from(message.to_string()),
            Instant::now(),
        )
        .await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 65_536).await.expect("body");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn invocations(state: &AppState) -> Vec<McpToolInvocationRecord> {
        state
            .store
            .list_mcp_tool_invocations(&McpToolInvocationQuery {
                page: 1,
                page_size: 50,
                ..Default::default()
            })
            .await
            .expect("invocation log")
            .items
    }

    /// The granted `search` tool as the upstream declares it, including members
    /// the gateway does not interpret.
    fn annotated_search_tool() -> Value {
        json!({
            "name": "search",
            "title": "Search notes",
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object", "properties": {"hits": {"type": "array"}}},
            "annotations": {"readOnlyHint": true, "openWorldHint": false, "x-extra": 1},
            "_meta": {"origin": "upstream"},
        })
    }

    #[tokio::test]
    async fn event_stream_replies_to_posts_are_watched_like_the_get_stream() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // The upstream answers `prompts/get` as an event stream.
        let response = dispatch_mcp_request(
            &state,
            &crate::http::request_policy::RequestPolicyContext::default(),
            &auth,
            "notes",
            &Method::POST,
            &headers,
            Bytes::from(
                json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/get", "params": {"name": "summarise"}})
                    .to_string(),
            ),
            Instant::now(),
        )
        .await;
        assert!(super::super::stream_reach::is_event_stream(&response));
        assert_eq!(state.mcp_stream_reach.watched(), 1);
        drop(response);
        assert_eq!(
            state.mcp_stream_reach.watched(),
            0,
            "a dropped stream stops being watched"
        );
    }

    #[tokio::test]
    async fn another_teams_team_only_server_does_not_exist_for_lifecycle_methods() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;
        let team = state
            .store
            .create_team("lifecycle", "Lifecycle")
            .await
            .unwrap();
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .unwrap()
            .unwrap();
        state
            .store
            .transfer_external_mcp_server_ownership(
                &gateway_core::TransferExternalMcpServerOwnershipRecord {
                    mcp_server_id: server.mcp_server_id,
                    ownership: gateway_core::McpServerOwnership::team(team.team_id, None),
                    expected_ownership: None,
                    updated_at: OffsetDateTime::now_utc(),
                    require_no_stranded_access: false,
                },
            )
            .await
            .unwrap();
        let initialize = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}},
        });

        // Outside the owning team, the server answers like a missing one and
        // its upstream is never contacted.
        let (status, body) = send(&state, &auth, initialize.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);

        // A member of the owning team reaches it without any grant.
        state
            .store
            .assign_team_membership(
                auth.owner_user_id.unwrap(),
                team.team_id,
                gateway_core::MembershipRole::Member,
            )
            .await
            .unwrap();
        let (status, body) = send(&state, &auth, initialize).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(upstream.requests.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn tools_list_keeps_declared_metadata_for_granted_tools() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC response");
        // The upstream also sends `execution.taskSupport`, which the gateway drops
        // because it does not proxy tasks.
        assert_eq!(body["result"]["tools"], json!([annotated_search_tool()]));
    }

    /// Adds a tag rule for the key's owner user on the `notes` server.
    async fn add_user_rule(
        state: &AppState,
        auth: &AuthenticatedApiKey,
        server: Option<Uuid>,
        tag: &str,
        effect: gateway_core::McpTagRuleEffect,
    ) {
        state
            .store
            .upsert_mcp_tool_tag_rule(
                &gateway_core::McpToolTagRuleKey {
                    subject_kind: McpToolGrantSubjectKind::User,
                    subject_id: auth.owner_user_id.expect("owner user"),
                    mcp_server_id: server,
                    tag: tag.to_string(),
                    effect,
                },
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("tag rule");
    }

    #[tokio::test]
    async fn tag_rules_govern_direct_tools_list_calls_and_resources() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .expect("server lookup")
            .expect("server");
        let search = state
            .store
            .get_active_mcp_tool_by_name(server.mcp_server_id, "search")
            .await
            .expect("tool lookup")
            .expect("search tool");
        // The stored tool declares no hints, so only an override makes it read-only.
        state
            .store
            .set_mcp_tool_annotation_overrides(
                search.mcp_tool_id,
                &gateway_core::McpAnnotationOverrides {
                    read_only: Some(true),
                    idempotent: Some(true),
                    ..Default::default()
                },
            )
            .await
            .expect("override");
        add_user_rule(
            &state,
            &auth,
            Some(server.mcp_server_id),
            "read_only",
            gateway_core::McpTagRuleEffect::Include,
        )
        .await;

        let list = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"});
        let (status, body) = send(&state, &auth, list.clone()).await;
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC response");
        let mut expected = annotated_search_tool();
        expected["annotations"]["idempotentHint"] = json!(true);
        assert_eq!(body["result"]["tools"], json!([expected]));
        let call = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "search", "arguments": {}},
        });
        let (status, _) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&state, &auth, read_resource()).await;
        assert_eq!(status, StatusCode::OK);

        // An exclude rule on every server wins over the include rule.
        add_user_rule(
            &state,
            &auth,
            None,
            "idempotent",
            gateway_core::McpTagRuleEffect::Exclude,
        )
        .await;
        let (status, body) = send(&state, &auth, list).await;
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC response");
        assert_eq!(body["result"]["tools"], json!([]));
        let (status, _) = send(&state, &auth, call).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send(&state, &auth, read_resource()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// Discovers `search` again, as `schema_hash`, with an upstream claim of
    /// `readOnlyHint`.
    async fn claim_read_only(
        state: &AppState,
        server: &gateway_core::ExternalMcpServerRecord,
        schema_hash: &str,
    ) {
        let now = OffsetDateTime::now_utc();
        state
            .store
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
                    expected_server: None,
                },
                &[UpsertExternalMcpToolRecord {
                    mcp_server_id: server.mcp_server_id,
                    upstream_name: "search".to_string(),
                    display_name: "Search".to_string(),
                    description: None,
                    input_schema: json!({"type": "object"}),
                    metadata: gateway_core::McpToolMetadata {
                        annotations: json!({"readOnlyHint": true}).as_object().cloned(),
                        ..Default::default()
                    },
                    schema_hash: schema_hash.to_string(),
                }],
            )
            .await
            .expect("rediscovery");
    }

    #[tokio::test]
    async fn upstream_hint_claims_alone_do_not_satisfy_derived_include_rules() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .expect("server lookup")
            .expect("server");
        claim_read_only(&state, &server, "search-v2").await;
        add_user_rule(
            &state,
            &auth,
            Some(server.mcp_server_id),
            "read_only",
            gateway_core::McpTagRuleEffect::Include,
        )
        .await;
        let call = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "search", "arguments": {}},
        });

        // The server takes upstream changes without review, so the upstream's
        // own `readOnlyHint` does not grant the tool.
        let (status, _) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // An admin override vouches for the hint.
        let search = state
            .store
            .get_active_mcp_tool_by_name(server.mcp_server_id, "search")
            .await
            .expect("tool lookup")
            .expect("search tool");
        state
            .store
            .set_mcp_tool_annotation_overrides(
                search.mcp_tool_id,
                &gateway_core::McpAnnotationOverrides {
                    read_only: Some(true),
                    ..Default::default()
                },
            )
            .await
            .expect("override");
        let (status, _) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::OK);

        // Turning approval on does not vouch for the definition discovery
        // approved before it.
        state
            .store
            .set_mcp_tool_annotation_overrides(search.mcp_tool_id, &Default::default())
            .await
            .expect("clear override");
        let (status, _) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        state
            .store
            .update_external_mcp_server(&gateway_core::UpdateExternalMcpServerRecord {
                mcp_server_id: server.mcp_server_id,
                display_name: server.display_name.clone(),
                description: server.description.clone(),
                server_url: server.server_url.clone(),
                auth_mode: server.auth_mode,
                auth_config: server.auth_config.clone(),
                timeout_ms: server.timeout_ms,
                owner_contact: None,
                visibility: server.ownership.visibility,
                retarget_owner_credentials_only: false,
                expected_server: None,
                expected_owner_team_id: server.ownership.owner_team_id,
                expected_visibility: server.ownership.visibility,
                vouch_for_destinations: false,
                updated_at: OffsetDateTime::now_utc(),
                approve_tool_changes: true,
            })
            .await
            .expect("enable approval");
        let (status, _) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // An admin approving the next definition does.
        claim_read_only(&state, &server, "search-v3").await;
        let pending = state
            .store
            .get_active_mcp_tool_by_name(server.mcp_server_id, "search")
            .await
            .expect("tool lookup")
            .expect("search tool")
            .review
            .pending_schema_version
            .expect("pending version");
        state
            .store
            .review_mcp_tool_version(
                search.mcp_tool_id,
                pending,
                gateway_core::McpToolReviewDecision::Approve,
                None,
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("approve");
        let (status, _) = send(&state, &auth, call).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn approval_servers_serve_the_approved_definition_and_withhold_pending_tools() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .expect("server lookup")
            .expect("server");
        // Without approval the live upstream definition passes through, so a
        // call records no stored version.
        let (status, _) = send(
            &state,
            &auth,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "search", "arguments": {}},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let passthrough = invocations(&state)
            .await
            .into_iter()
            .find(|invocation| invocation.mcp_method == "tools/call")
            .expect("passthrough call");
        assert_eq!(passthrough.tool_schema_version, None);
        state
            .store
            .update_external_mcp_server(&gateway_core::UpdateExternalMcpServerRecord {
                mcp_server_id: server.mcp_server_id,
                display_name: server.display_name.clone(),
                description: server.description.clone(),
                server_url: server.server_url.clone(),
                auth_mode: server.auth_mode,
                auth_config: server.auth_config.clone(),
                timeout_ms: server.timeout_ms,
                owner_contact: None,
                visibility: server.ownership.visibility,
                retarget_owner_credentials_only: false,
                expected_server: None,
                expected_owner_team_id: server.ownership.owner_team_id,
                expected_visibility: server.ownership.visibility,
                vouch_for_destinations: false,
                updated_at: OffsetDateTime::now_utc(),
                approve_tool_changes: true,
            })
            .await
            .expect("enable approval");

        // The upstream now declares metadata the gateway has not approved; the
        // approved definition is served instead. `_meta` is not versioned, so
        // it passes through.
        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC response");
        assert_eq!(
            body["result"]["tools"],
            json!([{
                "name": "search",
                "inputSchema": {"type": "object"},
                "_meta": {"origin": "upstream"},
            }])
        );

        let call = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "search", "arguments": {}},
        });
        let (status, _) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::OK);
        let called = invocations(&state)
            .await
            .into_iter()
            .find(|invocation| {
                invocation.mcp_method == "tools/call" && invocation.tool_schema_version.is_some()
            })
            .expect("call invocation");
        assert_eq!(called.tool_schema_version, Some(1));

        // Discovery sees the declared metadata as a contract change: the tool is
        // withheld, even with its explicit grant, until an admin approves it.
        gateway_service::McpRegistryService::new(state.store.clone())
            .refresh_discovery(server.mcp_server_id)
            .await
            .expect("discovery");
        let search = state
            .store
            .list_external_mcp_tools(server.mcp_server_id, false)
            .await
            .expect("tools")
            .into_iter()
            .find(|tool| tool.upstream_name == "search")
            .expect("search tool");
        assert_eq!(
            search.review.approval_status,
            gateway_core::McpToolApprovalStatus::PendingReview
        );
        let (status, _) = send(&state, &auth, call).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/list"}),
        )
        .await;
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC response");
        assert_eq!(body["result"]["tools"], json!([]));
    }

    #[tokio::test]
    async fn task_augmented_tool_calls_never_reach_the_upstream() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(
            &state,
            &auth,
            json!({
                "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": {"name": "search", "arguments": {}, "task": {"ttl": 60000}},
            }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert_eq!(body["id"], 5);
        assert!(
            body["error"]["message"].as_str().is_some_and(
                |message| message.contains("task-augmented tools/call is not supported")
            )
        );
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
    }

    const LAST_EVENT_ID: &str = "last-event-id";

    /// Requests the session upstream saw: method, `Mcp-Session-Id`, `Last-Event-ID`.
    type SeenRequests = Arc<std::sync::Mutex<Vec<(Method, Option<String>, Option<String>)>>>;

    /// An upstream that issues a session on `initialize` and, on `GET`, replays
    /// a tool result with event ids as a resumed stream would.
    async fn spawn_session_upstream(seen: SeenRequests) -> Upstream {
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::any(move |method: Method, headers: HeaderMap, body: Bytes| {
                let (counter, seen) = (counter.clone(), seen.clone());
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string)
                    };
                    seen.lock().unwrap().push((
                        method.clone(),
                        header(MCP_SESSION_ID),
                        header(LAST_EVENT_ID),
                    ));
                    if method == Method::GET {
                        let replayed = json!({
                            "jsonrpc": "2.0", "id": 4,
                            "result": {"content": [{"type": "text", "text": SECRET_TEXT}]},
                        });
                        let notice = json!({
                            "jsonrpc": "2.0", "method": "notifications/message",
                            "params": {"level": "info", "data": "hello"},
                        });
                        return (
                            [(CONTENT_TYPE, "text/event-stream")],
                            format!("id: 41\ndata: {replayed}\n\nid: 42\ndata: {notice}\n\n"),
                        )
                            .into_response();
                    }
                    if method == Method::DELETE {
                        return StatusCode::OK.into_response();
                    }
                    let message: Value = serde_json::from_slice(&body).unwrap_or_default();
                    (
                        [(MCP_SESSION_ID, "upstream-session-1")],
                        Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": {}})),
                    )
                        .into_response()
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
        Upstream {
            url,
            requests,
            task,
        }
    }

    async fn send_raw(
        state: &AppState,
        auth: &AuthenticatedApiKey,
        http_method: Method,
        extra_headers: &[(&'static str, &str)],
        body: &str,
    ) -> Response<Body> {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        for (name, value) in extra_headers {
            headers.insert(*name, HeaderValue::from_str(value).expect("header"));
        }
        dispatch_mcp_request(
            state,
            &crate::http::request_policy::RequestPolicyContext::default(),
            auth,
            "notes",
            &http_method,
            &headers,
            Bytes::from(body.to_string()),
            Instant::now(),
        )
        .await
    }

    /// A second key for the same user, granted the same tool.
    async fn second_key(state: &AppState, auth: &AuthenticatedApiKey) -> AuthenticatedApiKey {
        let now = OffsetDateTime::now_utc();
        let key = state
            .store
            .create_api_key(&NewApiKeyRecord {
                name: "Second key".to_string(),
                public_id: "second".to_string(),
                secret_hash: gateway_core::hash_gateway_key_secret("second-secret")
                    .expect("hash key"),
                model_grant_mode: ApiKeyModelGrantMode::All,
                owner_kind: ApiKeyOwnerKind::User,
                owner_user_id: auth.owner_user_id,
                owner_team_id: None,
                owner_service_account_id: None,
                access: Default::default(),
                created_at: now,
            })
            .await
            .expect("API key");
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .expect("server lookup")
            .expect("server");
        let search = state
            .store
            .get_active_mcp_tool_by_name(server.mcp_server_id, "search")
            .await
            .expect("tool lookup")
            .expect("search tool");
        state
            .store
            .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
                subject_kind: McpToolGrantSubjectKind::ApiKey,
                subject_id: key.id,
                target_kind: McpToolGrantTargetKind::Tool,
                target_id: search.mcp_tool_id,
                updated_at: now,
                expected_server: None,
                expected_subject_team: None,
            })
            .await
            .expect("tool grant");
        AuthenticatedApiKey {
            id: key.id,
            public_id: key.public_id,
            name: key.name,
            ..auth.clone()
        }
    }

    #[tokio::test]
    async fn sessions_are_bound_to_the_caller_and_event_streams_cannot_resume() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let seen = SeenRequests::default();
        let upstream = spawn_session_upstream(seen.clone()).await;
        let auth = seed_server(&state, &upstream.url, true).await;
        let other = second_key(&state, &auth).await;

        let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let response = send_raw(&state, &auth, Method::POST, &[], &initialize.to_string()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let session = response.headers()[MCP_SESSION_ID]
            .to_str()
            .expect("session id")
            .to_string();
        assert_ne!(
            session, "upstream-session-1",
            "the upstream id stays private"
        );

        // The stream reaches the upstream with its own session id and without
        // `Last-Event-ID`; event ids and the replayed result never reach the client.
        let response = send_raw(
            &state,
            &auth,
            Method::GET,
            &[(MCP_SESSION_ID, &session), (LAST_EVENT_ID, "40")],
            "",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let stream = String::from_utf8(
            to_bytes(response.into_body(), 65_536)
                .await
                .expect("body")
                .to_vec(),
        )
        .expect("UTF-8");
        assert!(!stream.contains("id: 4"), "{stream}");
        assert!(!stream.contains(SECRET_TEXT), "{stream}");
        assert!(stream.contains("notifications/message"), "{stream}");
        assert_eq!(
            seen.lock().unwrap().last().cloned(),
            Some((Method::GET, Some("upstream-session-1".to_string()), None))
        );

        // Another credential, or the raw upstream id, cannot use or end the session.
        let before = upstream.requests.load(Ordering::SeqCst);
        for (caller, session) in [(&other, session.as_str()), (&auth, "upstream-session-1")] {
            for method in [Method::DELETE, Method::GET] {
                let response =
                    send_raw(&state, caller, method, &[(MCP_SESSION_ID, session)], "").await;
                assert_eq!(response.status(), StatusCode::NOT_FOUND);
            }
        }
        assert_eq!(upstream.requests.load(Ordering::SeqCst), before);

        let response = send_raw(
            &state,
            &auth,
            Method::DELETE,
            &[(MCP_SESSION_ID, &session)],
            "",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            seen.lock().unwrap().last().cloned(),
            Some((Method::DELETE, Some("upstream-session-1".to_string()), None))
        );
    }

    #[tokio::test]
    async fn discovery_stores_declared_metadata_and_counts_it_as_drift() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        seed_server(&state, &upstream.url, true).await;
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .expect("server lookup")
            .expect("server");

        gateway_service::McpRegistryService::new(state.store.clone())
            .refresh_discovery(server.mcp_server_id)
            .await
            .expect("discovery");

        let tools = state
            .store
            .list_external_mcp_tools(server.mcp_server_id, false)
            .await
            .expect("tools");
        let search = tools
            .iter()
            .find(|tool| tool.upstream_name == "search")
            .expect("search tool");
        let declared = annotated_search_tool();
        assert_eq!(search.metadata.title.as_deref(), Some("Search notes"));
        assert_eq!(
            search.metadata.annotations,
            declared["annotations"].as_object().cloned()
        );
        assert_eq!(
            search.metadata.output_schema.as_ref(),
            Some(&declared["outputSchema"])
        );
        // The seeded row had no metadata, so the declared members are drift.
        assert_ne!(search.schema_hash, "search-v1");
        assert_eq!(search.schema_version, 2);
        let purge = tools
            .iter()
            .find(|tool| tool.upstream_name == "purge")
            .expect("purge tool");
        assert_eq!(purge.metadata, gateway_core::McpToolMetadata::default());
    }

    #[tokio::test]
    async fn team_servers_reach_only_the_addresses_their_url_allows() {
        // `localhost.` is a name other than `localhost`, so its spelling makes it
        // public, yet it resolves to loopback: a public name rebound inside.
        let resolves = tokio::net::lookup_host("localhost.:0")
            .await
            .is_ok_and(|mut addresses| addresses.next().is_some());
        if !resolves {
            eprintln!("skipping: `localhost.` does not resolve here");
            return;
        }
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(
            &state,
            &upstream.url.replace("127.0.0.1", "localhost."),
            true,
        )
        .await;
        let server = state
            .store
            .get_external_mcp_server_by_key("notes")
            .await
            .expect("server lookup")
            .expect("server");
        let call = json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": {"name": "search", "arguments": {}},
        });
        let registry = gateway_service::McpRegistryService::with_client(
            state.store.clone(),
            Arc::new(gateway_service::HttpMcpDiscoveryClient::new(
                state.mcp_http_clients.clone(),
            )),
        );
        let refresh = || registry.refresh_discovery(server.mcp_server_id);

        // A platform admin's server may point anywhere.
        let (status, body) = send(&state, &auth, call.clone()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            refresh().await.expect("discovery").status,
            ExternalMcpDiscoveryStatus::Success
        );
        let reached = upstream.requests.load(Ordering::SeqCst);
        assert!(reached > 0);

        // Once a team owns it, the same name is held to public addresses when
        // it connects, for the proxy and for discovery alike.
        let team = state
            .store
            .create_team("notes-owners", "Notes Owners")
            .await
            .expect("team");
        state
            .store
            .transfer_external_mcp_server_ownership(
                &gateway_core::TransferExternalMcpServerOwnershipRecord {
                    mcp_server_id: server.mcp_server_id,
                    ownership: gateway_core::McpServerOwnership {
                        owner_team_id: Some(team.team_id),
                        owner_contact: None,
                        visibility: gateway_core::McpServerVisibility::Organisation,
                    },
                    expected_ownership: None,
                    updated_at: OffsetDateTime::now_utc(),
                    require_no_stranded_access: false,
                },
            )
            .await
            .expect("transfer");
        let (status, body) = send(&state, &auth, call).await;
        assert_ne!(status, StatusCode::OK, "{body}");
        let refreshed = refresh().await.expect("discovery result");
        assert_eq!(refreshed.status, ExternalMcpDiscoveryStatus::Failed);
        assert_eq!(
            upstream.requests.load(Ordering::SeqCst),
            reached,
            "nothing reached the upstream"
        );
    }

    fn read_resource() -> Value {
        json!({"jsonrpc": "2.0", "id": 7, "method": "resources/read", "params": {"uri": "file:///notes.md"}})
    }

    #[tokio::test]
    async fn resource_read_without_a_server_grant_is_denied_and_logged() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;

        let (status, body) = send(&state, &auth, read_resource()).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body.contains("MCP server is not granted for this API key"));
        assert!(!body.contains(SECRET_TEXT));
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
        let logged = invocations(&state).await;
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].mcp_method, "resources/read");
        assert_eq!(logged[0].tool_display_key, "file:///notes.md");
        assert_eq!(logged[0].status, McpToolInvocationStatus::PolicyDenied);
        assert_eq!(
            logged[0].error_code.as_deref(),
            Some("mcp_server_not_granted")
        );
    }

    #[tokio::test]
    async fn a_denied_notification_gets_an_error_without_an_id() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;

        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "method": "resources/read", "params": {"uri": "file:///notes.md"}}),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert!(body.get("id").is_none());
        assert_eq!(
            body["error"]["message"],
            "MCP server is not granted for this API key"
        );
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn get_with_a_lifecycle_body_cannot_skip_the_event_stream_grant() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;

        let (status, _) = send_as(
            &state,
            &auth,
            &Method::GET,
            json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn prompt_list_without_a_server_grant_is_denied() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;

        let (status, _) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/list"}),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn resource_read_with_a_server_grant_is_forwarded_and_logged() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(&state, &auth, read_resource()).await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(SECRET_TEXT));
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 1);
        let logged = invocations(&state).await;
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].mcp_method, "resources/read");
        assert_eq!(logged[0].status, McpToolInvocationStatus::Success);
        assert_eq!(logged[0].policy_result, McpToolPolicyResult::Allowed);
        assert_eq!(logged[0].tool_id, None);
    }

    #[tokio::test]
    async fn lifecycle_requests_pass_through_without_grants() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, false).await;

        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_str(&body).expect("initialize result");
        // The gateway does not proxy tasks, so it must not advertise them.
        assert_eq!(body["result"]["capabilities"], json!({"logging": {}}));
        assert_eq!(body["result"]["protocolVersion"], "2025-11-25");
        let (status, _) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(upstream.requests.load(Ordering::SeqCst), 2);
        assert!(invocations(&state).await.is_empty());
    }

    #[tokio::test]
    async fn unknown_methods_are_denied_even_with_grants() {
        let (_directory, state) = crate::http::test_support::app_state().await;
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 4, "method": "vendor/export", "params": {}}),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert_eq!(body["id"], 4);
        assert_eq!(body["error"]["code"], -32601);
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
        let logged = invocations(&state).await;
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].mcp_method, "vendor/export");
        assert_eq!(
            logged[0].error_code.as_deref(),
            Some("mcp_method_not_allowed")
        );

        // A rejected notification keeps the error status, and its error has no id.
        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "method": "vendor/event"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert!(body.get("id").is_none());
        assert_eq!(body["error"]["code"], -32601);
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn result_guardrail_applies_to_resource_reads() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        deny_results(&mut state);
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(&state, &auth, read_resource()).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body.contains("denied by guardrail policy"));
        assert!(!body.contains(SECRET_TEXT));
        let logged = invocations(&state).await;
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].mcp_method, "resources/read");
        assert_eq!(logged[0].status, McpToolInvocationStatus::PolicyDenied);
        assert_eq!(
            logged[0].metadata["guardrail_result"]["guardrail_decision"],
            "denied"
        );
    }

    #[tokio::test]
    async fn result_guardrail_applies_to_prompt_results_over_sse() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        deny_results(&mut state);
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 8, "method": "prompts/get", "params": {"name": "summarise"}}),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body.contains(SECRET_TEXT));
        let logged = invocations(&state).await;
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].mcp_method, "prompts/get");
        assert_eq!(logged[0].tool_display_key, "summarise");
        assert_eq!(logged[0].status, McpToolInvocationStatus::PolicyDenied);
    }

    fn limit_tool_calls(state: &mut AppState, limit: i64) -> crate::http::test_support::TestClock {
        crate::http::test_support::install_rate_limits(
            state,
            vec![gateway_service::ConfiguredRateLimitPolicy {
                name: "notes calls".to_string(),
                operation: gateway_core::RateLimitOperation::McpToolCall,
                scope_kind: gateway_core::RateLimitScopeKind::ApiKey,
                model_key: None,
                mcp_server_key: Some("notes".to_string()),
                metric: gateway_core::RateLimitMetric::Requests,
                period: gateway_core::RateLimitPeriod::Minute,
                limit,
            }],
        )
    }

    fn search_call(id: i64) -> Value {
        json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "search", "arguments": {"q": "x"}},
        })
    }

    #[tokio::test]
    async fn rate_limited_tool_calls_get_a_json_rpc_429_and_are_logged() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        let clock = limit_tool_calls(&mut state, 1);
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, _) = send(&state, &auth, search_call(1)).await;
        assert_eq!(status, StatusCode::OK);
        let upstream_calls = upstream.requests.load(Ordering::SeqCst);

        let (status, body) = send(&state, &auth, search_call(2)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert_eq!(body["id"], 2);
        assert_eq!(body["error"]["code"], -32029);
        assert_eq!(body["error"]["data"]["error_code"], "rate_limited");
        assert_eq!(body["error"]["data"]["retry_after_seconds"], 51);
        assert_eq!(
            body["error"]["message"],
            "API key rate limit exceeded: 1 MCP tool calls per minute. Retry after 51 seconds"
        );
        assert_eq!(upstream.requests.load(Ordering::SeqCst), upstream_calls);

        let denied = invocations(&state)
            .await
            .into_iter()
            .find(|invocation| invocation.error_code.as_deref() == Some("rate_limited"))
            .expect("rate-limited invocation");
        assert_eq!(denied.status, McpToolInvocationStatus::PolicyDenied);
        assert_eq!(denied.policy_result, McpToolPolicyResult::Denied);
        assert_eq!(denied.mcp_method, "tools/call");
        assert_eq!(denied.tool_display_key, "search");
        assert_eq!(denied.metadata["rate_limit"]["policy"], "notes calls");
        assert_eq!(denied.metadata["rate_limit"]["period"], "minute");
        assert_eq!(denied.metadata["rate_limit"]["retry_after_seconds"], 51);

        // Other methods are not tool calls and are never limited.
        let (status, _) = send(
            &state,
            &auth,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        clock.advance(51);
        let (status, _) = send(&state, &auth, search_call(4)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_mcp_route_carries_rate_limit_headers() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        limit_tool_calls(&mut state, 1);
        let upstream = spawn_upstream().await;
        seed_server(&state, &upstream.url, true).await;
        let router = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
        let call = |id| {
            axum::http::Request::builder()
                .method("POST")
                .uri("/mcp/notes")
                .header(
                    "authorization",
                    format!("Bearer gwk_reader.{READER_SECRET}"),
                )
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .body(Body::from(search_call(id).to_string()))
                .unwrap()
        };

        use tower::ServiceExt as _;
        let admitted = router.clone().oneshot(call(1)).await.unwrap();
        assert_eq!(admitted.status(), StatusCode::OK);
        assert_eq!(admitted.headers()["x-ratelimit-limit"], "1");
        assert_eq!(admitted.headers()["x-ratelimit-remaining"], "0");
        // The sliding window frees the whole limit at the end of the next minute.
        assert_eq!(admitted.headers()["x-ratelimit-reset"], "110");
        assert!(admitted.headers().get("retry-after").is_none());

        let limited = router.oneshot(call(2)).await.unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(limited.headers()["retry-after"], "51");
        assert_eq!(limited.headers()["x-ratelimit-remaining"], "0");
        let body: Value =
            serde_json::from_slice(&to_bytes(limited.into_body(), 65_536).await.expect("body"))
                .expect("JSON-RPC error");
        assert_eq!(body["error"]["code"], -32029);
    }

    /// Resources and prompts read server data through the same upstream
    /// access as tools, so the server's MCP call limits count them too.
    #[tokio::test]
    async fn governed_methods_share_the_servers_call_limits() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        limit_tool_calls(&mut state, 1);
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, _) = send(&state, &auth, read_resource()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = send(&state, &auth, read_resource()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert_eq!(body["id"], 7);
        assert_eq!(body["error"]["code"], -32029);
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 1);
        let logged = invocations(&state).await;
        let limited = logged
            .iter()
            .find(|invocation| invocation.error_code.as_deref() == Some("rate_limited"))
            .expect("rate-limited invocation");
        assert_eq!(limited.mcp_method, "resources/read");
        assert_eq!(limited.status, McpToolInvocationStatus::PolicyDenied);
        // Tool calls share the counter.
        let (status, _) = send(&state, &auth, search_call(8)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn refused_notifications_get_errors_without_an_id() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        limit_tool_calls(&mut state, 1);
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;
        let mut notification = read_resource();
        notification
            .as_object_mut()
            .expect("request object")
            .remove("id");

        send(&state, &auth, read_resource()).await;
        let (status, body) = send(&state, &auth, notification.clone()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert!(body.get("id").is_none(), "{body}");
        assert_eq!(body["error"]["code"], -32029);

        crate::http::test_support::install_down_rate_limiter(&mut state);
        let (status, body) = send(&state, &auth, notification).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert!(body.get("id").is_none(), "{body}");
    }

    #[tokio::test]
    async fn closed_rate_limit_failures_return_a_json_rpc_503() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        crate::http::test_support::install_down_rate_limiter(&mut state);
        let upstream = spawn_upstream().await;
        let auth = seed_server(&state, &upstream.url, true).await;

        let (status, body) = send(&state, &auth, search_call(9)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let body: Value = serde_json::from_str(&body).expect("JSON-RPC error");
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["id"], 9);
        assert_eq!(body["error"]["code"], -32603);
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
        let logged = invocations(&state).await;
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].status, McpToolInvocationStatus::GatewayError);
    }
}

mod stream_reach {
    use std::time::Duration;

    use axum::body::to_bytes;
    use gateway_core::{
        AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode,
        ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
        ExternalMcpTransport, GlobalRole, McpAccessRepository, McpRegistryRepository,
        McpServerOwnership, McpServerVisibility, McpToolGrantSubjectKind, McpToolGrantTargetKind,
        MembershipRole, NewApiKeyRecord, NewExternalMcpServerRecord, SCOPE_MCP_PROXY,
        UpsertExternalMcpToolRecord, UpsertMcpToolGrantRecord, UserStatus,
    };
    use gateway_store::GatewayStore;

    use super::super::stream_reach::{McpStreamReachWatcher, StreamCaller, bound_to_reach};
    use super::*;

    const STREAMER_KEY: &str = "gwk_streamer.stream-secret";

    struct Seeded {
        state: AppState,
        _directory: tempfile::TempDir,
        team_id: Uuid,
        user_id: Uuid,
        api_key_id: Uuid,
        caller: StreamCaller,
        /// Rechecks every 50ms.
        watcher: std::sync::Arc<McpStreamReachWatcher>,
    }

    /// A team member with an API key and a grant on one tool of a server their
    /// team owns, published with `visibility`.
    async fn seed(visibility: McpServerVisibility) -> Seeded {
        let (directory, state) = crate::http::test_support::app_state().await;
        let store = &state.store;
        let now = OffsetDateTime::now_utc();
        let team = store
            .create_team("stream-team", "Stream Team")
            .await
            .unwrap();
        let user = store
            .create_identity_user(
                "Streamer",
                "streamer@example.test",
                "streamer@example.test",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .unwrap();
        store
            .assign_team_membership(user.user_id, team.team_id, MembershipRole::Member)
            .await
            .unwrap();
        let key = store
            .create_api_key(&NewApiKeyRecord {
                name: "Streamer".to_string(),
                public_id: "streamer".to_string(),
                secret_hash: gateway_core::hash_gateway_key_secret("stream-secret").unwrap(),
                model_grant_mode: ApiKeyModelGrantMode::All,
                owner_kind: ApiKeyOwnerKind::User,
                owner_user_id: Some(user.user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                access: Default::default(),
                created_at: now,
            })
            .await
            .unwrap();
        let server = store
            .create_external_mcp_server(&NewExternalMcpServerRecord {
                approve_tool_changes: false,
                ownership: McpServerOwnership {
                    visibility,
                    ..McpServerOwnership::team(team.team_id, None)
                },
                server_key: "team-stream".to_string(),
                display_name: "Team stream".to_string(),
                description: None,
                transport: ExternalMcpTransport::StreamableHttp,
                server_url: "https://8.8.8.8/mcp".to_string(),
                auth_mode: ExternalMcpAuthMode::None,
                auth_config: Map::new(),
                timeout_ms: 2_000,
                created_at: now,
            })
            .await
            .unwrap();
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
                    expected_server: None,
                },
                &[UpsertExternalMcpToolRecord {
                    mcp_server_id: server.mcp_server_id,
                    upstream_name: "search".to_string(),
                    display_name: "search".to_string(),
                    description: None,
                    input_schema: json!({"type": "object"}),
                    metadata: gateway_core::McpToolMetadata::default(),
                    schema_hash: "hash".to_string(),
                }],
            )
            .await
            .unwrap();
        store
            .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
                subject_kind: McpToolGrantSubjectKind::User,
                subject_id: user.user_id,
                target_kind: McpToolGrantTargetKind::Tool,
                target_id: tools[0].mcp_tool_id,
                updated_at: now,
                expected_server: None,
                expected_subject_team: None,
            })
            .await
            .unwrap();
        let auth = state
            .service
            .authenticate_bearer_token_for_scope(STREAMER_KEY, SCOPE_MCP_PROXY)
            .await
            .unwrap();
        Seeded {
            _directory: directory,
            team_id: team.team_id,
            user_id: user.user_id,
            api_key_id: key.id,
            caller: StreamCaller::of(&auth, "team-stream", server.mcp_server_id),
            watcher: McpStreamReachWatcher::new(state.store.clone(), Duration::from_millis(50)),
            state,
        }
    }

    /// Opens an upstream event stream that never ends on its own, watched by
    /// `seeded`'s watcher, and returns the task reading it.
    fn open(seeded: &Seeded) -> tokio::task::JoinHandle<Result<Bytes, axum::Error>> {
        let upstream = Response::new(Body::from_stream(futures_util::stream::pending::<
            Result<Bytes, std::io::Error>,
        >()));
        let response = bound_to_reach(&seeded.watcher, seeded.caller.clone(), upstream);
        tokio::spawn(to_bytes(response.into_body(), usize::MAX))
    }

    async fn assert_ends(reading: tokio::task::JoinHandle<Result<Bytes, axum::Error>>) {
        let ended = tokio::time::timeout(Duration::from_secs(5), reading).await;
        assert!(
            ended.is_ok(),
            "the stream ends once its caller is no longer reached"
        );
    }

    #[tokio::test]
    async fn team_only_streams_end_when_the_member_leaves_the_team() {
        let seeded = seed(McpServerVisibility::Team).await;
        let reading = open(&seeded);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!reading.is_finished(), "a member's stream stays open");

        seeded
            .state
            .store
            .remove_team_membership(seeded.team_id, seeded.user_id)
            .await
            .unwrap();
        assert_ends(reading).await;
    }

    #[tokio::test]
    async fn organisation_streams_end_when_their_api_key_is_revoked() {
        let seeded = seed(McpServerVisibility::Organisation).await;
        let reading = open(&seeded);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!reading.is_finished(), "a live key's stream stays open");

        // The grant survives, so only re-authenticating the key notices.
        seeded
            .state
            .store
            .revoke_api_key(seeded.api_key_id, OffsetDateTime::now_utc())
            .await
            .unwrap();
        assert_ends(reading).await;
    }

    #[tokio::test]
    async fn streams_end_when_their_key_loses_the_mcp_scope() {
        let seeded = seed(McpServerVisibility::Organisation).await;
        let reading = open(&seeded);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !reading.is_finished(),
            "an MCP-scoped key's stream stays open"
        );

        seeded
            .state
            .store
            .replace_api_key_access(
                seeded.api_key_id,
                &gateway_core::ApiKeyAccess::new(Some(vec!["inference".to_string()]), None)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ends(reading).await;
    }

    #[tokio::test]
    async fn many_streams_of_one_caller_cost_one_check_per_recheck() {
        let seeded = seed(McpServerVisibility::Team).await;
        let readings: Vec<_> = (0..20).map(|_| open(&seeded)).collect();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(readings.iter().all(|reading| !reading.is_finished()));
        assert_eq!(seeded.watcher.last_pairs_checked(), 1);

        // One change still ends every one of them.
        seeded
            .state
            .store
            .remove_team_membership(seeded.team_id, seeded.user_id)
            .await
            .unwrap();
        for reading in readings {
            assert_ends(reading).await;
        }
    }
}

#[test]
fn stored_definitions_are_served_while_approval_is_on_or_a_change_waits() {
    use gateway_core::{McpToolApprovalStatus, McpToolReviewState};
    let now = time::OffsetDateTime::UNIX_EPOCH;
    let mut server = gateway_core::ExternalMcpServerRecord {
        mcp_server_id: Uuid::new_v4(),
        server_key: "notes".to_string(),
        display_name: "Notes".to_string(),
        description: None,
        transport: gateway_core::ExternalMcpTransport::StreamableHttp,
        server_url: "https://notes.example.test/mcp".to_string(),
        auth_mode: gateway_core::ExternalMcpAuthMode::None,
        auth_config: Map::new(),
        timeout_ms: 1_000,
        status: gateway_core::ExternalMcpServerStatus::Active,
        last_discovery_status: None,
        last_discovery_at: None,
        last_successful_discovery_at: None,
        last_error_summary: None,
        last_tool_count: None,
        created_at: now,
        updated_at: now,
        disabled_at: None,
        ownership: Default::default(),
        team_destinations: false,
        approve_tool_changes: false,
    };
    let mut tool = gateway_core::ExternalMcpToolRecord {
        mcp_tool_id: Uuid::new_v4(),
        mcp_server_id: server.mcp_server_id,
        upstream_name: "search".to_string(),
        display_name: "search".to_string(),
        description: Some("Search notes".to_string()),
        input_schema: json!({"type": "object"}),
        metadata: Default::default(),
        policy: Default::default(),
        review: McpToolReviewState {
            approval_status: McpToolApprovalStatus::Approved,
            approved_schema_version: Some(1),
            pending_schema_version: None,
        },
        schema_hash: "sha256:a".to_string(),
        schema_version: 1,
        is_active: true,
        first_discovered_at: now,
        last_discovered_at: now,
        deactivated_at: None,
    };
    assert!(
        !serves_stored_definition(&server, &tool),
        "clean tools pass through"
    );
    tool.review.approval_status = McpToolApprovalStatus::PendingChange;
    tool.review.pending_schema_version = Some(2);
    assert!(
        serves_stored_definition(&server, &tool),
        "a waiting change stays hidden after approval is turned off"
    );
    tool.review = McpToolReviewState::default();
    server.approve_tool_changes = true;
    assert!(serves_stored_definition(&server, &tool));
}

/// Session ids signed with the public development secret could be forged for
/// any credential, so that secret is never the key.
#[tokio::test]
async fn sessions_are_never_signed_with_the_public_development_secret() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    state.identity_token_secret =
        std::sync::Arc::new(crate::http::audit::DEV_IDENTITY_TOKEN_SECRET.to_string());
    let key = state.mcp_session_signing_key();
    assert_ne!(
        &*key,
        crate::http::audit::DEV_IDENTITY_TOKEN_SECRET.as_bytes()
    );
    state.identity_token_secret = std::sync::Arc::new("a-configured-secret".to_string());
    assert_eq!(&*state.mcp_session_signing_key(), b"a-configured-secret");
}
