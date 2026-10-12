//! Backend-neutral helpers for temporary budget increases.
use std::collections::HashSet;

use gateway_core::{BudgetIncreaseRecord, StoreError};

/// Keep the newest increase per budget from rows ordered newest first, matching
/// `get_active_budget_increase` if more than one row applies at the same instant.
pub(crate) fn latest_per_budget(increases: Vec<BudgetIncreaseRecord>) -> Vec<BudgetIncreaseRecord> {
    let mut seen = HashSet::new();
    increases
        .into_iter()
        .filter(|increase| seen.insert(increase.budget_id))
        .collect()
}

/// Another increase was created for the budget after the caller checked eligibility.
pub(crate) fn active_increase_changed() -> StoreError {
    StoreError::Conflict(
        "the budget's active increase changed while this one was being created; reload and try again"
            .to_string(),
    )
}
