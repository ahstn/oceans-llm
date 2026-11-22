use super::*;

fn request(id: &str, second: i64, success: Option<bool>, cost: Option<i64>) -> SessionRequestFact {
    let occurred_at = OffsetDateTime::UNIX_EPOCH + Duration::seconds(second);
    SessionRequestFact {
        request_id: id.to_string(),
        ordinal: second,
        occurred_at,
        completed_at: Some(occurred_at + Duration::seconds(1)),
        terminal_success: success,
        usage: Some(SessionUsageFact {
            fresh_input_tokens: Some(100),
            cache_read_tokens: Some(0),
            output_tokens: Some(10),
            legacy_cost_10000: cost,
            normalized_cost_10000: cost,
            uncached_input_cost_10000: cost,
            pricing_policy_version: Some("test-pricing-v1".to_string()),
            ..SessionUsageFact::default()
        }),
        attempts: Vec::new(),
    }
}

fn trace(requests: Vec<SessionRequestFact>) -> SessionTrace {
    let activity_intervals = requests
        .iter()
        .filter_map(|request| {
            request
                .completed_at
                .and_then(|end| ActivityInterval::new(request.occurred_at, end))
        })
        .collect();
    SessionTrace {
        requests,
        activity_intervals,
        observations: vec![],
        lifecycle: SessionLifecycleState::Finalized,
        boundary_confidence: Confidence::High,
        evidence: TraceEvidence {
            session_observed: true,
            request_metadata_count: 1,
            response_payload_count: 1,
            truncated_payload_count: 0,
            direct_mcp_intervals: vec![],
            tool_invocations: Vec::new(),
        },
    }
}

#[test]
fn outcome_counts_requests_once_and_excludes_incomplete_requests() {
    let outcome = outcome_component(&[
        request("a", 0, Some(true), Some(1)),
        request("b", 1, Some(false), Some(1)),
        request("c", 2, None, Some(1)),
    ]);
    assert_eq!(outcome.state, GatewayOutcomeState::Partial);
    assert_eq!(outcome.factor_basis_points, 5_000);
    assert_eq!(outcome.successful_requests, 1);
    assert_eq!(outcome.determinate_requests, 2);
    assert_eq!(outcome.incomplete_requests, 1);
}

#[test]
fn unknown_outcome_uses_disclosed_half_prior() {
    let outcome = outcome_component(&[request("a", 0, None, None)]);
    assert_eq!(outcome.state, GatewayOutcomeState::Unknown);
    assert_eq!(outcome.factor_basis_points, 5_000);
}

#[test]
fn midrank_survival_handles_ties_and_policy_clamps() {
    assert_eq!(
        lower_is_better_efficiency_basis_points(2, &[1, 2, 2, 3]),
        Some(5_000)
    );
    assert_eq!(lower_is_better_efficiency_basis_points(99, &[1]), Some(100));
    assert_eq!(
        lower_is_better_efficiency_basis_points(0, &[1]),
        Some(10_000)
    );
    assert_eq!(lower_is_better_efficiency_basis_points(-1, &[1]), None);
}

#[test]
fn active_time_unions_overlap_and_bounded_gaps() {
    let epoch = OffsetDateTime::UNIX_EPOCH;
    let intervals = vec![
        ActivityInterval::new(epoch, epoch + Duration::seconds(10)).expect("interval"),
        ActivityInterval::new(epoch + Duration::seconds(5), epoch + Duration::seconds(20))
            .expect("interval"),
        ActivityInterval::new(epoch + Duration::seconds(30), epoch + Duration::seconds(40))
            .expect("interval"),
    ];
    assert_eq!(
        active_time_milliseconds(intervals, Duration::seconds(15)),
        40_000
    );
}

#[test]
fn failed_session_scores_zero() {
    assert_eq!(
        session_efficiency_score(0, Some(10_000), Some(10_000)),
        Some(0)
    );
}

#[test]
fn score_renormalizes_when_one_efficiency_component_is_missing() {
    assert_eq!(
        session_efficiency_score(10_000, Some(2_500), None),
        Some(59)
    );
    assert_eq!(
        session_efficiency_score(10_000, None, Some(2_500)),
        Some(67)
    );
    assert_eq!(session_efficiency_score(10_000, None, None), None);
}

#[test]
fn report_keeps_score_optional_without_a_cohort_and_exposes_coverage() {
    let report = analyze_session(
        &trace(vec![request("a", 0, Some(true), Some(100))]),
        &AnalysisPolicy::default(),
        None,
    )
    .expect("report");
    assert_eq!(report.gateway_outcome, GatewayOutcomeState::Succeeded);
    assert_eq!(report.score, None);
    assert_eq!(report.confidence, Confidence::Low);
    assert_eq!(report.coverage.outcome_percent, 100);
    assert_eq!(report.coverage.cohort_percent, 0);
    assert_eq!(
        report.diagnostics.token_and_cache.normalized_cost_10000,
        Some(100)
    );
}

#[test]
fn exact_cohort_requires_six_successful_sessions() {
    let cohort = CohortReference {
        cohort_version: "exact-v1".to_string(),
        fallback_level: 0,
        successful_costs_10000: vec![100; MIN_EXACT_COHORT_SIZE - 1],
        successful_active_time_ms: vec![1_000; MIN_EXACT_COHORT_SIZE - 1],
    };
    let report = analyze_session(
        &trace(vec![request("a", 0, Some(true), Some(100))]),
        &AnalysisPolicy::default(),
        Some(&cohort),
    )
    .expect("report");

    assert_eq!(report.score, None);
    assert_eq!(
        report.components.cohort_sample_size,
        MIN_EXACT_COHORT_SIZE - 1
    );
    assert_eq!(report.confidence, Confidence::Low);
}

#[test]
fn trace_validation_rejects_duplicates_negative_usage_and_invalid_intervals() {
    let duplicate = trace(vec![
        request("same", 0, Some(true), Some(1)),
        request("same", 1, Some(true), Some(1)),
    ]);
    assert!(matches!(
        analyze_session(&duplicate, &AnalysisPolicy::default(), None),
        Err(AnalysisError::DuplicateRequest(id)) if id == "same"
    ));

    let mut negative = request("negative", 0, Some(true), Some(1));
    negative.usage.as_mut().expect("usage").fresh_input_tokens = Some(-1);
    assert!(matches!(
        analyze_session(&trace(vec![negative]), &AnalysisPolicy::default(), None),
        Err(AnalysisError::InvalidUsage(id)) if id == "negative"
    ));

    let mut invalid_interval = request("interval", 0, Some(true), Some(1));
    invalid_interval.completed_at = Some(invalid_interval.occurred_at - Duration::SECOND);
    assert!(matches!(
        analyze_session(
            &trace(vec![invalid_interval]),
            &AnalysisPolicy::default(),
            None
        ),
        Err(AnalysisError::InvalidRequestInterval(id)) if id == "interval"
    ));
}

#[test]
fn policy_rejects_unknown_versions_and_unapproved_calibration() {
    let input = trace(vec![request("a", 0, Some(true), Some(1))]);
    let unsupported = AnalysisPolicy {
        analyzer_version: "future".to_string(),
        ..AnalysisPolicy::default()
    };
    assert!(matches!(
        analyze_session(&input, &unsupported, None),
        Err(AnalysisError::UnsupportedVersion {
            field: "analyzer version",
            ..
        })
    ));
    let unapproved = AnalysisPolicy {
        maturity: ScoreMaturity::Calibrated,
        ..AnalysisPolicy::default()
    };
    assert_eq!(
        analyze_session(&input, &unapproved, None),
        Err(AnalysisError::MissingCalibrationApproval)
    );
}

#[test]
fn report_scores_complete_trace_and_exposes_cache_and_overlap_diagnostics() {
    let cohort = CohortReference {
        cohort_version: "exact-v1".to_string(),
        fallback_level: 0,
        successful_costs_10000: vec![50; MIN_EXACT_COHORT_SIZE],
        successful_active_time_ms: vec![500; MIN_EXACT_COHORT_SIZE],
    };
    let mut session_request = request("a", 0, Some(true), Some(100));
    let usage = session_request.usage.as_mut().expect("usage");
    usage.cache_read_tokens = Some(50);
    usage.cache_creation_tokens = Some(10);
    usage.fresh_input_cost_10000 = Some(100);
    usage.cache_read_cost_10000 = Some(10);
    usage.cache_creation_cost_10000 = Some(20);
    usage.uncached_input_cost_10000 = Some(200);
    let mut session_trace = trace(vec![session_request]);
    session_trace.evidence.direct_mcp_intervals = vec![
        ActivityInterval::new(
            OffsetDateTime::UNIX_EPOCH,
            OffsetDateTime::UNIX_EPOCH + Duration::milliseconds(500),
        )
        .expect("interval"),
    ];

    let report =
        analyze_session(&session_trace, &AnalysisPolicy::default(), Some(&cohort)).expect("report");
    assert_eq!(report.score, Some(10));
    assert_eq!(report.gateway_outcome, GatewayOutcomeState::Succeeded);
    assert_eq!(report.components.active_time_ms, 1_000);
    assert_eq!(report.components.summed_work_time_ms, 1_500);
    assert_eq!(report.components.overlap_savings_ms, 500);
    assert_eq!(
        report.diagnostics.token_and_cache.cache_savings_10000,
        Some(70)
    );
    assert_eq!(
        report
            .diagnostics
            .token_and_cache
            .cache_savings_basis_points,
        Some(3_500)
    );
    assert_eq!(report.diagnostics.tools_and_changes.direct_mcp_calls, 1);
    assert_eq!(
        report.diagnostics.tools_and_changes.direct_mcp_duration_ms,
        Some(500)
    );
}

#[test]
fn orchestration_gaps_do_not_create_negative_overlap_savings() {
    let session_trace = trace(vec![
        request("a", 0, Some(true), Some(100)),
        request("b", 10, Some(true), Some(100)),
    ]);

    let report = analyze_session(&session_trace, &AnalysisPolicy::default(), None).expect("report");

    assert_eq!(report.components.summed_work_time_ms, 2_000);
    assert_eq!(report.components.active_time_ms, 11_000);
    assert_eq!(report.components.overlap_savings_ms, 0);
}

#[test]
fn report_exposes_extended_cache_context_reliability_and_change_diagnostics() {
    let mut session_request = request("a", 0, Some(true), Some(100));
    let usage = session_request.usage.as_mut().expect("usage");
    usage.cache_read_tokens = Some(50);
    usage.cache_creation_tokens = Some(10);
    usage.cache_creation_5m_tokens = Some(4);
    usage.cache_creation_1h_tokens = Some(6);
    usage.reasoning_tokens = Some(5);
    usage.provider_total_tokens = Some(165);
    usage.output_includes_reasoning = Some(true);
    session_request.attempts = vec![
        RequestAttemptFact {
            request_id: "a".to_string(),
            attempt_number: 1,
            produced_final_response: false,
            retryable: true,
            status: "provider_error".to_string(),
            status_code: Some(500),
            error_code: Some("upstream".to_string()),
            latency_ms: Some(100),
            provider_key: "anthropic".to_string(),
            upstream_model: "claude".to_string(),
            occurred_at_unix_ms: 0,
        },
        RequestAttemptFact {
            request_id: "a".to_string(),
            attempt_number: 2,
            produced_final_response: true,
            retryable: false,
            status: "succeeded".to_string(),
            status_code: Some(200),
            error_code: None,
            latency_ms: Some(900),
            provider_key: "openai".to_string(),
            upstream_model: "gpt".to_string(),
            occurred_at_unix_ms: 100,
        },
    ];
    let mut session_trace = trace(vec![session_request]);
    session_trace.observations = vec![InferredObservation {
        observation_id: Uuid::nil(),
        kind: InferredObservationKind::FileEditSuspected,
        source_request_id: "a".to_string(),
        parser_version: "test".to_string(),
        evidence: EvidenceQuality::Direct,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        facts: BoundedObservationFacts {
            supplied_tools: vec![BoundedToolDefinitionFact {
                name: "github.search_code".to_string(),
                server_key: Some("github".to_string()),
                token_estimate: 120,
            }],
            supplied_skills: vec![BoundedSkillFact {
                name: "review".to_string(),
                description_token_estimate: Some(50),
                body_token_estimate: Some(500),
                resource_token_estimate: Some(100),
                used: true,
                abandoned: Some(false),
            }],
            file_interactions: vec![
                BoundedFileInteractionFact {
                    opaque_file_id: "file-1".to_string(),
                    operation: FileOperation::Edit,
                    tool_name: Some("edit".to_string()),
                    succeeded: Some(true),
                    error_signature: None,
                },
                BoundedFileInteractionFact {
                    opaque_file_id: "file-1".to_string(),
                    operation: FileOperation::Edit,
                    tool_name: Some("edit".to_string()),
                    succeeded: Some(true),
                    error_signature: None,
                },
                BoundedFileInteractionFact {
                    opaque_file_id: "file-1".to_string(),
                    operation: FileOperation::Verify,
                    tool_name: Some("test".to_string()),
                    succeeded: Some(true),
                    error_signature: None,
                },
            ],
            finish_reason: Some("length".to_string()),
            ..BoundedObservationFacts::default()
        },
        limitations: Vec::new(),
    }];
    session_trace.evidence.tool_invocations = vec![ToolInvocationFact {
        request_id: "a".to_string(),
        server_key: Some("github".to_string()),
        tool_key: "github.search_code".to_string(),
        status: "failed".to_string(),
        error_code: Some("rate_limited".to_string()),
        latency_ms: Some(200),
        result_payload_truncated: false,
        occurred_at_unix_ms: 0,
    }];

    let report = analyze_session(&session_trace, &AnalysisPolicy::default(), None).expect("report");

    assert_eq!(
        report.diagnostics.token_and_cache.cache_creation_5m_tokens,
        Some(4)
    );
    assert_eq!(
        report.diagnostics.token_and_cache.cache_creation_1h_tokens,
        Some(6)
    );
    assert_eq!(report.diagnostics.reliability.wasted_attempts, 1);
    assert_eq!(report.diagnostics.reliability.failed_tool_invocations, 1);
    assert_eq!(
        report.diagnostics.outcome.rework_ratio_basis_points,
        Some(5_000)
    );
    assert_eq!(
        report.diagnostics.outcome.verification_rate_basis_points,
        Some(5_000)
    );
    assert_eq!(report.diagnostics.skills.used_skill_count, Some(1));
    assert_eq!(report.diagnostics.finish_reasons.items[0].reason, "length");
    let mut outcome_only_policy = AnalysisPolicy::default();
    outcome_only_policy.metrics.tool_metrics = false;
    let outcome_only =
        analyze_session(&session_trace, &outcome_only_policy, None).expect("outcome-only report");
    assert_eq!(
        outcome_only.diagnostics.outcome.rework_ratio_basis_points,
        Some(5_000)
    );
    assert_eq!(
        outcome_only
            .diagnostics
            .outcome
            .verification_rate_basis_points,
        Some(5_000)
    );
    assert_eq!(
        outcome_only
            .diagnostics
            .tools_and_changes
            .unique_opaque_files,
        0
    );
}

#[test]
fn provider_model_switches_use_session_ordinal_for_equal_timestamps() {
    let mut first = request("first", 0, Some(true), Some(100));
    first.ordinal = 0;
    first.usage.as_mut().expect("first usage").provider_key = Some("provider-a".to_string());
    first.usage.as_mut().expect("first usage").upstream_model = Some("model-a".to_string());

    let mut second = request("second", 0, Some(true), Some(100));
    second.ordinal = 1;
    second.usage.as_mut().expect("second usage").provider_key = Some("provider-b".to_string());
    second.usage.as_mut().expect("second usage").upstream_model = Some("model-b".to_string());

    let mut third = request("third", 0, Some(true), Some(100));
    third.ordinal = 2;
    third.usage.as_mut().expect("third usage").provider_key = Some("provider-a".to_string());
    third.usage.as_mut().expect("third usage").upstream_model = Some("model-a".to_string());

    assert_eq!(
        extended::provider_model_switches(&[third, first, second]),
        2
    );
}

#[test]
fn provider_model_switches_preserve_legacy_trace_order_without_ordinals() {
    let mut first = request("z-first", 0, Some(true), Some(100));
    first.ordinal = 0;
    first.usage.as_mut().expect("first usage").provider_key = Some("provider-a".to_string());
    first.usage.as_mut().expect("first usage").upstream_model = Some("model-a".to_string());

    let mut second = request("a-second", 0, Some(true), Some(100));
    second.ordinal = 0;
    second.usage.as_mut().expect("second usage").provider_key = Some("provider-b".to_string());
    second.usage.as_mut().expect("second usage").upstream_model = Some("model-b".to_string());

    let mut third = request("m-third", 0, Some(true), Some(100));
    third.ordinal = 0;
    third.usage.as_mut().expect("third usage").provider_key = Some("provider-a".to_string());
    third.usage.as_mut().expect("third usage").upstream_model = Some("model-a".to_string());

    assert_eq!(
        extended::provider_model_switches(&[first, second, third]),
        2
    );
}

#[test]
fn token_diagnostics_accept_legacy_cache_key_switch_field() {
    let diagnostics: TokenAndCacheDiagnostics = serde_json::from_value(serde_json::json!({
        "cache_key_switches": 3,
        "pricing_policy_versions": []
    }))
    .expect("legacy diagnostics");

    assert_eq!(diagnostics.provider_model_switches, 3);
    assert_eq!(
        serde_json::to_value(diagnostics).expect("current diagnostics")["provider_model_switches"],
        3
    );
}

#[test]
fn cache_profiles_classify_aggregate_writes_and_truncated_payloads_remain_unknown() {
    let mut cache_request = request("cache", 0, Some(true), Some(100));
    let usage = cache_request.usage.as_mut().expect("usage");
    usage.cache_creation_tokens = Some(120);
    usage.provider_key = Some("openai".to_string());
    usage.upstream_model = Some("gpt-5".to_string());
    let cache_policy = AnalysisPolicy {
        cache_profiles: vec![CacheProfileRule {
            provider_key_contains: Some("openai".to_string()),
            upstream_model_contains: Some("gpt".to_string()),
            minimum_cacheable_tokens: 1,
            default_ttl: CacheTtl::ThirtyMinutes,
        }],
        ..AnalysisPolicy::default()
    };
    let cache_report =
        analyze_session(&trace(vec![cache_request]), &cache_policy, None).expect("cache report");
    assert_eq!(
        cache_report
            .diagnostics
            .token_and_cache
            .cache_creation_30m_tokens,
        Some(120)
    );
    assert_eq!(
        cache_report
            .diagnostics
            .token_and_cache
            .cache_creation_5m_tokens,
        Some(0)
    );

    let mut truncated_trace = trace(vec![request("truncated", 0, Some(true), Some(100))]);
    truncated_trace.evidence.response_payload_count = 1;
    truncated_trace.evidence.truncated_payload_count = 1;
    let truncated_report =
        analyze_session(&truncated_trace, &AnalysisPolicy::default(), None).expect("report");
    assert_eq!(
        truncated_report
            .diagnostics
            .outcome
            .file_signal_coverage_percent,
        0
    );
    assert_eq!(truncated_report.diagnostics.outcome.zero_outcome, None);
    assert_eq!(truncated_report.coverage.response_payload_count, 1);
    assert_eq!(truncated_report.coverage.truncated_response_count, 1);
}

#[test]
fn tool_schema_diagnostics_use_actual_per_request_exposure() {
    let observation = |request_id: &str, tools: Vec<(&str, u64)>| InferredObservation {
        observation_id: Uuid::new_v4(),
        kind: InferredObservationKind::SessionMetadataClassified,
        source_request_id: request_id.to_string(),
        parser_version: "test".to_string(),
        evidence: EvidenceQuality::Direct,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        facts: BoundedObservationFacts {
            supplied_tools: tools
                .into_iter()
                .map(|(name, token_estimate)| BoundedToolDefinitionFact {
                    server_key: Some("github".to_string()),
                    name: name.to_string(),
                    token_estimate,
                })
                .collect(),
            ..BoundedObservationFacts::default()
        },
        limitations: Vec::new(),
    };
    let mut first_request = request("a", 0, Some(true), Some(100));
    first_request
        .usage
        .as_mut()
        .expect("first usage")
        .fresh_input_cost_10000 = Some(100);
    let mut second_request = request("b", 2, Some(true), Some(100));
    second_request
        .usage
        .as_mut()
        .expect("second usage")
        .fresh_input_cost_10000 = Some(100);
    let mut session_trace = trace(vec![first_request, second_request]);
    session_trace.observations = vec![
        observation("a", vec![("search", 100), ("read", 50)]),
        observation("b", vec![("search", 120)]),
        InferredObservation {
            observation_id: Uuid::new_v4(),
            kind: InferredObservationKind::ToolCallClassified,
            source_request_id: "b".to_string(),
            parser_version: "test".to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: OffsetDateTime::UNIX_EPOCH + Duration::seconds(2),
            facts: BoundedObservationFacts {
                tool_name: Some("search".to_string()),
                ..BoundedObservationFacts::default()
            },
            limitations: Vec::new(),
        },
    ];

    let report = analyze_session(&session_trace, &AnalysisPolicy::default(), None).expect("report");
    let github = &report.diagnostics.tools_and_changes.tool_servers[0];
    assert_eq!(github.exposed_tool_definitions, 2);
    assert_eq!(github.invoked_tool_definitions, 1);
    assert_eq!(github.invocation_count, 1);
    assert_eq!(github.schema_token_estimate_per_request, 135);
    assert_eq!(github.estimated_uncached_schema_cost_10000, Some(270));
    let search = report
        .diagnostics
        .reliability
        .tools
        .iter()
        .find(|tool| tool.tool_key == "search")
        .expect("generic tool reliability");
    assert_eq!(search.server_key.as_deref(), Some("github"));
    assert_eq!(search.invocation_count, 1);
}

#[test]
fn report_serialization_is_deterministic_and_round_trips() {
    let cohort = CohortReference {
        cohort_version: "exact-v1".to_string(),
        fallback_level: 0,
        successful_costs_10000: vec![50; MIN_EXACT_COHORT_SIZE],
        successful_active_time_ms: vec![500; MIN_EXACT_COHORT_SIZE],
    };
    let session_trace = trace(vec![request("a", 0, Some(true), Some(100))]);
    let report =
        analyze_session(&session_trace, &AnalysisPolicy::default(), Some(&cohort)).expect("report");
    let recomputed =
        analyze_session(&session_trace, &AnalysisPolicy::default(), Some(&cohort)).expect("report");
    let first = serde_json::to_string(&report).expect("serialize");
    let second = serde_json::to_string(&recomputed).expect("serialize");
    assert_eq!(first, second);
    assert_eq!(
        serde_json::from_str::<SessionEfficiencyReport>(&first).expect("deserialize"),
        report
    );
}

#[test]
fn file_operations_preserve_existing_fact_wire_strings() {
    for (wire, operation) in [
        ("read", FileOperation::Read),
        ("search", FileOperation::Search),
        ("create", FileOperation::Create),
        ("edit", FileOperation::Edit),
        ("overwrite", FileOperation::Overwrite),
        ("verify", FileOperation::Verify),
    ] {
        let legacy = serde_json::json!({
            "opaque_file_id": "file-1",
            "operation": wire,
            "tool_name": null,
            "succeeded": true,
            "error_signature": null
        });
        let fact: BoundedFileInteractionFact =
            serde_json::from_value(legacy.clone()).expect("existing fact");
        assert_eq!(fact.operation, operation);
        assert_eq!(operation.as_str(), wire);
        assert_eq!(FileOperation::try_from(wire).expect("operation"), operation);
        assert_eq!(serde_json::to_value(&fact).expect("serialize"), legacy);
    }

    // Existing metadata ingestion rejects unknown operations before they reach storage.
    assert!(FileOperation::try_from("rename").is_err());
    assert!(serde_json::from_value::<FileOperation>(serde_json::json!("rename")).is_err());
}

#[test]
fn context_samples_use_session_ordinal_then_request_id_for_equal_timestamps() {
    let mut first = request("a", 0, Some(true), Some(1));
    let mut second = request("b", 0, Some(true), Some(1));
    let mut third = request("c", 0, Some(true), Some(1));
    first.ordinal = 2;
    second.ordinal = 1;
    third.ordinal = 3;
    first.usage.as_mut().expect("usage").fresh_input_tokens = Some(200);
    second.usage.as_mut().expect("usage").fresh_input_tokens = Some(100);
    third.usage.as_mut().expect("usage").fresh_input_tokens = Some(300);
    let policy = AnalysisPolicy::default();
    let ordered = context_diagnostics(
        &[second.clone(), first.clone(), third.clone()],
        60_000,
        &[],
        &policy,
    );
    let shuffled = context_diagnostics(
        &[third.clone(), first.clone(), second.clone()],
        60_000,
        &[],
        &policy,
    );
    assert_eq!(ordered, shuffled);
    assert_eq!(shuffled.initial_prompt_tokens, Some(100));
    assert_eq!(shuffled.prompt_growth_per_turn, Some(100));
    assert_eq!(shuffled.median_prompt_tokens, Some(200));
    assert_eq!(shuffled.p90_prompt_tokens, Some(300));

    // A request ID makes even duplicate persisted ordinals deterministic.
    first.ordinal = 1;
    third.ordinal = 1;
    let tied = context_diagnostics(
        &[third.clone(), second.clone(), first.clone()],
        60_000,
        &[],
        &policy,
    );
    assert_eq!(tied.initial_prompt_tokens, Some(200));
    assert_eq!(
        tied,
        context_diagnostics(&[first, second, third], 60_000, &[], &policy)
    );
}

#[test]
fn context_samples_preserve_legacy_trace_order_without_ordinals() {
    let mut first = request("z-first", 0, Some(true), Some(1));
    let mut second = request("a-second", 0, Some(true), Some(1));
    first.usage.as_mut().expect("usage").fresh_input_tokens = Some(300);
    second.usage.as_mut().expect("usage").fresh_input_tokens = Some(100);
    let context = context_diagnostics(&[first, second], 60_000, &[], &AnalysisPolicy::default());
    assert_eq!(context.initial_prompt_tokens, Some(300));
    assert_eq!(context.prompt_growth_per_turn, Some(-200));
}

#[test]
fn reasoning_switches_break_timestamp_ties_by_request_and_observation_ids() {
    let observation = |request_id: &str, id: u128, hash: &str| InferredObservation {
        observation_id: Uuid::from_u128(id),
        kind: InferredObservationKind::SessionMetadataClassified,
        source_request_id: request_id.to_string(),
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        evidence: EvidenceQuality::Direct,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        facts: BoundedObservationFacts {
            reasoning_config_hash: Some(hash.to_string()),
            ..BoundedObservationFacts::default()
        },
        limitations: Vec::new(),
    };
    let first = observation("a", 2, "config-a");
    let second = observation("b", 1, "config-b");
    let third = observation("b", 3, "config-a");
    assert_eq!(
        extended::reasoning_config_switches(&[first.clone(), second.clone(), third.clone()]),
        Some(2)
    );
    assert_eq!(
        extended::reasoning_config_switches(&[third, first, second]),
        Some(2)
    );
}

#[test]
fn trace_validation_rejects_negative_cache_ttl_buckets_even_when_metrics_are_disabled() {
    for bucket in [
        CacheTtl::FiveMinutes,
        CacheTtl::ThirtyMinutes,
        CacheTtl::OneHour,
    ] {
        let mut session_request = request("negative-ttl", 0, Some(true), Some(1));
        let usage = session_request.usage.as_mut().expect("usage");
        match bucket {
            CacheTtl::FiveMinutes => usage.cache_creation_5m_tokens = Some(-1),
            CacheTtl::ThirtyMinutes => usage.cache_creation_30m_tokens = Some(-1),
            CacheTtl::OneHour => usage.cache_creation_1h_tokens = Some(-1),
            CacheTtl::Unknown => unreachable!("explicit buckets only"),
        }
        let input = trace(vec![session_request]);
        for enabled in [true, false] {
            let mut policy = AnalysisPolicy::default();
            policy.metrics.token_metrics = enabled;
            policy.metrics.cache_metrics = enabled;
            assert_eq!(
                analyze_session(&input, &policy, None),
                Err(AnalysisError::InvalidUsage("negative-ttl".to_string()))
            );
        }
    }

    let mut valid = request("valid-ttl", 0, Some(true), Some(1));
    let usage = valid.usage.as_mut().expect("usage");
    usage.cache_creation_5m_tokens = Some(0);
    usage.cache_creation_30m_tokens = Some(1);
    usage.cache_creation_1h_tokens = Some(2);
    assert!(analyze_session(&trace(vec![valid]), &AnalysisPolicy::default(), None).is_ok());
}
