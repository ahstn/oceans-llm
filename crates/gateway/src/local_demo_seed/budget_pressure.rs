//! Current-window spend for a few budgeted demo users, so Spend controls has rows near and
//! over their limits. Each target is topped up to a share of its configured budget at seed
//! time, so the rows land in the same state whatever day the seed runs.
use std::collections::HashMap;

use anyhow::Context;
use gateway_core::{ApiKeyRecord, BudgetRepository, BudgetScope, budget_window_utc};
use gateway_store::AnyStore;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::history::{HistoryRequest, insert_history_request};

pub(super) struct BudgetPressureTarget {
    pub api_key_public_id: &'static str,
    pub email: &'static str,
    /// Current-window spend to reach, as a percentage of the budget.
    pub percent_of_limit: i64,
}

/// The budgets themselves live in `gateway.yaml`.
pub(super) const BUDGET_PRESSURE_TARGETS: &[BudgetPressureTarget] = &[
    // Over a hard limit: requests are blocked until someone increases the budget.
    BudgetPressureTarget {
        api_key_public_id: "locdemogina1",
        email: "gina@research.local",
        percent_of_limit: 104,
    },
    BudgetPressureTarget {
        api_key_public_id: "locdemoerin1",
        email: "erin@research.local",
        percent_of_limit: 92,
    },
    BudgetPressureTarget {
        api_key_public_id: "locdemograce1",
        email: "grace@applied.local",
        percent_of_limit: 88,
    },
    // Inside the window by a whisker.
    BudgetPressureTarget {
        api_key_public_id: "locdemoheidi1",
        email: "heidi@applied.local",
        percent_of_limit: 86,
    },
    // Busy but not yet eligible for an increase.
    BudgetPressureTarget {
        api_key_public_id: "locdemojudy1",
        email: "judy@operations.local",
        percent_of_limit: 62,
    },
];

const MODEL: (&str, &str, &str) = (
    "claude-sonnet",
    "vertex-claude",
    "anthropic/claude-sonnet-4-6",
);
const HARNESS: (&str, &str, &str) = ("codex", "Codex", "codex/1.0.0 (local demo)");

pub(super) fn budget_pressure_request_ids() -> impl Iterator<Item = String> {
    BUDGET_PRESSURE_TARGETS.iter().map(request_id)
}

fn request_id(target: &BudgetPressureTarget) -> String {
    format!("demo-budget-pressure-{}", target.api_key_public_id)
}

pub(super) async fn seed_demo_budget_pressure(
    store: &AnyStore,
    api_keys: &HashMap<&'static str, ApiKeyRecord>,
    user_team_ids: &HashMap<&'static str, Option<Uuid>>,
    model_ids: &HashMap<&'static str, Uuid>,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    for target in BUDGET_PRESSURE_TARGETS {
        let api_key = api_keys
            .get(target.api_key_public_id)
            .ok_or_else(|| anyhow::anyhow!("missing demo key `{}`", target.api_key_public_id))?;
        let user_id = api_key
            .owner_user_id
            .ok_or_else(|| anyhow::anyhow!("budget pressure key must be user-owned"))?;
        let Some(budget) = store
            .get_active_budget_by_scope(&BudgetScope::User { user_id })
            .await
            .with_context(|| format!("failed loading budget for `{}`", target.email))?
        else {
            // The budget comes from gateway.yaml; a trimmed config simply skips the top-up.
            continue;
        };
        let window = budget_window_utc(budget.settings.cadence, now);
        let spent = store
            .sum_usage_cost_for_budget_scope_in_window(
                &budget.scope,
                window.period_start,
                window.observed_end,
            )
            .await
            .with_context(|| format!("failed summing spend for `{}`", target.email))?;
        let goal = budget.settings.amount_usd.as_scaled_i64() * target.percent_of_limit / 100;
        let top_up = goal - spent.as_scaled_i64();
        if top_up <= 0 {
            continue;
        }
        let request = HistoryRequest {
            request_id: request_id(target),
            occurred_at: (now - Duration::minutes(2)).max(window.period_start),
            model: MODEL,
            harness: HARNESS,
            // Splits the cost 40/60 across input and output at the history rates.
            prompt_tokens: top_up * 40 / 3,
            completion_tokens: top_up * 4,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_scaled: top_up,
        };
        let team_id = user_team_ids.get(target.email).copied().flatten();
        insert_history_request(store, api_key, team_id, model_ids, &request)
            .await
            .with_context(|| format!("failed inserting `{}`", request.request_id))?;
    }
    Ok(())
}
