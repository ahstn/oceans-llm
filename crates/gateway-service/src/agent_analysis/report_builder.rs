use super::*;

pub(super) async fn generate_report<S>(
    store: &S,
    agent_session_id: Uuid,
    versions: &AgentAnalysisDesiredVersions,
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
        .filter(|set| set.parser_version == versions.observation_parser_version)
        .collect::<Vec<_>>();
    if observation_sets.is_empty() && trace.latest_analysis.is_some() {
        return Ok(());
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
                "observation_set": observation_set.observation_set_id,
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
        observation_set_id: observation_set.observation_set_id,
        observation_parser_version: versions.observation_parser_version.clone(),
        pricing_policy_version: versions.pricing_policy_version.clone(),
        cohort_version: cohort.as_ref().map_or_else(
            || versions.cohort_version.clone(),
            |value| value.reference.cohort_version.clone(),
        ),
        cohort_fallback_level: cohort
            .as_ref()
            .map_or(7, |value| value.reference.fallback_level),
        cohort_sample_size: cohort.as_ref().map_or(0, |value| {
            value.reference.successful_costs_10000.len() as u64
        }),
        cohort_snapshot_digest: cohort.as_ref().map_or_else(
            || hash_identifier("no-cohort"),
            |value| value.snapshot_digest.clone(),
        ),
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
    if store.append_agent_session_analysis(&analysis).await? {
        store
            .mark_agent_session_analyses_stale(agent_session_id, Some(analysis.analysis_id))
            .await?;
    }
    Ok(())
}

struct DirectMcpEvidence {
    intervals: Vec<ActivityInterval>,
    invocations: Vec<ToolInvocationFact>,
    snapshot_digest: String,
}

async fn load_direct_mcp_evidence<S>(
    store: &S,
    session: &AgentSessionRecord,
    session_requests: &[AgentSessionRequestLinkRecord],
) -> Result<DirectMcpEvidence, GatewayError>
where
    S: McpToolInvocationRepository + Sync,
{
    let page_size = MAX_MCP_TOOL_INVOCATION_PAGE_SIZE;
    let mut intervals = Vec::new();
    let mut snapshot = Vec::new();
    let request_ids = session_requests
        .iter()
        .map(|request| request.request_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut invocations = Vec::new();
    for request_id in request_ids {
        let mut page = 1;
        while page <= MAX_DIRECT_MCP_SCAN_PAGES && invocations.len() < MAX_RELIABILITY_EVENTS {
            let result = store
                .list_mcp_tool_invocations(&McpToolInvocationQuery {
                    page,
                    page_size,
                    request_id: Some(request_id.to_string()),
                    user_id: session.user_id,
                    team_id: session.team_id,
                    occurred_at_start: Some(session.started_at),
                    occurred_at_end: Some(session.ended_at.unwrap_or(session.input_watermark_at)),
                    ..McpToolInvocationQuery::default()
                })
                .await?;
            for invocation in &result.items {
                if invocation.user_id != session.user_id || invocation.team_id != session.team_id {
                    continue;
                }
                let latency_ms = invocation.latency_ms.unwrap_or_default().max(0);
                let started_at = invocation.occurred_at - Duration::milliseconds(latency_ms);
                if intervals.len() < MAX_RELIABILITY_EVENTS
                    && let Some(interval) =
                        ActivityInterval::new(started_at, invocation.occurred_at)
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
                        request_id: invocation.request_id.clone(),
                        server_key: Some(invocation.server_display_key.clone()),
                        tool_key: invocation.tool_display_key.clone(),
                        status: invocation.status.as_str().to_string(),
                        error_code: invocation.error_code.clone(),
                        latency_ms: invocation.latency_ms,
                        result_payload_truncated: invocation.result_payload_truncated,
                        occurred_at_unix_ms: unix_timestamp_millis(invocation.occurred_at),
                    });
                }
            }
            if u64::from(page) * u64::from(page_size) >= result.total || result.items.is_empty() {
                break;
            }
            page = page.checked_add(1).ok_or_else(|| {
                GatewayError::Internal("MCP invocation page overflow".to_string())
            })?;
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

fn normalize_legacy_tool_inventory_limitations(observations: &mut [InferredObservation]) {
    for observation in observations {
        if observation.kind == InferredObservationKind::SessionMetadataClassified
            && !tool_inventory_is_estimated(
                observation.facts.supplied_tool_count,
                observation.facts.supplied_tools.len(),
            )
        {
            observation
                .limitations
                .retain(|code| *code != LimitationCode::ToolInventoryPotentialOnly);
        }
    }
}
fn session_usage_fact(record: &UsageLedgerRecord) -> SessionUsageFact {
    let priced = record.pricing_status == UsagePricingStatus::Priced;
    let reasoning_tokens = provider_reasoning_tokens(&record.provider_usage);
    let component_cost = |tokens: Option<i64>, rate: Option<Money4>| {
        tokens
            .zip(rate)
            .and_then(|(tokens, rate)| scaled_cost_for_tokens(tokens, rate).ok())
            .map(Money4::as_scaled_i64)
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
        output_cost_10000: component_cost(
            record.completion_tokens,
            record.output_cost_per_million_tokens,
        ),
        reasoning_cost_10000: None,
        legacy_cost_10000: None,
        normalized_cost_10000: priced.then_some(record.computed_cost_usd.as_scaled_i64()),
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

struct LoadedCohort {
    reference: CohortReference,
    snapshot_digest: String,
}

async fn load_successful_harness_cohort<S>(
    store: &S,
    current: &AgentSessionTraceRecord,
    versions: &AgentAnalysisDesiredVersions,
) -> Result<Option<LoadedCohort>, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let mut samples: [Vec<(Uuid, i64, i64)>; 4] = std::array::from_fn(|_| Vec::new());
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
        for candidate in page.items {
            if candidate.session.agent_session_id == current.session.agent_session_id {
                continue;
            }
            let Some(analysis) = candidate.latest_analysis else {
                continue;
            };
            if analysis.report.report_schema_version != versions.report_schema_version
                || analysis.boundary_policy_version != versions.boundary_policy_version
                || analysis.observation_parser_version != versions.observation_parser_version
                || analysis.report.analyzer_version != versions.analyzer_version
                || analysis.report.score_policy_version != versions.score_policy_version
                || analysis.pricing_policy_version != versions.pricing_policy_version
                || analysis.report.configuration_version != versions.configuration_version
                || analysis.report.gateway_outcome != GatewayOutcomeState::Succeeded
            {
                continue;
            }
            let (Some(cost), active_time) = (
                analysis.report.components.actual_cost_10000,
                analysis.report.components.active_time_ms,
            ) else {
                continue;
            };
            let sample = (analysis.analysis_id, cost, active_time);
            if samples[3].len() < MAX_COHORT_SAMPLES_PER_LEVEL {
                samples[3].push(sample);
            }
            if candidate.session.requested_model_key == current.session.requested_model_key {
                if samples[2].len() < MAX_COHORT_SAMPLES_PER_LEVEL {
                    samples[2].push(sample);
                }
                if candidate.session.operation == current.session.operation
                    && candidate.session.caller_class == current.session.caller_class
                {
                    if samples[1].len() < MAX_COHORT_SAMPLES_PER_LEVEL {
                        samples[1].push(sample);
                    }
                    if candidate.session.boundary_group_key == current.session.boundary_group_key
                        && samples[0].len() < MAX_COHORT_SAMPLES_PER_LEVEL
                    {
                        samples[0].push(sample);
                    }
                }
            }
        }
        if item_count < gateway_core::MAX_AGENT_SESSION_PAGE_SIZE as usize {
            break;
        }
        page_number = page_number.saturating_add(1);
    }
    let Some((fallback_level, mut samples)) = samples
        .into_iter()
        .enumerate()
        .find(|(_, samples)| samples.len() >= agent_session_analysis::MIN_EXACT_COHORT_SIZE)
    else {
        return Ok(None);
    };
    samples.sort_unstable_by_key(|sample| sample.0);
    let snapshot_digest = hash_identifier(
        &serde_json::to_string(&samples)
            .map_err(|error| GatewayError::Internal(error.to_string()))?,
    );
    let (costs, active_times): (Vec<_>, Vec<_>) = samples
        .into_iter()
        .map(|(_, cost, active_time)| (cost, active_time))
        .unzip();
    Ok(Some(LoadedCohort {
        reference: CohortReference {
            cohort_version: COHORT_VERSION.to_string(),
            fallback_level: u8::try_from(fallback_level).unwrap_or(3),
            successful_costs_10000: costs,
            successful_active_time_ms: active_times,
        },
        snapshot_digest,
    }))
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
    use super::{
        normalize_legacy_tool_inventory_limitations, provider_reasoning_tokens,
        response_payload_was_captured,
    };
    use agent_session_analysis::{
        BoundedObservationFacts, BoundedToolDefinitionFact, EvidenceQuality, InferredObservation,
        InferredObservationKind, LimitationCode,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    fn metadata_observation(
        supplied_tool_count: u32,
        retained_tools: usize,
    ) -> InferredObservation {
        InferredObservation {
            observation_id: Uuid::nil(),
            kind: InferredObservationKind::SessionMetadataClassified,
            source_request_id: "request".to_string(),
            parser_version: "passive-observations-v3".to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            facts: BoundedObservationFacts {
                supplied_tool_count: Some(supplied_tool_count),
                supplied_tools: (0..retained_tools)
                    .map(|index| BoundedToolDefinitionFact {
                        name: format!("tool-{index}"),
                        server_key: None,
                        token_estimate: 1,
                    })
                    .collect(),
                ..BoundedObservationFacts::default()
            },
            limitations: vec![LimitationCode::ToolInventoryPotentialOnly],
        }
    }

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
    fn legacy_tool_inventory_notice_is_kept_only_for_incomplete_inventory() {
        let complete = metadata_observation(1, 1);
        let incomplete = metadata_observation(2, 1);
        let mut observations = vec![complete, incomplete];

        normalize_legacy_tool_inventory_limitations(&mut observations);

        assert!(observations[0].limitations.is_empty());
        assert_eq!(
            observations[1].limitations,
            vec![LimitationCode::ToolInventoryPotentialOnly]
        );
    }
}
