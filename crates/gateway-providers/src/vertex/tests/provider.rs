use super::*;

use axum::extract::RawQuery;

#[test]
fn parses_upstream_model_family() {
    let (family, publisher, model_id) =
        parse_upstream_model("google/gemini-3.7-flash").expect("parse google");
    assert!(matches!(family, PublisherFamily::Google));
    assert_eq!(publisher, "google");
    assert_eq!(model_id, "gemini-3.7-flash");

    let (family, _, model_id) =
        parse_upstream_model("anthropic/claude-sonnet-4-6").expect("parse anthropic");
    assert!(matches!(family, PublisherFamily::Anthropic));
    assert_eq!(model_id, "claude-sonnet-4-6");

    for bad in ["bad-format", "google/", "/gemini-3.7-flash"] {
        let error = parse_upstream_model(bad).expect_err(bad);
        assert!(
            matches!(&error, VertexAdapterError::InvalidUpstreamModel(value) if value == bad),
            "{bad}: {error}"
        );
    }

    let error = parse_upstream_model("meta/llama").expect_err("unsupported family");
    assert!(
        matches!(&error, VertexAdapterError::UnsupportedPublisher(publisher) if publisher == "meta"),
        "{error}"
    );
    assert!(matches!(
        ProviderError::from(error),
        ProviderError::InvalidRequest(_)
    ));
}

#[test]
fn api_host_follows_location_family() {
    assert_eq!(
        vertex_api_host_for_location("global"),
        "aiplatform.googleapis.com"
    );
    assert_eq!(
        vertex_api_host_for_location("us"),
        "aiplatform.us.rep.googleapis.com"
    );
    assert_eq!(
        vertex_api_host_for_location("eu"),
        "aiplatform.eu.rep.googleapis.com"
    );
    assert_eq!(
        vertex_api_host_for_location("us-central1"),
        "us-central1-aiplatform.googleapis.com"
    );
    assert_eq!(
        vertex_api_host_for_location("europe-west4"),
        "europe-west4-aiplatform.googleapis.com"
    );
}

#[test]
fn model_endpoint_defaults_to_https_but_honors_explicit_scheme() {
    let default_host = vertex_provider_for_test("aiplatform.googleapis.com".to_string());
    let default_url = default_host.model_endpoint("google", "gemini-3.7-flash", "generateContent");
    assert!(default_url.starts_with("https://aiplatform.googleapis.com/"));
    assert!(default_url.ends_with(
        "projects/proj-123/locations/global/publishers/google/models/gemini-3.7-flash:generateContent"
    ));

    let bare_host_with_slash = vertex_provider_for_test("aiplatform.googleapis.com/".to_string());
    let bare_host_url =
        bare_host_with_slash.model_endpoint("google", "gemini-3.7-flash", "generateContent");
    assert!(bare_host_url.starts_with("https://aiplatform.googleapis.com/v1/"));

    let explicit_host = vertex_provider_for_test("http://127.0.0.1:8080/".to_string());
    let explicit_url =
        explicit_host.model_endpoint("google", "gemini-3.7-flash", "generateContent");
    assert!(explicit_url.starts_with("http://127.0.0.1:8080/v1/"));
}

#[test]
fn vertex_provider_advertises_tool_and_schema_capable_chat() {
    let provider = vertex_provider_for_test("http://127.0.0.1:1".to_string());
    let capabilities = provider.capabilities();

    assert!(capabilities.chat_completions);
    assert!(capabilities.stream);
    assert!(capabilities.tools);
    assert!(capabilities.vision);
    assert!(capabilities.json_schema);
    assert!(capabilities.developer_role);
    assert!(!capabilities.responses);
}

#[tokio::test]
async fn vertex_batch_inspection_forwards_route_headers() {
    let app = Router::new().route(
        "/v1/{*path}",
        get(|headers: HeaderMap| async move {
            assert_eq!(
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer test-token")
            );
            assert_eq!(
                headers
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok()),
                Some("req-1")
            );
            assert_eq!(
                headers
                    .get("x-goog-user-project")
                    .and_then(|value| value.to_str().ok()),
                Some("billing-project")
            );
            Json(json!({
                "name": "projects/proj-123/locations/global/batchPredictionJobs/123",
                "state": "JOB_STATE_RUNNING"
            }))
        }),
    );
    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let mut request_context = context("google/gemini-3.7-flash");
    request_context
        .extra_headers
        .insert("x-goog-user-project".to_string(), json!("billing-project"));

    let state = provider
        .inspect_batch(
            "projects/proj-123/locations/global/batchPredictionJobs/123",
            &request_context,
        )
        .await
        .expect("batch state");

    assert_eq!(state.status, gateway_core::BatchStatus::InProgress);
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
                    assert!(
                        path.ends_with("publishers/google/models/gemini-3.7-flash:generateContent")
                    );
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
                            "content": {"parts": [
                                {"text": "let me think", "thought": true},
                                {"text": "pong"}
                            ]},
                            "finishReason":"STOP"
                        }],
                        "usageMetadata": {
                            "promptTokenCount": 3,
                            "candidatesTokenCount": 1,
                            "thoughtsTokenCount": 4,
                            "totalTokenCount": 8
                        }
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
    request
        .extra
        .insert("reasoning_effort".to_string(), json!("low"));

    let response = provider
        .chat_completions(&request, &context("google/gemini-3.7-flash"))
        .await
        .expect("chat completion");

    assert_eq!(response["choices"][0]["message"]["content"], "pong");
    assert_eq!(
        response["choices"][0]["message"]["reasoning_content"],
        "let me think"
    );
    assert_eq!(response["choices"][0]["finish_reason"], "stop");
    assert_eq!(response["usage"]["prompt_tokens"], 3);
    assert_eq!(response["usage"]["completion_tokens"], 5);
    assert_eq!(response["usage"]["total_tokens"], 8);
    assert_eq!(
        response["usage"]["completion_tokens_details"]["reasoning_tokens"],
        4
    );

    let request_payload = captured.lock().await.clone().expect("captured request");
    assert_eq!(request_payload["contents"][0]["parts"][0]["text"], "ping");
    // Gemini 3.7+ ignores temperature; it is stripped rather than forwarded.
    assert!(
        request_payload["generationConfig"]
            .get("temperature")
            .is_none()
    );
    assert_eq!(
        request_payload["generationConfig"]["thinkingConfig"],
        json!({"thinkingLevel": "LOW", "includeThoughts": true})
    );
    assert!(request_payload.get("reasoning_effort").is_none());
}

#[tokio::test]
async fn vertex_provider_google_stream_requests_sse_and_maps_thought_and_text_deltas() {
    type CapturedRequest = Arc<Mutex<Option<(String, Value)>>>;
    let captured: CapturedRequest = Arc::new(Mutex::new(None));
    let state = captured.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |Path(path): Path<String>,
                 RawQuery(query): RawQuery,
                 State(captured): State<CapturedRequest>,
                 Json(payload): Json<Value>| async move {
                    assert!(path.ends_with(
                        "publishers/google/models/gemini-3.7-flash:streamGenerateContent"
                    ));
                    let query = query.expect("alt=sse query");
                    *captured.lock().await = Some((query, payload));

                    let frames = google_sse_frames(&[
                        json!({
                            "candidates": [{
                                "index": 0,
                                "content": {"parts": [{"text": "plan", "thought": true}]}
                            }]
                        }),
                        json!({
                            "candidates": [{
                                "index": 0,
                                "content": {"parts": [{"text": "hel"}]}
                            }]
                        }),
                        json!({
                            "candidates": [{
                                "index": 0,
                                "content": {"parts": [{"text": "lo"}]},
                                "finishReason": "STOP"
                            }],
                            "usageMetadata": {
                                "promptTokenCount": 2,
                                "candidatesTokenCount": 2,
                                "thoughtsTokenCount": 1,
                                "totalTokenCount": 5
                            }
                        }),
                    ]);
                    // Split mid-frame so the provider must reassemble SSE events.
                    let split = frames.len() / 2;
                    let chunks = vec![frames[..split].to_string(), frames[split..].to_string()];
                    let body = Body::from_stream(stream::iter(
                        chunks
                            .into_iter()
                            .map(|chunk| Ok::<_, Infallible>(Bytes::from(chunk))),
                    ));
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
    request
        .extra
        .insert("reasoning_effort".to_string(), json!("medium"));

    let stream = provider
        .chat_completions_stream(&request, &context("google/gemini-3.7-flash"))
        .await
        .expect("stream");

    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();
    let events = openai_stream_events(&rendered);

    let reasoning: String = events
        .iter()
        .filter_map(|event| event["choices"][0]["delta"]["reasoning_content"].as_str())
        .collect();
    let content: String = events
        .iter()
        .filter_map(|event| event["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(reasoning, "plan");
    assert_eq!(content, "hello");
    assert_eq!(rendered.matches("\"role\":\"assistant\"").count(), 1);

    let finish = events.last().expect("finish chunk");
    assert_eq!(finish["choices"][0]["finish_reason"], "stop");
    assert_eq!(finish["usage"]["prompt_tokens"], 2);
    assert_eq!(finish["usage"]["completion_tokens"], 3);
    assert_eq!(
        finish["usage"]["completion_tokens_details"]["reasoning_tokens"],
        1
    );
    assert!(rendered.trim_end().ends_with("data: [DONE]"));

    let (query, request_payload) = captured.lock().await.clone().expect("captured request");
    assert_eq!(query, "alt=sse");
    assert_eq!(request_payload["contents"][0]["parts"][0]["text"], "ping");
    assert_eq!(
        request_payload["generationConfig"]["thinkingConfig"],
        json!({"thinkingLevel": "MEDIUM", "includeThoughts": true})
    );
    assert!(request_payload.get("stream").is_none());
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
    assert!(
        request_payload["generationConfig"]
            .get("thinkingConfig")
            .is_none()
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
    let mut default_headers = BTreeMap::new();
    default_headers.insert(
        "anthropic-beta".to_string(),
        "files-api-2025-04-14".to_string(),
    );
    let provider = vertex_provider_with_headers(format!("http://{host}"), default_headers);
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
    assert_eq!(
        request_payload["anthropic_beta"],
        json!(["files-api-2025-04-14"])
    );
}
