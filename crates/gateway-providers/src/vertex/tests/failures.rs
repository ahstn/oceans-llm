use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::token::{AccessToken, AccessTokenSource, CachedAccessTokenSource};

fn user_request() -> CoreChatRequest {
    chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!("ping"),
        name: None,
        extra: BTreeMap::new(),
    }])
}

#[tokio::test]
async fn vertex_inference_http_errors_preserve_retry_after() {
    let app = Router::new().route(
        "/v1/{*path}",
        post(|| async {
            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("retry-after", "17")
                .body(Body::from("quota exhausted"))
                .expect("error response")
        }),
    );
    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let request = user_request();
    let chat_context = context("google/gemini-3.7-flash");
    let results = [
        provider
            .chat_completions(&request, &chat_context)
            .await
            .map(|_| ()),
        provider
            .chat_completions_stream(&request, &chat_context)
            .await
            .map(|_| ()),
        provider
            .embeddings(
                &embedding_request(json!("hello")),
                &context("google/gemini-embedding-001"),
            )
            .await
            .map(|_| ()),
    ];

    for result in results {
        let error = result.expect_err("quota error");
        assert!(error.is_retryable());
        match error {
            ProviderError::UpstreamHttp {
                status,
                body,
                retry_after,
            } => {
                assert_eq!(status, 429);
                assert_eq!(body, "quota exhausted");
                assert_eq!(retry_after, Some(std::time::Duration::from_secs(17)));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}

#[tokio::test]
async fn vertex_successful_chat_with_invalid_json_is_not_retryable() {
    let host =
        start_router(Router::new().route("/v1/{*path}", post(|| async { "invalid JSON" }))).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    for model in ["google/gemini-3.7-flash", "anthropic/claude-sonnet-4-6"] {
        let error = provider
            .chat_completions(&user_request(), &context(model))
            .await
            .expect_err("invalid upstream JSON");
        assert!(!error.is_retryable());
        assert!(matches!(
            error,
            ProviderError::PartialUsage {
                provider_usage: None,
                ..
            }
        ));
    }
}

#[tokio::test]
async fn vertex_google_normalization_failure_preserves_reported_usage() {
    let upstream = json!({
        "candidates": [{"finishReason": "MALFORMED_FUNCTION_CALL"}],
        "usageMetadata": {
            "promptTokenCount": 3,
            "candidatesTokenCount": 2,
            "totalTokenCount": 5
        }
    });
    let expected_usage = map_google_usage(&upstream);
    let host = start_router(Router::new().route(
        "/v1/{*path}",
        post(move || {
            let upstream = upstream.clone();
            async move { Json(upstream) }
        }),
    ))
    .await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let error = provider
        .chat_completions(&user_request(), &context("google/gemini-3.7-flash"))
        .await
        .expect_err("malformed upstream function call");

    assert!(!error.is_retryable());
    match error {
        ProviderError::PartialUsage {
            source,
            provider_usage,
        } => {
            assert_eq!(provider_usage, expected_usage);
            assert!(matches!(*source, ProviderError::Transport(_)));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn vertex_embedding_later_request_build_failure_preserves_usage() {
    struct FailingRefreshTokenSource(AtomicUsize);

    #[async_trait::async_trait]
    impl AccessTokenSource for FailingRefreshTokenSource {
        async fn fetch_token(&self) -> Result<AccessToken, ProviderError> {
            if self.0.fetch_add(1, Ordering::SeqCst) > 0 {
                return Err(ProviderError::Timeout);
            }
            Ok(AccessToken {
                token: "test-token".to_string(),
                expires_at: time::OffsetDateTime::now_utc(),
            })
        }
    }

    let requests_seen = Arc::new(AtomicUsize::new(0));
    let captured = requests_seen.clone();
    let host = start_router(Router::new().route(
        "/v1/{*path}",
        post(move || {
            captured.fetch_add(1, Ordering::SeqCst);
            async {
                Json(json!({
                    "embedding": {"values": [1.0]},
                    "usageMetadata": {"promptTokenCount": 4}
                }))
            }
        }),
    ))
    .await;
    let mut provider = vertex_provider_for_test(format!("http://{host}"));
    provider.access_token_source =
        CachedAccessTokenSource::new(Arc::new(FailingRefreshTokenSource(AtomicUsize::new(0))));
    let error = provider
        .embeddings(
            &embedding_request(json!(["first", "second"])),
            &context("google/gemini-embedding-2"),
        )
        .await
        .expect_err("token refresh after first embedding");

    assert!(!error.is_retryable());
    assert_eq!(requests_seen.load(Ordering::SeqCst), 1);
    match error {
        ProviderError::PartialUsage {
            source,
            provider_usage,
        } => {
            assert!(matches!(*source, ProviderError::Timeout));
            let usage = provider_usage.expect("first embedding usage");
            assert_eq!(usage["prompt_tokens"], 4);
            assert_eq!(usage["total_tokens"], 4);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn vertex_embedding_usage_overflow_is_not_retryable() {
    let host = start_router(Router::new().route(
        "/v1/{*path}",
        post(|| async {
            Json(json!({
                "predictions": [
                    {"embeddings": {
                        "values": [1.0],
                        "statistics": {"token_count": i64::MAX}
                    }},
                    {"embeddings": {
                        "values": [2.0],
                        "statistics": {"token_count": 1}
                    }}
                ]
            }))
        }),
    ))
    .await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let error = provider
        .embeddings(
            &embedding_request(json!(["first", "second"])),
            &context("google/text-embedding-005"),
        )
        .await
        .expect_err("upstream token count overflow");

    assert!(!error.is_retryable());
    assert!(matches!(
        error,
        ProviderError::PartialUsage {
            provider_usage: None,
            source,
        } if matches!(*source, ProviderError::Transport(ref message) if message.contains("overflow"))
    ));
}

#[tokio::test]
async fn vertex_embedding_malformed_success_keeps_unknown_usage() {
    for body in ["invalid JSON", r#"{"predictions": []}"#] {
        let host =
            start_router(Router::new().route("/v1/{*path}", post(move || async move { body })))
                .await;
        let provider = vertex_provider_for_test(format!("http://{host}"));
        let error = provider
            .embeddings(
                &embedding_request(json!("first")),
                &context("google/gemini-embedding-001"),
            )
            .await
            .expect_err("malformed successful response");

        assert!(!error.is_retryable());
        assert!(matches!(
            error,
            ProviderError::PartialUsage {
                provider_usage: None,
                ..
            }
        ));
    }
}

#[tokio::test]
async fn vertex_embedding_unreadable_error_preserves_status_and_prior_usage() {
    for prior_success in [false, true] {
        let host = start_router(Router::new().route(
            "/v1/{*path}",
            post(|Json(payload): Json<Value>| async move {
                if payload["content"]["parts"][0]["text"] == "first" {
                    return Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "embedding": {"values": [1.0]},
                                "usageMetadata": {"promptTokenCount": 4}
                            })
                            .to_string(),
                        ))
                        .expect("first embedding response");
                }
                let chunks = stream::once(async { Ok::<_, Infallible>(Bytes::from_static(b"{")) })
                    .chain(stream::pending());
                Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("retry-after", "17")
                    .body(Body::from_stream(chunks))
                    .expect("unreadable error response")
            }),
        ))
        .await;
        let mut provider = vertex_provider_for_test(format!("http://{host}"));
        provider.client = crate::http::provider_http_client_without_redirects(250)
            .expect("client with short timeout");
        let input = if prior_success {
            json!(["first", "second"])
        } else {
            json!("second")
        };
        let error = provider
            .embeddings(
                &embedding_request(input),
                &context("google/gemini-embedding-2"),
            )
            .await
            .expect_err("unreadable HTTP error");

        assert!(!error.is_retryable());
        let source = if prior_success {
            match error {
                ProviderError::PartialUsage {
                    source,
                    provider_usage,
                } => {
                    assert_eq!(provider_usage.expect("prior usage")["total_tokens"], 4);
                    *source
                }
                other => panic!("expected prior usage, got: {other}"),
            }
        } else {
            error
        };
        assert!(matches!(
            source,
            ProviderError::UpstreamHttp {
                status: 403,
                retry_after: Some(delay),
                ..
            } if delay == std::time::Duration::from_secs(17)
        ));
    }
}
