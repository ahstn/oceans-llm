use super::*;

#[test]
fn report_artifacts_advance_without_reparsing_retained_observations() {
    let versions = desired_versions_for_policy(&AnalysisPolicy::default());

    assert_eq!(versions.report_schema_version, "agent-session-report-v6");
    assert_eq!(versions.analyzer_version, "session-efficiency-v5");
    assert_eq!(
        versions.observation_parser_version,
        "passive-observations-v4"
    );
}
