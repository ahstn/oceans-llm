use super::*;

use crate::vertex::embeddings::extract_google_embedding_outputs;

fn inputs(count: usize) -> Value {
    Value::Array((0..count).map(|i| json!(format!("text-{i}"))).collect())
}

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
    assert_eq!(mapped.input_count, 1);
    assert_eq!(mapped.bodies[0]["instances"][0]["content"], "document text");
    assert_eq!(
        mapped.bodies[0]["instances"][0]["task_type"],
        "RETRIEVAL_DOCUMENT"
    );
    assert_eq!(mapped.bodies[0]["instances"][0]["title"], "Doc title");
    assert_eq!(mapped.bodies[0]["parameters"]["outputDimensionality"], 128);
    assert_eq!(mapped.bodies[0]["parameters"]["autoTruncate"], false);
}

#[test]
fn batches_predict_inputs_up_to_max_instances_per_body() {
    let count = VERTEX_PREDICT_MAX_INSTANCES + 1;
    let mut request = embedding_request(inputs(count));
    request
        .extra
        .insert("task_type".to_string(), json!("RETRIEVAL_QUERY"));
    let mut context = context("google/text-embedding-005");
    context
        .extra_body
        .insert("parameters".to_string(), json!({"autoTruncate": true}));

    let mapped = map_google_embedding_request(&request, &context, "text-embedding-005")
        .expect("mapped embeddings request");

    assert_eq!(mapped.input_count, count);
    assert_eq!(mapped.bodies.len(), 2);
    let first = mapped.bodies[0]["instances"]
        .as_array()
        .expect("first batch instances");
    let second = mapped.bodies[1]["instances"]
        .as_array()
        .expect("second batch instances");
    assert_eq!(first.len(), VERTEX_PREDICT_MAX_INSTANCES);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0]["content"], "text-0");
    assert_eq!(
        first[VERTEX_PREDICT_MAX_INSTANCES - 1]["content"],
        format!("text-{}", VERTEX_PREDICT_MAX_INSTANCES - 1)
    );
    assert_eq!(
        second[0]["content"],
        format!("text-{VERTEX_PREDICT_MAX_INSTANCES}")
    );
    for body in &mapped.bodies {
        assert_eq!(body["parameters"], json!({"autoTruncate": true}));
        for instance in body["instances"].as_array().expect("instances") {
            assert_eq!(instance["task_type"], "RETRIEVAL_QUERY");
        }
    }
}

#[test]
fn exactly_max_instances_fits_in_one_predict_body() {
    let request = embedding_request(inputs(VERTEX_PREDICT_MAX_INSTANCES));

    let mapped = map_google_embedding_request(
        &request,
        &context("google/text-embedding-005"),
        "text-embedding-005",
    )
    .expect("mapped embeddings request");

    assert_eq!(mapped.bodies.len(), 1);
    assert_eq!(mapped.input_count, VERTEX_PREDICT_MAX_INSTANCES);
    assert_eq!(
        mapped.bodies[0]["instances"]
            .as_array()
            .expect("instances")
            .len(),
        VERTEX_PREDICT_MAX_INSTANCES
    );
}

#[test]
fn gemini_embedding_2_keeps_one_embed_content_body_per_input() {
    let count = VERTEX_PREDICT_MAX_INSTANCES + 1;
    let request = embedding_request(inputs(count));

    let mapped = map_google_embedding_request(
        &request,
        &context("google/gemini-embedding-2"),
        "gemini-embedding-2",
    )
    .expect("mapped embeddings request");

    assert_eq!(mapped.input_count, count);
    assert_eq!(mapped.bodies.len(), count);
    assert_eq!(
        mapped.bodies[0],
        json!({"content": {"parts": [{"text": "text-0"}]}})
    );
    assert_eq!(
        mapped.bodies[count - 1]["content"]["parts"][0]["text"],
        format!("text-{}", count - 1)
    );
}

#[test]
fn extracts_every_prediction_numbered_from_first_index() {
    let response = json!({
        "predictions": [
            {"embeddings": {"values": [1.0, 1.5], "statistics": {"token_count": 2}}},
            {"embeddings": {"values": [2.0], "statistics": {"token_count": 5}}},
            {"embeddings": {"values": [3.0]}}
        ]
    });

    let outputs = extract_google_embedding_outputs(&response, 250, 3, "gemini-embedding-001")
        .expect("outputs");

    assert_eq!(outputs.len(), 3);
    assert_eq!(
        outputs
            .iter()
            .map(|output| output.index)
            .collect::<Vec<_>>(),
        vec![250, 251, 252]
    );
    assert_eq!(outputs[0].embedding, json!([1.0, 1.5]));
    assert_eq!(outputs[0].token_count, Some(2));
    assert_eq!(outputs[1].embedding, json!([2.0]));
    assert_eq!(outputs[1].token_count, Some(5));
    assert_eq!(outputs[2].embedding, json!([3.0]));
    assert_eq!(outputs[2].token_count, None);
}

#[test]
fn rejects_predict_responses_with_missing_or_non_numeric_embeddings() {
    let cases = [
        ("no predictions", json!({}), "missing predictions[0]"),
        (
            "empty predictions",
            json!({"predictions": []}),
            "missing predictions[0]",
        ),
        (
            "missing values in later prediction",
            json!({"predictions": [
                {"embeddings": {"values": [1.0]}},
                {"embeddings": {}}
            ]}),
            "missing embeddings.values",
        ),
        (
            "non-numeric value",
            json!({"predictions": [{"embeddings": {"values": [1.0, "x"]}}]}),
            "embedding values must be numbers",
        ),
    ];
    for (name, response, expected) in cases {
        let expected_count = response
            .get("predictions")
            .and_then(Value::as_array)
            .map_or(1, Vec::len)
            .max(1);
        let error =
            extract_google_embedding_outputs(&response, 0, expected_count, "gemini-embedding-001")
                .expect_err(name);
        assert!(
            matches!(&error, ProviderError::Transport(message) if message.contains(expected)),
            "{name}: {error}"
        );
    }
}

#[test]
fn rejects_predict_responses_whose_prediction_count_differs_from_the_batch() {
    for count in [1usize, 3] {
        let predictions: Vec<Value> = (0..count)
            .map(|_| json!({"embeddings": {"values": [1.0]}}))
            .collect();
        let response = json!({"predictions": predictions});
        let error = extract_google_embedding_outputs(&response, 0, 2, "gemini-embedding-001")
            .expect_err("mismatched prediction count");
        assert!(
            matches!(&error, ProviderError::Transport(message) if message.contains("expected 2 predictions")),
            "{count}: {error}"
        );
    }
}

#[test]
fn gemini_embedding_001_sends_one_instance_per_predict_body() {
    let request = embedding_request(json!(["first", "second"]));

    let mapped = map_google_embedding_request(
        &request,
        &context("google/gemini-embedding-001"),
        "gemini-embedding-001",
    )
    .expect("mapped embeddings request");

    assert_eq!(mapped.batch_sizes, vec![1, 1]);
    assert_eq!(mapped.bodies[0]["instances"], json!([{"content": "first"}]));
    assert_eq!(
        mapped.bodies[1]["instances"],
        json!([{"content": "second"}])
    );
    assert_eq!(predict_max_instances("gemini-embedding-001"), 1);
    assert_eq!(
        predict_max_instances("text-embedding-005"),
        VERTEX_PREDICT_MAX_INSTANCES
    );
}

#[test]
fn token_estimate_is_an_upper_bound_of_one_token_per_byte() {
    // Dense ASCII can tokenize to one token per character, so that is the bound.
    assert_eq!(estimated_tokens("abcd"), 4);
    assert_eq!(estimated_tokens("Zm9vYmFy"), 8);
    // Multi-byte scripts are bounded by their UTF-8 length (byte fallback).
    assert_eq!(estimated_tokens("日本語"), 9);
    assert_eq!(estimated_tokens("ok 🎉"), 7);
    assert_eq!(estimated_tokens(""), 0);
}

#[test]
fn predict_batches_are_bounded_by_estimated_tokens() {
    // Each input is just over half the budget, so no two fit in one body.
    let long = "x".repeat(VERTEX_PREDICT_MAX_TOKENS / 2 + 1);
    let request = embedding_request(json!([long, long, long]));

    let mapped = map_google_embedding_request(
        &request,
        &context("google/text-embedding-005"),
        "text-embedding-005",
    )
    .expect("mapped embeddings request");

    assert_eq!(mapped.bodies.len(), 3);
    assert_eq!(mapped.batch_sizes, vec![1, 1, 1]);
    assert_eq!(mapped.input_count, 3);

    // The same character count in CJK weighs three bytes per character: three inputs of a
    // third of the budget fit together in ASCII but each need their own body in CJK.
    let ascii = "x".repeat(VERTEX_PREDICT_MAX_TOKENS / 3);
    let cjk = "字".repeat(VERTEX_PREDICT_MAX_TOKENS / 3);
    for (input, expected_batches) in [(ascii, vec![3]), (cjk, vec![1, 1, 1])] {
        let request = embedding_request(json!([input, input, input]));
        let mapped = map_google_embedding_request(
            &request,
            &context("google/text-embedding-005"),
            "text-embedding-005",
        )
        .expect("mapped embeddings request");
        assert_eq!(mapped.batch_sizes, expected_batches);
    }
}

#[test]
fn single_oversized_input_still_gets_its_own_body() {
    let request = embedding_request(json!(["x".repeat(VERTEX_PREDICT_MAX_TOKENS * 4)]));

    let mapped = map_google_embedding_request(
        &request,
        &context("google/text-embedding-005"),
        "text-embedding-005",
    )
    .expect("mapped embeddings request");

    assert_eq!(mapped.batch_sizes, vec![1]);
}

#[test]
fn extracts_embed_content_output_at_first_index() {
    let response = json!({
        "embedding": {"values": [0.5, 0.25]},
        "usageMetadata": {"promptTokenCount": 7, "totalTokenCount": 999}
    });

    let outputs =
        extract_google_embedding_outputs(&response, 3, 1, "gemini-embedding-2").expect("outputs");

    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].index, 3);
    assert_eq!(outputs[0].embedding, json!([0.5, 0.25]));
    assert_eq!(outputs[0].token_count, Some(7));
}

#[test]
fn rejects_unknown_embedding_model_dimension_limits_without_panicking() {
    let mut request = embedding_request(json!("document text"));
    request.extra.insert("dimensions".to_string(), json!(128));

    let error = map_google_embedding_request(
        &request,
        &context("google/future-embedding-model"),
        "future-embedding-model",
    )
    .expect_err("unknown model must return an error");

    assert!(matches!(
        error,
        ProviderError::InvalidRequest(message)
            if message.contains("unsupported vertex embeddings model")
    ));
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
    assert_eq!(response["usage"], embedding_usage(3));

    let request_payload = captured.lock().await.clone().expect("captured request");
    assert_eq!(
        request_payload,
        json!({
            "instances": [{"content": "hello embeddings"}]
        })
    );
}

#[tokio::test]
async fn vertex_provider_google_embedding_array_batches_one_predict_body_and_preserves_order() {
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
                        let instances = payload["instances"]
                            .as_array()
                            .expect("instances")
                            .clone();
                        captured.lock().await.push(payload);
                        let predictions: Vec<Value> = instances
                            .iter()
                            .map(|instance| match instance["content"].as_str() {
                                Some("first") => json!({
                                    "embeddings": {
                                        "values": [1.0, 1.5],
                                        "statistics": {"token_count": 2, "billable_character_count": 999}
                                    }
                                }),
                                Some("second") => json!({
                                    "embeddings": {
                                        "values": [2.0, 2.5],
                                        "statistics": {"token_count": 5, "billable_character_count": 999}
                                    }
                                }),
                                other => panic!("unexpected embedding content: {other:?}"),
                            })
                            .collect();
                        Json(json!({"predictions": predictions}))
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
    assert_eq!(response["usage"], embedding_usage(7));

    let request_payloads = captured.lock().await.clone();
    assert_eq!(request_payloads.len(), 1);
    assert_eq!(
        request_payloads[0]["instances"],
        json!([{"content": "first"}, {"content": "second"}])
    );
}

#[tokio::test]
async fn vertex_provider_google_embedding_predict_splits_batches_and_reports_partial_usage() {
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let state = captured.clone();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |State(captured): State<Arc<Mutex<Vec<Value>>>>,
                 Json(payload): Json<Value>| async move {
                    let instances = payload["instances"]
                        .as_array()
                        .expect("instances")
                        .clone();
                    captured.lock().await.push(payload);
                    if instances.len() == VERTEX_PREDICT_MAX_INSTANCES {
                        let predictions: Vec<Value> = instances
                            .iter()
                            .map(|_| {
                                json!({
                                    "embeddings": {
                                        "values": [0.5],
                                        "statistics": {"token_count": 2}
                                    }
                                })
                            })
                            .collect();
                        (StatusCode::OK, Json(json!({"predictions": predictions})))
                    } else {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({"error": {"message": "quota exhausted"}})),
                        )
                    }
                },
            ),
        )
        .with_state(state);

    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let request = embedding_request(inputs(VERTEX_PREDICT_MAX_INSTANCES + 1));

    let error = provider
        .embeddings(&request, &context("google/text-embedding-005"))
        .await
        .expect_err("second batch failure should surface partial usage");

    match error {
        ProviderError::PartialUsage {
            source,
            provider_usage,
        } => {
            assert_eq!(
                provider_usage,
                Some(embedding_usage(2 * VERTEX_PREDICT_MAX_INSTANCES as i64))
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
    assert_eq!(
        request_payloads[0]["instances"]
            .as_array()
            .expect("first batch")
            .len(),
        VERTEX_PREDICT_MAX_INSTANCES
    );
    assert_eq!(
        request_payloads[1]["instances"],
        json!([{"content": format!("text-{VERTEX_PREDICT_MAX_INSTANCES}")}])
    );
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
    assert_eq!(response["usage"], embedding_usage(7));

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
            assert_eq!(provider_usage, Some(embedding_usage(4)));
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

fn embedding_usage(total_tokens: i64) -> Value {
    json!({
        "prompt_tokens": total_tokens,
        "total_tokens": total_tokens,
        "usage_source": "vertex_google_embeddings",
        "provider_usage": {
            "input_token_count_provenance": "provider_reported_aggregate"
        }
    })
}
