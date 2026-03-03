use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header::AUTHORIZATION},
};
use gateway_core::{
    ChatCompletionsRequest, EmbeddingsRequest, GatewayError, ModelsListResponse,
    protocol::openai::ModelCard,
};
use serde_json::json;

use crate::http::{error::AppError, state::AppState};

pub async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn readyz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.service.check_readiness().await?;
    Ok(Json(json!({ "status": "ready" })))
}

pub async fn api_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "gateway" }))
}

pub async fn v1_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModelsListResponse>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;

    let models = state.service.list_models_for_api_key(&auth).await?;
    let data = models
        .into_iter()
        .map(|model| ModelCard {
            id: model.model_key,
            object: "model".to_string(),
            created: 0,
            owned_by: "gateway".to_string(),
        })
        .collect::<Vec<_>>();

    Ok(Json(ModelsListResponse {
        object: "list".to_string(),
        data,
    }))
}

pub async fn v1_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let resolved = state.service.resolve_request(&auth, &request.model).await?;

    let has_adapter = resolved
        .routes
        .first()
        .and_then(|route| state.providers.get(&route.provider_key))
        .is_some();

    tracing::info!(
        request_model = %request.model,
        resolved_model = %resolved.model.model_key,
        route_count = resolved.routes.len(),
        provider_adapter_available = has_adapter,
        "chat completion request resolved"
    );

    Err(AppError(GatewayError::NotImplemented(
        "chat completion execution is intentionally deferred in this foundation phase".to_string(),
    )))
}

pub async fn v1_embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let resolved = state.service.resolve_request(&auth, &request.model).await?;

    let has_adapter = resolved
        .routes
        .first()
        .and_then(|route| state.providers.get(&route.provider_key))
        .is_some();

    tracing::info!(
        request_model = %request.model,
        resolved_model = %resolved.model.model_key,
        route_count = resolved.routes.len(),
        provider_adapter_available = has_adapter,
        "embeddings request resolved"
    );

    Err(AppError(GatewayError::NotImplemented(
        "embeddings execution is intentionally deferred in this foundation phase".to_string(),
    )))
}

fn extract_authorization_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
}
