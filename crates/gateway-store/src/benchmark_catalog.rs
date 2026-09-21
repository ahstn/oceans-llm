use std::collections::HashSet;

use gateway_core::{BenchmarkSyncState, ModelBenchmarkBinding, ModelBenchmarkScore, StoreError};
use uuid::Uuid;

pub(crate) fn validate_bindings(
    source: &str,
    bindings: &[ModelBenchmarkBinding],
) -> Result<HashSet<Uuid>, StoreError> {
    let mut model_ids = HashSet::with_capacity(bindings.len());
    for binding in bindings {
        if binding.source != source {
            return Err(StoreError::Conflict(format!(
                "benchmark binding source `{}` does not match `{source}`",
                binding.source
            )));
        }
        if !model_ids.insert(binding.model_id) {
            return Err(StoreError::Conflict(format!(
                "duplicate benchmark binding for model `{}`",
                binding.model_id
            )));
        }
    }
    Ok(model_ids)
}

pub(crate) fn validate_scores(
    scores: &[ModelBenchmarkScore],
    state: &BenchmarkSyncState,
) -> Result<(), StoreError> {
    let mut keys = HashSet::with_capacity(scores.len());
    for score in scores {
        if score.source != state.source
            || score.benchmark_version != state.benchmark_version
            || score.fetched_at != state.last_successful_refresh_at
        {
            return Err(StoreError::Conflict(
                "benchmark score provenance does not match sync state".to_string(),
            ));
        }
        if !score.value.is_finite() {
            return Err(StoreError::Serialization(
                "benchmark score value must be finite".to_string(),
            ));
        }
        if !keys.insert((
            score.model_id,
            score.source.as_str(),
            score.metric_key.as_str(),
        )) {
            return Err(StoreError::Conflict(format!(
                "duplicate benchmark score `{}` for model `{}`",
                score.metric_key, score.model_id
            )));
        }
    }
    Ok(())
}
