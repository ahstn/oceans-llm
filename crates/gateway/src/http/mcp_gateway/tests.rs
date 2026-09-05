use super::*;
use axum::http::{HeaderMap, HeaderValue};

#[test]
fn guarded_mcp_sse_extracts_enveloped_and_bare_payloads() {
    let result = json!({"jsonrpc": "2.0", "id": 1, "result": {"content": "allowed"}});
    let error = json!({"jsonrpc": "2.0", "id": 1, "error": {"message": "private content"}});
    let bare = json!({"content": "non-standard"});

    assert_eq!(
        guarded_mcp_sse_payload(&result),
        (Some("result"), json!({"content": "allowed"}))
    );
    assert_eq!(
        guarded_mcp_sse_payload(&error),
        (Some("error"), json!({"message": "private content"}))
    );
    assert_eq!(guarded_mcp_sse_payload(&bare), (None, bare));
}

#[test]
fn auth_extractor_accepts_authorization_only() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    assert_eq!(
        extract_mcp_gateway_api_key(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_accepts_explicit_header_only() {
    let mut headers = HeaderMap::new();
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    assert_eq!(
        extract_mcp_gateway_api_key(&headers).expect("token"),
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
        extract_mcp_gateway_api_key(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_rejects_missing_credentials() {
    let headers = HeaderMap::new();
    let error = extract_mcp_gateway_api_key(&headers).expect_err("missing");
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
    let error = extract_mcp_gateway_api_key(&headers).expect_err("malformed");
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
    let error = extract_mcp_gateway_api_key(&headers).expect_err("conflict");
    assert!(matches!(error, AuthError::ConflictingApiKeyHeaders));
}

#[tokio::test]
async fn guarded_mcp_sse_rejects_malformed_events_without_forwarding_payloads() {
    let (_directory, mut state) = crate::http::test_support::app_state().await;
    std::sync::Arc::make_mut(&mut state.guardrail_config)
        .default
        .enabled = true;
    for separator in ["\n", "\r\n"] {
        let source = format!(
            "data: {{\"result\":{{\"content\":\"private-valid-payload\"}}}}{separator}{separator}data: private-invalid-payload{separator}{separator}"
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
