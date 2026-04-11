use axum::{Json, extract::State, http::HeaderMap};
use gateway_service::{AdminModelSummary, AdminModelsService};

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{AdminModelView, Envelope, envelope},
    error::AppError,
    state::AppState,
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/models",
    responses((status = 200, body = Envelope<Vec<AdminModelView>>)),
    security(("session_cookie" = []))
)]
pub async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<Vec<AdminModelView>>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let service = AdminModelsService::new(state.store.clone());
    let models = service.list_models().await?;
    Ok(Json(envelope(
        models.into_iter().map(map_model_summary).collect(),
    )))
}

fn map_model_summary(model: AdminModelSummary) -> AdminModelView {
    AdminModelView {
        id: model.id,
        resolved_model_key: model.resolved_model_key,
        alias_of: model.alias_of,
        description: model.description,
        tags: model.tags,
        status: model.status.as_str().to_string(),
        provider_key: model.provider_key,
        provider_label: model.provider_label,
        provider_icon_key: model
            .provider_icon_key
            .map(|value| value.as_str().to_string()),
        upstream_model: model.upstream_model,
        model_icon_key: model.model_icon_key.map(|value| value.as_str().to_string()),
    }
}
