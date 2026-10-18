use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    http::{HeaderMap, Response, header::CONTENT_TYPE},
    routing::post,
};
use futures_util::{StreamExt, stream};
use gateway_mcp::{
    CLIENT_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_HEADER, McpClientError, StreamableHttpClient,
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

async fn serve(router: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let endpoint = format!(
        "http://{}/mcp",
        listener.local_addr().expect("server address")
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve requests");
    });
    (endpoint, task)
}

#[tokio::test]
async fn rejects_json_responses_for_a_different_request() {
    for response_id in [json!(2), json!("1"), Value::Null] {
        let response = json!({
            "jsonrpc": "2.0",
            "id": response_id,
            "result": {"protocolVersion": "2025-03-26", "capabilities": {}}
        });
        let router = Router::new().route(
            "/mcp",
            post(move || {
                let response = response.clone();
                async move { Json(response) }
            }),
        );
        let (endpoint, server) = serve(router).await;
        let client = StreamableHttpClient::new(&endpoint, Duration::from_secs(5)).expect("client");

        let result = client.initialize().await;

        server.abort();
        assert!(
            matches!(result, Err(McpClientError::InvalidResponse { .. })),
            "response id {response_id} must not satisfy initialize id 1: {result:?}"
        );
    }
}

#[tokio::test]
async fn skips_unrelated_sse_results_before_decoding_the_requested_result() {
    let body = concat!(
        "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":\"a different result shape\"}\n\n",
        "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{}}}\n\n",
    );
    for (format, response_body) in [
        ("LF", body.to_string()),
        ("CRLF", body.replace('\n', "\r\n")),
        ("CR", body.replace('\n', "\r")),
        (
            "mixed with BOM",
            format!("\u{feff}{}", body.replacen("\n\n", "\r\n\r", 1)),
        ),
        (
            "BOM before matching response",
            format!("\u{feff}{}", body.split_once("\n\n").expect("two events").1),
        ),
    ] {
        let router = Router::new().route(
            "/mcp",
            post(move || {
                let body = response_body.clone();
                async move { ([(CONTENT_TYPE, "text/event-stream")], body) }
            }),
        );
        let (endpoint, server) = serve(router).await;
        let client = StreamableHttpClient::new(&endpoint, Duration::from_secs(5)).expect("client");

        let result = client.initialize().await;

        server.abort();
        assert_eq!(
            result.expect(format).protocol_version,
            "2025-03-26",
            "{format}"
        );
    }
}

#[tokio::test]
async fn classifies_a_timeout_while_reading_the_body() {
    let router = Router::new().route(
        "/mcp",
        post(|| async {
            let chunks = stream::once(async { Ok::<_, Infallible>("{\"jsonrpc\":\"2.0\",") })
                .chain(stream::pending());
            Response::builder()
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from_stream(chunks))
                .expect("streaming response")
        }),
    );
    let (endpoint, server) = serve(router).await;
    let client = StreamableHttpClient::new(
        &format!("{endpoint}?api_key=timeout-query-secret"),
        Duration::from_millis(500),
    )
    .expect("client");

    let result = client.initialize().await;

    server.abort();
    assert!(matches!(result, Err(McpClientError::Timeout)), "{result:?}");
}

#[tokio::test]
async fn transport_errors_do_not_disclose_the_upstream_url() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind port");
    let address = listener.local_addr().expect("server address").to_string();
    drop(listener);
    let endpoint = format!(
        "http://upstream-user:upstream-password@{address}/private-mcp?api_key=query-secret"
    );
    let client = StreamableHttpClient::new(&endpoint, Duration::from_secs(5)).expect("client");

    let error = client.initialize().await.expect_err("connection refused");

    assert!(matches!(error, McpClientError::Transport(_)), "{error:?}");
    let diagnostic = format!("{error} {error:?}");
    for private_value in [
        address.as_str(),
        "upstream-user",
        "upstream-password",
        "private-mcp",
        "query-secret",
    ] {
        assert!(!diagnostic.contains(private_value), "{diagnostic}");
    }
}

#[tokio::test]
async fn protocol_override_matches_the_initialize_body_and_header() {
    let router = Router::new().route(
        "/mcp",
        post(
            |headers: HeaderMap, Json(request): Json<Value>| async move {
                assert_eq!(headers[MCP_PROTOCOL_VERSION_HEADER], "2025-11-25");
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": {
                        "protocolVersion": request["params"]["protocolVersion"],
                        "capabilities": {}
                    }
                }))
            },
        ),
    );
    let (endpoint, server) = serve(router).await;
    let client = StreamableHttpClient::new(&endpoint, Duration::from_secs(5))
        .expect("client")
        .with_protocol_version("2025-11-25");

    let result = client.initialize().await;

    server.abort();
    assert_eq!(result.expect("initialized").protocol_version, "2025-11-25");
}

#[tokio::test]
async fn discovery_follows_an_older_version_the_server_chooses() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let router = Router::new().route(
        "/mcp",
        post(move |headers: HeaderMap, Json(request): Json<Value>| {
            let recorded = recorded.clone();
            async move {
                let method = request["method"].as_str().unwrap_or_default().to_string();
                let version = headers[MCP_PROTOCOL_VERSION_HEADER]
                    .to_str()
                    .expect("version header")
                    .to_string();
                recorded.lock().expect("lock").push((
                    method.clone(),
                    version,
                    request["params"]["protocolVersion"].clone(),
                ));
                let result = match method.as_str() {
                    "initialize" => json!({"protocolVersion": "2025-03-26", "capabilities": {}}),
                    "tools/list" => json!({"tools": [{
                        "name": "search",
                        "inputSchema": {"type": "object"},
                        "annotations": {"title": "Search issues"}
                    }]}),
                    _ => json!({}),
                };
                Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
            }
        }),
    );
    let (endpoint, server) = serve(router).await;
    let client = StreamableHttpClient::new(&endpoint, Duration::from_secs(5)).expect("client");

    let tools = client.list_tools(None).await;

    server.abort();
    assert_eq!(
        tools.expect("tools")[0].title.as_deref(),
        Some("Search issues")
    );
    let seen = seen.lock().expect("lock");
    assert_eq!(
        seen[0],
        (
            "initialize".to_string(),
            CLIENT_PROTOCOL_VERSION.to_string(),
            json!(CLIENT_PROTOCOL_VERSION)
        )
    );
    for (method, version, _) in &seen[1..] {
        assert_eq!(version, "2025-03-26", "{method}");
    }
    assert_eq!(seen.len(), 3);
}
