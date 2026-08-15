use super::*;

#[test]
fn maps_vertex_embedding_aliases_to_predict_payload() {
    let mut request = embedding_request(json!("document text"));
    request.extra.insert("dimensions".to_string(), json!(128));
    request
        .extra
        .insert("output_dimensionality".to_string(), json!(128));
    request
        .extra
        .insert("outputDimensionality".to_string(), json!(128));
    request
        .extra
        .insert("task_type".to_string(), json!("retrieval document"));
    request
        .extra
        .insert("input_type".to_string(), json!("document"));
    request
        .extra
        .insert("title".to_string(), json!("Doc title"));
    request
        .extra
        .insert("auto_truncate".to_string(), json!(false));
    request
        .extra
        .insert("autoTruncate".to_string(), json!(false));

    let mapped = map_google_embedding_request(
        &request,
        &context("google/gemini-embedding-001"),
        "gemini-embedding-001",
    )
    .expect("mapped embeddings request");

    assert_eq!(mapped.bodies.len(), 1);
    assert_eq!(mapped.bodies[0]["instances"][0]["content"], "document text");
    assert_eq!(
        mapped.bodies[0]["instances"][0]["task_type"],
        "RETRIEVAL_DOCUMENT"
    );
    assert_eq!(mapped.bodies[0]["instances"][0]["title"], "Doc title");
    assert_eq!(mapped.bodies[0]["parameters"]["outputDimensionality"], 128);
    assert_eq!(mapped.bodies[0]["parameters"]["autoTruncate"], false);
}

#[tokio::test]
async fn rejects_invalid_vertex_embedding_inputs_and_alias_conflicts() {
    let cases = [
        (
            "base64 encoding",
            json!("hello"),
            vec![("encoding_format", json!("base64"))],
            "encoding_format `base64` is not supported",
        ),
        (
            "token array",
            json!([1, 2, 3]),
            Vec::new(),
            "input must be a string or array of strings",
        ),
        (
            "nested array",
            json!([["hello"]]),
            Vec::new(),
            "input must be a string or array of strings",
        ),
        (
            "non-string scalar",
            json!(42),
            Vec::new(),
            "input must be a string or array of strings",
        ),
        (
            "empty string",
            json!(""),
            Vec::new(),
            "input strings must not be empty",
        ),
        (
            "empty array",
            json!([]),
            Vec::new(),
            "input array must contain at least one string",
        ),
        (
            "invalid task",
            json!("hello"),
            vec![("task_type", json!("search"))],
            "unsupported vertex embeddings task_type",
        ),
        (
            "conflicting dimensions",
            json!("hello"),
            vec![
                ("dimensions", json!(128)),
                ("outputDimensionality", json!(256)),
            ],
            "conflicting vertex embeddings dimensionality fields",
        ),
        (
            "conflicting task aliases",
            json!("hello"),
            vec![
                ("task_type", json!("RETRIEVAL_QUERY")),
                ("input_type", json!("RETRIEVAL_DOCUMENT")),
            ],
            "conflicting vertex embeddings task_type and input_type fields",
        ),
        (
            "conflicting auto truncate aliases",
            json!("hello"),
            vec![
                ("auto_truncate", json!(true)),
                ("autoTruncate", json!(false)),
            ],
            "conflicting vertex embeddings auto_truncate and autoTruncate fields",
        ),
    ];
    let provider = vertex_provider_for_test("http://127.0.0.1:1".to_string());

    for (name, input, extra, expected) in cases {
        let mut request = embedding_request(input);
        for (key, value) in extra {
            request.extra.insert(key.to_string(), value);
        }

        let error = provider
            .embeddings(&request, &context("google/gemini-embedding-001"))
            .await
            .expect_err(name)
            .to_string();

        assert!(
            error.contains(expected),
            "{name}: expected `{error}` to contain `{expected}`"
        );
    }
}

#[tokio::test]
async fn vertex_embeddings_rejects_unsupported_google_chat_model_before_http() {
    let provider = vertex_provider_for_test("http://127.0.0.1:1".to_string());
    let request = embedding_request(json!("hello"));

    let error = provider
        .embeddings(&request, &context("google/gemini-2.0-flash"))
        .await
        .expect_err("chat model must not be accepted for embeddings")
        .to_string();

    assert!(error.contains("not a supported text embedding model"));
    assert!(error.contains("gemini-embedding-001"));
}

#[tokio::test]
async fn vertex_embeddings_rejects_title_without_document_task_before_http() {
    let requests_seen = Arc::new(Mutex::new(0usize));
    let state = requests_seen.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |State(requests_seen): State<Arc<Mutex<usize>>>| async move {
                    *requests_seen.lock().await += 1;
                    Json(json!({
                        "predictions": [{
                            "embeddings": {
                                "values": [0.25],
                                "statistics": {"token_count": 1}
                            }
                        }]
                    }))
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let mut request = embedding_request(json!("query text"));
    request
        .extra
        .insert("task_type".to_string(), json!("RETRIEVAL_QUERY"));
    request
        .extra
        .insert("title".to_string(), json!("Doc title"));

    let error = provider
        .embeddings(&request, &context("google/gemini-embedding-001"))
        .await
        .expect_err("title must require RETRIEVAL_DOCUMENT")
        .to_string();

    assert!(error.contains("title is only supported with task_type RETRIEVAL_DOCUMENT"));
    assert_eq!(*requests_seen.lock().await, 0);
}

#[tokio::test]
async fn vertex_provider_google_embedding_string_executes_predict_mapping() {
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
                        path.ends_with("publishers/google/models/gemini-embedding-001:predict")
                    );
                    assert_eq!(
                        headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok()),
                        Some("Bearer test-token")
                    );
                    *captured.lock().await = Some(payload);
                    Json(json!({
                        "predictions": [{
                            "embeddings": {
                                "values": [0.25, 0.5],
                                "statistics": {"token_count": 3, "billable_character_count": 999}
                            }
                        }]
                    }))
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let request = embedding_request(json!("hello embeddings"));

    let response = provider
        .embeddings(&request, &context("google/gemini-embedding-001"))
        .await
        .expect("embedding response");

    assert_eq!(response["object"], "list");
    assert_eq!(response["model"], "fast");
    assert_eq!(response["data"][0]["object"], "embedding");
    assert_eq!(response["data"][0]["index"], 0);
    assert_eq!(response["data"][0]["embedding"], json!([0.25, 0.5]));
    assert_eq!(
        response["usage"],
        json!({"prompt_tokens": 3, "total_tokens": 3})
    );

    let request_payload = captured.lock().await.clone().expect("captured request");
    assert_eq!(
        request_payload,
        json!({
            "instances": [{"content": "hello embeddings"}]
        })
    );
}

#[tokio::test]
async fn vertex_provider_google_embedding_array_fans_out_and_preserves_order() {
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let state = captured.clone();
    let app = Router::new()
            .route(
                "/v1/{*path}",
                post(
                    |Path(path): Path<String>,
                     State(captured): State<Arc<Mutex<Vec<Value>>>>,
                     Json(payload): Json<Value>| async move {
                        assert!(path.ends_with(
                            "publishers/google/models/text-embedding-005:predict"
                        ));
                        let content = payload["instances"][0]["content"]
                            .as_str()
                            .expect("string content")
                            .to_string();
                        captured.lock().await.push(payload);
                        match content.as_str() {
                            "first" => Json(json!({
                                "predictions": [{
                                    "embeddings": {
                                        "values": [1.0, 1.5],
                                        "statistics": {"token_count": 2, "billable_character_count": 999}
                                    }
                                }]
                            })),
                            "second" => Json(json!({
                                "predictions": [{
                                    "embeddings": {
                                        "values": [2.0, 2.5],
                                        "statistics": {"token_count": 5, "billable_character_count": 999}
                                    }
                                }]
                            })),
                            other => panic!("unexpected embedding content: {other}"),
                        }
                    },
                ),
            )
            .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let request = embedding_request(json!(["first", "second"]));

    let response = provider
        .embeddings(&request, &context("google/text-embedding-005"))
        .await
        .expect("embedding response");

    assert_eq!(response["data"][0]["index"], 0);
    assert_eq!(response["data"][0]["embedding"], json!([1.0, 1.5]));
    assert_eq!(response["data"][1]["index"], 1);
    assert_eq!(response["data"][1]["embedding"], json!([2.0, 2.5]));
    assert_eq!(
        response["usage"],
        json!({"prompt_tokens": 7, "total_tokens": 7})
    );

    let request_payloads = captured.lock().await.clone();
    assert_eq!(request_payloads.len(), 2);
    assert_eq!(request_payloads[0]["instances"][0]["content"], "first");
    assert_eq!(request_payloads[1]["instances"][0]["content"], "second");
}

#[tokio::test]
async fn vertex_provider_google_gemini_embedding_2_executes_embed_content_mapping() {
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let state = captured.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |Path(path): Path<String>,
                 State(captured): State<Arc<Mutex<Vec<Value>>>>,
                 Json(payload): Json<Value>| async move {
                    assert!(
                        path.ends_with("publishers/google/models/gemini-embedding-2:embedContent")
                    );
                    let text = payload["content"]["parts"][0]["text"]
                        .as_str()
                        .expect("text part")
                        .to_string();
                    captured.lock().await.push(payload);
                    match text.as_str() {
                        "first" => Json(json!({
                            "embedding": {"values": [1.0, 1.5]},
                            "usageMetadata": {
                                "promptTokenCount": 2,
                                "totalTokenCount": 999
                            }
                        })),
                        "second" => Json(json!({
                            "embedding": {"values": [2.0, 2.5]},
                            "usageMetadata": {
                                "promptTokenCount": 5,
                                "totalTokenCount": 999
                            }
                        })),
                        other => panic!("unexpected embedding text: {other}"),
                    }
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let mut request = embedding_request(json!(["first", "second"]));
    request.extra.insert("dimensions".to_string(), json!(256));
    request
        .extra
        .insert("output_dimensionality".to_string(), json!(256));
    request
        .extra
        .insert("outputDimensionality".to_string(), json!(256));

    let response = provider
        .embeddings(&request, &context("google/gemini-embedding-2"))
        .await
        .expect("embedding response");

    assert_eq!(response["object"], "list");
    assert_eq!(response["model"], "fast");
    assert_eq!(response["data"][0]["index"], 0);
    assert_eq!(response["data"][0]["embedding"], json!([1.0, 1.5]));
    assert_eq!(response["data"][1]["index"], 1);
    assert_eq!(response["data"][1]["embedding"], json!([2.0, 2.5]));
    assert_eq!(
        response["usage"],
        json!({"prompt_tokens": 7, "total_tokens": 7})
    );

    let request_payloads = captured.lock().await.clone();
    assert_eq!(request_payloads.len(), 2);
    assert_eq!(
        request_payloads[0],
        json!({
            "content": {"parts": [{"text": "first"}]},
            "embedContentConfig": {"outputDimensionality": 256}
        })
    );
    assert_eq!(
        request_payloads[1],
        json!({
            "content": {"parts": [{"text": "second"}]},
            "embedContentConfig": {"outputDimensionality": 256}
        })
    );
}

#[tokio::test]
async fn vertex_provider_google_gemini_embedding_2_rejects_predict_only_fields_before_http() {
    let requests_seen = Arc::new(Mutex::new(0usize));
    let state = requests_seen.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |State(requests_seen): State<Arc<Mutex<usize>>>| async move {
                    *requests_seen.lock().await += 1;
                    Json(json!({
                        "embedding": {"values": [0.25]},
                        "usageMetadata": {"promptTokenCount": 1}
                    }))
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let cases = [
        ("task_type", json!("RETRIEVAL_QUERY")),
        ("input_type", json!("query")),
        ("title", json!("Doc title")),
        ("auto_truncate", json!(false)),
    ];

    for (field, value) in cases {
        let mut request = embedding_request(json!("hello embeddings"));
        request.extra.insert(field.to_string(), value);

        let error = provider
            .embeddings(&request, &context("google/gemini-embedding-2"))
            .await
            .expect_err(field)
            .to_string();

        assert!(
            error.contains(&format!("does not support `{field}`")),
            "{field}: unexpected error `{error}`"
        );
    }
    assert_eq!(*requests_seen.lock().await, 0);
}

#[tokio::test]
async fn vertex_provider_google_gemini_embedding_2_returns_partial_usage_after_fanout_failure() {
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let state = captured.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |Path(path): Path<String>,
                 State(captured): State<Arc<Mutex<Vec<Value>>>>,
                 Json(payload): Json<Value>| async move {
                    assert!(
                        path.ends_with("publishers/google/models/gemini-embedding-2:embedContent")
                    );
                    let text = payload["content"]["parts"][0]["text"]
                        .as_str()
                        .expect("text part")
                        .to_string();
                    captured.lock().await.push(payload);
                    match text.as_str() {
                        "first" => (
                            StatusCode::OK,
                            Json(json!({
                                "embedding": {"values": [1.0, 1.5]},
                                "usageMetadata": {"promptTokenCount": 4}
                            })),
                        ),
                        "second" => (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({"error": {"message": "quota exhausted"}})),
                        ),
                        other => panic!("unexpected embedding text: {other}"),
                    }
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let request = embedding_request(json!(["first", "second"]));

    let error = provider
        .embeddings(&request, &context("google/gemini-embedding-2"))
        .await
        .expect_err("second fan-out call should return provider error with partial usage");

    match error {
        ProviderError::PartialUsage {
            source,
            provider_usage,
        } => {
            assert_eq!(
                provider_usage,
                Some(json!({"prompt_tokens": 4, "total_tokens": 4}))
            );
            match *source {
                ProviderError::UpstreamHttp { status, body } => {
                    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS.as_u16());
                    assert!(body.contains("quota exhausted"));
                }
                other => panic!("unexpected partial usage source: {other}"),
            }
        }
        other => panic!("unexpected error: {other}"),
    }

    let request_payloads = captured.lock().await.clone();
    assert_eq!(request_payloads.len(), 2);
    assert_eq!(request_payloads[0]["content"]["parts"][0]["text"], "first");
    assert_eq!(request_payloads[1]["content"]["parts"][0]["text"], "second");
}
