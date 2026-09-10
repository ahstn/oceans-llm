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
async fn body_limit_error_preserves_endpoint_protocol() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    seed_key(&state).await;
    let app = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    let body = Bytes::from(vec![b' '; DEFAULT_MAX_BYTES + 1]);
    for path in ["/v1/responses", "/v1/messages", "/messages"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("x-request-id", "protocol-test")
                    .header(
                        if path.ends_with("messages") {
                            "x-api-key"
                        } else {
                            "authorization"
                        },
                        if path.ends_with("messages") {
                            "gwk_bodytest.test-secret"
                        } else {
                            "Bearer gwk_bodytest.test-secret"
                        },
                    )
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response.headers()["x-request-id"], "protocol-test");
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        if path == "/v1/responses" {
            assert!(value.get("type").is_none());
        } else {
            assert_eq!(value["type"], "error", "{path}");
        }
        assert_eq!(value["error"]["request_id"], "protocol-test");
        assert_eq!(value["error"]["limit_bytes"], DEFAULT_MAX_BYTES);
    }
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
    assert_oversized(false).await;
}

#[tokio::test]
async fn oversized_declared_json_returns_limit_and_request_id() {
    assert_oversized(true).await;
}

async fn assert_oversized(declared: bool) {
    let chunks = futures_util::stream::iter([
        Ok::<_, std::io::Error>(Bytes::from(vec![b' '; DEFAULT_MAX_BYTES])),
        Ok(Bytes::from_static(b"{}")),
    ]);
    let mut request = Request::builder().method("POST").uri("/");
    if declared {
        request = request.header("content-length", DEFAULT_MAX_BYTES + 2);
    }
    let response = router()
        .oneshot(
            request
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
async fn unauthenticated_inference_never_polls_body() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    let app = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    for path in [
        "/v1/responses",
        "/v1/chat/completions",
        "/v1/embeddings",
        "/v1/messages",
        "/messages",
    ] {
        for authorization in [None, Some("Bearer invalid")] {
            let body =
                futures_util::stream::poll_fn(|_| -> Poll<Option<Result<Bytes, std::io::Error>>> {
                    panic!("unauthenticated body was polled")
                });
            let mut request = Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json");
            if let Some(value) = authorization {
                request = request.header("authorization", value);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::from_stream(body)).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            if path.ends_with("messages") {
                let value: serde_json::Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap())
                        .unwrap();
                assert_eq!(value["type"], "error");
            }
        }
    }
}

#[tokio::test]
async fn authenticated_responses_accepts_body_above_old_limit() {
    let (_directory, state) = crate::http::test_support::app_state().await;
    seed_key(&state).await;
    let app = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    let body =
        serde_json::to_vec(&json!({"model": "missing", "input": "a".repeat(3 * 1024 * 1024)}))
            .unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("authorization", "Bearer gwk_bodytest.test-secret")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

async fn seed_key(state: &crate::http::state::AppState) {
    use gateway_core::{
        AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, GlobalRole,
        NewApiKeyRecord, UserStatus,
    };
    use gateway_store::GatewayStore;
    let user = state
        .store
        .create_identity_user(
            "Body test",
            "body@example.test",
            "body@example.test",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .unwrap();
    state
        .store
        .create_api_key(&NewApiKeyRecord {
            name: "Body test".into(),
            public_id: "bodytest".into(),
            secret_hash: gateway_core::hash_gateway_key_secret("test-secret").unwrap(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            created_at: time::OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
}
