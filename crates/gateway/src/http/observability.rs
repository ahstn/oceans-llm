use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, RequestLogDetail, RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord,
    RequestTag, RequestTags,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    error::AppError,
    identity::{Envelope, envelope, format_timestamp},
    request_tags::parse_bespoke_tag_filter,
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 100;
const MAX_PAGE_SIZE: u32 = 500;

#[derive(Debug, Deserialize, Default)]
pub struct RequestLogListQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    request_id: Option<String>,
    model_key: Option<String>,
    provider_key: Option<String>,
    status_code: Option<i64>,
    user_id: Option<String>,
    team_id: Option<String>,
    service: Option<String>,
    component: Option<String>,
    env: Option<String>,
    tag: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RequestLogPageView {
    items: Vec<RequestLogSummaryView>,
    page: u32,
    page_size: u32,
    total: u64,
}

#[derive(Debug, Serialize)]
pub struct RequestLogSummaryView {
    request_log_id: String,
    request_id: String,
    api_key_id: String,
    user_id: Option<String>,
    team_id: Option<String>,
    model_key: String,
    resolved_model_key: String,
    provider_key: String,
    status_code: Option<i64>,
    latency_ms: Option<i64>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
    error_code: Option<String>,
    has_payload: bool,
    request_payload_truncated: bool,
    response_payload_truncated: bool,
    request_tags: RequestTagsView,
    metadata: Value,
    occurred_at: String,
}

#[derive(Debug, Serialize)]
pub struct RequestTagsView {
    service: Option<String>,
    component: Option<String>,
    env: Option<String>,
    bespoke: Vec<RequestTagView>,
}

#[derive(Debug, Serialize)]
pub struct RequestTagView {
    key: String,
    value: String,
}

#[derive(Debug, Serialize)]
pub struct RequestLogDetailView {
    log: RequestLogSummaryView,
    payload: Option<RequestLogPayloadView>,
}

#[derive(Debug, Serialize)]
pub struct RequestLogPayloadView {
    request_json: Value,
    response_json: Value,
}

pub async fn list_request_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RequestLogListQuery>,
) -> Result<Json<Envelope<RequestLogPageView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let query = RequestLogQuery {
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
        bespoke_tag: parse_optional_tag_filter(query.tag.as_deref())?,
    };

    let page = state.service.list_request_logs(&query).await?;
    Ok(Json(envelope(RequestLogPageView {
        items: page.items.iter().map(summary_view).collect(),
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })))
}

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
    RequestLogSummaryView {
        request_log_id: log.request_log_id.to_string(),
        request_id: log.request_id.clone(),
        api_key_id: log.api_key_id.to_string(),
        user_id: log.user_id.map(|value| value.to_string()),
        team_id: log.team_id.map(|value| value.to_string()),
        model_key: log.model_key.clone(),
        resolved_model_key: log.resolved_model_key.clone(),
        provider_key: log.provider_key.clone(),
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
        metadata: Value::Object(log.metadata.clone()),
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

fn parse_optional_tag_filter(value: Option<&str>) -> Result<Option<RequestTag>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    parse_bespoke_tag_filter(value).map(Some).map_err(AppError)
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
