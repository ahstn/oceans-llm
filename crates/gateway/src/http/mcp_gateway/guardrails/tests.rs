use super::*;
use gateway_guardrails::{
    ContentTransformation, FailureDisposition, GuardPhase, GuardrailEngine, ManagedCheckConfig,
    ManagedCheckKind, ManagedOutcome, ManagedService, PolicyMode, ReasonCode,
    test_utils::StubManagedEvaluator,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[test]
fn guarded_mcp_extracts_enveloped_and_bare_payloads() {
    let result = json!({"jsonrpc": "2.0", "id": 1, "result": {"content": "allowed"}});
    let error = json!({"jsonrpc": "2.0", "id": 1, "error": {"message": "private content"}});
    let bare = json!({"content": "non-standard"});

    assert_eq!(
        guarded_mcp_payload(&result),
        Ok((Some("result"), json!({"content": "allowed"})))
    );
    assert_eq!(
        guarded_mcp_payload(&error),
        Ok((Some("error"), json!({"message": "private content"})))
    );
    assert_eq!(guarded_mcp_payload(&bare), Ok((None, bare)));
}

#[tokio::test]
async fn guarded_json_and_sse_reject_mixed_result_and_error_envelopes() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    Arc::make_mut(&mut state.guardrail_config).default.enabled = true;
    for envelope in [
        json!({"jsonrpc": "2.0", "id": 42, "result": {}, "error": {"message": "private-error"}}),
        json!({"jsonrpc": "2.0", "id": 42, "result": null, "error": {"message": "private-error"}}),
        json!({"jsonrpc": "2.0", "id": 42, "result": {"content": "private-result"}, "error": null}),
    ] {
        for content_type in ["application/json", "text/event-stream"] {
            let body = if content_type == "text/event-stream" {
                format!("data: {envelope}\n\n")
            } else {
                envelope.to_string()
            };
            let upstream = Response::builder()
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap();
            let (response, evaluation, denied) = enforce_direct_mcp_result(
                &state,
                "test-server",
                "test-tool",
                Some(&json!(42)),
                Uuid::new_v4(),
                upstream,
            )
            .await;
            assert!(denied);
            assert!(
                evaluation.is_none(),
                "reject ambiguous envelopes before evaluation"
            );
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            let bytes = to_bytes(response.into_body(), 4_096).await.unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("private-"));
            let error: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(error["id"], 42);
            assert_eq!(error["error"]["code"], GUARDRAIL_POLICY_DENIED_CODE);
            assert!(error.get("result").is_none());
        }
    }
}

#[tokio::test]
async fn guarded_mcp_sse_rejects_malformed_events_without_forwarding_payloads() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    std::sync::Arc::make_mut(&mut state.guardrail_config)
        .default
        .enabled = true;
    for (prefix, line, boundary) in [
        ("", "\n", "\n\n"),
        ("", "\r\n", "\r\n\r\n"),
        ("", "\r", "\r\r"),
        ("", "\r", "\n\r\n"),
        ("\u{feff}", "\r", "\r\r"),
    ] {
        let source = format!(
            "{prefix}: heartbeat{line}data: {{\"result\":{{\"content\":\"private-valid-payload\"}}}}{boundary}: heartbeat{line}data: private-invalid-payload{boundary}"
        );
        let upstream = Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(source))
            .expect("upstream SSE response");
        let (response, evaluation, denied) = enforce_direct_mcp_result(
            &state,
            "test-server",
            "test-tool",
            Some(&json!(42)),
            Uuid::new_v4(),
            upstream,
        )
        .await;
        assert!(denied);
        assert!(
            evaluation.is_none(),
            "malformed data must fail before evaluation"
        );
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), 4096)
            .await
            .expect("error body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC error");
        assert_eq!(body["id"], 42);
        assert_eq!(body["error"]["code"], GUARDRAIL_POLICY_DENIED_CODE);
        assert_eq!(
            body["error"]["message"],
            "MCP result was not valid guarded SSE"
        );
        assert!(!String::from_utf8_lossy(&bytes).contains("private-"));
    }
}

#[tokio::test]
async fn guarded_sse_transforms_all_payloads_and_preserves_envelopes() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    let config = Arc::make_mut(&mut state.guardrail_config);
    config.default.enabled = true;
    config.default.mode = PolicyMode::Deny;
    config.default.managed_checks = vec!["redact".into()];
    config.managed_checks.insert(
        "redact".into(),
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
    let replacement = json!([{"content": "redacted"}, {"message": "redacted"}]);
    state.guardrail_engine = Arc::new(GuardrailEngine::new(
        Vec::new(),
        BTreeMap::from([(
            "redact".into(),
            Arc::new(StubManagedEvaluator::new(
                "redact",
                ManagedService::GoogleModelArmor,
                [Ok(ManagedOutcome::Transformed {
                    transformation: ContentTransformation::new(replacement.to_string()),
                    reason_code: ReasonCode::new("test.redacted").unwrap(),
                    metadata: Default::default(),
                })],
            )) as Arc<dyn gateway_guardrails::ManagedEvaluator>,
        )]),
    ));
    let source = concat!(
        "\u{feff}: heartbeat\revent: message\rid: first\r",
        "data: {\"jsonrpc\":\"2.0\",\"id\":1,\n",
        "data: \"result\":{\"content\":\"private-result\"}}\r\n\r\n",
        "event: message\nid: second\n",
        "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"message\":\"private-error\"}}\r\r",
        ": tail\n\n",
    );
    let response = Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(source))
        .unwrap();
    let (response, evaluation, denied) = enforce_direct_mcp_result(
        &state,
        "test-server",
        "test-tool",
        Some(&json!(1)),
        Uuid::new_v4(),
        response,
    )
    .await;
    assert!(!denied);
    assert_eq!(evaluation.unwrap().action, DecisionAction::Transformed);
    let bytes = to_bytes(response.into_body(), 4_096).await.unwrap();
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(!body.contains("private-"));
    assert!(body.contains(": heartbeat\nevent: message\nid: first\n"));
    assert!(body.contains("event: message\nid: second\n"));
    assert!(body.ends_with(": tail\n\n"));
    let events: Vec<Value> = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).unwrap())
        .collect();
    assert_eq!(
        events,
        vec![
            json!({"jsonrpc": "2.0", "id": 1, "result": {"content": "redacted"}}),
            json!({"jsonrpc": "2.0", "id": 2, "error": {"message": "redacted"}}),
        ]
    );
}
