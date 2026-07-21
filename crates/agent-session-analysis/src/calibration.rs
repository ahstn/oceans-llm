use serde::{Deserialize, Serialize};

use crate::{Confidence, GatewayOutcomeState};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationSample {
    pub task_id: String,
    pub predicted_group: String,
    pub reviewed_group: String,
    pub score: Option<u8>,
    pub outcome: GatewayOutcomeState,
    pub confidence: Confidence,
    pub coverage_percent: u8,
    pub normalized_cost_10000: Option<i64>,
    pub active_time_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationPolicy {
    pub policy_version: String,
    pub minimum_reviewed_tasks: usize,
    pub maximum_grouping_error_basis_points: u16,
    pub minimum_scored_tasks: usize,
    pub minimum_median_coverage_percent: u8,
    pub maximum_score_shift_under_sensitivity: u8,
}

impl Default for CalibrationPolicy {
    fn default() -> Self {
        Self {
            policy_version: "task-calibration-v1".to_string(),
            minimum_reviewed_tasks: 100,
            maximum_grouping_error_basis_points: 500,
            minimum_scored_tasks: 50,
            minimum_median_coverage_percent: 80,
            maximum_score_shift_under_sensitivity: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationDistribution {
    pub count: usize,
    pub minimum: Option<i64>,
    pub p50: Option<i64>,
    pub p90: Option<i64>,
    pub maximum: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationAssessment {
    pub policy_version: String,
    pub reviewed_task_count: usize,
    pub scored_task_count: usize,
    pub grouping_error_basis_points: u16,
    pub median_coverage_percent: u8,
    pub score_distribution: CalibrationDistribution,
    pub cost_distribution_10000: CalibrationDistribution,
    pub active_time_distribution_ms: CalibrationDistribution,
    pub outcome_counts: [usize; 4],
    pub confidence_counts: [usize; 3],
    pub maximum_score_shift_under_sensitivity: u8,
    pub grouping_review_passed: bool,
    pub coverage_review_passed: bool,
    pub sample_size_passed: bool,
    pub sensitivity_review_passed: bool,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationError {
    DuplicateTask(String),
    InvalidCoverage { task_id: String, value: u8 },
    NegativeCost(String),
    NegativeActiveTime(String),
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateTask(task_id) => {
                write!(formatter, "duplicate calibration task `{task_id}`")
            }
            Self::InvalidCoverage { task_id, value } => {
                write!(formatter, "task `{task_id}` has invalid coverage `{value}`")
            }
            Self::NegativeCost(task_id) => {
                write!(formatter, "task `{task_id}` has negative normalized cost")
            }
            Self::NegativeActiveTime(task_id) => {
                write!(formatter, "task `{task_id}` has negative active time")
            }
        }
    }
}

impl std::error::Error for CalibrationError {}

fn percentile(mut values: Vec<i64>, numerator: usize, denominator: usize) -> Option<i64> {
    if values.is_empty() || denominator == 0 {
        return None;
    }
    values.sort_unstable();
    let index = ((values.len() - 1) * numerator).div_ceil(denominator);
    values.get(index).copied()
}

fn distribution(values: Vec<i64>) -> CalibrationDistribution {
    CalibrationDistribution {
        count: values.len(),
        minimum: values.iter().min().copied(),
        p50: percentile(values.clone(), 1, 2),
        p90: percentile(values.clone(), 9, 10),
        maximum: values.iter().max().copied(),
    }
}

fn outcome_index(value: GatewayOutcomeState) -> usize {
    match value {
        GatewayOutcomeState::Succeeded => 0,
        GatewayOutcomeState::Partial => 1,
        GatewayOutcomeState::Failed => 2,
        GatewayOutcomeState::Unknown => 3,
    }
}

fn confidence_index(value: Confidence) -> usize {
    match value {
        Confidence::Low => 0,
        Confidence::Medium => 1,
        Confidence::High => 2,
    }
}

pub fn assess_calibration(
    samples: &[CalibrationSample],
    policy: &CalibrationPolicy,
    maximum_score_shift_under_sensitivity: u8,
) -> Result<CalibrationAssessment, CalibrationError> {
    let mut task_ids = std::collections::HashSet::with_capacity(samples.len());
    let mut grouping_errors = 0_usize;
    let mut scores = Vec::new();
    let mut costs = Vec::new();
    let mut times = Vec::with_capacity(samples.len());
    let mut coverages = Vec::with_capacity(samples.len());
    let mut outcome_counts = [0_usize; 4];
    let mut confidence_counts = [0_usize; 3];

    for sample in samples {
        if !task_ids.insert(sample.task_id.as_str()) {
            return Err(CalibrationError::DuplicateTask(sample.task_id.clone()));
        }
        if sample.coverage_percent > 100 {
            return Err(CalibrationError::InvalidCoverage {
                task_id: sample.task_id.clone(),
                value: sample.coverage_percent,
            });
        }
        if sample.normalized_cost_10000.is_some_and(|cost| cost < 0) {
            return Err(CalibrationError::NegativeCost(sample.task_id.clone()));
        }
        if sample.active_time_ms < 0 {
            return Err(CalibrationError::NegativeActiveTime(sample.task_id.clone()));
        }
        grouping_errors += usize::from(sample.predicted_group != sample.reviewed_group);
        scores.extend(sample.score.map(i64::from));
        costs.extend(sample.normalized_cost_10000);
        times.push(sample.active_time_ms);
        coverages.push(i64::from(sample.coverage_percent));
        outcome_counts[outcome_index(sample.outcome)] += 1;
        confidence_counts[confidence_index(sample.confidence)] += 1;
    }

    let reviewed_task_count = samples.len();
    let scored_task_count = scores.len();
    let grouping_error_basis_points = grouping_errors
        .saturating_mul(10_000)
        .saturating_add(reviewed_task_count / 2)
        .checked_div(reviewed_task_count)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(10_000);
    let median_coverage_percent = percentile(coverages, 1, 2)
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0);
    let grouping_review_passed =
        grouping_error_basis_points <= policy.maximum_grouping_error_basis_points;
    let coverage_review_passed = median_coverage_percent >= policy.minimum_median_coverage_percent;
    let sample_size_passed = reviewed_task_count >= policy.minimum_reviewed_tasks
        && scored_task_count >= policy.minimum_scored_tasks;
    let sensitivity_review_passed =
        maximum_score_shift_under_sensitivity <= policy.maximum_score_shift_under_sensitivity;

    Ok(CalibrationAssessment {
        policy_version: policy.policy_version.clone(),
        reviewed_task_count,
        scored_task_count,
        grouping_error_basis_points,
        median_coverage_percent,
        score_distribution: distribution(scores),
        cost_distribution_10000: distribution(costs),
        active_time_distribution_ms: distribution(times),
        outcome_counts,
        confidence_counts,
        maximum_score_shift_under_sensitivity,
        grouping_review_passed,
        coverage_review_passed,
        sample_size_passed,
        sensitivity_review_passed,
        approved: grouping_review_passed
            && coverage_review_passed
            && sample_size_passed
            && sensitivity_review_passed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(index: usize) -> CalibrationSample {
        CalibrationSample {
            task_id: format!("task-{index}"),
            predicted_group: "group-a".to_string(),
            reviewed_group: "group-a".to_string(),
            score: Some(75),
            outcome: GatewayOutcomeState::Succeeded,
            confidence: Confidence::High,
            coverage_percent: 90,
            normalized_cost_10000: Some(100),
            active_time_ms: 1_000,
        }
    }

    #[test]
    fn calibration_requires_every_gate() {
        let samples = (0..100).map(sample).collect::<Vec<_>>();
        let assessment =
            assess_calibration(&samples, &CalibrationPolicy::default(), 5).expect("assessment");
        assert!(assessment.approved);
        assert_eq!(assessment.grouping_error_basis_points, 0);
        assert_eq!(assessment.score_distribution.p50, Some(75));

        let failed =
            assess_calibration(&samples, &CalibrationPolicy::default(), 11).expect("assessment");
        assert!(!failed.approved);
        assert!(!failed.sensitivity_review_passed);
    }

    #[test]
    fn grouping_error_uses_reviewed_denominator() {
        let mut samples = (0..100).map(sample).collect::<Vec<_>>();
        for sample in samples.iter_mut().take(6) {
            sample.reviewed_group = "group-b".to_string();
        }
        let assessment =
            assess_calibration(&samples, &CalibrationPolicy::default(), 0).expect("assessment");
        assert_eq!(assessment.grouping_error_basis_points, 600);
        assert!(!assessment.grouping_review_passed);
        assert!(!assessment.approved);
    }

    #[test]
    fn duplicate_or_invalid_samples_are_rejected() {
        let duplicate = vec![sample(1), sample(1)];
        assert!(matches!(
            assess_calibration(&duplicate, &CalibrationPolicy::default(), 0),
            Err(CalibrationError::DuplicateTask(id)) if id == "task-1"
        ));
        let mut invalid = sample(2);
        invalid.coverage_percent = 101;
        assert!(matches!(
            assess_calibration(&[invalid], &CalibrationPolicy::default(), 0),
            Err(CalibrationError::InvalidCoverage { value: 101, .. })
        ));
    }
}
