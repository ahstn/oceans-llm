use std::{
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    time::Instant,
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    AdminApiKeyRepository, AgentSessionListQuery, AgentSessionSourceRecord,
    AgentSessionTraceRecord, AuthError, BudgetRepository, Confidence, GatewayError,
    GatewayOutcomeState, GlobalRole, IdentityRepository, MAX_MCP_TOOL_INVOCATION_PAGE_SIZE,
    MAX_REQUEST_LOG_PAGE_SIZE, McpTokenOverheadRepository, McpToolInvocationDetail,
    McpToolInvocationPayloadRecord, McpToolInvocationQuery, McpToolInvocationRecord,
    McpToolInvocationStatus, McpToolPolicyResult, ProviderConnection, ProviderRepository,
    RequestAttemptRecord, RequestLogDetail, RequestLogPayloadRecord, RequestLogQuery,
    RequestLogRecord, RequestLogRepository, RequestMcpTokenOverheadRecord, RequestTag, RequestTags,
    ScoreMaturity, SessionLifecycleState, UsageLedgerRecord,
};
use gateway_service::{
    model_icon_key_from_metadata, provider_icon_key_from_metadata, resolve_model_icon_key,
    resolve_provider_display,
};
use gateway_store::GatewayStore;
use serde::Serialize;
use serde_json::{Map, Value};
use time::{Duration, OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::http::{
    admin_auth::{
        AdminDataScope, require_active_session, require_agent_analysis_scope,
        require_authenticated_session,
    },
    admin_contract::{
        AgentAnalysisMetricPolicyView, AgentContextDiagnosticsView, AgentFileInteractionFactView,
        AgentFinishReasonDiagnosticsView, AgentFinishReasonItemView, AgentObservationCoverageView,
        AgentObservationFactsView, AgentObservationView, AgentOutcomeDiagnosticsView,
        AgentReliabilityDiagnosticsView, AgentRequestAttemptView, AgentSessionAnalysisIdentityView,
        AgentSessionDetailView, AgentSessionDiagnosticsView, AgentSessionEfficiencyComponentsView,
        AgentSessionEfficiencyReportView, AgentSessionListRequestQuery, AgentSessionOutcomeView,
        AgentSessionPageView, AgentSessionRequestView, AgentSessionSourceView,
        AgentSessionSummaryView, AgentSkillDiagnosticItemView, AgentSkillDiagnosticsView,
        AgentSuppliedSkillFactView, AgentSuppliedToolFactView, AgentTelemetryCoverageView,
        AgentTokenAndCacheDiagnosticsView, AgentToolAndChangeDiagnosticsView,
        AgentToolReliabilityItemView, AgentToolServerDiagnosticsView, Envelope,
        HarnessUsageChartHarnessView, HarnessUsageLeaderView, HarnessUsageQuery,
        HarnessUsageSeriesPointView, HarnessUsageSeriesValueView, HarnessUsageView,
        LeaderboardChartUserView, LeaderboardHarnessView, LeaderboardLeaderView, LeaderboardQuery,
        LeaderboardSeriesPointView, LeaderboardSeriesValueView, LeaderboardView,
        McpToolInvocationDetailView, McpToolInvocationListQuery, McpToolInvocationPageView,
        McpToolInvocationPayloadView, McpToolInvocationSummaryView, OpenAiErrorEnvelopeView,
        RequestAttemptView, RequestLogDetailView, RequestLogListQuery, RequestLogPageView,
        RequestLogPayloadCaptureModeView, RequestLogPayloadPolicyView, RequestLogPayloadView,
        RequestLogSummaryView, RequestMcpTokenOverheadView, RequestTagView, RequestTagsView,
        RequestToolCardinalityAveragesView, RequestToolCardinalityView, envelope, format_timestamp,
    },
    error::AppError,
    request_tags::build_bespoke_tag_filter,
    response_cache::{CacheStatus, ResponseCache},
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;

pub(crate) mod agent_sessions;

pub use agent_sessions::{get_agent_session_detail, list_agent_sessions};

pub(crate) mod request_logs;

pub use request_logs::{get_request_log_detail, list_request_logs};

const DEFAULT_PAGE_SIZE: u32 = 100;
const LEADERBOARD_BUCKET_HOURS: u8 = 12;
const LEADERBOARD_CHART_USERS: usize = 5;
const LEADERBOARD_LIMIT: u32 = 30;
const HARNESS_USAGE_CHART_HARNESSES: usize = 5;
const HARNESS_USAGE_LIMIT: u32 = 30;

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
    require_active_session(&state, &headers).await?;

    let range = parse_leaderboard_range(query.range.as_deref())?;
    let view = load_cached_admin_view(
        &state,
        &state.leaderboard_cache,
        range.as_str().to_string(),
        "leaderboard",
        || build_usage_leaderboard(&state, range),
    )
    .await?;

    Ok(Json(envelope(view)))
}

async fn build_usage_leaderboard(
    state: &AppState,
    range: LeaderboardRange,
) -> Result<LeaderboardView, AppError> {
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
            most_used_harness: leader
                .most_used_harness
                .map(|harness| LeaderboardHarnessView {
                    key: harness.key,
                    label: harness.label,
                }),
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

    Ok(LeaderboardView {
        range: range.as_str().to_string(),
        bucket_hours: LEADERBOARD_BUCKET_HOURS,
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        chart_users,
        series,
        leaders,
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/harness-usage",
    params(HarnessUsageQuery),
    responses((status = 200, body = Envelope<HarnessUsageView>)),
    security(("session_cookie" = []))
)]
pub async fn get_harness_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HarnessUsageQuery>,
) -> Result<Json<Envelope<HarnessUsageView>>, AppError> {
    require_active_session(&state, &headers).await?;

    let range = parse_leaderboard_range(query.range.as_deref())?;
    let view = load_cached_admin_view(
        &state,
        &state.harness_usage_cache,
        range.as_str().to_string(),
        "agent_harnesses",
        || build_harness_usage(&state, range),
    )
    .await?;

    Ok(Json(envelope(view)))
}

async fn build_harness_usage(
    state: &AppState,
    range: LeaderboardRange,
) -> Result<HarnessUsageView, AppError> {
    let (window_start, window_end) = leaderboard_window_bounds_utc(range.days())?;
    let leaders = state
        .store
        .list_harness_usage_leaders(window_start, window_end, HARNESS_USAGE_LIMIT)
        .await?;
    let top_chart_harnesses = leaders
        .iter()
        .take(HARNESS_USAGE_CHART_HARNESSES)
        .collect::<Vec<_>>();
    let chart_harnesses = top_chart_harnesses
        .iter()
        .enumerate()
        .map(|(index, leader)| HarnessUsageChartHarnessView {
            rank: (index + 1) as u32,
            agent_harness_key: leader.agent_harness_key.clone(),
            agent_harness_label: leader.agent_harness_label.clone(),
            total_requests: leader.request_count,
        })
        .collect::<Vec<_>>();
    let chart_harness_keys = top_chart_harnesses
        .iter()
        .map(|leader| leader.agent_harness_key.clone())
        .collect::<Vec<_>>();
    let bucket_rows = if chart_harness_keys.is_empty() {
        Vec::new()
    } else {
        state
            .store
            .list_harness_usage_bucket_aggregates(
                window_start,
                window_end,
                LEADERBOARD_BUCKET_HOURS,
                &chart_harness_keys,
            )
            .await?
    };

    let mut bucket_map = BTreeMap::<i64, HashMap<String, i64>>::new();
    for row in bucket_rows {
        bucket_map
            .entry(row.bucket_start.unix_timestamp())
            .or_default()
            .insert(row.agent_harness_key, row.request_count);
    }

    let bucket_width = Duration::hours(i64::from(LEADERBOARD_BUCKET_HOURS));
    let bucket_count = (range.days() as usize * 24) / usize::from(LEADERBOARD_BUCKET_HOURS);
    let mut series = Vec::with_capacity(bucket_count);
    for bucket_index in 0..bucket_count {
        let bucket_start = window_start + (bucket_width * (bucket_index as i32));
        let values = chart_harness_keys
            .iter()
            .map(|agent_harness_key| HarnessUsageSeriesValueView {
                agent_harness_key: agent_harness_key.clone(),
                request_count: bucket_map
                    .get(&bucket_start.unix_timestamp())
                    .and_then(|values| values.get(agent_harness_key))
                    .copied()
                    .unwrap_or(0),
            })
            .collect();
        series.push(HarnessUsageSeriesPointView {
            bucket_start: format_timestamp(bucket_start),
            values,
        });
    }

    let leaders = leaders
        .into_iter()
        .enumerate()
        .map(|(index, leader)| HarnessUsageLeaderView {
            rank: (index + 1) as u32,
            agent_harness_key: leader.agent_harness_key,
            agent_harness_label: leader.agent_harness_label,
            total_requests: leader.request_count,
            prompt_tokens: leader.prompt_tokens,
            completion_tokens: leader.completion_tokens,
            total_tokens: leader.total_tokens,
        })
        .collect();

    Ok(HarnessUsageView {
        range: range.as_str().to_string(),
        bucket_hours: LEADERBOARD_BUCKET_HOURS,
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        chart_harnesses,
        series,
        leaders,
    })
}

async fn load_cached_admin_view<V, F, Fut>(
    state: &AppState,
    cache: &ResponseCache<String, V>,
    cache_key: String,
    view: &'static str,
    loader: F,
) -> Result<V, AppError>
where
    V: Clone,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<V, AppError>>,
{
    let cached = cache
        .get_or_load(cache_key, || async {
            let started_at = Instant::now();
            let result = loader().await;
            state.metrics.record_admin_view_cache_load(
                view,
                if result.is_ok() { "success" } else { "error" },
                started_at.elapsed(),
            );
            result
        })
        .await;
    let (result, status) = match cached {
        Ok(cached) => cached,
        Err((error, status)) => {
            record_cache_request(state, view, status);
            return Err(error);
        }
    };
    record_cache_request(state, view, status);
    Ok(result)
}

fn record_cache_request(state: &AppState, view: &str, status: CacheStatus) {
    let result = match status {
        CacheStatus::Hit => "hit",
        CacheStatus::Miss => "miss",
    };
    state.metrics.record_admin_view_cache_request(view, result);
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/mcp-invocations",
    params(McpToolInvocationListQuery),
    responses((status = 200, body = Envelope<McpToolInvocationPageView>)),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_tool_invocations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpToolInvocationListQuery>,
) -> Result<Json<Envelope<McpToolInvocationPageView>>, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;

    let query = McpToolInvocationQuery {
        page: query.page.unwrap_or(DEFAULT_PAGE).max(1),
        page_size: query
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_MCP_TOOL_INVOCATION_PAGE_SIZE),
        request_id: empty_to_none(query.request_id),
        server_display_key: empty_to_none(query.server_display_key),
        server_display_name: empty_to_none(query.server_display_name),
        tool_display_key: empty_to_none(query.tool_display_key),
        tool_display_name: empty_to_none(query.tool_display_name),
        api_key_id: parse_optional_uuid(query.api_key_id.as_deref(), "api_key_id")?,
        user_id: scoped_user_id(
            current_user.user_id,
            current_user.global_role,
            parse_optional_uuid(query.user_id.as_deref(), "user_id")?,
        ),
        team_id: parse_optional_uuid(query.team_id.as_deref(), "team_id")?,
        status: parse_optional_mcp_status(query.status.as_deref())?,
        policy_result: parse_optional_mcp_policy_result(query.policy_result.as_deref())?,
        occurred_at_start: parse_optional_timestamp(
            query.occurred_at_start.as_deref(),
            "occurred_at_start",
        )?,
        occurred_at_end: parse_optional_timestamp(
            query.occurred_at_end.as_deref(),
            "occurred_at_end",
        )?,
    };

    let page = state.service.list_mcp_tool_invocations(&query).await?;
    let items = page.items.iter().map(mcp_invocation_summary_view).collect();
    Ok(Json(envelope(McpToolInvocationPageView {
        items,
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/mcp-invocations/{mcp_tool_invocation_id}",
    params(("mcp_tool_invocation_id" = String, Path, description = "MCP tool invocation identifier")),
    responses(
        (status = 200, body = Envelope<McpToolInvocationDetailView>),
        (status = 404, body = OpenAiErrorEnvelopeView, description = "MCP tool invocation not found")
    ),
    security(("session_cookie" = []))
)]
pub async fn get_mcp_tool_invocation_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(mcp_tool_invocation_id): Path<Uuid>,
) -> Result<Json<Envelope<McpToolInvocationDetailView>>, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;

    let detail = state
        .service
        .get_mcp_tool_invocation_detail(mcp_tool_invocation_id)
        .await?;
    require_owned_record(
        current_user.user_id,
        current_user.global_role,
        detail.invocation.user_id,
    )?;
    Ok(Json(envelope(mcp_invocation_detail_view(detail))))
}

fn scoped_user_id(
    current_user_id: Uuid,
    global_role: GlobalRole,
    requested_user_id: Option<Uuid>,
) -> Option<Uuid> {
    if global_role == GlobalRole::PlatformAdmin {
        requested_user_id
    } else {
        Some(current_user_id)
    }
}

fn require_owned_record(
    current_user_id: Uuid,
    global_role: GlobalRole,
    record_user_id: Option<Uuid>,
) -> Result<(), AppError> {
    if global_role == GlobalRole::PlatformAdmin || record_user_id == Some(current_user_id) {
        return Ok(());
    }

    Err(AppError(GatewayError::Auth(
        AuthError::InsufficientPrivileges,
    )))
}

fn mcp_invocation_detail_view(detail: McpToolInvocationDetail) -> McpToolInvocationDetailView {
    McpToolInvocationDetailView {
        invocation: mcp_invocation_summary_view(&detail.invocation),
        payload: detail.payload.map(mcp_invocation_payload_view),
    }
}

fn mcp_invocation_payload_view(
    payload: McpToolInvocationPayloadRecord,
) -> McpToolInvocationPayloadView {
    McpToolInvocationPayloadView {
        arguments_json: payload.arguments_json,
        result_json: payload.result_json,
    }
}

fn mcp_invocation_summary_view(
    invocation: &McpToolInvocationRecord,
) -> McpToolInvocationSummaryView {
    McpToolInvocationSummaryView {
        mcp_tool_invocation_id: invocation.mcp_tool_invocation_id.to_string(),
        request_log_id: invocation.request_log_id.map(|value| value.to_string()),
        request_id: invocation.request_id.clone(),
        api_key_id: invocation.api_key_id.map(|value| value.to_string()),
        user_id: invocation.user_id.map(|value| value.to_string()),
        team_id: invocation.team_id.map(|value| value.to_string()),
        owner_kind: invocation.owner_kind.as_str().to_string(),
        server_id: invocation.server_id.map(|value| value.to_string()),
        server_display_key: invocation.server_display_key.clone(),
        server_display_name: invocation.server_display_name.clone(),
        tool_id: invocation.tool_id.map(|value| value.to_string()),
        tool_display_key: invocation.tool_display_key.clone(),
        tool_display_name: invocation.tool_display_name.clone(),
        status: invocation.status.as_str().to_string(),
        policy_result: invocation.policy_result.as_str().to_string(),
        latency_ms: invocation.latency_ms,
        error_code: invocation.error_code.clone(),
        has_payload: invocation.has_payload,
        arguments_payload_truncated: invocation.arguments_payload_truncated,
        result_payload_truncated: invocation.result_payload_truncated,
        arguments_payload_redacted: invocation.arguments_payload_redacted,
        result_payload_redacted: invocation.result_payload_redacted,
        metadata: invocation.metadata.clone(),
        occurred_at: format_timestamp(invocation.occurred_at),
    }
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalized_filter(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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

fn parse_optional_mcp_status(
    value: Option<&str>,
) -> Result<Option<McpToolInvocationStatus>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    McpToolInvocationStatus::from_db(value)
        .map(Some)
        .ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(format!(
                "invalid MCP invocation status `{value}`"
            )))
        })
}

fn parse_optional_mcp_policy_result(
    value: Option<&str>,
) -> Result<Option<McpToolPolicyResult>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    McpToolPolicyResult::from_db(value)
        .map(Some)
        .ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(format!(
                "invalid MCP policy result `{value}`"
            )))
        })
}

fn parse_optional_timestamp(
    value: Option<&str>,
    field_name: &str,
) -> Result<Option<OffsetDateTime>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    OffsetDateTime::parse(value, &Rfc3339)
        .map(Some)
        .map_err(|error| {
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
    use super::*;

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
    fn regular_user_query_scope_ignores_requested_user() {
        let current_user_id = Uuid::new_v4();

        assert_eq!(
            scoped_user_id(current_user_id, GlobalRole::User, Some(Uuid::new_v4()),),
            Some(current_user_id)
        );
    }

    #[test]
    fn platform_admin_query_scope_preserves_requested_user() {
        let requested_user_id = Uuid::new_v4();

        assert_eq!(
            scoped_user_id(
                Uuid::new_v4(),
                GlobalRole::PlatformAdmin,
                Some(requested_user_id),
            ),
            Some(requested_user_id)
        );
    }

    #[test]
    fn regular_user_can_open_only_owned_observability_records() {
        let current_user_id = Uuid::new_v4();

        assert!(
            require_owned_record(current_user_id, GlobalRole::User, Some(current_user_id)).is_ok()
        );
        assert!(
            require_owned_record(current_user_id, GlobalRole::User, Some(Uuid::new_v4())).is_err()
        );
        assert!(require_owned_record(current_user_id, GlobalRole::User, None).is_err());
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
}
