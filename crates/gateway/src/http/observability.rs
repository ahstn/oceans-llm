use std::collections::{BTreeMap, HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    AdminApiKeyRepository, AgentSessionAnalysisRepository, AgentSessionListQuery,
    AgentSessionSourceRecord, AgentSessionTraceRecord, AuthError, BudgetRepository, Confidence,
    GatewayError, GatewayOutcomeState, GlobalRole, IdentityRepository,
    MAX_MCP_TOOL_INVOCATION_PAGE_SIZE, MAX_REQUEST_LOG_PAGE_SIZE, McpTokenOverheadRepository,
    McpToolInvocationDetail, McpToolInvocationPayloadRecord, McpToolInvocationQuery,
    McpToolInvocationRecord, McpToolInvocationStatus, McpToolPolicyResult, ProviderConnection,
    ProviderRepository, RequestAttemptRecord, RequestLogDetail, RequestLogPayloadRecord,
    RequestLogQuery, RequestLogRecord, RequestLogRepository, RequestMcpTokenOverheadRecord,
    RequestTag, RequestTags, ScoreMaturity, SessionLifecycleState,
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
        AdminDataScope, require_agent_analysis_scope, require_authenticated_session,
        require_platform_admin,
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
        LeaderboardChartUserView, LeaderboardLeaderView, LeaderboardQuery,
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
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;
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
    require_platform_admin(&state, &headers).await?;

    let range = parse_leaderboard_range(query.range.as_deref())?;
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
        })
        .collect();

    Ok(Json(envelope(HarnessUsageView {
        range: range.as_str().to_string(),
        bucket_hours: LEADERBOARD_BUCKET_HOURS,
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        chart_harnesses,
        series,
        leaders,
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/agent-sessions",
    params(AgentSessionListRequestQuery),
    responses(
        (status = 200, body = Envelope<AgentSessionPageView>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 403, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_agent_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AgentSessionListRequestQuery>,
) -> Result<Json<Envelope<AgentSessionPageView>>, AppError> {
    let scope = require_agent_analysis_scope(&state, &headers).await?;
    let requested_team_id = parse_optional_uuid(query.team_id.as_deref(), "team_id")?;
    let team_id = match scope {
        AdminDataScope::Platform => requested_team_id,
        AdminDataScope::Team(team_id) => {
            if requested_team_id.is_some_and(|requested| requested != team_id) {
                return Err(AppError(GatewayError::Auth(
                    AuthError::InsufficientPrivileges,
                )));
            }
            Some(team_id)
        }
    };
    let lifecycle = match query.lifecycle.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("open") => Some(SessionLifecycleState::Open),
        Some("finalized") => Some(SessionLifecycleState::Finalized),
        Some(value) => {
            return Err(AppError(GatewayError::InvalidRequest(format!(
                "unsupported lifecycle `{value}`"
            ))));
        }
    };
    let page_number = query.page.unwrap_or(DEFAULT_PAGE);
    let page_size = query.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if page_number == 0 || !(1..=gateway_core::MAX_AGENT_SESSION_PAGE_SIZE).contains(&page_size) {
        return Err(AppError(GatewayError::InvalidRequest(format!(
            "page must be at least 1 and page_size must be between 1 and {}",
            gateway_core::MAX_AGENT_SESSION_PAGE_SIZE
        ))));
    }
    let started_after = parse_optional_timestamp(query.started_after.as_deref(), "started_after")?;
    let started_before =
        parse_optional_timestamp(query.started_before.as_deref(), "started_before")?;
    if started_after
        .zip(started_before)
        .is_some_and(|(after, before)| after >= before)
    {
        return Err(AppError(GatewayError::InvalidRequest(
            "started_after must be earlier than started_before".to_string(),
        )));
    }
    if query
        .minimum_coverage_percent
        .is_some_and(|value| value > 100)
    {
        return Err(AppError(GatewayError::InvalidRequest(
            "minimum_coverage_percent must be between 0 and 100".to_string(),
        )));
    }
    let request_tag_key = normalized_filter(query.request_tag_key);
    let request_tag_value = normalized_filter(query.request_tag_value);
    if request_tag_key.is_none() && request_tag_value.is_some() {
        return Err(AppError(GatewayError::InvalidRequest(
            "request_tag_key is required when request_tag_value is provided".to_string(),
        )));
    }
    let gateway_outcome = match query.gateway_outcome.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("succeeded") => Some(GatewayOutcomeState::Succeeded),
        Some("partial") => Some(GatewayOutcomeState::Partial),
        Some("failed") => Some(GatewayOutcomeState::Failed),
        Some("unknown") => Some(GatewayOutcomeState::Unknown),
        Some(value) => {
            return Err(AppError(GatewayError::InvalidRequest(format!(
                "unsupported gateway outcome `{value}`"
            ))));
        }
    };
    let score_maturity = match query.score_maturity.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("experimental") => Some(ScoreMaturity::Experimental),
        Some("calibrated") => Some(ScoreMaturity::Calibrated),
        Some(value) => {
            return Err(AppError(GatewayError::InvalidRequest(format!(
                "unsupported score maturity `{value}`"
            ))));
        }
    };
    let page = state
        .store
        .list_agent_sessions(&AgentSessionListQuery {
            page: page_number,
            page_size,
            ownership_scope_key: None,
            agent_session_source_id: parse_optional_uuid(
                query.session_source_id.as_deref(),
                "session_source_id",
            )?,
            user_id: parse_optional_uuid(query.user_id.as_deref(), "user_id")?,
            team_id,
            service_account_id: parse_optional_uuid(
                query.service_account_id.as_deref(),
                "service_account_id",
            )?,
            harness_key: normalized_filter(query.harness_key),
            requested_model_key: normalized_filter(query.requested_model_key),
            operation: normalized_filter(query.operation),
            caller_class: normalized_filter(query.caller_class),
            gateway_outcome,
            score_maturity,
            minimum_coverage_percent: query.minimum_coverage_percent,
            normalized_session_id: normalized_filter(query.external_session_id),
            request_tag_key,
            request_tag_value,
            lifecycle,
            started_after,
            started_before,
            input_watermark_before: None,
            score_confidence: match query.score_confidence.as_deref().map(str::trim) {
                None | Some("") => None,
                Some("low") => Some(Confidence::Low),
                Some("medium") => Some(Confidence::Medium),
                Some("high") => Some(Confidence::High),
                Some(value) => {
                    return Err(AppError(GatewayError::InvalidRequest(format!(
                        "unsupported score confidence `{value}`"
                    ))));
                }
            },
        })
        .await?;
    Ok(Json(envelope(AgentSessionPageView {
        items: page
            .items
            .iter()
            .map(|trace| {
                agent_session_summary(trace, state.agent_analysis.calibrated_score_visible)
            })
            .collect(),
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/agent-sessions/{session_id}",
    params(("session_id" = Uuid, Path, description = "Agent session identifier")),
    responses(
        (status = 200, body = Envelope<AgentSessionDetailView>),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 403, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn get_agent_session_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
) -> Result<Json<Envelope<AgentSessionDetailView>>, AppError> {
    let scope = require_agent_analysis_scope(&state, &headers).await?;
    let trace = state
        .store
        .load_agent_session_trace(session_id)
        .await?
        .ok_or_else(|| {
            AppError(GatewayError::Store(gateway_core::StoreError::NotFound(
                "agent session not found".to_string(),
            )))
        })?;
    authorize_agent_analysis_owner(
        &state,
        scope,
        trace.session.team_id,
        trace.session.user_id,
        trace.session.service_account_id,
    )
    .await?;
    let observation_sets = state.store.load_agent_observation_sets(session_id).await?;
    let observation_count = observation_sets
        .iter()
        .map(|set| set.observations.len())
        .sum::<usize>();
    let mut observation_history_truncated = observation_count
        > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize
        || observation_sets.len() > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize;
    let mut supplied_tool_budget = gateway_core::MAX_AGENT_SESSION_NESTED_FACTS;
    let mut supplied_skill_budget = gateway_core::MAX_AGENT_SESSION_NESTED_FACTS;
    let mut file_interaction_budget = gateway_core::MAX_AGENT_SESSION_NESTED_FACTS;
    let observations = observation_sets
        .iter()
        .flat_map(|set| set.observations.iter())
        .take(gateway_core::MAX_AGENT_SESSION_REQUESTS as usize)
        .map(|observation| {
            let supplied_tool_count = observation
                .facts
                .supplied_tools
                .len()
                .min(supplied_tool_budget);
            let supplied_skill_count = observation
                .facts
                .supplied_skills
                .len()
                .min(supplied_skill_budget);
            let file_interaction_count = observation
                .facts
                .file_interactions
                .len()
                .min(file_interaction_budget);
            observation_history_truncated |= supplied_tool_count
                < observation.facts.supplied_tools.len()
                || supplied_skill_count < observation.facts.supplied_skills.len()
                || file_interaction_count < observation.facts.file_interactions.len();
            supplied_tool_budget -= supplied_tool_count;
            supplied_skill_budget -= supplied_skill_count;
            file_interaction_budget -= file_interaction_count;
            AgentObservationView {
                observation_id: observation.observation_id.to_string(),
                kind: enum_name(observation.kind),
                source_request_id: observation.source_request_id.clone(),
                parser_version: observation.parser_version.clone(),
                evidence: enum_name(observation.evidence),
                occurred_at: format_timestamp(observation.occurred_at),
                facts: AgentObservationFactsView {
                    message_count: observation.facts.message_count,
                    prompt_bytes: observation.facts.prompt_bytes,
                    supplied_tool_count: observation.facts.supplied_tool_count,
                    tool_schema_bytes: observation.facts.tool_schema_bytes,
                    tool_schema_token_estimate: observation.facts.tool_schema_token_estimate,
                    supplied_tools: observation
                        .facts
                        .supplied_tools
                        .iter()
                        .take(supplied_tool_count)
                        .map(|tool| AgentSuppliedToolFactView {
                            name: tool.name.clone(),
                            server_key: tool.server_key.clone(),
                            token_estimate: tool.token_estimate,
                        })
                        .collect(),
                    supplied_skills: observation
                        .facts
                        .supplied_skills
                        .iter()
                        .take(supplied_skill_count)
                        .map(|skill| AgentSuppliedSkillFactView {
                            name: skill.name.clone(),
                            description_token_estimate: skill.description_token_estimate,
                            body_token_estimate: skill.body_token_estimate,
                            resource_token_estimate: skill.resource_token_estimate,
                            used: skill.used,
                            abandoned: skill.abandoned,
                        })
                        .collect(),
                    file_interactions: observation
                        .facts
                        .file_interactions
                        .iter()
                        .take(file_interaction_count)
                        .map(|file| AgentFileInteractionFactView {
                            opaque_file_id: file.opaque_file_id.clone(),
                            operation: file.operation.clone(),
                            tool_name: file.tool_name.clone(),
                            succeeded: file.succeeded,
                            error_signature: file.error_signature.clone(),
                        })
                        .collect(),
                    reasoning_config_hash: observation.facts.reasoning_config_hash.clone(),
                    cache_requested: observation.facts.cache_requested,
                    finish_reason: observation.facts.finish_reason.clone(),
                    incomplete_reason: observation.facts.incomplete_reason.clone(),
                    tool_name: observation.facts.tool_name.clone(),
                    tool_schema_hash: observation.facts.tool_schema_hash.clone(),
                    opaque_file_id: observation.facts.opaque_file_id.clone(),
                    file_kind: observation.facts.file_kind.clone(),
                    result_bytes: observation.facts.result_bytes,
                    error_signature: observation.facts.error_signature.clone(),
                    attributes: Value::Object(observation.facts.attributes.clone()),
                },
                limitations: observation
                    .limitations
                    .iter()
                    .copied()
                    .map(enum_name)
                    .collect(),
            }
        })
        .collect();
    let requests = trace
        .requests
        .iter()
        .take(gateway_core::MAX_AGENT_SESSION_REQUESTS as usize)
        .map(|request| AgentSessionRequestView {
            request_id: request.request_id.clone(),
            request_log_id: request.request_log_id.map(|value| value.to_string()),
            usage_event_id: request.usage_event_id.map(|value| value.to_string()),
            ordinal: request.ordinal,
            execution_id: request.execution_id.clone(),
            parent_execution_id: request.parent_execution_id.clone(),
            correlation_confidence: enum_name(request.correlation_confidence),
            limitation_codes: request
                .limitation_codes
                .iter()
                .copied()
                .map(enum_name)
                .collect(),
            occurred_at: format_timestamp(request.occurred_at),
            completed_at: request.completed_at.map(format_timestamp),
            terminal_success: request.terminal_success,
        })
        .collect();
    let analysis = trace
        .latest_analysis
        .as_ref()
        .map(agent_session_analysis_identity);
    let report = trace.latest_analysis.as_ref().map(|analysis| {
        agent_session_efficiency_report(
            &analysis.report,
            state.agent_analysis.calibrated_score_visible,
        )
    });
    let coverage = observation_sets
        .last()
        .map(|_| AgentObservationCoverageView {
            request_metadata: observation_sets
                .iter()
                .any(|set| coverage_flag(&set.coverage, "request_metadata")),
            response_payload: observation_sets
                .iter()
                .any(|set| coverage_flag(&set.coverage, "response_payload")),
            response_payload_truncated: observation_sets
                .iter()
                .any(|set| coverage_flag(&set.coverage, "response_payload_truncated")),
        });
    let session_source = trace.session_source.as_ref().map(agent_session_source_view);
    Ok(Json(envelope(AgentSessionDetailView {
        session: agent_session_summary(&trace, state.agent_analysis.calibrated_score_visible),
        session_source,
        requests,
        request_history_truncated: trace.requests.len()
            > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize,
        observation_history_truncated,
        observations,
        analysis,
        report,
        coverage,
    })))
}

async fn authorize_agent_analysis_owner(
    state: &AppState,
    scope: AdminDataScope,
    owner_team_id: Option<Uuid>,
    user_id: Option<Uuid>,
    service_account_id: Option<Uuid>,
) -> Result<(), AppError> {
    let AdminDataScope::Team(team_id) = scope else {
        return Ok(());
    };
    let authorized = if let Some(owner_team_id) = owner_team_id {
        owner_team_id == team_id
    } else if let Some(user_id) = user_id {
        state
            .store
            .get_team_membership_for_user(user_id)
            .await?
            .is_some_and(|membership| membership.team_id == team_id)
    } else if let Some(service_account_id) = service_account_id {
        state
            .store
            .get_service_account_by_id(service_account_id)
            .await?
            .is_some_and(|service_account| service_account.team_id == team_id)
    } else {
        false
    };
    if !authorized {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
        )));
    }
    Ok(())
}

fn agent_session_analysis_identity(
    analysis: &gateway_core::AgentSessionAnalysisRecord,
) -> AgentSessionAnalysisIdentityView {
    AgentSessionAnalysisIdentityView {
        analysis_id: analysis.analysis_id.to_string(),
        input_watermark_at: format_timestamp(analysis.input_watermark_at),
        observation_set_id: analysis.observation_set_id.to_string(),
        boundary_policy_version: analysis.boundary_policy_version.clone(),
        observation_parser_version: analysis.observation_parser_version.clone(),
        pricing_policy_version: analysis.pricing_policy_version.clone(),
        cohort_version: analysis.cohort_version.clone(),
        cohort_fallback_level: analysis.cohort_fallback_level,
        cohort_sample_size: analysis.cohort_sample_size,
        cohort_snapshot_digest: analysis.cohort_snapshot_digest.clone(),
        analyzed_at: format_timestamp(analysis.analyzed_at),
        expires_at: format_timestamp(analysis.expires_at),
    }
}

fn agent_session_efficiency_report(
    report: &gateway_core::SessionEfficiencyReport,
    calibrated_score_visible: bool,
) -> AgentSessionEfficiencyReportView {
    let components = &report.components;
    let diagnostics = &report.diagnostics;
    let score_visible = calibrated_score_visible
        && report.maturity == ScoreMaturity::Calibrated
        && report
            .calibration_approval_id
            .as_deref()
            .is_some_and(|value| !value.is_empty());
    AgentSessionEfficiencyReportView {
        report_schema_version: report.report_schema_version.clone(),
        analyzer_version: report.analyzer_version.clone(),
        score_policy_version: report.score_policy_version.clone(),
        observation_parser_version: report.observation_parser_version.clone(),
        calibration_approval_id: report.calibration_approval_id.clone(),
        configuration_version: report.configuration_version.clone(),
        maturity: enum_name(report.maturity),
        confidence: enum_name(report.confidence),
        gateway_outcome: enum_name(report.gateway_outcome),
        score: score_visible.then_some(report.score).flatten(),
        coverage: AgentTelemetryCoverageView {
            outcome_percent: report.coverage.outcome_percent,
            cost_percent: report.coverage.cost_percent,
            timing_percent: report.coverage.timing_percent,
            payload_percent: report.coverage.payload_percent,
            cohort_percent: report.coverage.cohort_percent,
            overall_percent: report.coverage.overall_percent,
        },
        components: AgentSessionEfficiencyComponentsView {
            outcome: AgentSessionOutcomeView {
                state: enum_name(components.outcome.state),
                factor_basis_points: components.outcome.factor_basis_points,
                successful_requests: components.outcome.successful_requests,
                determinate_requests: components.outcome.determinate_requests,
                incomplete_requests: components.outcome.incomplete_requests,
            },
            cost_efficiency_basis_points: components.cost_efficiency_basis_points,
            active_time_efficiency_basis_points: components.active_time_efficiency_basis_points,
            actual_cost_10000: components.actual_cost_10000,
            active_time_ms: components.active_time_ms,
            wall_time_ms: components.wall_time_ms,
            summed_work_time_ms: components.summed_work_time_ms,
            excluded_gap_time_ms: components.excluded_gap_time_ms,
            overlap_savings_ms: components.overlap_savings_ms,
            unknown_wait_time_ms: components.unknown_wait_time_ms,
            cohort_version: components.cohort_version.clone(),
            cohort_fallback_level: components.cohort_fallback_level,
            cohort_sample_size: components.cohort_sample_size,
        },
        diagnostics: AgentSessionDiagnosticsView {
            token_and_cache: AgentTokenAndCacheDiagnosticsView {
                fresh_input_tokens: diagnostics.token_and_cache.fresh_input_tokens,
                cache_read_tokens: diagnostics.token_and_cache.cache_read_tokens,
                cache_creation_tokens: diagnostics.token_and_cache.cache_creation_tokens,
                total_input_tokens: diagnostics.token_and_cache.total_input_tokens,
                output_tokens: diagnostics.token_and_cache.output_tokens,
                reasoning_tokens: diagnostics.token_and_cache.reasoning_tokens,
                visible_output_tokens: diagnostics.token_and_cache.visible_output_tokens,
                cache_creation_5m_tokens: diagnostics.token_and_cache.cache_creation_5m_tokens,
                cache_creation_30m_tokens: diagnostics.token_and_cache.cache_creation_30m_tokens,
                cache_creation_1h_tokens: diagnostics.token_and_cache.cache_creation_1h_tokens,
                provider_total_tokens: diagnostics.token_and_cache.provider_total_tokens,
                legacy_cost_10000: diagnostics.token_and_cache.legacy_cost_10000,
                normalized_cost_10000: diagnostics.token_and_cache.normalized_cost_10000,
                cache_read_cost_10000: diagnostics.token_and_cache.cache_read_cost_10000,
                cache_creation_cost_10000: diagnostics.token_and_cache.cache_creation_cost_10000,
                uncached_input_cost_10000: diagnostics.token_and_cache.uncached_input_cost_10000,
                cache_savings_10000: diagnostics.token_and_cache.cache_savings_10000,
                cache_savings_basis_points: diagnostics.token_and_cache.cache_savings_basis_points,
                cache_read_write_ratio_basis_points: diagnostics
                    .token_and_cache
                    .cache_read_write_ratio_basis_points,
                cache_write_amplification_basis_points: diagnostics
                    .token_and_cache
                    .cache_write_amplification_basis_points,
                silent_cache_threshold_miss_requests: diagnostics
                    .token_and_cache
                    .silent_cache_threshold_miss_requests,
                cache_key_switches: diagnostics.token_and_cache.cache_key_switches,
                reasoning_config_switches: diagnostics.token_and_cache.reasoning_config_switches,
                pricing_policy_versions: diagnostics
                    .token_and_cache
                    .pricing_policy_versions
                    .clone(),
            },
            context: AgentContextDiagnosticsView {
                initial_prompt_tokens: diagnostics.context.initial_prompt_tokens,
                median_prompt_tokens: diagnostics.context.median_prompt_tokens,
                p90_prompt_tokens: diagnostics.context.p90_prompt_tokens,
                maximum_prompt_tokens: diagnostics.context.maximum_prompt_tokens,
                input_boundary_tokens: diagnostics.context.input_boundary_tokens,
                reserved_output_tokens: diagnostics.context.reserved_output_tokens,
                peak_input_utilization_basis_points: diagnostics
                    .context
                    .peak_input_utilization_basis_points,
                requests_over_input_boundary: diagnostics.context.requests_over_input_boundary,
                repeated_requests_over_input_boundary: diagnostics
                    .context
                    .repeated_requests_over_input_boundary,
                score_penalty_points: diagnostics.context.score_penalty_points,
                prompt_growth_per_turn: diagnostics.context.prompt_growth_per_turn,
                prompt_growth_per_active_minute: diagnostics
                    .context
                    .prompt_growth_per_active_minute,
                suspected_compactions: diagnostics.context.suspected_compactions,
                suspected_context_resets: diagnostics.context.suspected_context_resets,
            },
            tools_and_changes: AgentToolAndChangeDiagnosticsView {
                supplied_tool_definitions: diagnostics.tools_and_changes.supplied_tool_definitions,
                supplied_tool_schema_bytes: diagnostics
                    .tools_and_changes
                    .supplied_tool_schema_bytes,
                observed_tool_calls: diagnostics.tools_and_changes.observed_tool_calls,
                classified_tool_calls: diagnostics.tools_and_changes.classified_tool_calls,
                file_reads_suspected: diagnostics.tools_and_changes.file_reads_suspected,
                file_searches_suspected: diagnostics.tools_and_changes.file_searches_suspected,
                file_creates_suspected: diagnostics.tools_and_changes.file_creates_suspected,
                file_edits_suspected: diagnostics.tools_and_changes.file_edits_suspected,
                file_overwrites_suspected: diagnostics.tools_and_changes.file_overwrites_suspected,
                unique_opaque_files: diagnostics.tools_and_changes.unique_opaque_files,
                verification_results_classified: diagnostics
                    .tools_and_changes
                    .verification_results_classified,
                rework_spans_suspected: diagnostics.tools_and_changes.rework_spans_suspected,
                direct_mcp_calls: diagnostics.tools_and_changes.direct_mcp_calls,
                direct_mcp_duration_ms: diagnostics.tools_and_changes.direct_mcp_duration_ms,
                tool_servers: diagnostics
                    .tools_and_changes
                    .tool_servers
                    .iter()
                    .map(|server| AgentToolServerDiagnosticsView {
                        server_key: server.server_key.clone(),
                        exposed_tool_definitions: server.exposed_tool_definitions,
                        invoked_tool_definitions: server.invoked_tool_definitions,
                        invocation_count: server.invocation_count,
                        failed_count: server.failed_count,
                        schema_token_estimate_per_request: server.schema_token_estimate_per_request,
                        estimated_uncached_schema_cost_10000: server
                            .estimated_uncached_schema_cost_10000,
                    })
                    .collect(),
            },
            skills: AgentSkillDiagnosticsView {
                instrumented_request_count: diagnostics.skills.instrumented_request_count,
                available_skill_count: diagnostics.skills.available_skill_count,
                used_skill_count: diagnostics.skills.used_skill_count,
                unused_skill_count: diagnostics.skills.unused_skill_count,
                description_tokens_per_request: diagnostics.skills.description_tokens_per_request,
                loaded_body_tokens: diagnostics.skills.loaded_body_tokens,
                loaded_resource_tokens: diagnostics.skills.loaded_resource_tokens,
                items: diagnostics
                    .skills
                    .items
                    .iter()
                    .map(|item| AgentSkillDiagnosticItemView {
                        name: item.name.clone(),
                        available_request_count: item.available_request_count,
                        used_request_count: item.used_request_count,
                        abandoned_request_count: item.abandoned_request_count,
                        description_token_estimate: item.description_token_estimate,
                        loaded_body_tokens: item.loaded_body_tokens,
                        loaded_resource_tokens: item.loaded_resource_tokens,
                    })
                    .collect(),
            },
            reliability: AgentReliabilityDiagnosticsView {
                attempt_coverage_percent: diagnostics.reliability.attempt_coverage_percent,
                total_attempts: diagnostics.reliability.total_attempts,
                wasted_attempts: diagnostics.reliability.wasted_attempts,
                wasted_attempt_latency_ms: diagnostics.reliability.wasted_attempt_latency_ms,
                wasted_attempt_cost_10000: diagnostics.reliability.wasted_attempt_cost_10000,
                tool_invocations: diagnostics.reliability.tool_invocations,
                failed_tool_invocations: diagnostics.reliability.failed_tool_invocations,
                truncated_tool_results: diagnostics.reliability.truncated_tool_results,
                attempts: diagnostics
                    .reliability
                    .attempts
                    .iter()
                    .map(|attempt| AgentRequestAttemptView {
                        request_id: attempt.request_id.clone(),
                        attempt_number: attempt.attempt_number,
                        produced_final_response: attempt.produced_final_response,
                        retryable: attempt.retryable,
                        status: attempt.status.clone(),
                        status_code: attempt.status_code,
                        error_code: attempt.error_code.clone(),
                        latency_ms: attempt.latency_ms,
                        provider_key: attempt.provider_key.clone(),
                        upstream_model: attempt.upstream_model.clone(),
                        occurred_at_unix_ms: attempt.occurred_at_unix_ms,
                    })
                    .collect(),
                tools: diagnostics
                    .reliability
                    .tools
                    .iter()
                    .map(|tool| AgentToolReliabilityItemView {
                        server_key: tool.server_key.clone(),
                        tool_key: tool.tool_key.clone(),
                        invocation_count: tool.invocation_count,
                        failed_count: tool.failed_count,
                        truncated_result_count: tool.truncated_result_count,
                        latency_ms: tool.latency_ms,
                        post_error_input_tokens: tool.post_error_input_tokens,
                    })
                    .collect(),
            },
            outcome: AgentOutcomeDiagnosticsView {
                file_signal_coverage_percent: diagnostics.outcome.file_signal_coverage_percent,
                cost_per_file_touched_10000: diagnostics.outcome.cost_per_file_touched_10000,
                cost_per_successful_session_10000: diagnostics
                    .outcome
                    .cost_per_successful_session_10000,
                rework_ratio_basis_points: diagnostics.outcome.rework_ratio_basis_points,
                verification_rate_basis_points: diagnostics.outcome.verification_rate_basis_points,
                zero_outcome: diagnostics.outcome.zero_outcome,
                repeated_file_interactions_suspected: diagnostics
                    .outcome
                    .repeated_file_interactions_suspected,
                files_with_repeated_interactions_suspected: diagnostics
                    .outcome
                    .files_with_repeated_interactions_suspected,
                failed_file_interactions: diagnostics.outcome.failed_file_interactions,
            },
            finish_reasons: AgentFinishReasonDiagnosticsView {
                instrumented_request_count: diagnostics.finish_reasons.instrumented_request_count,
                length_limited_requests: diagnostics.finish_reasons.length_limited_requests,
                items: diagnostics
                    .finish_reasons
                    .items
                    .iter()
                    .map(|item| AgentFinishReasonItemView {
                        reason: item.reason.clone(),
                        count: item.count,
                    })
                    .collect(),
            },
            enabled_metrics: AgentAnalysisMetricPolicyView {
                token_metrics: diagnostics.enabled_metrics.token_metrics,
                cache_metrics: diagnostics.enabled_metrics.cache_metrics,
                context_metrics: diagnostics.enabled_metrics.context_metrics,
                tool_metrics: diagnostics.enabled_metrics.tool_metrics,
                skill_metrics: diagnostics.enabled_metrics.skill_metrics,
                reliability_metrics: diagnostics.enabled_metrics.reliability_metrics,
                outcome_metrics: diagnostics.enabled_metrics.outcome_metrics,
                finish_reason_metrics: diagnostics.enabled_metrics.finish_reason_metrics,
            },
            semantic_verification_available: diagnostics.semantic_verification_available,
        },
        limitations: report.limitations.iter().copied().map(enum_name).collect(),
    }
}

fn coverage_flag(coverage: &Value, field: &str) -> bool {
    coverage
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
fn agent_session_source_view(session: &AgentSessionSourceRecord) -> AgentSessionSourceView {
    AgentSessionSourceView {
        session_source_id: session.agent_session_source_id.to_string(),
        external_session_id: session.normalized_session_id.clone(),
        adapter_namespace: session.adapter_namespace.clone(),
        adapter_version: session.adapter_version.clone(),
        source_provenance: session.source_provenance.clone(),
        harness_key: session.harness_key.clone(),
        harness_label: session.harness_label.clone(),
        first_seen_at: format_timestamp(session.first_seen_at),
        last_seen_at: format_timestamp(session.last_seen_at),
    }
}

fn agent_session_summary(
    trace: &AgentSessionTraceRecord,
    calibrated_score_visible: bool,
) -> AgentSessionSummaryView {
    let analysis = trace.latest_analysis.as_ref();
    let report = analysis.map(|analysis| &analysis.report);
    let score_visible = report.is_some_and(|report| {
        calibrated_score_visible
            && report.maturity == ScoreMaturity::Calibrated
            && report
                .calibration_approval_id
                .as_deref()
                .is_some_and(|value| !value.is_empty())
    });
    AgentSessionSummaryView {
        session_id: trace.session.agent_session_id.to_string(),
        session_source_id: trace
            .session
            .agent_session_source_id
            .map(|value| value.to_string()),
        external_session_id: trace
            .session_source
            .as_ref()
            .map(|source| source.normalized_session_id.clone()),
        ownership_scope_key: trace.session.ownership_scope_key.clone(),
        user_id: trace.session.user_id.map(|value| value.to_string()),
        team_id: trace.session.team_id.map(|value| value.to_string()),
        service_account_id: trace
            .session
            .service_account_id
            .map(|value| value.to_string()),
        harness_key: Some(trace.session.harness_key.clone()),
        harness_label: trace.session_source.as_ref().map_or_else(
            || Some(trace.session.harness_key.clone()),
            |source| Some(source.harness_label.clone()),
        ),
        requested_model_key: trace.session.requested_model_key.clone(),
        operation: trace.session.operation.clone(),
        caller_class: trace.session.caller_class.clone(),
        session_source_observed: trace.session.agent_session_source_id.is_some(),
        lifecycle: enum_name(trace.session.lifecycle),
        boundary_confidence: enum_name(trace.session.boundary_confidence),
        started_at: format_timestamp(trace.session.started_at),
        ended_at: trace.session.ended_at.map(format_timestamp),
        request_count: u64::try_from(trace.requests.len()).unwrap_or(u64::MAX),
        tool_call_count: report
            .map(|report| report.diagnostics.tools_and_changes.observed_tool_calls),
        mcp_call_count: report.map(|report| report.diagnostics.tools_and_changes.direct_mcp_calls),
        efficiency_score: score_visible
            .then(|| report.and_then(|report| report.score))
            .flatten(),
        score_confidence: report.map(|report| enum_name(report.confidence)),
        score_maturity: report.map(|report| enum_name(report.maturity)),
        gateway_outcome: report.map(|report| enum_name(report.gateway_outcome)),
        telemetry_coverage_percent: report.map(|report| report.coverage.overall_percent),
        cohort_version: report.and_then(|report| report.components.cohort_version.clone()),
        cohort_fallback_level: report.and_then(|report| report.components.cohort_fallback_level),
        cohort_sample_size: report.map(|report| report.components.cohort_sample_size),
        calibration_approval_id: report.and_then(|report| report.calibration_approval_id.clone()),
        normalized_cost_usd: report
            .and_then(|report| report.components.actual_cost_10000)
            .map(|value| value as f64 / 10_000.0),
        active_time_ms: report
            .and_then(|report| u64::try_from(report.components.active_time_ms).ok()),
        wall_time_ms: report.and_then(|report| u64::try_from(report.components.wall_time_ms).ok()),
        report_schema_version: report.map(|report| report.report_schema_version.clone()),
        analyzer_version: report.map(|report| report.analyzer_version.clone()),
        score_policy_version: report.map(|report| report.score_policy_version.clone()),
        pricing_policy_version: analysis.map(|analysis| analysis.pricing_policy_version.clone()),
        limitations: report
            .map(|report| report.limitations.iter().copied().map(enum_name).collect())
            .unwrap_or_default(),
    }
}

fn enum_name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
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
    let current_user = require_authenticated_session(&state, &headers).await?;

    let request_log_query = RequestLogQuery {
        page: query.page.unwrap_or(DEFAULT_PAGE).max(1),
        page_size: query
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_REQUEST_LOG_PAGE_SIZE),
        request_id: empty_to_none(query.request_id),
        model_key: empty_to_none(query.model_key),
        provider_key: empty_to_none(query.provider_key),
        status_code: query.status_code,
        user_id: scoped_user_id(
            current_user.user_id,
            current_user.global_role,
            parse_optional_uuid(query.user_id.as_deref(), "user_id")?,
        ),
        team_id: parse_optional_uuid(query.team_id.as_deref(), "team_id")?,
        service_account_id: parse_optional_uuid(
            query.service_account_id.as_deref(),
            "service_account_id",
        )?,
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
    let callers = request_caller_directory(&state, &page.items).await?;
    let items = page
        .items
        .iter()
        .map(|log| summary_view(log, providers.get(log.provider_key.as_str()), &callers))
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
    let current_user = require_authenticated_session(&state, &headers).await?;

    let detail = state.service.get_request_log_detail(request_log_id).await?;
    require_owned_record(
        current_user.user_id,
        current_user.global_role,
        detail.log.user_id,
    )?;
    let provider = provider_connection(&state, detail.log.provider_key.as_str()).await?;
    let callers = request_caller_directory(&state, std::slice::from_ref(&detail.log)).await?;
    let mcp_token_overhead = state
        .store
        .get_request_mcp_token_overhead(&detail.log.request_id)
        .await?;
    Ok(Json(envelope(detail_view(
        detail,
        provider.as_ref(),
        &callers,
        mcp_token_overhead,
    )?)))
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

/// Display names for the api keys, users, and service accounts referenced by
/// a page of request logs, resolved once per distinct id. Missing entries are
/// expected: callers may have been deleted after the log was recorded.
#[derive(Debug, Default)]
struct RequestCallerDirectory {
    api_key_names: HashMap<Uuid, String>,
    users: HashMap<Uuid, (String, String)>,
    service_account_names: HashMap<Uuid, String>,
}

async fn request_caller_directory(
    state: &AppState,
    logs: &[RequestLogRecord],
) -> Result<RequestCallerDirectory, AppError> {
    let mut directory = RequestCallerDirectory::default();

    let api_key_ids: HashSet<Uuid> = logs.iter().map(|log| log.api_key_id).collect();
    for api_key_id in api_key_ids {
        if let Some(api_key) = state.store.get_api_key_by_id(api_key_id).await? {
            directory.api_key_names.insert(api_key_id, api_key.name);
        }
    }

    let user_ids: HashSet<Uuid> = logs.iter().filter_map(|log| log.user_id).collect();
    for user_id in user_ids {
        if let Some(identity_user) = state.store.get_identity_user(user_id).await? {
            directory
                .users
                .insert(user_id, (identity_user.user.name, identity_user.user.email));
        }
    }

    let service_account_ids: HashSet<Uuid> = logs
        .iter()
        .filter_map(|log| log.service_account_id)
        .collect();
    for service_account_id in service_account_ids {
        if let Some(service_account) = state
            .store
            .get_service_account_by_id(service_account_id)
            .await?
        {
            directory
                .service_account_names
                .insert(service_account_id, service_account.service_account_name);
        }
    }

    Ok(directory)
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

fn summary_view(
    log: &RequestLogRecord,
    provider: Option<&ProviderConnection>,
    callers: &RequestCallerDirectory,
) -> Result<RequestLogSummaryView, AppError> {
    let provider_icon_key = provider_icon_key_from_metadata(&log.metadata)
        .or_else(|| Some(resolve_provider_display(log.provider_key.as_str(), provider).icon_key))
        .map(Into::into);
    let model_icon_key = model_icon_key_from_metadata(&log.metadata)
        .or_else(|| {
            resolve_model_icon_key([log.resolved_model_key.as_str(), log.model_key.as_str()])
        })
        .map(Into::into);

    let user = log.user_id.and_then(|user_id| callers.users.get(&user_id));

    Ok(RequestLogSummaryView {
        request_log_id: log.request_log_id.to_string(),
        request_id: log.request_id.clone(),
        api_key_id: log.api_key_id.to_string(),
        api_key_name: callers.api_key_names.get(&log.api_key_id).cloned(),
        user_id: log.user_id.map(|value| value.to_string()),
        user_name: user.map(|(name, _)| name.clone()),
        user_email: user.map(|(_, email)| email.clone()),
        team_id: log.team_id.map(|value| value.to_string()),
        service_account_id: log.service_account_id.map(|value| value.to_string()),
        service_account_name: log
            .service_account_id
            .and_then(|id| callers.service_account_names.get(&id).cloned()),
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
        agent_harness_key: log.agent_harness_key.clone(),
        agent_harness_label: log.agent_harness_label.clone(),
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
    callers: &RequestCallerDirectory,
    mcp_token_overhead: Option<RequestMcpTokenOverheadRecord>,
) -> Result<RequestLogDetailView, AppError> {
    Ok(RequestLogDetailView {
        log: summary_view(&detail.log, provider, callers)?,
        user_agent_raw: detail.log.user_agent_raw,
        payload: detail.payload.map(payload_view),
        attempts: detail.attempts.into_iter().map(attempt_view).collect(),
        mcp_token_overhead: mcp_token_overhead.map(mcp_token_overhead_view),
    })
}

fn mcp_token_overhead_view(overhead: RequestMcpTokenOverheadRecord) -> RequestMcpTokenOverheadView {
    RequestMcpTokenOverheadView {
        provider_family: overhead.provider_family,
        model_or_encoding: overhead.model_or_encoding,
        exposed_tool_count: overhead.exposed_tool_count,
        estimated_definition_tokens: overhead.estimated_definition_tokens,
        estimated_result_tokens: overhead.estimated_result_tokens,
        estimator_source: overhead.estimator_source.as_str().to_string(),
        confidence: overhead.confidence.as_str().to_string(),
        cache_hit_count: overhead.cache_hit_count,
        cache_miss_count: overhead.cache_miss_count,
        context_window_tokens: overhead.context_window_tokens,
        context_window_percent_bps: overhead.context_window_percent_bps,
        metadata: overhead.metadata,
    }
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

        let summary = summary_view(&log, Some(&provider), &RequestCallerDirectory::default())
            .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenRouter)
        ));
    }

    #[test]
    fn summary_view_falls_back_to_provider_key_when_provider_config_is_unavailable() {
        let log = request_log_record(payload_policy_metadata());

        let summary = summary_view(&log, None, &RequestCallerDirectory::default())
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

        let summary = summary_view(&log, Some(&provider), &RequestCallerDirectory::default())
            .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::Anthropic)
        ));
    }

    #[test]
    fn summary_view_requires_payload_policy_metadata() {
        let log = request_log_record(Map::new());

        let error = summary_view(&log, None, &RequestCallerDirectory::default())
            .expect_err("summary should fail");

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

        let error = summary_view(&log, None, &RequestCallerDirectory::default())
            .expect_err("summary should fail");

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

        let error = summary_view(&log, None, &RequestCallerDirectory::default())
            .expect_err("summary should fail");

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

        let error = summary_view(&log, None, &RequestCallerDirectory::default())
            .expect_err("summary should fail");

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

    fn request_log_record(metadata: Map<String, Value>) -> RequestLogRecord {
        RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req_123".to_string(),
            api_key_id: Uuid::new_v4(),
            user_id: None,
            team_id: None,
            service_account_id: None,
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
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
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
