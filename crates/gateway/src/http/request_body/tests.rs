use axum::{Router, body::to_bytes, extract::DefaultBodyLimit, middleware, routing::post};
use serde_json::json;
use tower::ServiceExt;

use super::*;

fn router() -> Router {
    Router::new()
        .route(
            "/",
            post(|Json(_): Json<serde_json::Value>| async { StatusCode::NO_CONTENT }),
        )
        .layer(DefaultBodyLimit::max(DEFAULT_MAX_BYTES))
        .layer(middleware::from_fn(observe_request_body))
}

#[tokio::test]
async fn accepts_json_at_64_mib_without_a_content_length() {
    let body = format!("\"{}\"", "a".repeat(DEFAULT_MAX_BYTES - 2));
    let response = router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn oversized_chunked_json_returns_limit_and_request_id() {
    let chunks = futures_util::stream::iter([
        Ok::<_, std::io::Error>(Bytes::from(vec![b' '; DEFAULT_MAX_BYTES])),
        Ok(Bytes::from_static(b"{}")),
    ]);
    let response = router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .header("x-request-id", "body-test")
                .body(Body::from_stream(chunks))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    let canonical = OpenAiErrorEnvelope::from_gateway_error(&GatewayError::PayloadTooLarge {
        limit_bytes: DEFAULT_MAX_BYTES,
    });
    assert_eq!(value["error"]["code"], canonical.error.code.unwrap());
    assert_eq!(value["error"]["type"], canonical.error.error_type);
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with(&canonical.error.message)
    );
    assert_eq!(value["error"]["limit_bytes"], DEFAULT_MAX_BYTES);
    assert_eq!(value["error"]["received_bytes"], DEFAULT_MAX_BYTES + 2);
    assert_eq!(value["error"]["request_id"], "body-test");
}

#[tokio::test]
async fn upstream_413_is_not_replaced() {
    let router = Router::new()
        .route(
            "/",
            post(|| async { (StatusCode::PAYLOAD_TOO_LARGE, "provider limit") }),
        )
        .layer(middleware::from_fn(observe_request_body));
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "provider limit"
    );
}

#[tokio::test]
async fn responses_route_accepts_body_above_old_limit_before_authentication() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let router = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    let body = serde_json::to_vec(&json!({"model": "test", "input": "a".repeat(3 * 1024 * 1024)}))
        .unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
