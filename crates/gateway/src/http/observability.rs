use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, RequestLogDetail, RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord,
    RequestTag, RequestTags,
};
use gateway_service::{
    model_icon_key_from_metadata, provider_icon_key_from_metadata, resolve_model_icon_key,
    resolve_provider_display,
};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{
        Envelope, OpenAiErrorEnvelopeView, RequestLogDetailView, RequestLogListQuery,
        RequestLogPageView, RequestLogPayloadView, RequestLogSummaryView, RequestTagView,
        RequestTagsView, envelope, format_timestamp,
    },
    error::AppError,
    request_tags::build_bespoke_tag_filter,
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 100;
const MAX_PAGE_SIZE: u32 = 500;

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/request-logs",
    params(RequestLogListQuery),
    responses((status = 200, body = Envelope<RequestLogPageView>)),
    security(("session_cookie" = []))
)]
pub async fn list_request_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RequestLogListQuery>,
) -> Result<Json<Envelope<RequestLogPageView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let request_log_query = RequestLogQuery {
        page: query.page.unwrap_or(DEFAULT_PAGE).max(1),
        page_size: query
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
        request_id: empty_to_none(query.request_id),
        model_key: empty_to_none(query.model_key),
        provider_key: empty_to_none(query.provider_key),
        status_code: query.status_code,
        user_id: parse_optional_uuid(query.user_id.as_deref(), "user_id")?,
        team_id: parse_optional_uuid(query.team_id.as_deref(), "team_id")?,
        service: empty_to_none(query.service),
        component: empty_to_none(query.component),
        env: empty_to_none(query.env),
        tag_key: None,
        tag_value: None,
    };
    let (tag_key, tag_value) = parse_optional_tag_filter(query.tag_key, query.tag_value)?;
    let query = RequestLogQuery {
        tag_key,
        tag_value,
        ..request_log_query
    };

    let page = state.service.list_request_logs(&query).await?;
    Ok(Json(envelope(RequestLogPageView {
        items: page.items.iter().map(summary_view).collect(),
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/request-logs/{request_log_id}",
    params(("request_log_id" = String, Path, description = "Request log identifier")),
    responses(
        (status = 200, body = Envelope<RequestLogDetailView>),
        (status = 404, body = OpenAiErrorEnvelopeView, description = "Request log not found")
    ),
    security(("session_cookie" = []))
)]
pub async fn get_request_log_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_log_id): Path<Uuid>,
) -> Result<Json<Envelope<RequestLogDetailView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let detail = state.service.get_request_log_detail(request_log_id).await?;
    Ok(Json(envelope(detail_view(detail))))
}

fn summary_view(log: &RequestLogRecord) -> RequestLogSummaryView {
    let provider_icon_key = provider_icon_key_from_metadata(&log.metadata)
        .map(|value| value.as_str().to_string())
        .or_else(|| {
            Some(
                resolve_provider_display(log.provider_key.as_str(), None)
                    .icon_key
                    .as_str()
                    .to_string(),
            )
        });
    let model_icon_key = model_icon_key_from_metadata(&log.metadata)
        .or_else(|| {
            resolve_model_icon_key([log.resolved_model_key.as_str(), log.model_key.as_str()])
        })
        .map(|value| value.as_str().to_string());

    RequestLogSummaryView {
        request_log_id: log.request_log_id.to_string(),
        request_id: log.request_id.clone(),
        api_key_id: log.api_key_id.to_string(),
        user_id: log.user_id.map(|value| value.to_string()),
        team_id: log.team_id.map(|value| value.to_string()),
        model_key: log.model_key.clone(),
        resolved_model_key: log.resolved_model_key.clone(),
        model_icon_key,
        provider_key: log.provider_key.clone(),
        provider_icon_key,
        status_code: log.status_code,
        latency_ms: log.latency_ms,
        prompt_tokens: log.prompt_tokens,
        completion_tokens: log.completion_tokens,
        total_tokens: log.total_tokens,
        error_code: log.error_code.clone(),
        has_payload: log.has_payload,
        request_payload_truncated: log.request_payload_truncated,
        response_payload_truncated: log.response_payload_truncated,
        request_tags: request_tags_view(&log.request_tags),
        metadata: log.metadata.clone(),
        occurred_at: format_timestamp(log.occurred_at),
    }
}

fn detail_view(detail: RequestLogDetail) -> RequestLogDetailView {
    RequestLogDetailView {
        log: summary_view(&detail.log),
        payload: detail.payload.map(payload_view),
    }
}

fn payload_view(payload: RequestLogPayloadRecord) -> RequestLogPayloadView {
    RequestLogPayloadView {
        request_json: payload.request_json,
        response_json: payload.response_json,
    }
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn parse_optional_tag_filter(
    key: Option<String>,
    value: Option<String>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let key = empty_to_none(key);
    let value = empty_to_none(value);

    match (key, value) {
        (None, None) => Ok((None, None)),
        (Some(_), None) => Err(AppError(GatewayError::InvalidRequest(
            "request log tag filters require both `tag_key` and `tag_value`".to_string(),
        ))),
        (None, Some(_)) => Err(AppError(GatewayError::InvalidRequest(
            "request log tag filters require both `tag_key` and `tag_value`".to_string(),
        ))),
        (Some(key), Some(value)) => {
            let tag = build_bespoke_tag_filter(&key, &value).map_err(AppError)?;
            Ok((Some(tag.key), Some(tag.value)))
        }
    }
}

fn request_tags_view(tags: &RequestTags) -> RequestTagsView {
    RequestTagsView {
        service: tags.service.clone(),
        component: tags.component.clone(),
        env: tags.env.clone(),
        bespoke: tags.bespoke.iter().map(request_tag_view).collect(),
    }
}

fn request_tag_view(tag: &RequestTag) -> RequestTagView {
    RequestTagView {
        key: tag.key.clone(),
        value: tag.value.clone(),
    }
}

fn parse_optional_uuid(value: Option<&str>, field_name: &str) -> Result<Option<Uuid>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    Uuid::parse_str(value).map(Some).map_err(|error| {
        AppError(GatewayError::InvalidRequest(format!(
            "invalid {field_name} `{value}`: {error}"
        )))
    })
}
