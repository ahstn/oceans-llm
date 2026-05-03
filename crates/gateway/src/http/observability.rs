use std::collections::{BTreeMap, HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    BudgetRepository, GatewayError, ProviderConnection, ProviderRepository, RequestAttemptRecord,
    RequestLogDetail, RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestTag,
    RequestTags,
};
use gateway_service::{
    model_icon_key_from_metadata, provider_icon_key_from_metadata, resolve_model_icon_key,
    resolve_provider_display,
};
use serde_json::{Map, Value};
use time::{Duration, OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{
        Envelope, LeaderboardChartUserView, LeaderboardLeaderView, LeaderboardQuery,
        LeaderboardSeriesPointView, LeaderboardSeriesValueView, LeaderboardView,
        OpenAiErrorEnvelopeView, RequestAttemptView, RequestLogDetailView, RequestLogListQuery,
        RequestLogPageView, RequestLogPayloadCaptureModeView, RequestLogPayloadPolicyView,
        RequestLogPayloadView, RequestLogSummaryView, RequestTagView, RequestTagsView,
        RequestToolCardinalityAveragesView, RequestToolCardinalityView, envelope, format_timestamp,
    },
    error::AppError,
    request_tags::build_bespoke_tag_filter,
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 100;
const MAX_PAGE_SIZE: u32 = 500;
const LEADERBOARD_BUCKET_HOURS: u8 = 12;
const LEADERBOARD_CHART_USERS: usize = 5;
const LEADERBOARD_LIMIT: u32 = 30;

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/leaderboard",
    params(LeaderboardQuery),
    responses((status = 200, body = Envelope<LeaderboardView>)),
    security(("session_cookie" = []))
)]
pub async fn get_usage_leaderboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<Envelope<LeaderboardView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let range = parse_leaderboard_range(query.range.as_deref())?;
    let (window_start, window_end) = leaderboard_window_bounds_utc(range.days())?;
    let leaders = state
        .store
        .list_usage_user_leaderboard(window_start, window_end, LEADERBOARD_LIMIT)
        .await?;
    let chart_users = leaders
        .iter()
        .take(LEADERBOARD_CHART_USERS)
        .enumerate()
        .map(|(index, leader)| LeaderboardChartUserView {
            rank: (index + 1) as u32,
            user_id: leader.user_id.to_string(),
            user_name: leader.user_name.clone(),
            total_spend_usd_10000: leader.priced_cost_usd.as_scaled_i64(),
        })
        .collect::<Vec<_>>();
    let chart_user_ids = leaders
        .iter()
        .take(LEADERBOARD_CHART_USERS)
        .map(|leader| leader.user_id)
        .collect::<Vec<_>>();
    let bucket_rows = state
        .store
        .list_usage_user_bucket_aggregates(
            window_start,
            window_end,
            LEADERBOARD_BUCKET_HOURS,
            &chart_user_ids,
        )
        .await?;

    let mut bucket_map = BTreeMap::<i64, HashMap<Uuid, i64>>::new();
    for row in bucket_rows {
        bucket_map
            .entry(row.bucket_start.unix_timestamp())
            .or_default()
            .insert(row.user_id, row.priced_cost_usd.as_scaled_i64());
    }

    let bucket_width = Duration::hours(i64::from(LEADERBOARD_BUCKET_HOURS));
    let bucket_count = (range.days() as usize * 24) / usize::from(LEADERBOARD_BUCKET_HOURS);
    let mut series = Vec::with_capacity(bucket_count);
    for bucket_index in 0..bucket_count {
        let bucket_start = window_start + (bucket_width * (bucket_index as i32));
        let values = chart_user_ids
            .iter()
            .map(|user_id| LeaderboardSeriesValueView {
                user_id: user_id.to_string(),
                spend_usd_10000: bucket_map
                    .get(&bucket_start.unix_timestamp())
                    .and_then(|values| values.get(user_id))
                    .copied()
                    .unwrap_or(0),
            })
            .collect();
        series.push(LeaderboardSeriesPointView {
            bucket_start: format_timestamp(bucket_start),
            values,
        });
    }

    let leaders = leaders
        .into_iter()
        .enumerate()
        .map(|(index, leader)| LeaderboardLeaderView {
            rank: (index + 1) as u32,
            user_id: leader.user_id.to_string(),
            user_name: leader.user_name,
            total_spend_usd_10000: leader.priced_cost_usd.as_scaled_i64(),
            most_used_model: leader.top_model_key,
            total_requests: leader.total_request_count,
            tool_cardinality_averages: RequestToolCardinalityAveragesView {
                referenced_mcp_server_count: leader
                    .tool_cardinality_averages
                    .referenced_mcp_server_count,
                exposed_tool_count: leader.tool_cardinality_averages.exposed_tool_count,
                invoked_tool_count: leader.tool_cardinality_averages.invoked_tool_count,
                filtered_tool_count: leader.tool_cardinality_averages.filtered_tool_count,
            },
        })
        .collect();

    Ok(Json(envelope(LeaderboardView {
        range: range.as_str().to_string(),
        bucket_hours: LEADERBOARD_BUCKET_HOURS,
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        chart_users,
        series,
        leaders,
    })))
}

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
    let items = page
        .items
        .iter()
        .map(|log| summary_view(log, providers.get(log.provider_key.as_str())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(envelope(RequestLogPageView {
        items,
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
    Ok(Json(envelope(detail_view(detail, provider.as_ref())?)))
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
) -> Result<RequestLogSummaryView, AppError> {
    let provider_icon_key = provider_icon_key_from_metadata(&log.metadata)
        .or_else(|| Some(resolve_provider_display(log.provider_key.as_str(), provider).icon_key))
        .map(Into::into);
    let model_icon_key = model_icon_key_from_metadata(&log.metadata)
        .or_else(|| {
            resolve_model_icon_key([log.resolved_model_key.as_str(), log.model_key.as_str()])
        })
        .map(Into::into);

    Ok(RequestLogSummaryView {
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
        payload_policy: payload_policy_view(&log.metadata)?,
        request_tags: request_tags_view(&log.request_tags),
        tool_cardinality: RequestToolCardinalityView {
            referenced_mcp_server_count: log.tool_cardinality.referenced_mcp_server_count,
            exposed_tool_count: log.tool_cardinality.exposed_tool_count,
            invoked_tool_count: log.tool_cardinality.invoked_tool_count,
            filtered_tool_count: log.tool_cardinality.filtered_tool_count,
        },
        metadata: log.metadata.clone(),
        occurred_at: format_timestamp(log.occurred_at),
    })
}

fn payload_policy_view(
    metadata: &Map<String, Value>,
) -> Result<RequestLogPayloadPolicyView, AppError> {
    let policy = metadata
        .get("payload_policy")
        .and_then(Value::as_object)
        .ok_or_else(|| payload_policy_contract_error("missing payload_policy object"))?;

    Ok(RequestLogPayloadPolicyView {
        capture_mode: match required_payload_policy_string(policy, "capture_mode")? {
            "disabled" => RequestLogPayloadCaptureModeView::Disabled,
            "summary_only" => RequestLogPayloadCaptureModeView::SummaryOnly,
            "redacted_payloads" => RequestLogPayloadCaptureModeView::RedactedPayloads,
            other => {
                return Err(payload_policy_contract_error(format!(
                    "unknown capture_mode `{other}`"
                )));
            }
        },
        request_max_bytes: required_positive_payload_policy_u64(policy, "request_max_bytes")?,
        response_max_bytes: required_positive_payload_policy_u64(policy, "response_max_bytes")?,
        stream_max_events: required_positive_payload_policy_u64(policy, "stream_max_events")?,
        version: required_payload_policy_string(policy, "version")?.to_string(),
    })
}

fn required_payload_policy_string<'a>(
    policy: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, AppError> {
    policy
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| payload_policy_contract_error(format!("missing string field `{field}`")))
}

fn required_positive_payload_policy_u64(
    policy: &Map<String, Value>,
    field: &str,
) -> Result<u64, AppError> {
    let value = policy
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| payload_policy_contract_error(format!("missing u64 field `{field}`")))?;
    if value == 0 {
        return Err(payload_policy_contract_error(format!(
            "field `{field}` must be greater than zero"
        )));
    }
    Ok(value)
}

fn payload_policy_contract_error(message: impl Into<String>) -> AppError {
    AppError(GatewayError::Internal(format!(
        "invalid request log payload_policy metadata: {}",
        message.into()
    )))
}

fn detail_view(
    detail: RequestLogDetail,
    provider: Option<&ProviderConnection>,
) -> Result<RequestLogDetailView, AppError> {
    Ok(RequestLogDetailView {
        log: summary_view(&detail.log, provider)?,
        payload: detail.payload.map(payload_view),
        attempts: detail.attempts.into_iter().map(attempt_view).collect(),
    })
}

fn attempt_view(attempt: RequestAttemptRecord) -> RequestAttemptView {
    RequestAttemptView {
        request_attempt_id: attempt.request_attempt_id.to_string(),
        request_log_id: attempt.request_log_id.to_string(),
        request_id: attempt.request_id,
        attempt_number: attempt.attempt_number,
        route_id: attempt.route_id.to_string(),
        provider_key: attempt.provider_key,
        upstream_model: attempt.upstream_model,
        status: attempt.status.as_str().to_string(),
        status_code: attempt.status_code,
        error_code: attempt.error_code,
        error_detail: attempt.error_detail,
        error_detail_truncated: attempt.error_detail_truncated,
        retryable: attempt.retryable,
        terminal: attempt.terminal,
        produced_final_response: attempt.produced_final_response,
        stream: attempt.stream,
        started_at: format_timestamp(attempt.started_at),
        completed_at: attempt.completed_at.map(format_timestamp),
        latency_ms: attempt.latency_ms,
        metadata: attempt.metadata,
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

#[derive(Clone, Copy)]
enum LeaderboardRange {
    SevenDays,
    ThirtyOneDays,
}

impl LeaderboardRange {
    fn as_str(self) -> &'static str {
        match self {
            Self::SevenDays => "7d",
            Self::ThirtyOneDays => "31d",
        }
    }

    fn days(self) -> u16 {
        match self {
            Self::SevenDays => 7,
            Self::ThirtyOneDays => 31,
        }
    }
}

fn parse_leaderboard_range(value: Option<&str>) -> Result<LeaderboardRange, AppError> {
    match value.unwrap_or("7d") {
        "7d" => Ok(LeaderboardRange::SevenDays),
        "31d" => Ok(LeaderboardRange::ThirtyOneDays),
        other => Err(AppError(GatewayError::InvalidRequest(format!(
            "range must be either `7d` or `31d`, got `{other}`"
        )))),
    }
}

fn leaderboard_window_bounds_utc(
    window_days: u16,
) -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    let now_utc = OffsetDateTime::now_utc().to_offset(UtcOffset::UTC);
    let bucket_seconds = i64::from(LEADERBOARD_BUCKET_HOURS) * 60 * 60;
    let now_seconds = now_utc.unix_timestamp();
    let window_end_seconds = ((now_seconds / bucket_seconds) + 1) * bucket_seconds;
    let window_end = OffsetDateTime::from_unix_timestamp(window_end_seconds).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "invalid leaderboard window end: {error}"
        )))
    })?;
    let window_start = window_end - Duration::days(i64::from(window_days));
    Ok((window_start, window_end))
}

#[cfg(test)]
mod tests {
    use gateway_service::REQUEST_LOG_PROVIDER_ICON_KEY;
    use serde_json::{Map, Value, json};
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn summary_view_uses_provider_display_config_when_metadata_is_missing() {
        let log = request_log_record(payload_policy_metadata());
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

        let summary = summary_view(&log, Some(&provider))
            .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenRouter)
        ));
    }

    #[test]
    fn summary_view_falls_back_to_provider_key_when_provider_config_is_unavailable() {
        let log = request_log_record(payload_policy_metadata());

        let summary = summary_view(&log, None)
            .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenAI)
        ));
    }

    #[test]
    fn summary_view_prefers_stored_metadata_over_provider_fallbacks() {
        let mut metadata = payload_policy_metadata();
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

        let summary = summary_view(&log, Some(&provider))
            .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::Anthropic)
        ));
    }

    #[test]
    fn summary_view_requires_payload_policy_metadata() {
        let log = request_log_record(Map::new());

        let error = summary_view(&log, None).expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("missing payload_policy object")
        );
    }

    #[test]
    fn summary_view_rejects_unknown_payload_policy_capture_mode() {
        let mut metadata = payload_policy_metadata();
        metadata["payload_policy"]
            .as_object_mut()
            .expect("policy")
            .insert("capture_mode".to_string(), json!("legacy"));
        let log = request_log_record(metadata);

        let error = summary_view(&log, None).expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("unknown capture_mode `legacy`")
        );
    }

    #[test]
    fn summary_view_rejects_malformed_payload_policy_metadata() {
        let mut metadata = payload_policy_metadata();
        metadata["payload_policy"]
            .as_object_mut()
            .expect("policy")
            .insert("request_max_bytes".to_string(), json!("65536"));
        let log = request_log_record(metadata);

        let error = summary_view(&log, None).expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("missing u64 field `request_max_bytes`")
        );
    }

    #[test]
    fn summary_view_rejects_zero_payload_policy_limits() {
        let mut metadata = payload_policy_metadata();
        metadata["payload_policy"]
            .as_object_mut()
            .expect("policy")
            .insert("stream_max_events".to_string(), json!(0));
        let log = request_log_record(metadata);

        let error = summary_view(&log, None).expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("field `stream_max_events` must be greater than zero")
        );
    }

    #[test]
    fn parse_leaderboard_range_defaults_to_seven_days() {
        let range = parse_leaderboard_range(None);
        assert!(matches!(range, Ok(LeaderboardRange::SevenDays)));
    }

    #[test]
    fn parse_leaderboard_range_rejects_unknown_values() {
        let error = parse_leaderboard_range(Some("14d"));
        match error {
            Err(error) => assert!(
                error
                    .0
                    .to_string()
                    .contains("range must be either `7d` or `31d`")
            ),
            Ok(_) => panic!("expected invalid range to fail"),
        }
    }

    #[test]
    fn leaderboard_window_bounds_align_to_half_day_utc() {
        let result = leaderboard_window_bounds_utc(7);
        assert!(result.is_ok(), "leaderboard window bounds should be valid");
        let (window_start, window_end) = result.unwrap_or_else(|_| unreachable!());
        let bucket_seconds = i64::from(LEADERBOARD_BUCKET_HOURS) * 60 * 60;

        assert_eq!(window_end.unix_timestamp() % bucket_seconds, 0);
        assert_eq!(
            window_end - window_start,
            Duration::days(7),
            "expected exactly seven days of data"
        );
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
            tool_cardinality: gateway_core::RequestToolCardinality::default(),
            metadata,
            occurred_at: OffsetDateTime::now_utc(),
        }
    }

    fn payload_policy_metadata() -> Map<String, Value> {
        let mut metadata = Map::new();
        metadata.insert(
            "payload_policy".to_string(),
            json!({
                "capture_mode": "redacted_payloads",
                "request_max_bytes": 65536,
                "response_max_bytes": 65536,
                "stream_max_events": 128,
                "version": "builtin:v1"
            }),
        );
        metadata
    }
}
