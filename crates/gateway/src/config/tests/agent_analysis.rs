use super::*;
use serial_test::serial;

// These variables configure a process-wide startup boundary. Restore them even
// when an assertion fails, and serialize every test that resolves this policy.
struct AnalysisEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl AnalysisEnvironment {
    fn clear() -> Self {
        Self(
            [
                "AGENT_ANALYSIS_ENABLED",
                "AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED",
                "AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED",
                "AGENT_ANALYSIS_TEAM_ADMIN_ENABLED",
                "AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID",
                "AGENT_ANALYSIS_REPORT_RETENTION_DAYS",
                "AGENT_ANALYSIS_QUEUE_RETENTION_DAYS",
            ]
            .into_iter()
            .map(|key| {
                let previous = env::var_os(key);
                unsafe {
                    env::remove_var(key);
                }
                (key, previous)
            })
            .collect(),
        )
    }
}

impl Drop for AnalysisEnvironment {
    fn drop(&mut self) {
        for (key, previous) in &self.0 {
            unsafe {
                match previous {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }
}

#[test]
#[serial]
fn parses_agent_analysis_policy_and_cache_profiles() {
    let _environment = AnalysisEnvironment::clear();
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
agent_analysis:
  enabled: true
  report_retention_days: 120
  queue_retention_days: 14
  context_input_boundary_tokens: 180000
  context_reserved_output_tokens: 64000
  context_penalty_points_per_repeated_excess: 3
  metrics:
    tokens: true
    cache: true
    context: false
    tools: true
    skills: false
    reliability: true
    outcomes: true
    finish_reasons: false
  cache_profiles:
    - provider_key_contains: anthropic
      upstream_model_contains: opus
      minimum_cacheable_tokens: 4096
      default_ttl: one_hour
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let analysis = config.agent_analysis;
    assert_eq!(analysis.report_retention_days, 120);
    assert_eq!(analysis.queue_retention_days, 14);
    assert_eq!(analysis.context_input_boundary_tokens, 180_000);
    assert_eq!(analysis.context_reserved_output_tokens, 64_000);
    assert_eq!(analysis.context_penalty_points_per_repeated_excess, 3);
    assert!(!analysis.metrics.context);
    assert!(!analysis.metrics.skills);
    assert!(!analysis.metrics.finish_reasons);
    assert_eq!(analysis.cache_profiles.len(), 1);
    assert_eq!(analysis.cache_profiles[0].minimum_cacheable_tokens, 4_096);
    assert!(matches!(
        analysis.cache_profiles[0].default_ttl,
        AgentAnalysisCacheTtlConfig::OneHour
    ));
    let loaded = analysis.resolve().expect("runtime policy");
    assert_eq!(loaded.report_retention, time::Duration::days(120));
    assert_eq!(loaded.queue_retention, time::Duration::days(14));
    assert_eq!(loaded.policy.context_input_boundary_tokens, 180_000);
    assert_eq!(loaded.policy.context_reserved_output_tokens, 64_000);
    assert_eq!(loaded.policy.context_penalty_points_per_repeated_excess, 3);
    assert_eq!(
        loaded.policy.metrics,
        gateway_service::AnalysisMetricPolicy {
            token_metrics: true,
            cache_metrics: true,
            context_metrics: false,
            tool_metrics: true,
            skill_metrics: false,
            reliability_metrics: true,
            outcome_metrics: true,
            finish_reason_metrics: false,
        }
    );
    let custom = &loaded.policy.cache_profiles[0];
    assert_eq!(custom.provider_key_contains.as_deref(), Some("anthropic"));
    assert_eq!(custom.upstream_model_contains.as_deref(), Some("opus"));
    assert_eq!(custom.minimum_cacheable_tokens, 4096);
    assert_eq!(custom.default_ttl, gateway_service::CacheTtl::OneHour);
    // Custom rules must precede built-in matches so operator overrides win.
    let bedrock = &loaded.policy.cache_profiles[1];
    assert_eq!(bedrock.provider_key_contains.as_deref(), Some("bedrock"));
    assert_eq!(bedrock.upstream_model_contains.as_deref(), Some("gpt-5.6"));
    assert_eq!(
        bedrock.default_ttl,
        gateway_service::CacheTtl::ThirtyMinutes
    );
    assert_eq!(
        loaded.policy.maturity,
        gateway_core::ScoreMaturity::Experimental
    );
    assert!(loaded.capabilities.passive_analysis_enabled);
    assert!(!loaded.capabilities.access_for(true, false).allowed);
}

#[test]
fn rejects_agent_analysis_cache_profile_without_a_match() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
agent_analysis:
  cache_profiles:
    - minimum_cacheable_tokens: 1024
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    assert!(
        format!("{error:#}")
            .contains("agent analysis cache profiles require a provider or model match"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn validates_agent_analysis_limits_at_config_load() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    for (setting, message) in [
        (
            "calibration_approval_id: ' padded '",
            "must be trimmed and at most 256 bytes",
        ),
        (
            "report_retention_days: 36501",
            "retention must not exceed 36500 days",
        ),
        (
            "queue_retention_days: 36501",
            "retention must not exceed 36500 days",
        ),
        (
            "context_input_boundary_tokens: 0",
            "context_input_boundary_tokens must be > 0",
        ),
        (
            "context_reserved_output_tokens: -1",
            "context_reserved_output_tokens must be >= 0",
        ),
        (
            "cache_profiles: [{provider_key_contains: anthropic, minimum_cacheable_tokens: 0}]",
            "token minimums must be > 0",
        ),
    ] {
        write_config(&path, &format!("agent_analysis:\n  {setting}\n"));
        let error = GatewayConfig::from_path(&path).expect_err(setting);
        assert!(
            format!("{error:#}").contains(message),
            "{setting}: {error:#}"
        );
    }
}

#[test]
#[serial]
fn environment_overrides_gate_calibrated_and_team_access() {
    let _environment = AnalysisEnvironment::clear();
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    write_config(
        &path,
        "agent_analysis:\n  calibration_approval_id: file-approval\n",
    );
    let config = GatewayConfig::from_path(&path).expect("config");
    unsafe {
        env::set_var("AGENT_ANALYSIS_ENABLED", " OFF ");
        env::set_var("AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED", "yes");
        env::set_var("AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED", "1");
        env::set_var("AGENT_ANALYSIS_TEAM_ADMIN_ENABLED", "on");
        env::set_var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID", " env-approval ");
    }
    let loaded = config.agent_analysis.resolve().expect("approved runtime");
    assert!(!loaded.capabilities.passive_analysis_enabled);
    assert_eq!(
        loaded.policy.maturity,
        gateway_core::ScoreMaturity::Calibrated
    );
    assert_eq!(
        loaded.policy.calibration_approval_id.as_deref(),
        Some("env-approval")
    );
    let admin = loaded.capabilities.access_for(true, false);
    assert!(admin.allowed && admin.score_visible && admin.shadow_visible);
    let team = loaded.capabilities.access_for(false, true);
    assert!(team.allowed && team.score_visible && !team.shadow_visible);
    assert!(!loaded.capabilities.access_for(false, false).allowed);

    unsafe {
        env::set_var("AGENT_ANALYSIS_TEAM_ADMIN_ENABLED", "false");
    }
    let platform_only = config.agent_analysis.resolve().expect("platform only");
    assert!(!platform_only.capabilities.access_for(false, true).allowed);
    unsafe {
        env::set_var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID", " ");
    }
    let error = config
        .agent_analysis
        .resolve()
        .err()
        .expect("empty approval fails");
    assert!(
        error
            .to_string()
            .contains("requires agent_analysis.calibration_approval_id")
    );
    unsafe {
        env::set_var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID", "x".repeat(257));
    }
    let error = config
        .agent_analysis
        .resolve()
        .err()
        .expect("long approval fails");
    assert!(error.to_string().contains("must not exceed 256 bytes"));
    unsafe {
        env::remove_var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID");
        env::set_var("AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED", "0");
        env::set_var("AGENT_ANALYSIS_TEAM_ADMIN_ENABLED", "true");
    }
    let error = config
        .agent_analysis
        .resolve()
        .err()
        .expect("team without calibration fails");
    assert!(
        error
            .to_string()
            .contains("team agent analytics require calibrated score visibility")
    );
}

#[test]
#[serial]
fn runtime_rejects_invalid_flags_and_bounds_retention_overrides() {
    let _environment = AnalysisEnvironment::clear();
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    write_config(
        &path,
        "agent_analysis:\n  report_retention_days: 120\n  queue_retention_days: 14\n",
    );
    let config = GatewayConfig::from_path(&path).expect("config");
    unsafe {
        env::set_var("AGENT_ANALYSIS_ENABLED", "sometimes");
    }
    let error = config
        .agent_analysis
        .resolve()
        .err()
        .expect("invalid boolean");
    assert_eq!(
        error.to_string(),
        "AGENT_ANALYSIS_ENABLED must be a boolean"
    );
    unsafe {
        env::set_var("AGENT_ANALYSIS_ENABLED", "no");
        env::set_var(
            "AGENT_ANALYSIS_REPORT_RETENTION_DAYS",
            "18446744073709551615",
        );
        env::set_var("AGENT_ANALYSIS_QUEUE_RETENTION_DAYS", "0");
    }
    let loaded = config.agent_analysis.resolve().expect("bounded retention");
    assert_eq!(loaded.report_retention, time::Duration::days(36_500));
    assert_eq!(loaded.queue_retention, time::Duration::ZERO);
    unsafe {
        env::set_var("AGENT_ANALYSIS_REPORT_RETENTION_DAYS", "invalid");
        env::set_var("AGENT_ANALYSIS_QUEUE_RETENTION_DAYS", "-1");
    }
    let loaded = config.agent_analysis.resolve().expect("fallback retention");
    assert_eq!(loaded.report_retention, time::Duration::days(120));
    assert_eq!(loaded.queue_retention, time::Duration::days(14));
}

#[test]
#[serial]
fn configured_cache_ttls_survive_runtime_policy_loading() {
    use gateway_service::CacheTtl;
    let _environment = AnalysisEnvironment::clear();
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    for (ttl, expected) in [
        ("five_minutes", CacheTtl::FiveMinutes),
        ("thirty_minutes", CacheTtl::ThirtyMinutes),
        ("one_hour", CacheTtl::OneHour),
        ("unknown", CacheTtl::Unknown),
    ] {
        write_config(
            &path,
            &format!(
                "agent_analysis:\n  cache_profiles:\n    - upstream_model_contains: custom\n      minimum_cacheable_tokens: 512\n      default_ttl: {ttl}\n"
            ),
        );
        let config = GatewayConfig::from_path(&path).expect("config");
        let loaded = config.agent_analysis.resolve().expect("runtime policy");
        let rule = &loaded.policy.cache_profiles[0];
        assert_eq!(rule.default_ttl, expected);
        assert_eq!(rule.upstream_model_contains.as_deref(), Some("custom"));
        assert_eq!(rule.minimum_cacheable_tokens, 512);
        assert_eq!(rule.provider_key_contains, None);
    }
}
