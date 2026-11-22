use time::Duration;
use uuid::Uuid;

use crate::versions::{AnalysisVersions, VersionCompatibility};
use crate::{CohortReference, GatewayOutcomeState, MIN_EXACT_COHORT_SIZE};

pub const COHORT_VERSION: &str = "successful-boundary-group-v2";
pub const MAX_COHORT_SCAN_PAGES: u32 = 25;
pub const MAX_COHORT_SAMPLES_PER_LEVEL: usize = 2_000;
pub const COHORT_LOOKBACK: Duration = Duration::days(90);
pub const NO_COHORT_FALLBACK_LEVEL: u8 = 7;

#[must_use]
pub fn no_cohort_snapshot_digest() -> String {
    crate::identity::hashing::hash_identifier("no-cohort")
}

#[derive(Debug, Clone, Copy)]
pub struct CohortSession<'a> {
    pub session_id: Uuid,
    pub requested_model_key: &'a str,
    pub operation: &'a str,
    pub caller_class: &'a str,
    pub boundary_group_key: &'a str,
}

/// The repository adapter supplies candidates in its existing paginated scan order.
#[derive(Debug, Clone, Copy)]
pub struct CohortCandidate<'a> {
    pub session: CohortSession<'a>,
    pub analysis_id: Uuid,
    pub versions: AnalysisVersions<'a>,
    pub gateway_outcome: GatewayOutcomeState,
    pub actual_cost_10000: Option<i64>,
    pub active_time_ms: i64,
}

pub struct SelectedCohort {
    pub reference: CohortReference,
    pub snapshot_digest: String,
}

/// Retains only bounded sample tuples, not reports or persistence records.
pub struct CohortSelection<'a> {
    current: CohortSession<'a>,
    versions: AnalysisVersions<'a>,
    samples: [Vec<(Uuid, i64, i64)>; 4],
}

impl<'a> CohortSelection<'a> {
    #[must_use]
    pub fn new(current: CohortSession<'a>, versions: AnalysisVersions<'a>) -> Self {
        Self {
            current,
            versions,
            samples: std::array::from_fn(|_| Vec::new()),
        }
    }

    pub fn consider(&mut self, candidate: CohortCandidate<'_>) {
        if candidate.session.session_id == self.current.session_id
            || !candidate
                .versions
                .is_compatible_with(self.versions, VersionCompatibility::Cohort)
            || candidate.gateway_outcome != GatewayOutcomeState::Succeeded
        {
            return;
        }
        let Some(cost) = candidate.actual_cost_10000 else {
            return;
        };
        let sample = (candidate.analysis_id, cost, candidate.active_time_ms);
        let model_matches =
            candidate.session.requested_model_key == self.current.requested_model_key;
        let operation_and_caller_match = model_matches
            && candidate.session.operation == self.current.operation
            && candidate.session.caller_class == self.current.caller_class;
        let matches = [
            operation_and_caller_match
                && candidate.session.boundary_group_key == self.current.boundary_group_key,
            operation_and_caller_match,
            model_matches,
            true,
        ];
        for (samples, matches) in self.samples.iter_mut().zip(matches) {
            if matches && samples.len() < MAX_COHORT_SAMPLES_PER_LEVEL {
                samples.push(sample);
            }
        }
    }

    pub fn finish(self) -> Result<Option<SelectedCohort>, serde_json::Error> {
        let Some((fallback_level, mut samples)) = self
            .samples
            .into_iter()
            .enumerate()
            .find(|(_, samples)| samples.len() >= MIN_EXACT_COHORT_SIZE)
        else {
            return Ok(None);
        };
        samples.sort_unstable_by_key(|sample| sample.0);
        // Persisted snapshot identity is SHA256 of the JSON tuple array, UUID-sorted.
        let snapshot_digest =
            crate::identity::hashing::hash_identifier(&serde_json::to_string(&samples)?);
        let (costs, active_times) = samples
            .into_iter()
            .map(|(_, cost, active_time)| (cost, active_time))
            .unzip();
        Ok(Some(SelectedCohort {
            reference: CohortReference {
                cohort_version: COHORT_VERSION.to_string(),
                fallback_level: u8::try_from(fallback_level).unwrap_or(3),
                successful_costs_10000: costs,
                successful_active_time_ms: active_times,
            },
            snapshot_digest,
        }))
    }
}

#[cfg(test)]
mod tests;
