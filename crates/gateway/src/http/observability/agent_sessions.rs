use super::*;
use gateway_core::{AgentSessionReportRepository, AgentSessionTraceRepository};

mod presenter;

use presenter::*;

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
    let access = agent_analysis_access(&state, scope);
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
            .map(|trace| agent_session_summary(trace, access.score_visible))
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
    let access = agent_analysis_access(&state, scope);
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
    let report = trace
        .latest_analysis
        .as_ref()
        .map(|analysis| agent_session_efficiency_report(&analysis.report, access.score_visible));
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
        session: agent_session_summary(&trace, access.score_visible),
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

fn agent_analysis_access(
    state: &AppState,
    scope: AdminDataScope,
) -> crate::config::AgentAnalysisAccessDecision {
    match scope {
        AdminDataScope::Platform => state.agent_analysis.access_for(true, false),
        AdminDataScope::Team(_) => state.agent_analysis.access_for(false, true),
    }
}
