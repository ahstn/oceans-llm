use super::*;
use axum::body::to_bytes;
use gateway_core::{GuardrailDecisionQuery, GuardrailDecisionRepository};
use gateway_guardrails::{
    ContentTransformation, FailureDisposition, GuardrailEngine, ManagedCheckConfig,
    ManagedCheckKind, ManagedOutcome, ManagedService, PolicyMode, ReasonCode,
    test_utils::StubManagedEvaluator,
};
use sha2::{Digest, Sha256};
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
    let (_directory, state) = guarded_state(PolicyMode::Deny, 1 << 20, 1_000).await;
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
            .await
            .expect("guarded result");
            assert!(denied);
            assert!(
                only_stream_buffer_deny(evaluation.as_ref()),
                "reject ambiguous envelopes before content evaluation"
            );
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            let bytes = to_bytes(response.into_body(), 4_096).await.unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("private-"));
            let error: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(error["id"], 42);
            assert_eq!(error["error"]["code"], JSON_RPC_POLICY_DENIED);
            assert!(error.get("result").is_none());
        }
    }
}

#[tokio::test]
async fn guarded_mcp_sse_rejects_malformed_events_without_forwarding_payloads() {
    let (_directory, state) = guarded_state(PolicyMode::Deny, 1 << 20, 1_000).await;
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
        .await
        .expect("guarded result");
        assert!(denied);
        assert!(
            only_stream_buffer_deny(evaluation.as_ref()),
            "malformed data must fail before content evaluation"
        );
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), 4096)
            .await
            .expect("error body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC error");
        assert_eq!(body["id"], 42);
        assert_eq!(body["error"]["code"], JSON_RPC_POLICY_DENIED);
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
    .await
    .expect("guarded result");
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

async fn guarded_state(
    mode: PolicyMode,
    stream_buffer_bytes: usize,
    stream_buffer_timeout_ms: u64,
) -> (tempfile::TempDir, AppState) {
    let (directory, mut state) = crate::http::test_support::app_state().await;
    let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
    policy.enabled = true;
    policy.mode = mode;
    policy.stream_buffer_bytes = stream_buffer_bytes;
    policy.stream_buffer_timeout_ms = stream_buffer_timeout_ms;
    (directory, state)
}

/// Whether the only decision is the `stream_buffer` deny for content that could not be
/// inspected, so no content evaluator saw the result.
fn only_stream_buffer_deny(evaluation: Option<&GuardrailEvaluation>) -> bool {
    evaluation.is_some_and(|evaluation| {
        evaluation.denied()
            && evaluation.decisions.iter().all(|decision| {
                decision.evaluator == "stream_buffer"
                    && decision.failure_disposition == Some(FailureDisposition::FailClosed)
            })
    })
}

async fn stream_buffer_hashes(state: &AppState) -> Vec<String> {
    for _ in 0..50 {
        let items = state
            .store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: 10,
                evaluator: Some("stream_buffer".into()),
                ..Default::default()
            })
            .await
            .expect("decisions")
            .items;
        if !items.is_empty() {
            return items.into_iter().map(|item| item.content_hash).collect();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Vec::new()
}

/// Returns `action:reason_code:failure_disposition` for each `stream_buffer` decision.
async fn stream_buffer_decisions(state: &AppState) -> Vec<String> {
    // Decisions are persisted in the background, so wait for the write.
    for _ in 0..50 {
        let items = state
            .store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: 10,
                phase: Some("mcp_result".into()),
                evaluator: Some("stream_buffer".into()),
                ..Default::default()
            })
            .await
            .expect("decisions")
            .items;
        if !items.is_empty() {
            return items
                .iter()
                .map(|item| {
                    format!(
                        "{}:{}:{}",
                        item.action,
                        item.reason_code,
                        item.failure_disposition.as_deref().unwrap_or("none")
                    )
                })
                .collect();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Vec::new()
}

async fn enforce(state: &AppState, upstream: Response<Body>) -> (Response<Body>, bool) {
    let (response, _, denied) = enforce_direct_mcp_result(
        state,
        "test-server",
        "test-tool",
        Some(&json!(7)),
        Uuid::new_v4(),
        upstream,
    )
    .await
    .expect("guarded result");
    (response, denied)
}

fn upstream_response(content_type: &'static str, body: Body) -> Response<Body> {
    Response::builder()
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .body(body)
        .expect("upstream response")
}

fn chunked_body(chunks: &[&'static str]) -> Body {
    Body::from_stream(stream::iter(
        chunks
            .iter()
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::from_static(chunk.as_bytes())))
            .collect::<Vec<_>>(),
    ))
}

/// A body that sends `first`, then stalls past the buffer timeout before sending `rest`.
fn stalled_body(first: &'static str, rest: &'static str) -> Body {
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    tokio::spawn(async move {
        let _ = sender.send(Ok(Bytes::from_static(first.as_bytes()))).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = sender.send(Ok(Bytes::from_static(rest.as_bytes()))).await;
    });
    Body::from_stream(stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|chunk| (chunk, receiver))
    }))
}

async fn body_text(response: Response<Body>) -> String {
    let bytes = tokio::time::timeout(
        Duration::from_secs(2),
        to_bytes(response.into_body(), 1 << 20),
    )
    .await
    .expect("body completes")
    .expect("body");
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}

async fn assert_denied(response: Response<Body>, status: StatusCode, message: &str) {
    assert_eq!(response.status(), status);
    let body: Value = serde_json::from_str(&body_text(response).await).expect("JSON-RPC error");
    assert_eq!(body["id"], 7);
    assert_eq!(body["error"]["code"], JSON_RPC_POLICY_DENIED);
    assert_eq!(body["error"]["message"], message);
}

const RESULT_CHUNKS: [&str; 3] = [
    "{\"jsonrpc\":\"2.0\",\"id\":7,",
    "\"result\":{\"content\":\"oversized result\"}",
    "}",
];

#[tokio::test]
async fn audit_mode_releases_oversized_result_unchanged() {
    let (_directory, state) = guarded_state(PolicyMode::Audit, 24, 1_000).await;
    let upstream = upstream_response("application/json", chunked_body(&RESULT_CHUNKS));

    let (response, denied) = enforce(&state, upstream).await;

    assert!(!denied);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, RESULT_CHUNKS.concat());
    assert_eq!(
        stream_buffer_decisions(&state).await,
        ["audit:stream.buffer_exceeded:fail_open"]
    );
}

#[tokio::test]
async fn uninspected_result_hash_covers_the_overflowing_chunk() {
    let (_directory, state) = guarded_state(PolicyMode::Deny, 8, 1_000).await;
    let result = RESULT_CHUNKS.concat();
    let upstream = upstream_response("application/json", Body::from(result.clone()));

    let (_, denied) = enforce(&state, upstream).await;

    assert!(denied);
    assert_eq!(
        stream_buffer_hashes(&state).await,
        [format!("sha256:{:x}", Sha256::digest(result.as_bytes()))],
        "an oversized first chunk is hashed, not an empty prefix"
    );
}

#[tokio::test]
async fn deny_mode_rejects_oversized_result() {
    let (_directory, state) = guarded_state(PolicyMode::Deny, 24, 1_000).await;
    let upstream = upstream_response("application/json", chunked_body(&RESULT_CHUNKS));

    let (response, evaluation, denied) = enforce_direct_mcp_result(
        &state,
        "test-server",
        "test-tool",
        Some(&json!(7)),
        Uuid::new_v4(),
        upstream,
    )
    .await
    .expect("guarded result");

    assert!(denied);
    assert!(
        evaluation.is_some_and(|evaluation| evaluation.denied()),
        "the invocation log receives the deny decision"
    );
    assert_denied(
        response,
        StatusCode::PAYLOAD_TOO_LARGE,
        "MCP result exceeded the configured guardrail buffer",
    )
    .await;
    assert_eq!(
        stream_buffer_decisions(&state).await,
        ["deny:stream.buffer_exceeded:fail_closed"]
    );
}

#[tokio::test]
async fn audit_mode_releases_slow_result_unchanged() {
    let (_directory, state) = guarded_state(PolicyMode::Audit, 1 << 20, 50).await;
    let upstream = upstream_response(
        "text/event-stream",
        stalled_body("event: message\n", "data: {\"result\":{}}\n\n"),
    );

    let (response, denied) = enforce(&state, upstream).await;

    assert!(!denied);
    assert_eq!(
        body_text(response).await,
        "event: message\ndata: {\"result\":{}}\n\n"
    );
    assert_eq!(
        stream_buffer_decisions(&state).await,
        ["audit:stream.buffer_timeout:fail_open"]
    );
}

#[tokio::test]
async fn deny_mode_rejects_slow_result() {
    let (_directory, state) = guarded_state(PolicyMode::Deny, 1 << 20, 50).await;
    let upstream = upstream_response(
        "text/event-stream",
        stalled_body("event: message\n", "data: {\"result\":{}}\n\n"),
    );

    let (response, denied) = enforce(&state, upstream).await;

    assert!(denied);
    assert_denied(
        response,
        StatusCode::GATEWAY_TIMEOUT,
        "MCP result did not complete within the guardrail buffer timeout",
    )
    .await;
    assert_eq!(
        stream_buffer_decisions(&state).await,
        ["deny:stream.buffer_timeout:fail_closed"]
    );
}

#[tokio::test]
async fn audit_mode_releases_unparseable_results_unchanged() {
    let invalid_sse = "data: {\"result\":{}}\n\ndata: not-json\n\n";
    let mixed_envelope = "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{},\"error\":{}}";
    for (content_type, source) in [
        ("text/event-stream", invalid_sse),
        ("application/json", mixed_envelope),
    ] {
        let (_directory, state) = guarded_state(PolicyMode::Audit, 1 << 20, 1_000).await;
        let upstream = upstream_response(content_type, Body::from(source));

        let (response, denied) = enforce(&state, upstream).await;

        assert!(!denied, "{content_type}");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_text(response).await, source);
        assert_eq!(
            stream_buffer_decisions(&state).await,
            ["audit:stream.unparseable:fail_open"]
        );
    }
}

#[tokio::test]
async fn deny_mode_rejects_invalid_json_rpc_result() {
    let (_directory, state) = guarded_state(PolicyMode::Deny, 1 << 20, 1_000).await;
    let upstream = upstream_response(
        "application/json",
        Body::from("{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{},\"error\":{}}"),
    );

    let (response, denied) = enforce(&state, upstream).await;

    assert!(denied);
    assert_denied(
        response,
        StatusCode::FORBIDDEN,
        "MCP result was not a valid guarded response",
    )
    .await;
    assert_eq!(
        stream_buffer_decisions(&state).await,
        ["deny:stream.unparseable:fail_closed"]
    );
}

#[tokio::test]
async fn result_body_failure_is_an_upstream_error_in_both_modes() {
    for mode in [PolicyMode::Audit, PolicyMode::Deny] {
        let (_directory, state) = guarded_state(mode, 1 << 20, 1_000).await;
        let body = Body::from_stream(stream::iter([
            Ok(Bytes::from_static(b"{\"jsonrpc\":\"2.0\",")),
            Err(std::io::Error::other("connection reset")),
        ]));
        let upstream = upstream_response("application/json", body);

        let error = enforce_direct_mcp_result(
            &state,
            "test-server",
            "test-tool",
            Some(&json!(7)),
            Uuid::new_v4(),
            upstream,
        )
        .await
        .expect_err("a failed body is not released as a successful result");

        assert!(
            matches!(error, GatewayError::Provider(ProviderError::Transport(_))),
            "{mode:?}: {error:?}"
        );
        assert!(stream_buffer_decisions(&state).await.is_empty(), "{mode:?}");
    }
}
