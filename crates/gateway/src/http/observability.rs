use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{RequestLogRepository, StoreError};
use serde::Serialize;
use serde_json::Value;

use crate::http::{
    error::AppError,
    identity::{Envelope, envelope, format_timestamp, require_platform_admin},
    state::AppState,
};

const DEFAULT_REQUEST_LOG_LIMIT: usize = 500;

#[derive(Debug, Serialize)]
pub(crate) struct RequestLogsPage {
    items: Vec<RequestLogView>,
    page: usize,
    page_size: usize,
    total: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct RequestLogView {
    id: String,
    model: String,
    provider: String,
    upstream_model: String,
    status_code: i64,
    latency_ms: i64,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
    stream: bool,
    fallback_used: bool,
    attempt_count: i64,
    payload_available: bool,
    error_code: Option<String>,
    timestamp: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RequestLogDetailView {
    request_id: String,
    request_json: Value,
    response_json: Value,
    request_bytes: i64,
    response_bytes: i64,
    request_truncated: bool,
    response_truncated: bool,
    request_sha256: String,
    response_sha256: String,
    timestamp: String,
}

pub async fn list_request_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<RequestLogsPage>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let items = state.store.list_request_logs(DEFAULT_REQUEST_LOG_LIMIT).await?;
    let views = items
        .into_iter()
        .map(|item| RequestLogView {
            id: item.request_id,
            model: item.model_key,
            provider: item.provider_key,
            upstream_model: item.upstream_model,
            status_code: item.status_code.unwrap_or_default(),
            latency_ms: item.latency_ms.unwrap_or_default(),
            prompt_tokens: item.prompt_tokens,
            completion_tokens: item.completion_tokens,
            total_tokens: item.total_tokens,
            stream: item.stream,
            fallback_used: item.fallback_used,
            attempt_count: item.attempt_count,
            payload_available: item.payload_available,
            error_code: item.error_code,
            timestamp: format_timestamp(item.occurred_at),
        })
        .collect::<Vec<_>>();

    let total = views.len();
    Ok(Json(envelope(RequestLogsPage {
        items: views,
        page: 1,
        page_size: total,
        total,
    })))
}

pub async fn get_request_log_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<Envelope<RequestLogDetailView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let payload = state
        .store
        .get_request_log_payload_by_request_id(&request_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("request log `{request_id}` not found")))?;

    Ok(Json(envelope(RequestLogDetailView {
        request_id,
        request_json: payload.request_json,
        response_json: payload.response_json,
        request_bytes: payload.request_bytes,
        response_bytes: payload.response_bytes,
        request_truncated: payload.request_truncated,
        response_truncated: payload.response_truncated,
        request_sha256: payload.request_sha256,
        response_sha256: payload.response_sha256,
        timestamp: format_timestamp(payload.occurred_at),
    })))
}
