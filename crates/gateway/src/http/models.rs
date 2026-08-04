use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use gateway_core::GlobalRole;
use gateway_service::{
    AdminModelSummary, AdminModelsService, EffectiveMetadataSource, EffectiveMetadataSourceKind,
};

use crate::http::{
    admin_auth::{require_active_session, require_platform_admin},
    admin_contract::{
        AdminModelAllowlistView, AdminModelClientConfigBlockView,
        AdminModelClientConfigSetupItemView, AdminModelClientConfigView, AdminModelListQuery,
        AdminModelPageView, AdminModelView, EffectiveMetadataSourceKindView,
        EffectiveMetadataSourceView, Envelope, GenerateModelClientConfigsRequest,
        GenerateModelClientConfigsResponse, RefreshModelPricingCatalogResponse, envelope,
        format_timestamp,
    },
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
    let actor = require_active_session(&state, &headers).await?;
    let include_allowlist = actor.global_role == GlobalRole::PlatformAdmin;

    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    let service = admin_models_service(&state);
    let models = service.list_models().await?;
    let total = models.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let items = models
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(|model| map_model_summary(model, include_allowlist))
        .collect();

    Ok(Json(envelope(AdminModelPageView {
        items,
        page,
        page_size,
        total,
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/models/client-configs",
    request_body = GenerateModelClientConfigsRequest,
    responses((status = 200, body = Envelope<GenerateModelClientConfigsResponse>)),
    security(("session_cookie" = []))
)]
pub async fn generate_model_client_configs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GenerateModelClientConfigsRequest>,
) -> Result<Json<Envelope<GenerateModelClientConfigsResponse>>, AppError> {
    require_active_session(&state, &headers).await?;

    let service = admin_models_service(&state);
    let client_configurations = service
        .render_client_configurations(&request.model_keys)
        .await?
        .into_iter()
        .map(|config| AdminModelClientConfigView {
            key: config.key,
            label: config.label,
            model_ids: config.model_ids,
            setup: config
                .setup
                .into_iter()
                .map(|item| AdminModelClientConfigSetupItemView {
                    label: item.label,
                    value: item.value,
                    href: item.href,
                })
                .collect(),
            blocks: config
                .blocks
                .into_iter()
                .map(|block| AdminModelClientConfigBlockView {
                    label: block.label,
                    filename: block.filename,
                    content: block.content,
                })
                .collect(),
            notes: config.notes,
        })
        .collect();

    Ok(Json(envelope(GenerateModelClientConfigsResponse {
        client_configurations,
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/models/pricing-catalog/refresh",
    responses((status = 200, body = Envelope<RefreshModelPricingCatalogResponse>)),
    security(("session_cookie" = []))
)]
pub async fn refresh_model_pricing_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<RefreshModelPricingCatalogResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    state.service.refresh_pricing_catalog_now().await?;

    Ok(Json(envelope(RefreshModelPricingCatalogResponse {
        refreshed: true,
    })))
}

fn admin_models_service(state: &AppState) -> AdminModelsService<gateway_store::AnyStore> {
    let service = AdminModelsService::new(state.store.clone());
    match state.client_config_gateway_base_url.as_ref().as_deref() {
        Some(gateway_base_url) => {
            service.with_client_config_gateway_base_url(gateway_base_url.to_string())
        }
        None => service,
    }
}

fn map_model_summary(model: AdminModelSummary, include_allowlist: bool) -> AdminModelView {
    let allowlist = if include_allowlist {
        model.allowlist.map(|policy| AdminModelAllowlistView {
            users: policy.users,
            teams: policy.teams,
        })
    } else {
        None
    };

    AdminModelView {
        id: model.id,
        model_id: model.model_id,
        resolved_model_key: model.resolved_model_key,
        alias_of: model.alias_of,
        description: model.description,
        tags: model.tags,
        allowlist,
        status: model.status.into(),
        provider_key: model.provider_key,
        provider_label: model.provider_label,
        provider_icon_key: model.provider_icon_key.map(Into::into),
        upstream_model: model.upstream_model,
        model_icon_key: model.model_icon_key.map(Into::into),
        input_cost_per_million_tokens_usd_10000: model.input_cost_per_million_tokens_usd_10000,
        output_cost_per_million_tokens_usd_10000: model.output_cost_per_million_tokens_usd_10000,
        cache_read_cost_per_million_tokens_usd_10000: model
            .cache_read_cost_per_million_tokens_usd_10000,
        cache_write_cost_per_million_tokens_usd_10000: model
            .cache_write_cost_per_million_tokens_usd_10000,
        pricing_source: model.pricing_source.map(map_metadata_source),
        pricing_varies_by_route: model.pricing_varies_by_route,
        context_window_tokens: model.context_window_tokens,
        context_window_source: model.context_window_source.map(map_metadata_source),
        input_window_tokens: model.input_window_tokens,
        output_window_tokens: model.output_window_tokens,
        supports_streaming: model.supports_streaming,
        supports_vision: model.supports_vision,
        supports_tool_calling: model.supports_tool_calling,
        supports_structured_output: model.supports_structured_output,
        supports_attachments: model.supports_attachments,
        client_configurations: model
            .client_configurations
            .into_iter()
            .map(|config| AdminModelClientConfigView {
                key: config.key,
                label: config.label,
                model_ids: config.model_ids,
                setup: config
                    .setup
                    .into_iter()
                    .map(|item| AdminModelClientConfigSetupItemView {
                        label: item.label,
                        value: item.value,
                        href: item.href,
                    })
                    .collect(),
                blocks: config
                    .blocks
                    .into_iter()
                    .map(|block| AdminModelClientConfigBlockView {
                        label: block.label,
                        filename: block.filename,
                        content: block.content,
                    })
                    .collect(),
                notes: config.notes,
            })
            .collect(),
    }
}

fn map_metadata_source(source: EffectiveMetadataSource) -> EffectiveMetadataSourceView {
    EffectiveMetadataSourceView {
        kind: match source.kind {
            EffectiveMetadataSourceKind::ConfiguredOverride => {
                EffectiveMetadataSourceKindView::ConfiguredOverride
            }
            EffectiveMetadataSourceKind::Catalog => EffectiveMetadataSourceKindView::Catalog,
            EffectiveMetadataSourceKind::Mixed => EffectiveMetadataSourceKindView::Mixed,
        },
        catalog_source: source.catalog_source,
        catalog_etag: source.catalog_etag,
        catalog_fetched_at: source.catalog_fetched_at.map(format_timestamp),
    }
}
