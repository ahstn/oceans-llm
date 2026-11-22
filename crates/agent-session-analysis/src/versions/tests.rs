use super::*;

fn versions() -> AnalysisVersions<'static> {
    AnalysisVersions {
        report_schema_version: "schema",
        boundary_policy_version: "boundary",
        observation_parser_version: "parser",
        report_observation_parser_version: "parser",
        analyzer_version: "analyzer",
        score_policy_version: "score",
        pricing_policy_version: "pricing",
        cohort_version: "cohort",
        configuration_version: "configuration",
        score_maturity: ScoreMaturity::Experimental,
        calibration_approval_id: None,
    }
}

#[test]
fn preserves_distinct_cohort_supported_and_current_version_contracts() {
    let desired = versions();
    let mut candidate = desired;
    candidate.cohort_version = "old-cohort";
    candidate.score_maturity = ScoreMaturity::Calibrated;
    candidate.calibration_approval_id = Some("approval");
    candidate.report_observation_parser_version = "old-report-parser";
    assert!(candidate.is_compatible_with(desired, VersionCompatibility::Cohort));
    assert!(!candidate.is_compatible_with(desired, VersionCompatibility::Supported));
    assert!(!candidate.is_compatible_with(desired, VersionCompatibility::CurrentReport));

    candidate = desired;
    candidate.configuration_version = "old-configuration";
    assert!(candidate.is_compatible_with(desired, VersionCompatibility::Supported));
    assert!(!candidate.is_compatible_with(desired, VersionCompatibility::Cohort));
    assert!(!candidate.is_compatible_with(desired, VersionCompatibility::CurrentReport));
}

#[test]
fn common_version_mismatches_are_always_incompatible() {
    let desired = versions();
    for field in 0..6 {
        let mut candidate = desired;
        match field {
            0 => candidate.report_schema_version = "old",
            1 => candidate.boundary_policy_version = "old",
            2 => candidate.observation_parser_version = "old",
            3 => candidate.analyzer_version = "old",
            4 => candidate.score_policy_version = "old",
            _ => candidate.pricing_policy_version = "old",
        }
        for mode in [
            VersionCompatibility::Supported,
            VersionCompatibility::Cohort,
            VersionCompatibility::CurrentReport,
        ] {
            assert!(!candidate.is_compatible_with(desired, mode));
        }
    }
}

#[test]
fn current_reports_require_each_runtime_identity_field() {
    let desired = versions();
    assert!(desired.is_compatible_with(desired, VersionCompatibility::CurrentReport));
    for field in 0..5 {
        let mut candidate = desired;
        match field {
            0 => candidate.cohort_version = "old",
            1 => candidate.configuration_version = "old",
            2 => candidate.report_observation_parser_version = "old",
            3 => candidate.score_maturity = ScoreMaturity::Calibrated,
            _ => candidate.calibration_approval_id = Some("old"),
        }
        assert!(!candidate.is_compatible_with(desired, VersionCompatibility::CurrentReport));
    }
}

#[test]
fn configuration_identity_preserves_policy_json_or_explicit_version() {
    let mut policy = AnalysisPolicy::default();
    let serialized = serde_json::to_string(&policy).unwrap();
    assert_eq!(
        configuration_version(&policy).unwrap(),
        crate::identity::hashing::hash_identifier(&serialized),
    );
    policy.configuration_version = "explicit-configuration".to_string();
    assert_eq!(
        configuration_version(&policy).unwrap(),
        "explicit-configuration"
    );
}

#[test]
fn calibrated_versions_require_a_nonempty_approval_identity() {
    let mut candidate = versions();
    assert!(candidate.has_required_calibration_approval());
    candidate.score_maturity = ScoreMaturity::Calibrated;
    assert!(!candidate.has_required_calibration_approval());
    candidate.calibration_approval_id = Some("");
    assert!(!candidate.has_required_calibration_approval());
    candidate.calibration_approval_id = Some("approval");
    assert!(candidate.has_required_calibration_approval());
}
