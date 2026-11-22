use super::worker::analysis_versions;
use super::*;
use agent_session_analysis::{
    InferredObservation, ObservationSetId,
    cohort::{
        COHORT_LOOKBACK, CohortCandidate, CohortSelection, CohortSession, MAX_COHORT_SCAN_PAGES,
        NO_COHORT_FALLBACK_LEVEL, SelectedCohort, no_cohort_snapshot_digest,
    },
    observations::normalize_legacy_tool_inventory_limitations,
    versions::AnalysisVersions,
};
use gateway_core::McpToolInvocationBatchQuery;

pub(super) async fn generate_report<S>(
    store: &S,
    queue: &AgentAnalysisQueueRecord,
    now: OffsetDateTime,
    report_retention: Duration,
    configured_policy: &AnalysisPolicy,
) -> Result<(), GatewayError>
where
    S: AgentSessionAnalysisRepository
        + BudgetRepository
        + McpToolInvocationRepository
        + RequestLogRepository
        + Sync,
{
    let agent_session_id = queue.agent_session_id;
    let versions = &queue.desired_versions;
    let lease = gateway_core::AgentAnalysisLease {
        queue_item_id: queue.queue_item_id,
        owner: queue
            .lease_owner
            .as_deref()
            .ok_or_else(|| GatewayError::Internal("analysis job has no lease owner".to_string()))?,
        epoch: queue.lease_epoch,
    };
    ensure_supported_versions(versions)?;
    let trace = store
        .load_agent_session_trace(agent_session_id)
        .await?
        .ok_or_else(|| GatewayError::Internal("agent session disappeared".to_string()))?;
    if trace.session.boundary_policy_version != versions.boundary_policy_version {
        return Err(GatewayError::Internal(format!(
            "agent session boundary policy `{}` does not match queued version `{}`",
            trace.session.boundary_policy_version, versions.boundary_policy_version
        )));
    }
    if trace.requests.len() > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize {
        return Err(GatewayError::Internal(format!(
            "agent session `{agent_session_id}` exceeds the bounded request limit"
        )));
    }

    let Some(ReportObservations {
        observation_set_id,
        observations,
        request_metadata_count,
        response_payload_count,
        truncated_payload_count,
    }) = load_report_observations(store, &trace, &versions.observation_parser_version).await?
    else {
        return Ok(());
    };
    let RequestEvidence {
        requests,
        intervals,
    } = load_request_evidence(store, &trace, configured_policy).await?;

    let DirectMcpEvidence {
        intervals: direct_mcp_intervals,
        invocations: direct_tool_invocations,
        snapshot_digest: direct_mcp_snapshot_digest,
    } = load_direct_mcp_evidence(store, &trace.session, &trace.requests).await?;
    let cohort = load_successful_harness_cohort(store, &trace, versions).await?;
    let report = agent_session_analysis::analyze_session(
        &SessionTrace {
            requests,
            activity_intervals: intervals,
            observations,
            lifecycle: trace.session.lifecycle,
            boundary_confidence: trace.session.boundary_confidence,
            evidence: TraceEvidence {
                session_observed: trace.session.agent_session_source_id.is_some(),
                request_metadata_count,
                response_payload_count,
                truncated_payload_count,
                direct_mcp_intervals,
                tool_invocations: direct_tool_invocations,
            },
        },
        &AnalysisPolicy {
            report_schema_version: versions.report_schema_version.clone(),
            analyzer_version: versions.analyzer_version.clone(),
            score_policy_version: versions.score_policy_version.clone(),
            observation_parser_version: versions.observation_parser_version.clone(),
            configuration_version: versions.configuration_version.clone(),
            orchestration_gap: configured_policy.orchestration_gap,
            maturity: versions.score_maturity,
            calibration_approval_id: versions.calibration_approval_id.clone(),
            metrics: configured_policy.metrics.clone(),
            context_input_boundary_tokens: configured_policy.context_input_boundary_tokens,
            context_reserved_output_tokens: configured_policy.context_reserved_output_tokens,
            context_penalty_points_per_repeated_excess: configured_policy
                .context_penalty_points_per_repeated_excess,
            cache_profiles: configured_policy.cache_profiles.clone(),
        },
        cohort.as_ref().map(|value| &value.reference),
    )
    .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let analysis = AgentSessionAnalysisRecord {
        analysis_id: stable_uuid(
            ANALYSIS_ID_NAMESPACE,
            &json!({
                "session": agent_session_id,
                "watermark": trace.session.input_watermark_at.unix_timestamp_nanos(),
                "generation": trace.input_generation,
                "observation_set": observation_set_id,
                "versions": versions,
                "cohort_snapshot": cohort.as_ref().map(|value| &value.snapshot_digest),
                "direct_mcp_snapshot": direct_mcp_snapshot_digest,
            })
            .to_string(),
        ),
        agent_session_id,
        configuration_version: versions.configuration_version.clone(),
        boundary_policy_version: versions.boundary_policy_version.clone(),
        input_watermark_at: trace.session.input_watermark_at,
        input_generation: trace.input_generation,
        observation_set_id,
        observation_parser_version: versions.observation_parser_version.clone(),
        pricing_policy_version: versions.pricing_policy_version.clone(),
        cohort_version: cohort.as_ref().map_or_else(
            || versions.cohort_version.clone(),
            |value| value.reference.cohort_version.clone(),
        ),
        cohort_fallback_level: cohort.as_ref().map_or(NO_COHORT_FALLBACK_LEVEL, |value| {
            value.reference.fallback_level
        }),
        cohort_sample_size: cohort.as_ref().map_or(0, |value| {
            value.reference.successful_costs_10000.len() as u64
        }),
        cohort_snapshot_digest: cohort
            .as_ref()
            .map_or_else(no_cohort_snapshot_digest, |value| {
                value.snapshot_digest.clone()
            }),
        direct_mcp_snapshot_digest,
        analyzed_at: now,
        report,
        stale: false,
        superseded_by_analysis_id: None,
        expires_at: now + report_retention,
        ownership_scope_key: trace.session.ownership_scope_key.clone(),
        user_id: trace.session.user_id,
        service_account_id: trace.session.service_account_id,
    };
    store
        .append_agent_session_analysis(&analysis, Some(lease))
        .await?;
    Ok(())
}

struct ReportObservations {
    observation_set_id: ObservationSetId,
    observations: Vec<InferredObservation>,
    request_metadata_count: u32,
    response_payload_count: u32,
    truncated_payload_count: u32,
}

/// No matching parser observations retain an existing analysis without publishing another.
async fn load_report_observations<S>(
    store: &S,
    trace: &AgentSessionTraceRecord,
    parser_version: &str,
) -> Result<Option<ReportObservations>, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let agent_session_id = trace.session.agent_session_id;
    let all_observation_sets = store.load_agent_observation_sets(agent_session_id).await?;
    if all_observation_sets.is_empty() {
        return Err(GatewayError::Internal(
            "agent session has no observation set".to_string(),
        ));
    }
    if all_observation_sets.len() > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize {
        return Err(GatewayError::Internal(format!(
            "agent session `{agent_session_id}` exceeds the bounded observation-set limit"
        )));
    }
    let observation_sets = all_observation_sets
        .into_iter()
        .filter(|set| set.parser_version == parser_version)
        .collect::<Vec<_>>();
    if observation_sets.is_empty() && trace.latest_analysis.is_some() {
        return Ok(None);
    }
    if observation_sets.is_empty() {
        return Err(GatewayError::Internal(
            "agent session has no observations for the requested parser version".to_string(),
        ));
    }
    let observation_set = observation_sets
        .last()
        .expect("observation sets are known to be non-empty");
    let mut observations = observation_sets
        .iter()
        .flat_map(|set| set.observations.iter().cloned())
        .collect::<Vec<_>>();
    normalize_legacy_tool_inventory_limitations(&mut observations);
    let request_metadata_count =
        observation_coverage_count(&observation_sets, trace.requests.len(), |coverage| {
            coverage_flag(coverage, "request_metadata")
        });
    let response_payload_count =
        observation_coverage_count(&observation_sets, trace.requests.len(), |coverage| {
            response_payload_was_captured(coverage)
        });
    let truncated_payload_count =
        observation_coverage_count(&observation_sets, usize::MAX, |coverage| {
            coverage_flag(coverage, "response_payload_truncated")
        });
    Ok(Some(ReportObservations {
        observation_set_id: observation_set.observation_set_id,
        observations,
        request_metadata_count,
        response_payload_count,
        truncated_payload_count,
    }))
}

struct RequestEvidence {
    requests: Vec<SessionRequestFact>,
    intervals: Vec<ActivityInterval>,
}

async fn load_request_evidence<S>(
    store: &S,
    trace: &AgentSessionTraceRecord,
    configured_policy: &AnalysisPolicy,
) -> Result<RequestEvidence, GatewayError>
where
    S: AgentSessionAnalysisRepository + BudgetRepository + Sync,
{
    let agent_session_id = trace.session.agent_session_id;
    let mut attempts_by_request = BTreeMap::<String, Vec<RequestAttemptFact>>::new();
    if configured_policy.metrics.reliability_metrics {
        let attempt_limit = u32::try_from(MAX_RELIABILITY_EVENTS).unwrap_or(u32::MAX);
        for attempt in store
            .list_agent_session_request_attempts(agent_session_id, attempt_limit)
            .await?
        {
            attempts_by_request
                .entry(attempt.request_id.clone())
                .or_default()
                .push(RequestAttemptFact {
                    request_id: attempt.request_id,
                    attempt_number: attempt.attempt_number,
                    produced_final_response: attempt.produced_final_response,
                    retryable: attempt.retryable,
                    status: attempt.status.as_str().to_string(),
                    status_code: attempt.status_code,
                    error_code: attempt.error_code,
                    latency_ms: attempt.latency_ms,
                    provider_key: attempt.provider_key,
                    upstream_model: attempt.upstream_model,
                    occurred_at_unix_ms: unix_timestamp_millis(attempt.started_at),
                });
        }
    }

    let mut requests = Vec::with_capacity(trace.requests.len());
    let mut intervals = Vec::with_capacity(trace.requests.len());
    let request_ids = trace
        .requests
        .iter()
        .map(|link| link.request_id.clone())
        .collect::<Vec<_>>();
    let mut usage_by_request = store
        .get_usage_ledgers_by_request_ids_and_scope(
            &request_ids,
            &trace.session.ownership_scope_key,
        )
        .await?
        .into_iter()
        .map(|usage| (usage.request_id.clone(), usage))
        .collect::<BTreeMap<_, _>>();
    for link in &trace.requests {
        let usage = usage_by_request.remove(&link.request_id);
        if let Some(interval) = link
            .completed_at
            .and_then(|completed_at| ActivityInterval::new(link.occurred_at, completed_at))
        {
            intervals.push(interval);
        }
        let attempts = attempts_by_request
            .remove(&link.request_id)
            .unwrap_or_default();
        requests.push(SessionRequestFact {
            request_id: link.request_id.clone(),
            ordinal: link.ordinal,
            occurred_at: link.occurred_at,
            completed_at: link.completed_at,
            terminal_success: link.terminal_success,
            usage: usage.as_ref().map(session_usage_fact),
            attempts,
        });
    }
    Ok(RequestEvidence {
        requests,
        intervals,
    })
}

struct DirectMcpEvidence {
    intervals: Vec<ActivityInterval>,
    invocations: Vec<ToolInvocationFact>,
    snapshot_digest: String,
}

/// Uses the first 2,048 rows in request ID, then newest-first order.
async fn load_direct_mcp_evidence<S>(
    store: &S,
    session: &AgentSessionRecord,
    session_requests: &[AgentSessionRequestLinkRecord],
) -> Result<DirectMcpEvidence, GatewayError>
where
    S: McpToolInvocationRepository + Sync,
{
    let request_ids = session_requests
        .iter()
        .map(|request| request.request_id.as_str())
        .collect::<Vec<_>>();
    let rows = store
        .list_mcp_tool_invocations_by_request_ids(&McpToolInvocationBatchQuery {
            request_ids: &request_ids,
            user_id: session.user_id,
            team_id: session.team_id,
            occurred_at_start: session.started_at,
            occurred_at_end: session.ended_at.unwrap_or(session.input_watermark_at),
            limit: u32::try_from(MAX_RELIABILITY_EVENTS).unwrap_or(u32::MAX),
        })
        .await?;
    let mut intervals = Vec::with_capacity(rows.len());
    let mut snapshot = Vec::with_capacity(rows.len());
    let mut invocations = Vec::with_capacity(rows.len());
    for invocation in rows {
        let latency_ms = invocation.latency_ms.unwrap_or_default().max(0);
        let started_at = invocation.occurred_at - Duration::milliseconds(latency_ms);
        if intervals.len() < MAX_RELIABILITY_EVENTS
            && let Some(interval) = ActivityInterval::new(started_at, invocation.occurred_at)
        {
            intervals.push(interval);
        }
        if snapshot.len() < MAX_RELIABILITY_EVENTS {
            snapshot.push((
                invocation.mcp_tool_invocation_id,
                invocation.occurred_at.unix_timestamp_nanos(),
                invocation.latency_ms,
            ));
        }
        if invocations.len() < MAX_RELIABILITY_EVENTS {
            invocations.push(ToolInvocationFact {
                request_id: invocation.request_id,
                server_key: Some(invocation.server_display_key),
                tool_key: invocation.tool_display_key,
                status: invocation.status.as_str().to_string(),
                error_code: invocation.error_code,
                latency_ms: invocation.latency_ms,
                result_payload_truncated: invocation.result_payload_truncated,
                occurred_at_unix_ms: unix_timestamp_millis(invocation.occurred_at),
            });
        }
    }
    snapshot.sort_unstable();
    Ok(DirectMcpEvidence {
        invocations,
        intervals,
        snapshot_digest: hash_identifier(
            &serde_json::to_string(&snapshot)
                .map_err(|error| GatewayError::Internal(error.to_string()))?,
        ),
    })
}

fn observation_coverage_count(
    sets: &[AgentObservationSetRecord],
    maximum: usize,
    is_covered: impl Fn(&Value) -> bool,
) -> u32 {
    sets.iter()
        .filter(|set| is_covered(&set.coverage))
        .count()
        .min(maximum)
        .try_into()
        .unwrap_or(u32::MAX)
}

fn coverage_flag(coverage: &Value, key: &str) -> bool {
    coverage.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn response_payload_was_captured(coverage: &Value) -> bool {
    coverage_flag(coverage, "response_payload")
        || coverage_flag(coverage, "response_payload_truncated")
}

fn session_usage_fact(record: &UsageLedgerRecord) -> SessionUsageFact {
    let priced = record.pricing_status == UsagePricingStatus::Priced;
    let reasoning_tokens = provider_reasoning_tokens(&record.provider_usage);
    let component_cost = |tokens: Option<i64>, rate: Option<Money4>| {
        tokens
            .zip(rate)
            .and_then(|(tokens, rate)| scaled_cost_for_tokens(tokens, rate).ok())
            .map(|cost| cost.round_half_up_to_money4().as_scaled_i64())
    };
    // Image tokens are part of the completion tokens; with an image rate they
    // are charged at it and the rest at the output rate, as in usage pricing.
    let output_cost_10000 = match (
        record.image_output_tokens,
        record.image_output_cost_per_million_tokens,
    ) {
        (Some(image_tokens), Some(image_rate)) => component_cost(
            record
                .completion_tokens
                .map(|tokens| tokens.saturating_sub(image_tokens)),
            record.output_cost_per_million_tokens,
        )
        .zip(component_cost(Some(image_tokens), Some(image_rate)))
        .and_then(|(text, image)| text.checked_add(image)),
        _ => component_cost(
            record.completion_tokens,
            record.output_cost_per_million_tokens,
        ),
    };
    SessionUsageFact {
        fresh_input_tokens: record.uncached_input_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_write_tokens,
        output_tokens: record.completion_tokens,
        reasoning_tokens,
        provider_total_tokens: record.total_tokens,
        cache_creation_5m_tokens: None,
        cache_creation_30m_tokens: None,
        cache_creation_1h_tokens: None,
        output_includes_reasoning: reasoning_tokens.map(|_| true),
        fresh_input_cost_10000: component_cost(
            record.uncached_input_tokens,
            record.input_cost_per_million_tokens,
        ),
        cache_read_cost_10000: component_cost(
            record.cache_read_tokens,
            record.cache_read_cost_per_million_tokens,
        ),
        cache_creation_cost_10000: component_cost(
            record.cache_write_tokens,
            record.cache_write_cost_per_million_tokens,
        ),
        output_cost_10000,
        reasoning_cost_10000: None,
        legacy_cost_10000: None,
        normalized_cost_10000: priced.then_some(
            record
                .computed_cost_usd
                .round_half_up_to_money4()
                .as_scaled_i64(),
        ),
        uncached_input_cost_10000: component_cost(
            record.prompt_tokens,
            record.input_cost_per_million_tokens,
        ),
        provider_key: Some(record.provider_key.clone()),
        upstream_model: Some(record.upstream_model.clone()),
        pricing_policy_version: Some(PRICING_POLICY_VERSION.to_string()),
    }
}

fn provider_reasoning_tokens(provider_usage: &Value) -> Option<i64> {
    let usage = provider_usage.as_object()?;
    let nested_usage = usage.get("provider_usage").and_then(Value::as_object);
    std::iter::once(usage)
        .chain(nested_usage)
        .flat_map(|usage| {
            ["completion_tokens_details", "output_tokens_details"]
                .into_iter()
                .filter_map(move |key| usage.get(key).and_then(Value::as_object))
        })
        .find_map(|details| {
            details
                .get("reasoning_tokens")
                .and_then(Value::as_i64)
                .filter(|tokens| *tokens >= 0)
        })
}

async fn load_successful_harness_cohort<S>(
    store: &S,
    current: &AgentSessionTraceRecord,
    versions: &AgentAnalysisDesiredVersions,
) -> Result<Option<SelectedCohort>, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let mut selection = CohortSelection::new(
        cohort_session(&current.session),
        analysis_versions(versions),
    );
    let mut page_number = 1;
    while page_number <= MAX_COHORT_SCAN_PAGES {
        let page = store
            .list_agent_sessions(&AgentSessionListQuery {
                harness_key: Some(current.session.harness_key.clone()),
                lifecycle: Some(SessionLifecycleState::Finalized),
                ownership_scope_key: Some(current.session.ownership_scope_key.clone()),
                input_watermark_before: Some(current.session.input_watermark_at),
                started_after: Some(current.session.started_at - COHORT_LOOKBACK),
                page: page_number,
                page_size: gateway_core::MAX_AGENT_SESSION_PAGE_SIZE,
                ..Default::default()
            })
            .await?;
        let item_count = page.items.len();
        for candidate in &page.items {
            let Some(analysis) = &candidate.latest_analysis else {
                continue;
            };
            selection.consider(CohortCandidate {
                session: cohort_session(&candidate.session),
                analysis_id: analysis.analysis_id,
                versions: AnalysisVersions::from_report(
                    &analysis.report,
                    &analysis.boundary_policy_version,
                    &analysis.observation_parser_version,
                    &analysis.pricing_policy_version,
                    &analysis.cohort_version,
                ),
                gateway_outcome: analysis.report.gateway_outcome,
                actual_cost_10000: analysis.report.components.actual_cost_10000,
                active_time_ms: analysis.report.components.active_time_ms,
            });
        }
        if item_count < gateway_core::MAX_AGENT_SESSION_PAGE_SIZE as usize {
            break;
        }
        page_number = page_number.saturating_add(1);
    }
    selection
        .finish()
        .map_err(|error| GatewayError::Internal(error.to_string()))
}

fn cohort_session(session: &AgentSessionRecord) -> CohortSession<'_> {
    CohortSession {
        session_id: session.agent_session_id,
        requested_model_key: &session.requested_model_key,
        operation: &session.operation,
        caller_class: &session.caller_class,
        boundary_group_key: &session.boundary_group_key,
    }
}

fn unix_timestamp_millis(value: OffsetDateTime) -> i64 {
    i64::try_from(value.unix_timestamp_nanos().div_euclid(1_000_000)).unwrap_or_else(|_| {
        if value < OffsetDateTime::UNIX_EPOCH {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{provider_reasoning_tokens, response_payload_was_captured, session_usage_fact};
    use agent_session_analysis::{
        AnalysisPolicy, Confidence, SessionLifecycleState, SessionRequestFact, SessionTrace,
        TraceEvidence, analyze_session,
    };
    use gateway_core::{Money4, Money9, UsageLedgerRecord, UsagePricingStatus};
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn reads_reasoning_tokens_from_openai_usage_shapes() {
        assert_eq!(
            provider_reasoning_tokens(&serde_json::json!({
                "completion_tokens_details": { "reasoning_tokens": 21 }
            })),
            Some(21)
        );
        assert_eq!(
            provider_reasoning_tokens(&serde_json::json!({
                "provider_usage": {
                    "output_tokens_details": { "reasoning_tokens": 34 }
                }
            })),
            Some(34)
        );
    }

    #[test]
    fn rejects_invalid_reasoning_token_counts() {
        assert_eq!(
            provider_reasoning_tokens(&serde_json::json!({
                "completion_tokens_details": { "reasoning_tokens": -1 }
            })),
            None
        );
        assert_eq!(provider_reasoning_tokens(&serde_json::json!({})), None);
    }

    #[test]
    fn truncated_responses_count_as_captured_response_payloads() {
        assert!(response_payload_was_captured(&serde_json::json!({
            "response_payload": true,
            "response_payload_truncated": false,
        })));
        assert!(response_payload_was_captured(&serde_json::json!({
            "response_payload": false,
            "response_payload_truncated": true,
        })));
        assert!(!response_payload_was_captured(&serde_json::json!({
            "response_payload": false,
            "response_payload_truncated": false,
        })));
    }

    #[test]
    fn overflowing_output_components_do_not_prevent_a_consumer_report() {
        let record = UsageLedgerRecord {
            usage_event_id: Uuid::nil(),
            request_id: "overflow-output".to_string(),
            ownership_scope_key: "test".to_string(),
            api_key_id: Uuid::nil(),
            user_id: None,
            team_id: None,
            service_account_id: None,
            actor_user_id: None,
            model_id: None,
            model_route_id: None,
            provider_key: "provider".to_string(),
            upstream_model: "model".to_string(),
            prompt_tokens: Some(0),
            uncached_input_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            cache_write_1h_tokens: None,
            completion_tokens: Some(2_000_000),
            image_output_tokens: Some(1_000_000),
            total_tokens: Some(2_000_000),
            provider_usage: serde_json::json!({}),
            pricing_status: UsagePricingStatus::Priced,
            unpriced_reason: None,
            pricing_row_id: None,
            pricing_provider_id: None,
            pricing_model_id: None,
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: Some(Money4::from_scaled(i64::MAX)),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            cache_write_1h_cost_per_million_tokens: None,
            image_output_cost_per_million_tokens: Some(Money4::from_scaled(i64::MAX)),
            pricing_tier_above_input_tokens: None,
            computed_cost_usd: Money9::from_nanos(12_300_000),
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            requested_service_tier: None,
            served_service_tier: None,
        };
        let usage = session_usage_fact(&record);
        assert_eq!(usage.output_cost_10000, None);
        let report = analyze_session(
            &SessionTrace {
                requests: vec![SessionRequestFact {
                    request_id: record.request_id,
                    ordinal: 1,
                    occurred_at: record.occurred_at,
                    completed_at: Some(record.occurred_at),
                    terminal_success: Some(true),
                    usage: Some(usage),
                    attempts: Vec::new(),
                }],
                activity_intervals: Vec::new(),
                observations: Vec::new(),
                lifecycle: SessionLifecycleState::Finalized,
                boundary_confidence: Confidence::High,
                evidence: TraceEvidence::default(),
            },
            &AnalysisPolicy::default(),
            None,
        )
        .expect("overflowing optional component costs must not invalidate a report");
        let consumer_report = serde_json::to_value(report).unwrap();
        assert_eq!(consumer_report["components"]["actual_cost_10000"], 123);
        assert_eq!(
            consumer_report["diagnostics"]["token_and_cache"]["output_tokens"],
            2_000_000
        );
    }
}
