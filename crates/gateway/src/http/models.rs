use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use gateway_service::{AdminModelSummary, AdminModelsService};

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{AdminModelListQuery, AdminModelPageView, AdminModelView, Envelope, envelope},
    error::AppError,
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 30;
const MAX_PAGE_SIZE: u32 = 100;

#[utoipa::path(
    get,
    path = "/api/v1/admin/models",
    params(AdminModelListQuery),
    responses((status = 200, body = Envelope<AdminModelPageView>)),
    security(("session_cookie" = []))
)]
pub async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminModelListQuery>,
) -> Result<Json<Envelope<AdminModelPageView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    let service = AdminModelsService::new(state.store.clone());
    let models = service.list_models().await?;
    let total = models.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let items = models
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(map_model_summary)
        .collect();

    Ok(Json(envelope(AdminModelPageView {
        items,
        page,
        page_size,
        total,
    })))
}

fn map_model_summary(model: AdminModelSummary) -> AdminModelView {
    AdminModelView {
        id: model.id,
        resolved_model_key: model.resolved_model_key,
        alias_of: model.alias_of,
        description: model.description,
        tags: model.tags,
        status: model.status.into(),
        provider_key: model.provider_key,
        provider_label: model.provider_label,
        provider_icon_key: model.provider_icon_key.map(Into::into),
        upstream_model: model.upstream_model,
        model_icon_key: model.model_icon_key.map(Into::into),
        input_cost_per_million_tokens_usd_10000: model.input_cost_per_million_tokens_usd_10000,
        output_cost_per_million_tokens_usd_10000: model.output_cost_per_million_tokens_usd_10000,
        context_window_tokens: model.context_window_tokens,
        input_window_tokens: model.input_window_tokens,
        output_window_tokens: model.output_window_tokens,
        supports_streaming: model.supports_streaming,
        supports_vision: model.supports_vision,
        supports_tool_calling: model.supports_tool_calling,
        supports_structured_output: model.supports_structured_output,
        supports_attachments: model.supports_attachments,
    }
}
