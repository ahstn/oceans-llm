//! Backend-neutral helpers for temporary budget increases.
use std::collections::HashSet;

use gateway_core::{BudgetCadence, BudgetIncreaseBasis, BudgetIncreaseRecord, Money4, StoreError};
use uuid::Uuid;

/// Keep the newest increase per budget from rows ordered newest first, matching
/// `get_active_budget_increase` if more than one row applies at the same instant.
pub(crate) fn latest_per_budget(increases: Vec<BudgetIncreaseRecord>) -> Vec<BudgetIncreaseRecord> {
    let mut seen = HashSet::new();
    increases
        .into_iter()
        .filter(|increase| seen.insert(increase.budget_id))
        .collect()
}

/// The budget row as read under its lock, for comparison with a `BudgetIncreaseBasis`.
pub(crate) struct LockedBudget {
    pub is_active: bool,
    pub cadence: String,
    pub amount_10000: i64,
    pub timezone: String,
}

/// Fail with a conflict unless the locked budget and its active increase still match
/// what the caller validated the new increase against.
pub(crate) fn check_increase_basis(
    locked: Option<LockedBudget>,
    active_increase_id: Option<Uuid>,
    basis: &BudgetIncreaseBasis,
) -> Result<(), StoreError> {
    let budget_unchanged = locked.is_some_and(|budget| {
        budget.is_active
            && BudgetCadence::from_db(&budget.cadence) == Some(basis.settings.cadence)
            && Money4::from_scaled(budget.amount_10000) == basis.settings.amount_usd
            && budget.timezone == basis.settings.timezone
    });
    if budget_unchanged && active_increase_id == basis.active_increase_id {
        return Ok(());
    }
    Err(StoreError::Conflict(
        "the budget changed while this increase was being created; reload and try again"
            .to_string(),
    ))
}
