use super::*;

struct SessionMeasurements {
    outcome: OutcomeComponent,
    actual_cost_10000: Option<i64>,
    active_time_ms: i64,
    wall_time_ms: i64,
    summed_work_time_ms: i64,
    excluded_gap_time_ms: i64,
    overlap_savings_ms: i64,
    cost_efficiency_basis_points: Option<u16>,
    active_time_efficiency_basis_points: Option<u16>,
    context: ContextDiagnostics,
    coverage: TelemetryCoverage,
}

impl SessionMeasurements {
    fn score(&self) -> Option<u8> {
        session_efficiency_score(
            self.outcome.factor_basis_points,
            self.cost_efficiency_basis_points,
            self.active_time_efficiency_basis_points,
        )
        .map(|score| score.saturating_sub(self.context.score_penalty_points))
    }
}

struct SessionClassification {
    confidence: Confidence,
    limitations: Vec<LimitationCode>,
}

pub fn analyze_session(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
    cohort: Option<&CohortReference>,
) -> Result<SessionEfficiencyReport, AnalysisError> {
    validate_policy(policy)?;
    validate_trace(trace)?;

    let measurements = measure_session(trace, policy, cohort);
    let diagnostics = diagnose_session(trace, policy, &measurements);
    let classification = classify_session(trace, cohort, &measurements);

    Ok(build_report(
        policy,
        cohort,
        measurements,
        diagnostics,
        classification,
    ))
}

fn measure_session(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
    cohort: Option<&CohortReference>,
) -> SessionMeasurements {
    let outcome = outcome_component(&trace.requests);
    let actual_cost_10000 = sum_usage(&trace.requests, |usage| usage.normalized_cost_10000);
    let (active_time_ms, wall_time_ms, summed_work_time_ms, overlap_savings_ms) =
        measure_time(trace, policy.orchestration_gap);
    let scoring_cohort = usable_scoring_cohort(cohort);
    let cost_efficiency_basis_points = scoring_cohort.and_then(|cohort| {
        actual_cost_10000.and_then(|cost| {
            lower_is_better_efficiency_basis_points(cost, &cohort.successful_costs_10000)
        })
    });
    let active_time_efficiency_basis_points = scoring_cohort.and_then(|cohort| {
        lower_is_better_efficiency_basis_points(active_time_ms, &cohort.successful_active_time_ms)
    });
    let context = if policy.metrics.context_metrics {
        context_diagnostics(&trace.requests, active_time_ms, &trace.observations, policy)
    } else {
        ContextDiagnostics {
            input_boundary_tokens: policy.context_input_boundary_tokens,
            reserved_output_tokens: policy.context_reserved_output_tokens,
            ..ContextDiagnostics::default()
        }
    };

    SessionMeasurements {
        outcome,
        actual_cost_10000,
        active_time_ms,
        wall_time_ms,
        summed_work_time_ms,
        excluded_gap_time_ms: wall_time_ms.saturating_sub(active_time_ms).max(0),
        overlap_savings_ms,
        cost_efficiency_basis_points,
        active_time_efficiency_basis_points,
        context,
        coverage: measure_coverage(trace, scoring_cohort.is_some()),
    }
}

fn measure_time(trace: &SessionTrace, orchestration_gap: Duration) -> (i64, i64, i64, i64) {
    let intervals = trace
        .activity_intervals
        .iter()
        .chain(&trace.evidence.direct_mcp_intervals)
        .cloned()
        .collect::<Vec<_>>();
    let summed_work_time_ms = intervals.iter().fold(0_i64, |total, interval| {
        let duration = (interval.ended_at - interval.started_at).whole_milliseconds();
        total.saturating_add(i64::try_from(duration).unwrap_or(i64::MAX))
    });
    let concurrent_time_ms = active_time_milliseconds(intervals.clone(), Duration::ZERO);
    let active_time_ms = active_time_milliseconds(intervals, orchestration_gap);
    let started_at = trace
        .requests
        .iter()
        .map(|request| request.occurred_at)
        .min();
    let ended_at = trace
        .requests
        .iter()
        .map(|request| request.completed_at.unwrap_or(request.occurred_at))
        .max();
    let wall_time_ms = started_at
        .zip(ended_at)
        .map(|(start, end)| i64::try_from((end - start).whole_milliseconds()).unwrap_or(i64::MAX))
        .unwrap_or_default()
        .max(0);
    let overlap_savings_ms = summed_work_time_ms
        .saturating_sub(concurrent_time_ms)
        .max(0);
    (
        active_time_ms,
        wall_time_ms,
        summed_work_time_ms,
        overlap_savings_ms,
    )
}

fn usable_scoring_cohort(cohort: Option<&CohortReference>) -> Option<&CohortReference> {
    cohort.filter(|cohort| {
        cohort.fallback_level > 0
            || (cohort.successful_costs_10000.len() >= MIN_EXACT_COHORT_SIZE
                && cohort.successful_active_time_ms.len() >= MIN_EXACT_COHORT_SIZE)
    })
}

fn measure_coverage(trace: &SessionTrace, has_scoring_cohort: bool) -> TelemetryCoverage {
    let request_count = trace.requests.len();
    let outcome_percent = coverage_percent(
        outcome_component(&trace.requests).determinate_requests as usize,
        request_count,
    );
    let cost_percent = coverage_percent(
        trace
            .requests
            .iter()
            .filter(|request| {
                request
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.normalized_cost_10000)
                    .is_some()
            })
            .count(),
        request_count,
    );
    let timing_percent = coverage_percent(
        trace
            .requests
            .iter()
            .filter(|request| request.completed_at.is_some())
            .count(),
        request_count,
    );
    let payload_percent = coverage_percent(
        usize::try_from(trace.evidence.response_payload_count).unwrap_or(usize::MAX),
        request_count,
    );
    let cohort_percent = u8::from(has_scoring_cohort) * 100;
    let overall_percent = u8::try_from(
        (u16::from(outcome_percent)
            + u16::from(cost_percent)
            + u16::from(timing_percent)
            + u16::from(payload_percent)
            + u16::from(cohort_percent))
            / 5,
    )
    .unwrap_or(100);

    TelemetryCoverage {
        outcome_percent,
        cost_percent,
        timing_percent,
        payload_percent,
        cohort_percent,
        overall_percent,
    }
}

fn diagnose_session(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
    measurements: &SessionMeasurements,
) -> SessionDiagnostics {
    let measured_tools = if policy.metrics.tool_metrics || policy.metrics.outcome_metrics {
        tool_and_change_diagnostics(
            &trace.observations,
            &trace.evidence.direct_mcp_intervals,
            &trace.evidence.tool_invocations,
            &trace.requests,
        )
    } else {
        ToolAndChangeDiagnostics::default()
    };
    let file_writes = measured_tools
        .file_creates_suspected
        .saturating_add(measured_tools.file_edits_suspected)
        .saturating_add(measured_tools.file_overwrites_suspected);
    let skills = if policy.metrics.skill_metrics {
        extended::skill_diagnostics(&trace.observations)
    } else {
        Default::default()
    };
    let reliability = if policy.metrics.reliability_metrics {
        extended::reliability_diagnostics(&trace.requests, &trace.evidence, &trace.observations)
    } else {
        Default::default()
    };
    let finish_reasons = if policy.metrics.finish_reason_metrics {
        extended::finish_reason_diagnostics(&trace.observations)
    } else {
        Default::default()
    };
    let outcome = outcome_diagnostics(trace, policy, measurements, &measured_tools, file_writes);
    let tools_and_changes = if policy.metrics.tool_metrics {
        measured_tools
    } else {
        Default::default()
    };

    SessionDiagnostics {
        token_and_cache: token_and_cache_diagnostics(trace, policy),
        context: measurements.context.clone(),
        skills,
        reliability,
        outcome,
        finish_reasons,
        tools_and_changes,
        enabled_metrics: policy.metrics.clone(),
        semantic_verification_available: false,
    }
}

fn outcome_diagnostics(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
    measurements: &SessionMeasurements,
    tools: &ToolAndChangeDiagnostics,
    file_writes: u32,
) -> OutcomeDiagnostics {
    if !policy.metrics.outcome_metrics {
        return Default::default();
    }
    extended::outcome_diagnostics(
        &trace.requests,
        &trace.observations,
        &trace.evidence,
        extended::OutcomeMetricInputs {
            gateway_outcome: measurements.outcome.state,
            actual_cost_10000: measurements.actual_cost_10000,
            file_writes,
            unique_files: tools.unique_opaque_files,
            rework: tools.rework_spans_suspected,
            verification: tools.verification_results_classified,
        },
    )
}

fn token_and_cache_diagnostics(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
) -> TokenAndCacheDiagnostics {
    let token_metrics = policy.metrics.token_metrics;
    let cache_metrics = policy.metrics.cache_metrics;
    let cache_read = enabled_sum(cache_metrics, &trace.requests, |usage| {
        usage.cache_read_tokens
    });
    let cache_creation = enabled_sum(cache_metrics, &trace.requests, |usage| {
        usage.cache_creation_tokens
    });
    let uncached_input_cost = enabled_sum(cache_metrics, &trace.requests, |usage| {
        usage.uncached_input_cost_10000
    });
    let actual_input_cost = enabled_sum(cache_metrics, &trace.requests, |usage| {
        usage
            .fresh_input_cost_10000
            .zip(usage.cache_read_cost_10000)
            .and_then(|(fresh, read)| fresh.checked_add(read))
            .zip(usage.cache_creation_cost_10000)
            .and_then(|(partial, creation)| partial.checked_add(creation))
    });
    let cache_savings = uncached_input_cost
        .zip(actual_input_cost)
        .and_then(|(uncached, actual)| uncached.checked_sub(actual));

    TokenAndCacheDiagnostics {
        fresh_input_tokens: enabled_sum(token_metrics, &trace.requests, |usage| {
            usage.fresh_input_tokens
        }),
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        total_input_tokens: enabled_sum(
            token_metrics,
            &trace.requests,
            extended::total_input_tokens,
        ),
        output_tokens: enabled_sum(token_metrics, &trace.requests, |usage| usage.output_tokens),
        reasoning_tokens: enabled_sum(token_metrics, &trace.requests, |usage| {
            usage.reasoning_tokens
        }),
        visible_output_tokens: enabled_sum(
            token_metrics,
            &trace.requests,
            extended::visible_output_tokens,
        ),
        cache_creation_5m_tokens: cache_creation_for_ttl(trace, policy, CacheTtl::FiveMinutes),
        cache_creation_30m_tokens: cache_creation_for_ttl(trace, policy, CacheTtl::ThirtyMinutes),
        cache_creation_1h_tokens: cache_creation_for_ttl(trace, policy, CacheTtl::OneHour),
        provider_total_tokens: enabled_sum(token_metrics, &trace.requests, |usage| {
            usage.provider_total_tokens
        }),
        legacy_cost_10000: sum_usage(&trace.requests, |usage| usage.legacy_cost_10000),
        normalized_cost_10000: sum_usage(&trace.requests, |usage| usage.normalized_cost_10000),
        cache_read_cost_10000: enabled_sum(cache_metrics, &trace.requests, |usage| {
            usage.cache_read_cost_10000
        }),
        cache_creation_cost_10000: enabled_sum(cache_metrics, &trace.requests, |usage| {
            usage.cache_creation_cost_10000
        }),
        uncached_input_cost_10000: uncached_input_cost,
        cache_savings_10000: cache_savings,
        cache_savings_basis_points: cache_savings
            .zip(uncached_input_cost)
            .and_then(|(savings, uncached)| ratio_basis_points(savings, uncached)),
        cache_read_write_ratio_basis_points: cache_read
            .zip(cache_creation)
            .and_then(|(read, write)| ratio_basis_points(read, write)),
        cache_write_amplification_basis_points: cache_creation
            .zip(cache_read)
            .and_then(|(write, read)| ratio_basis_points(write, read)),
        silent_cache_threshold_miss_requests: cache_metrics
            .then(|| {
                extended::silent_cache_threshold_misses(
                    &trace.requests,
                    &trace.observations,
                    &policy.cache_profiles,
                )
            })
            .flatten(),
        cache_key_switches: if cache_metrics {
            extended::cache_key_switches(&trace.requests)
        } else {
            Default::default()
        },
        reasoning_config_switches: cache_metrics
            .then(|| extended::reasoning_config_switches(&trace.observations))
            .flatten(),
        pricing_policy_versions: trace
            .requests
            .iter()
            .filter_map(|request| request.usage.as_ref()?.pricing_policy_version.as_deref())
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    }
}

fn enabled_sum(
    enabled: bool,
    requests: &[SessionRequestFact],
    selector: fn(&SessionUsageFact) -> Option<i64>,
) -> Option<i64> {
    enabled.then(|| sum_usage(requests, selector)).flatten()
}

fn cache_creation_for_ttl(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
    ttl: CacheTtl,
) -> Option<i64> {
    policy.metrics.cache_metrics.then(|| {
        extended::cache_creation_tokens_for_ttl(&trace.requests, &policy.cache_profiles, ttl)
    })?
}

fn classify_session(
    trace: &SessionTrace,
    cohort: Option<&CohortReference>,
    measurements: &SessionMeasurements,
) -> SessionClassification {
    let mut limitations = vec![LimitationCode::SemanticVerificationUnavailable];
    if measurements.outcome.incomplete_requests > 0 {
        limitations.push(LimitationCode::RequestIncomplete);
    }
    if !trace.evidence.session_observed {
        limitations.push(LimitationCode::SessionUnobserved);
    }
    if trace.evidence.response_payload_count == 0 {
        limitations.push(LimitationCode::PayloadUnavailable);
    }
    if trace.evidence.truncated_payload_count > 0 {
        limitations.push(LimitationCode::PayloadTruncated);
    }
    if trace.requests.iter().any(|request| request.usage.is_none()) {
        limitations.push(LimitationCode::UsageUnavailable);
    }
    if measurements.actual_cost_10000.is_none() {
        limitations.push(LimitationCode::PricingUnavailable);
    }
    if cohort.is_some_and(|cohort| cohort.fallback_level > 0) {
        limitations.push(LimitationCode::CohortFallback);
    }
    limitations.extend(
        trace
            .observations
            .iter()
            .flat_map(|observation| observation.limitations.iter().copied()),
    );
    limitations.sort_unstable_by_key(|value| *value as u8);
    limitations.dedup();

    SessionClassification {
        confidence: score_confidence(trace, cohort, measurements),
        limitations,
    }
}

fn score_confidence(
    trace: &SessionTrace,
    cohort: Option<&CohortReference>,
    measurements: &SessionMeasurements,
) -> Confidence {
    if trace.lifecycle == SessionLifecycleState::Open
        || measurements.outcome.state == GatewayOutcomeState::Unknown
        || measurements.score().is_none()
        || measurements.coverage.overall_percent < 60
    {
        return Confidence::Low;
    }
    if trace.boundary_confidence == Confidence::High
        && measurements.coverage.overall_percent >= 80
        && cohort.is_some_and(|cohort| {
            cohort.fallback_level == 0
                && cohort.successful_costs_10000.len() >= MIN_EXACT_COHORT_SIZE
                && cohort.successful_active_time_ms.len() >= MIN_EXACT_COHORT_SIZE
        })
    {
        Confidence::High
    } else {
        Confidence::Medium
    }
}

fn build_report(
    policy: &AnalysisPolicy,
    cohort: Option<&CohortReference>,
    measurements: SessionMeasurements,
    diagnostics: SessionDiagnostics,
    classification: SessionClassification,
) -> SessionEfficiencyReport {
    let score = measurements.score();
    SessionEfficiencyReport {
        report_schema_version: policy.report_schema_version.clone(),
        analyzer_version: policy.analyzer_version.clone(),
        score_policy_version: policy.score_policy_version.clone(),
        observation_parser_version: policy.observation_parser_version.clone(),
        configuration_version: policy.configuration_version.clone(),
        maturity: policy.maturity,
        calibration_approval_id: policy.calibration_approval_id.clone(),
        confidence: classification.confidence,
        gateway_outcome: measurements.outcome.state,
        score,
        coverage: measurements.coverage,
        components: SessionEfficiencyComponents {
            outcome: measurements.outcome,
            cost_efficiency_basis_points: measurements.cost_efficiency_basis_points,
            active_time_efficiency_basis_points: measurements.active_time_efficiency_basis_points,
            actual_cost_10000: measurements.actual_cost_10000,
            active_time_ms: measurements.active_time_ms,
            wall_time_ms: measurements.wall_time_ms,
            summed_work_time_ms: measurements.summed_work_time_ms,
            excluded_gap_time_ms: measurements.excluded_gap_time_ms,
            overlap_savings_ms: measurements.overlap_savings_ms,
            context_penalty_points: diagnostics.context.score_penalty_points,
            unknown_wait_time_ms: None,
            cohort_version: cohort.map(|cohort| cohort.cohort_version.clone()),
            cohort_fallback_level: cohort.map(|cohort| cohort.fallback_level),
            cohort_sample_size: cohort.map_or(0, |cohort| {
                cohort
                    .successful_costs_10000
                    .len()
                    .min(cohort.successful_active_time_ms.len())
            }),
        },
        diagnostics,
        limitations: classification.limitations,
    }
}
