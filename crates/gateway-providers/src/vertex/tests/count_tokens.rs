use super::*;

type Captured = Arc<Mutex<Option<(String, Value)>>>;

fn user(text: &str) -> CoreChatMessage {
    CoreChatMessage {
        role: "user".to_string(),
        content: Value::String(text.to_string()),
        name: None,
        extra: BTreeMap::new(),
    }
}

#[tokio::test]
async fn claude_count_tokens_posts_unversioned_model_to_shared_endpoint() {
    let captured: Captured = Arc::default();
    let app = Router::new()
        .route(
            "/v1/{*path}",
            post(
                |Path(path): Path<String>,
                 State(captured): State<Captured>,
                 Json(payload): Json<Value>| async move {
                    *captured.lock().await = Some((path, payload));
                    Json(json!({"input_tokens": 17}))
                },
            ),
        )
        .with_state(captured.clone());
    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let mut request = chat_request(vec![user("ping")]);
    request.extra.insert("max_tokens".to_string(), json!(64));
    request.extra.insert("temperature".to_string(), json!(0.2));

    let value = provider
        .count_tokens(&request, &context("anthropic/claude-sonnet-4-5@20250929"))
        .await
        .expect("count");

    assert_eq!(value, json!({"input_tokens": 17}));
    let (path, body) = captured.lock().await.take().expect("upstream call");
    assert_eq!(
        path,
        "projects/proj-123/locations/global/publishers/anthropic/models/count-tokens:rawPredict"
    );
    assert_eq!(body["model"], "claude-sonnet-4-5");
    assert_eq!(body["anthropic_version"], "vertex-2023-10-16");
    assert_eq!(body["messages"][0]["role"], "user");
    for dropped in ["max_tokens", "stream", "temperature"] {
        assert!(body.get(dropped).is_none(), "{dropped} in {body}");
    }
}

#[tokio::test]
async fn gemini_count_tokens_is_not_implemented() {
    let provider = vertex_provider_for_test("http://127.0.0.1:9".to_string());

    let error = provider
        .count_tokens(
            &chat_request(vec![user("ping")]),
            &context("google/gemini-3.7-flash"),
        )
        .await
        .expect_err("gemini has no Messages count");

    assert!(
        matches!(&error, ProviderError::NotImplemented(message) if message.contains("google")),
        "{error:?}"
    );
}
