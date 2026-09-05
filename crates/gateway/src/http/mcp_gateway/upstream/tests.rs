use std::collections::BTreeMap;

use axum::{Json, Router, body::to_bytes, response::IntoResponse, routing::any};
use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpServerRecord, ExternalMcpServerStatus, ExternalMcpTransport,
};
use serde_json::json;
use time::OffsetDateTime;
use tokio::{net::TcpListener, task::JoinHandle};
use uuid::Uuid;

use super::*;

struct TestUpstream {
    upstream: McpGatewayUpstream,
    task: JoinHandle<()>,
}

impl TestUpstream {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock MCP");
        let address = listener.local_addr().expect("mock address");
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve mock MCP");
        });
        let now = OffsetDateTime::now_utc();
        Self {
            upstream: McpGatewayUpstream {
                server: ExternalMcpServerRecord {
                    mcp_server_id: Uuid::new_v4(),
                    server_key: "upstream".into(),
                    display_name: "Upstream".into(),
                    description: None,
                    transport: ExternalMcpTransport::StreamableHttp,
                    server_url: format!("http://{address}/mcp"),
                    auth_mode: ExternalMcpAuthMode::None,
                    auth_config: Default::default(),
                    timeout_ms: 1000,
                    status: ExternalMcpServerStatus::Active,
                    last_discovery_status: None,
                    last_discovery_at: None,
                    last_successful_discovery_at: None,
                    last_error_summary: None,
                    last_tool_count: None,
                    created_at: now,
                    updated_at: now,
                    disabled_at: None,
                },
                headers: None,
            },
            task,
        }
    }
}

impl Drop for TestUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn proxy_preserves_protocol_headers_without_forwarding_caller_credentials_or_cookies() {
    let router = Router::new().fallback(any(|headers: HeaderMap, body: Bytes| async move {
        let received = headers
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().expect("visible header").to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut response = (
            StatusCode::ACCEPTED,
            [
                (MCP_PROTOCOL_VERSION, "2025-03-26"),
                ("set-cookie", "upstream-secret=never-forward"),
                ("x-upstream-secret", "never-forward"),
                ("cache-control", "public, max-age=3600"),
            ],
            Json(json!({"headers": received, "body": std::str::from_utf8(&body).unwrap()})),
        )
            .into_response();
        response
            .headers_mut()
            .append(MCP_SESSION_ID, HeaderValue::from_static("session-one"));
        response
            .headers_mut()
            .append(MCP_SESSION_ID, HeaderValue::from_static("session-two"));
        response
    }));
    let mut server = TestUpstream::start(router).await;
    server.upstream.headers = Some(BTreeMap::from([(
        "authorization".into(),
        "Bearer managed-credential".into(),
    )]));
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("HTTP client");
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("accept", "application/json"),
        ("content-type", "application/json"),
        (MCP_PROTOCOL_VERSION, "2025-03-26"),
        (MCP_SESSION_ID, "session-request"),
        (LAST_EVENT_ID, "event-5"),
        ("authorization", "Bearer caller-key"),
        ("x-oceans-api-key", "caller-key"),
        ("cookie", "session=caller-cookie"),
        ("x-private-header", "private"),
    ] {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }

    for buffered in [false, true] {
        let response = if buffered {
            let response = proxy_buffered(
                &client,
                &Method::POST,
                &headers,
                Bytes::from_static(b"request-body"),
                &server.upstream,
            )
            .await
            .expect("buffered proxy");
            response_from_parts(
                response.status,
                &response.headers,
                Body::from(response.body),
            )
        } else {
            proxy_upstream(
                &client,
                &Method::POST,
                &headers,
                Bytes::from_static(b"request-body"),
                &server.upstream,
            )
            .await
            .expect("streamed proxy")
        };
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        assert_eq!(response.headers()[MCP_PROTOCOL_VERSION], "2025-03-26");
        assert_eq!(
            response
                .headers()
                .get_all(MCP_SESSION_ID)
                .iter()
                .collect::<Vec<_>>(),
            vec!["session-one", "session-two"]
        );
        assert!(!response.headers().contains_key("set-cookie"));
        assert!(!response.headers().contains_key("x-upstream-secret"));
        let body = to_bytes(response.into_body(), 8192)
            .await
            .expect("proxy body");
        let echoed: Value = serde_json::from_slice(&body).expect("echo response");
        assert_eq!(echoed["body"], "request-body");
        let received = &echoed["headers"];
        assert_eq!(received["authorization"], "Bearer managed-credential");
        for name in [
            ACCEPT.as_str(),
            CONTENT_TYPE.as_str(),
            MCP_PROTOCOL_VERSION,
            MCP_SESSION_ID,
            LAST_EVENT_ID,
        ] {
            assert_eq!(
                received[name],
                headers[name].to_str().expect("inbound header")
            );
        }
        for name in ["x-oceans-api-key", "cookie", "x-private-header"] {
            assert!(
                received.get(name).is_none(),
                "caller {name} must not reach upstream"
            );
        }
    }
}

#[test]
fn sse_filters_every_tools_result_and_preserves_valid_progress_and_error_events() {
    let progress = json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": 1, "progressToken": "t"}});
    let error = json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32603, "message": "Discovery failed"}});
    let result = json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [{"name": "allowed"}, {"name": "blocked"}, {"title": "invalid"}]}});
    let bom_prefixed = format!("\u{feff}data: {result}\n\n");
    let filtered = filter_tools_list_sse(bom_prefixed.as_bytes(), &HashSet::from(["allowed"]))
        .expect("filter first event after BOM");
    let text = std::str::from_utf8(&filtered).unwrap();
    let data = text
        .trim()
        .strip_prefix("data: ")
        .expect("first data event");
    let event: Value = serde_json::from_str(data).unwrap();
    assert_eq!(event["result"]["tools"], json!([{"name": "allowed"}]));
    for newline in ["\n", "\r\n", "\r"] {
        let body = format!(
            "event: message{newline}data: {progress}{newline}{newline}data: {error}{newline}{newline}id: 3{newline}data: {result}{newline}{newline}data: {result}{newline}{newline}"
        );
        let filtered = filter_tools_list_sse(body.as_bytes(), &HashSet::from(["allowed"]))
            .expect("valid event sequence");
        let text = std::str::from_utf8(&filtered).expect("filtered UTF-8");
        let events = text
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|data| serde_json::from_str::<Value>(data).expect("JSON event"))
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0], progress);
        assert_eq!(events[1], error);
        for event in &events[2..] {
            assert_eq!(event["result"]["tools"], json!([{"name": "allowed"}]));
        }
        assert!(text.contains("id: 3\n"));
        assert!(text.ends_with("\n\n"));
    }
}

#[test]
fn sse_rejects_malformed_envelopes_and_cannot_hide_tools_in_an_error_or_notification() {
    for value in [
        json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
        json!({"jsonrpc": "2.0", "id": 1, "error": {"message": "no code"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": "invalid"}),
        json!({"method": "notifications/progress"}),
    ] {
        let body = format!("data: {value}\n\n");
        assert!(filter_tools_list_sse(body.as_bytes(), &HashSet::new()).is_err());
    }
    for envelope in [
        json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32603, "message": "error"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/progress"}),
    ] {
        let mut value = envelope;
        value["result"] = json!({"tools": [{"name": "blocked"}]});
        let body = format!("data: {value}\n\n");
        let filtered =
            filter_tools_list_sse(body.as_bytes(), &HashSet::new()).expect("filtered result");
        assert!(!std::str::from_utf8(&filtered).unwrap().contains("blocked"));
        let filtered = filter_tools_list_json(
            &serde_json::to_vec(&value).unwrap(),
            &HashSet::new(),
            Some(&json!(5)),
        )
        .expect("filtered JSON result");
        let result: Value = serde_json::from_slice(&filtered).unwrap();
        assert_eq!(result["id"], 5);
        assert_eq!(result["result"]["tools"], json!([]));
    }
}

#[tokio::test]
async fn transport_failures_do_not_disclose_the_upstream_url() {
    let mut server = TestUpstream::start(Router::new()).await;
    server.task.abort();
    server.upstream.server.server_url =
        "http://127.0.0.1:0/private-mcp?api_key=upstream-secret".into();
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("HTTP client");
    for buffered in [false, true] {
        let error = if buffered {
            proxy_buffered(
                &client,
                &Method::POST,
                &HeaderMap::new(),
                Bytes::new(),
                &server.upstream,
            )
            .await
            .err()
            .expect("transport failure")
        } else {
            proxy_upstream(
                &client,
                &Method::POST,
                &HeaderMap::new(),
                Bytes::new(),
                &server.upstream,
            )
            .await
            .expect_err("transport failure")
        };
        assert!(matches!(
            error,
            GatewayError::Provider(ProviderError::Transport(_))
        ));
        let message = error.to_string();
        for private in ["upstream-secret", "api_key", "private-mcp", "127.0.0.1"] {
            assert!(!message.contains(private), "URL leaked in transport error");
        }
    }
}

#[tokio::test]
async fn buffered_bodies_enforce_the_limit_with_and_without_content_length() {
    for advertised in [false, true] {
        for length in [
            MAX_MCP_REWRITE_BODY_BYTES as usize,
            MAX_MCP_REWRITE_BODY_BYTES as usize + 1,
        ] {
            let router = Router::new().fallback(any(move || async move {
                let bytes = Bytes::from(vec![b'x'; length]);
                if advertised {
                    Body::from(bytes)
                } else {
                    Body::from_stream(futures_util::stream::once(async {
                        Ok::<_, io::Error>(bytes)
                    }))
                }
            }));
            let server = TestUpstream::start(router).await;
            let client = reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("HTTP client");
            let result = proxy_buffered(
                &client,
                &Method::POST,
                &HeaderMap::new(),
                Bytes::new(),
                &server.upstream,
            )
            .await;
            if length == MAX_MCP_REWRITE_BODY_BYTES as usize {
                assert_eq!(result.expect("body at limit").body.len(), length);
            } else {
                assert!(matches!(result, Err(GatewayError::PayloadTooLarge { .. })));
            }
        }
    }
}

#[tokio::test]
async fn timeout_applies_to_buffered_and_finite_responses_but_not_receive_streams() {
    let router = Router::new().fallback(any(|| async {
        Body::from_stream(futures_util::stream::once(async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            Ok::<_, io::Error>(Bytes::from_static(b"complete"))
        }))
    }));
    let mut server = TestUpstream::start(router).await;
    server.upstream.server.timeout_ms = 100;
    server
        .upstream
        .server
        .server_url
        .push_str("?api_key=upstream-secret");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("HTTP client");

    for (method, accept, receive_stream) in [
        (Method::POST, "application/json", false),
        (Method::POST, "text/event-stream", true),
        (Method::GET, "application/json", true),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        let response = proxy_upstream(&client, &method, &headers, Bytes::new(), &server.upstream)
            .await
            .expect("response headers before timeout");
        let body =
            tokio::time::timeout(Duration::from_secs(2), to_bytes(response.into_body(), 1024))
                .await
                .expect("body terminates");
        if receive_stream {
            assert_eq!(body.expect("receive stream stays open"), "complete");
        } else {
            let error = body.expect_err("finite response times out").to_string();
            assert!(!error.contains("upstream-secret"));
        }
    }
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    let result = proxy_buffered(
        &client,
        &Method::GET,
        &headers,
        Bytes::new(),
        &server.upstream,
    )
    .await;
    assert!(matches!(
        result,
        Err(GatewayError::Provider(ProviderError::Timeout))
    ));
}
