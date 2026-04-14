use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, ProviderConnection, ProviderRepository, RequestLogDetail,
    RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestTag, RequestTags,
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
    let providers = provider_connections_by_key(&state, &page.items).await?;
    Ok(Json(envelope(RequestLogPageView {
        items: page
            .items
            .iter()
            .map(|log| summary_view(log, providers.get(log.provider_key.as_str())))
            .collect(),
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
    let provider = provider_connection(&state, detail.log.provider_key.as_str()).await?;
    Ok(Json(envelope(detail_view(detail, provider.as_ref()))))
}

async fn provider_connections_by_key(
    state: &AppState,
    logs: &[RequestLogRecord],
) -> Result<HashMap<String, ProviderConnection>, AppError> {
    let provider_keys: HashSet<_> = logs
        .iter()
        .filter(|log| provider_icon_key_from_metadata(&log.metadata).is_none())
        .map(|log| log.provider_key.clone())
        .collect();

    let mut providers = HashMap::new();
    for provider_key in provider_keys {
        if let Some(provider) = provider_connection(state, provider_key.as_str()).await? {
            providers.insert(provider_key, provider);
        }
    }

    Ok(providers)
}

async fn provider_connection(
    state: &AppState,
    provider_key: &str,
) -> Result<Option<ProviderConnection>, AppError> {
    state
        .store
        .get_provider_by_key(provider_key)
        .await
        .map_err(|error| AppError(error.into()))
}

fn summary_view(
    log: &RequestLogRecord,
    provider: Option<&ProviderConnection>,
) -> RequestLogSummaryView {
    let provider_icon_key = provider_icon_key_from_metadata(&log.metadata)
        .or_else(|| Some(resolve_provider_display(log.provider_key.as_str(), provider).icon_key))
        .map(Into::into);
    let model_icon_key = model_icon_key_from_metadata(&log.metadata)
        .or_else(|| {
            resolve_model_icon_key([log.resolved_model_key.as_str(), log.model_key.as_str()])
        })
        .map(Into::into);

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

fn detail_view(
    detail: RequestLogDetail,
    provider: Option<&ProviderConnection>,
) -> RequestLogDetailView {
    RequestLogDetailView {
        log: summary_view(&detail.log, provider),
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

#[cfg(test)]
mod tests {
    use gateway_service::REQUEST_LOG_PROVIDER_ICON_KEY;
    use serde_json::{Map, Value, json};
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn summary_view_uses_provider_display_config_when_metadata_is_missing() {
        let log = request_log_record(Map::new());
        let provider = ProviderConnection {
            provider_key: "router".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://openrouter.ai/api/v1",
                "display": {
                    "label": "OpenRouter",
                    "icon_key": "openrouter"
                }
            }),
            secrets: None,
        };

        let summary = summary_view(&log, Some(&provider));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenRouter)
        ));
    }

    #[test]
    fn summary_view_falls_back_to_provider_key_when_provider_config_is_unavailable() {
        let log = request_log_record(Map::new());

        let summary = summary_view(&log, None);

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenAI)
        ));
    }

    #[test]
    fn summary_view_prefers_stored_metadata_over_provider_fallbacks() {
        let mut metadata = Map::new();
        metadata.insert(
            REQUEST_LOG_PROVIDER_ICON_KEY.to_string(),
            Value::String("anthropic".to_string()),
        );
        let log = request_log_record(metadata);
        let provider = ProviderConnection {
            provider_key: "router".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://openrouter.ai/api/v1",
                "display": {
                    "label": "OpenRouter",
                    "icon_key": "openrouter"
                }
            }),
            secrets: None,
        };

        let summary = summary_view(&log, Some(&provider));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::Anthropic)
        ));
    }

    fn request_log_record(metadata: Map<String, Value>) -> RequestLogRecord {
        RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req_123".to_string(),
            api_key_id: Uuid::new_v4(),
            user_id: None,
            team_id: None,
            model_key: "router-model".to_string(),
            resolved_model_key: "router-model".to_string(),
            provider_key: "router".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: false,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags::default(),
            metadata,
            occurred_at: OffsetDateTime::now_utc(),
        }
    }
}
