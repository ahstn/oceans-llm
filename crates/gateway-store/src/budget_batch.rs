//! Wire rows for set-based budget queries shared by both SQL backends.
use gateway_core::{BudgetScope, BudgetScopeWindow, BudgetUpsert, StoreError};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::shared::serialize_json;

#[derive(Serialize)]
struct ScopeRow<'a> {
    scope_key: String,
    scope_kind: &'static str,
    user_id: Option<Uuid>,
    service_account_id: Option<Uuid>,
    model_id: Option<Uuid>,
    upstream_model: Option<&'a str>,
}

impl<'a> From<&'a BudgetScope> for ScopeRow<'a> {
    fn from(scope: &'a BudgetScope) -> Self {
        Self {
            scope_key: scope.scope_key(),
            scope_kind: scope.kind().as_str(),
            user_id: scope.user_id(),
            service_account_id: scope.service_account_id(),
            model_id: scope.model_id(),
            upstream_model: scope.upstream_model().map(str::trim),
        }
    }
}

#[derive(Serialize)]
struct UpsertRow<'a> {
    #[serde(flatten)]
    scope: ScopeRow<'a>,
    budget_id: Uuid,
    cadence: &'static str,
    amount_10000: i64,
    hard_limit: i32,
    timezone: &'a str,
    source_kind: &'static str,
    source_key: Option<&'a str>,
    expected_kind: Option<&'static str>,
    expected_key: Option<&'a str>,
    updated_at: i64,
}

pub(crate) fn serialize_upserts(
    upserts: &[BudgetUpsert<'_>],
    now: OffsetDateTime,
) -> Result<String, StoreError> {
    let rows = upserts
        .iter()
        .map(|write| UpsertRow {
            scope: ScopeRow::from(&write.scope),
            budget_id: Uuid::new_v4(),
            cadence: write.settings.cadence.as_str(),
            amount_10000: write.settings.amount_usd.as_scaled_i64(),
            hard_limit: i32::from(write.settings.hard_limit),
            timezone: &write.settings.timezone,
            source_kind: write.source.kind.as_str(),
            source_key: write.source.key.as_deref(),
            expected_kind: write
                .expected_current_source
                .map(|source| source.kind.as_str()),
            expected_key: write
                .expected_current_source
                .and_then(|source| source.key.as_deref()),
            updated_at: now.unix_timestamp(),
        })
        .collect::<Vec<_>>();
    serialize_json(&rows)
}

#[derive(Serialize)]
struct WindowRow<'a> {
    #[serde(flatten)]
    scope: ScopeRow<'a>,
    window_start: i64,
    window_end: i64,
}

pub(crate) fn serialize_windows(windows: &[BudgetScopeWindow<'_>]) -> Result<String, StoreError> {
    let rows = windows
        .iter()
        .map(|window| WindowRow {
            scope: ScopeRow::from(window.scope),
            window_start: window.window_start.unix_timestamp(),
            window_end: window.window_end.unix_timestamp(),
        })
        .collect::<Vec<_>>();
    serialize_json(&rows)
}
