//! Backend-neutral helpers for temporary budget increases.
use std::collections::HashSet;

use gateway_core::BudgetIncreaseRecord;

/// Keep the newest increase per budget from rows ordered newest first.
/// Concurrent creates can leave two unsuperseded rows; the newest one wins,
/// matching `get_active_budget_increase`.
pub(crate) fn latest_per_budget(increases: Vec<BudgetIncreaseRecord>) -> Vec<BudgetIncreaseRecord> {
    let mut seen = HashSet::new();
    increases
        .into_iter()
        .filter(|increase| seen.insert(increase.budget_id))
        .collect()
}
