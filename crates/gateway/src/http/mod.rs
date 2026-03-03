pub mod error;
pub mod handlers;
pub mod state;

use admin_ui::{AdminUiConfig, mount_admin_ui};
use axum::{
    Router,
    routing::{get, post},
};
use http::HeaderName;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use self::{handlers::*, state::AppState};

pub fn build_router(state: AppState, admin_ui: AdminUiConfig) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");

    let api_router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/v1/health", get(api_health))
        .route("/v1/models", get(v1_models))
        .route("/v1/chat/completions", post(v1_chat_completions))
        .route("/v1/embeddings", post(v1_embeddings))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing");

                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id
                )
            }),
        )
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

    mount_admin_ui(api_router, admin_ui)
}
