use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

use crate::http::test_support::{app_state, seed_stream_cancellation_test};

const KEY: &str = "gwk_streamtest.cancel-secret";
const UNRELATED_OAUTH: &str = "Bearer sk-ant-oat01-unrelated-upstream-token";

/// One request per route family that accepts a gateway API key.
fn route_families() -> Vec<(Method, &'static str, serde_json::Value)> {
    let chat = json!({"model": "fast", "messages": [{"role": "user", "content": "hi"}]});
    vec![
        (Method::GET, "/v1/models", json!(null)),
        (Method::GET, "/v1/model-metadata", json!(null)),
        (Method::POST, "/v1/chat/completions", chat.clone()),
        (
            Method::POST,
            "/v1/messages",
            json!({"model": "fast", "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}),
        ),
        (
            Method::POST,
            "/mcp",
            json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        ),
        (Method::GET, "/api/v1/batches", json!(null)),
        (Method::GET, "/api/v1/skills", json!(null)),
    ]
}

async fn status(
    state: &crate::http::state::AppState,
    method: &Method,
    path: &str,
    body: &serde_json::Value,
    headers: &[(&str, &str)],
) -> StatusCode {
    send(state, method, path, body, headers).await.status()
}

async fn send(
    state: &crate::http::state::AppState,
    method: &Method,
    path: &str,
    body: &serde_json::Value,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    crate::http::build_router(state.clone(), admin_ui::AdminUiConfig::default())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn error_message(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn litellm_api_key_authenticates_every_route_family() {
    let (_directory, state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    let bearer = format!("Bearer {KEY}");
    for (method, path, body) in route_families() {
        assert_eq!(
            status(&state, &method, path, &body, &[]).await,
            StatusCode::UNAUTHORIZED,
            "{path} must reject anonymous callers"
        );
        assert_eq!(
            status(
                &state,
                &method,
                path,
                &body,
                &[("x-litellm-api-key", "gwk_streamtest.wrong")]
            )
            .await,
            StatusCode::UNAUTHORIZED,
            "{path} must verify the alias credential"
        );
        for headers in [
            vec![("x-litellm-api-key", KEY)],
            vec![("x-litellm-api-key", bearer.as_str())],
            // Claude Code may send an unrelated OAuth token in `Authorization`.
            vec![
                ("x-litellm-api-key", KEY),
                ("authorization", UNRELATED_OAUTH),
            ],
            vec![
                ("x-litellm-api-key", KEY),
                ("x-api-key", "sk-ant-unrelated"),
            ],
        ] {
            let status = status(&state, &method, path, &body, &headers).await;
            assert_ne!(
                status,
                StatusCode::UNAUTHORIZED,
                "{path} with {headers:?} must authenticate"
            );
        }
    }
}

#[tokio::test]
async fn litellm_api_key_conflicts_with_a_different_oceans_api_key() {
    let (_directory, state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    for (method, path, body) in [
        (
            Method::POST,
            "/mcp",
            json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        ),
        (Method::GET, "/api/v1/batches", json!(null)),
        (Method::GET, "/api/v1/skills", json!(null)),
    ] {
        let headers = [
            ("x-litellm-api-key", KEY),
            ("x-oceans-api-key", "gwk_streamtest.other"),
        ];
        assert_eq!(
            status(&state, &method, path, &body, &headers).await,
            StatusCode::UNAUTHORIZED,
            "{path} must reject conflicting key headers"
        );
        let headers = [("x-litellm-api-key", KEY), ("x-oceans-api-key", KEY)];
        assert_ne!(
            status(&state, &method, path, &body, &headers).await,
            StatusCode::UNAUTHORIZED,
            "{path} accepts matching key headers"
        );
    }
}

#[tokio::test]
async fn litellm_tags_are_validated_like_oceans_tags_on_inference_routes() {
    let (_directory, state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    let chat = json!({"model": "fast", "messages": [{"role": "user", "content": "hi"}]});
    let path = "/v1/chat/completions";
    for (header, value) in [("x-oceans-tags", "Bad=value"), ("x-litellm-tags", "Bad")] {
        let response = send(
            &state,
            &Method::POST,
            path,
            &chat,
            &[("x-litellm-api-key", KEY), (header, value)],
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{header}");
        let message = error_message(response).await;
        // Alias tags fail with the same validation message as `x-oceans-tags`.
        assert!(
            message.ends_with(&format!(
                "header `{header}` key `Bad` must start with a lowercase ASCII letter"
            )),
            "{header}: {message}"
        );
    }
    let response = send(
        &state,
        &Method::POST,
        path,
        &chat,
        &[
            ("x-litellm-api-key", KEY),
            ("x-oceans-tags", "team=core"),
            ("x-litellm-tags", "team:payments"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(error_message(response).await.contains("different values"));
}
