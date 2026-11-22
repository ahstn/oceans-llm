use crate::{AnalysisPolicy, ScoreMaturity};

/// Borrowed version metadata, independent of persistence and queue records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisVersions<'a> {
    pub report_schema_version: &'a str,
    pub boundary_policy_version: &'a str,
    pub observation_parser_version: &'a str,
    pub report_observation_parser_version: &'a str,
    pub analyzer_version: &'a str,
    pub score_policy_version: &'a str,
    pub pricing_policy_version: &'a str,
    pub cohort_version: &'a str,
    pub configuration_version: &'a str,
    pub score_maturity: ScoreMaturity,
    pub calibration_approval_id: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub enum VersionCompatibility {
    /// Supported executable versions; configuration and calibration are runtime policy.
    Supported,
    /// Historical cohort eligibility deliberately ignores cohort and calibration versions.
    Cohort,
    /// Recomputing a report requires the entire persisted version tuple to match.
    CurrentReport,
}

impl<'a> AnalysisVersions<'a> {
    #[must_use]
    pub fn from_report(
        report: &'a crate::SessionEfficiencyReport,
        boundary_policy_version: &'a str,
        observation_parser_version: &'a str,
        pricing_policy_version: &'a str,
        cohort_version: &'a str,
    ) -> Self {
        Self {
            report_schema_version: &report.report_schema_version,
            boundary_policy_version,
            observation_parser_version,
            report_observation_parser_version: &report.observation_parser_version,
            analyzer_version: &report.analyzer_version,
            score_policy_version: &report.score_policy_version,
            pricing_policy_version,
            cohort_version,
            configuration_version: &report.configuration_version,
            score_maturity: report.maturity,
            calibration_approval_id: report.calibration_approval_id.as_deref(),
        }
    }
}

impl AnalysisVersions<'_> {
    #[must_use]
    pub fn is_compatible_with(self, desired: Self, compatibility: VersionCompatibility) -> bool {
        if self.report_schema_version != desired.report_schema_version
            || self.boundary_policy_version != desired.boundary_policy_version
            || self.observation_parser_version != desired.observation_parser_version
            || self.analyzer_version != desired.analyzer_version
            || self.score_policy_version != desired.score_policy_version
            || self.pricing_policy_version != desired.pricing_policy_version
        {
            return false;
        }
        match compatibility {
            VersionCompatibility::Supported => self.cohort_version == desired.cohort_version,
            VersionCompatibility::Cohort => {
                self.configuration_version == desired.configuration_version
            }
            VersionCompatibility::CurrentReport => {
                self.cohort_version == desired.cohort_version
                    && self.configuration_version == desired.configuration_version
                    && self.report_observation_parser_version
                        == desired.report_observation_parser_version
                    && self.score_maturity == desired.score_maturity
                    && self.calibration_approval_id == desired.calibration_approval_id
            }
        }
    }

    #[must_use]
    pub fn has_required_calibration_approval(self) -> bool {
        self.score_maturity != ScoreMaturity::Calibrated
            || self
                .calibration_approval_id
                .is_some_and(|id| !id.is_empty())
    }
}

pub fn configuration_version(policy: &AnalysisPolicy) -> Result<String, serde_json::Error> {
    if policy.configuration_version.is_empty() {
        serde_json::to_string(policy).map(|json| crate::identity::hashing::hash_identifier(&json))
    } else {
        Ok(policy.configuration_version.clone())
    }
}

#[cfg(test)]
mod tests {
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
}
