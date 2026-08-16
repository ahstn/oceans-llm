use super::*;

#[test]
fn parses_upstream_model_family() {
    let (family, _, _) = parse_upstream_model("google/gemini-2.0-flash").expect("parse");
    assert!(matches!(family, super::PublisherFamily::Google));
    parse_upstream_model("bad-format").expect_err("invalid format");
    parse_upstream_model("meta/llama").expect_err("unsupported family");
}

#[test]
fn model_endpoint_defaults_to_https_but_honors_explicit_scheme() {
    let default_host = vertex_provider_for_test("aiplatform.googleapis.com".to_string());
    let default_url = default_host.model_endpoint("google", "gemini-2.0-flash", "generateContent");
    assert!(default_url.starts_with("https://aiplatform.googleapis.com/"));

    let bare_host_with_slash = vertex_provider_for_test("aiplatform.googleapis.com/".to_string());
    let bare_host_url =
        bare_host_with_slash.model_endpoint("google", "gemini-2.0-flash", "generateContent");
    assert!(bare_host_url.starts_with("https://aiplatform.googleapis.com/v1/"));

    let explicit_host = vertex_provider_for_test("http://127.0.0.1:8080/".to_string());
    let explicit_url =
        explicit_host.model_endpoint("google", "gemini-2.0-flash", "generateContent");
    assert!(explicit_url.starts_with("http://127.0.0.1:8080/"));
}

#[test]
fn vertex_provider_advertises_tool_capable_chat() {
    let provider = vertex_provider_for_test("http://127.0.0.1:1".to_string());
    let capabilities = provider.capabilities();

    assert!(capabilities.chat_completions);
    assert!(capabilities.stream);
    assert!(capabilities.tools);
    assert!(capabilities.developer_role);
}

#[tokio::test]
async fn vertex_provider_google_non_stream_executes_real_http_mapping() {
    let captured = Arc::new(Mutex::new(None::<Value>));
    let state = captured.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |Path(path): Path<String>,
                 State(captured): State<Arc<Mutex<Option<Value>>>>,
                 headers: HeaderMap,
                 Json(payload): Json<Value>| async move {
                    assert!(path.ends_with(":generateContent"));
                    assert_eq!(
                        headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok()),
                        Some("Bearer test-token")
                    );
                    *captured.lock().await = Some(payload);
                    Json(json!({
                        "responseId": "resp-google-1",
                        "candidates": [{
                            "index": 0,
                            "content": {"parts": [{"text":"pong"}]},
                            "finishReason":"STOP"
                        }]
                    }))
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));

    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("ping".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert("temperature".to_string(), json!(0.2));

    let response = provider
        .chat_completions(&request, &context("google/gemini-2.0-flash"))
        .await
        .expect("chat completion");

    assert_eq!(response["choices"][0]["message"]["content"], "pong");

    let request_payload = captured.lock().await.clone().expect("captured request");
    assert_eq!(request_payload["contents"][0]["parts"][0]["text"], "ping");
    assert_eq!(
        request_payload["generationConfig"]["temperature"],
        json!(0.2)
    );
}

#[tokio::test]
async fn vertex_provider_google_video_executes_http_request_and_response_mapping() {
    let captured = Arc::new(Mutex::new(None::<Value>));
    let state = captured.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |Path(path): Path<String>,
                 State(captured): State<Arc<Mutex<Option<Value>>>>,
                 Json(payload): Json<Value>| async move {
                    assert!(path.ends_with(":generateContent"));
                    *captured.lock().await = Some(payload);
                    Json(json!({
                        "responseId": "resp-google-video-1",
                        "candidates": [{
                            "index": 0,
                            "content": {
                                "parts": [{
                                    "text": "A red title card appears."
                                }]
                            },
                            "finishReason": "STOP"
                        }]
                    }))
                },
            ),
        )
        .with_state(state);
    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let signed_url = "https://media.example.invalid/known-event.mp4?expires=1&signature=secret";
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {"type": "text", "text": "What event occurs?"},
            {
                "type": "video_url",
                "video_url": {
                    "url": signed_url,
                    "mime_type": "video/mp4"
                }
            }
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let response = provider
        .chat_completions(&request, &context("google/gemini-2.0-flash"))
        .await
        .expect("chat completion");

    assert_eq!(
        response["choices"][0]["message"]["content"],
        "A red title card appears."
    );
    let request_payload = captured.lock().await.clone().expect("captured request");
    assert_eq!(
        request_payload["contents"][0]["parts"][1]["fileData"],
        json!({
            "fileUri": signed_url,
            "mimeType": "video/mp4"
        })
    );
}

#[tokio::test]
async fn vertex_provider_anthropic_stream_handles_fragmented_crlf_events() {
    let captured = Arc::new(Mutex::new(None::<Value>));
    let state = captured.clone();
    let app = Router::new()
            .route(
                "/v1/{*path}",
                post(
                    |Path(path): Path<String>,
                     State(captured): State<Arc<Mutex<Option<Value>>>>,
                     headers: HeaderMap,
                     Json(payload): Json<Value>| async move {
                        assert!(path.ends_with(
                            "publishers/anthropic/models/claude-sonnet-4-6:streamRawPredict"
                        ));
                        assert_eq!(
                            headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok()),
                            Some("Bearer test-token")
                        );
                        *captured.lock().await = Some(payload);

                        let chunks = vec![
                            "event: message_start\r\n",
                            "data: {\"type\":\"message_start\"}\r\n",
                            "\r\nevent: content_block_delta\r\n",
                            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\r\n\r\n",
                            "event: content_block_delta\r\n",
                            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\r\n\r\n",
                            "event: message_delta\r\n",
                            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\r\n\r\n",
                            "event: message_stop\r\n",
                            "data: {\"type\":\"message_stop\"}\r\n\r\n",
                        ];

                        let body = Body::from_stream(stream::iter(chunks.into_iter().map(
                            |chunk| Ok::<_, Infallible>(Bytes::from(chunk)),
                        )));
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .body(body)
                            .expect("stream response")
                    },
                ),
            )
            .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("ping".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.stream = true;

    let stream = provider
        .chat_completions_stream(&request, &context("anthropic/claude-sonnet-4-6"))
        .await
        .expect("stream");

    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(rendered.contains("\"content\":\"hel\""));
    assert!(rendered.contains("\"content\":\"lo\""));
    assert_eq!(rendered.matches("\"role\":\"assistant\"").count(), 1);
    assert!(rendered.contains("\"finish_reason\":\"stop\""));
    assert!(rendered.contains("data: [DONE]"));

    let request_payload = captured.lock().await.clone().expect("captured request");
    assert_eq!(
        request_payload["anthropic_version"],
        Value::String("vertex-2023-10-16".to_string())
    );
    assert!(request_payload.get("model").is_none());
    assert_eq!(request_payload["stream"], Value::Bool(true));
}
