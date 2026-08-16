use super::*;

pub(super) fn agent_session_analysis_identity(
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

pub(super) fn agent_session_efficiency_report(
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

pub(super) fn coverage_flag(coverage: &Value, field: &str) -> bool {
    coverage
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
pub(super) fn agent_session_source_view(
    session: &AgentSessionSourceRecord,
) -> AgentSessionSourceView {
    AgentSessionSourceView {
        session_source_id: session.agent_session_source_id.to_string(),
        session_source_hash: session.normalized_session_id.clone(),
        adapter_namespace: session.adapter_namespace.clone(),
        adapter_version: session.adapter_version.clone(),
        source_provenance: session.source_provenance.clone(),
        harness_key: session.harness_key.clone(),
        harness_label: session.harness_label.clone(),
        first_seen_at: format_timestamp(session.first_seen_at),
        last_seen_at: format_timestamp(session.last_seen_at),
    }
}

pub(super) fn agent_session_summary(
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
        session_source_hash: trace
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

pub(super) fn enum_name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}
