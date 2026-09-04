//! Set-based budget operations. JSON input keeps bind counts independent of directory size.
use super::*;
use crate::budget_batch::{serialize_upserts, serialize_windows};
use crate::shared::serialize_json;
use gateway_core::{BudgetScopeWindow, BudgetUpsert};
use std::collections::HashMap;

const STATES: &str = r#"
WITH ranked AS (
    SELECT budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
           upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
           created_at, updated_at, source_kind, source_key,
           ROW_NUMBER() OVER (
        PARTITION BY scope_key ORDER BY updated_at DESC, created_at DESC, is_active DESC, budget_id DESC
    ) AS position
    FROM budgets WHERE scope_key IN (SELECT jsonb_array_elements_text($1::jsonb))
)
SELECT budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
           upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
           created_at, updated_at, source_kind, source_key
FROM ranked WHERE is_active = 1 OR position = 1
"#;

const UPSERT: &str = r#"
WITH input AS (
    SELECT * FROM jsonb_to_recordset($1::jsonb) AS rows(
        scope_key text,
        scope_kind text,
        user_id text,
        service_account_id text,
        model_id text,
        upstream_model text,
        budget_id text,
        cadence text,
        amount_10000 bigint,
        hard_limit integer,
        timezone text,
        source_kind text,
        source_key text,
        expected_kind text,
        expected_key text,
        updated_at bigint
    )
)
INSERT INTO budgets (budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
           upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
           created_at, updated_at, source_kind, source_key)
SELECT budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
       upstream_model, cadence, amount_10000, hard_limit, timezone, 1,
       updated_at, updated_at, source_kind, source_key
FROM input i
WHERE (expected_kind IS NOT NULL AND EXISTS (
    SELECT 1 FROM budgets b WHERE b.scope_key = i.scope_key AND b.is_active = 1
      AND b.source_kind = i.expected_kind AND b.source_key IS NOT DISTINCT FROM i.expected_key
)) OR (expected_kind IS NULL AND NOT EXISTS (
    SELECT 1 FROM budgets b WHERE b.scope_key = i.scope_key AND b.is_active = 1
) AND NOT EXISTS (
    SELECT 1 FROM (SELECT is_active, source_kind, source_key FROM budgets b
        WHERE b.scope_key = i.scope_key
        ORDER BY updated_at DESC, created_at DESC, is_active DESC, budget_id DESC LIMIT 1) latest
    WHERE latest.is_active = 0 AND latest.source_kind = 'manual' AND latest.source_key = 'deactivated'
))
ON CONFLICT(scope_key) WHERE is_active = 1 DO UPDATE SET
    cadence = excluded.cadence, amount_10000 = excluded.amount_10000,
    hard_limit = excluded.hard_limit, timezone = excluded.timezone,
    source_kind = excluded.source_kind, source_key = excluded.source_key,
    updated_at = excluded.updated_at
WHERE EXISTS (SELECT 1 FROM input i WHERE i.scope_key = budgets.scope_key
    AND i.expected_kind = budgets.source_kind AND i.expected_key IS NOT DISTINCT FROM budgets.source_key)
"#;

const DEACTIVATE: &str = r#"
WITH input AS (
    SELECT * FROM jsonb_to_recordset($1::jsonb) AS rows(
        budget_id text,
        source_kind text,
        source_key text
    )
)
UPDATE budgets SET is_active = 0, updated_at = $2
WHERE is_active = 1 AND EXISTS (SELECT 1 FROM input i WHERE i.budget_id = budgets.budget_id
    AND i.source_kind = budgets.source_kind AND i.source_key IS NOT DISTINCT FROM budgets.source_key)
"#;

const SUMS: &str = r#"
WITH input AS (
    SELECT * FROM jsonb_to_recordset($1::jsonb) AS rows(
        scope_key text,
        scope_kind text,
        user_id text,
        service_account_id text,
        model_id text,
        upstream_model text,
        window_start bigint,
        window_end bigint
    )
)
SELECT i.scope_key, COALESCE(SUM(u.computed_cost_10000), 0)::bigint AS total
FROM input i LEFT JOIN usage_cost_events u ON
    u.pricing_status IN ('priced', 'legacy_estimated')
    AND u.occurred_at >= i.window_start AND u.occurred_at < i.window_end
    AND ((i.scope_kind = 'user' AND u.user_id = i.user_id)
      OR (i.scope_kind = 'service_account' AND u.service_account_id = i.service_account_id)
      OR (i.scope_kind = 'user_model' AND u.user_id = i.user_id AND
         ((i.model_id IS NOT NULL AND u.model_id = i.model_id)
          OR (i.model_id IS NULL AND u.model_id IS NULL AND TRIM(u.upstream_model) = i.upstream_model))))
GROUP BY i.scope_key
"#;

pub(super) async fn get_budget_states_by_scope_keys(
    store: &PostgresStore,
    scope_keys: &[String],
) -> Result<Vec<BudgetRecord>, StoreError> {
    if scope_keys.is_empty() {
        return Ok(Vec::new());
    }
    let input = serialize_json(scope_keys)?;
    let rows = sqlx::query(STATES)
        .bind(input)
        .fetch_all(&store.pool)
        .await
        .map_err(to_query_error)?;
    rows.iter().map(decode_budget_record).collect()
}

pub(super) async fn upsert_active_budgets_with_source_guard(
    store: &PostgresStore,
    upserts: &[BudgetUpsert<'_>],
    updated_at: OffsetDateTime,
) -> Result<(), StoreError> {
    if upserts.is_empty() {
        return Ok(());
    }
    let input = serialize_upserts(upserts, updated_at)?;
    sqlx::query(UPSERT)
        .bind(input)
        .execute(&store.pool)
        .await
        .map_err(to_write_error)?;
    Ok(())
}

pub(super) async fn deactivate_budgets_by_source(
    store: &PostgresStore,
    budgets: &[&BudgetRecord],
    updated_at: OffsetDateTime,
) -> Result<(), StoreError> {
    if budgets.is_empty() {
        return Ok(());
    }
    let rows = budgets.iter().map(|budget| serde_json::json!({
        "budget_id": budget.budget_id, "source_kind": budget.source.kind.as_str(), "source_key": budget.source.key
    })).collect::<Vec<_>>();
    let input = serialize_json(&rows)?;
    sqlx::query(DEACTIVATE)
        .bind(input)
        .bind(updated_at.unix_timestamp())
        .execute(&store.pool)
        .await
        .map_err(to_write_error)?;
    Ok(())
}

pub(super) async fn sum_usage_cost_by_budget_scope(
    store: &PostgresStore,
    windows: &[BudgetScopeWindow<'_>],
) -> Result<HashMap<String, Money4>, StoreError> {
    if windows.is_empty() {
        return Ok(HashMap::new());
    }
    let input = serialize_windows(windows)?;
    let mut totals = HashMap::new();
    let rows = sqlx::query(SUMS)
        .bind(input)
        .fetch_all(&store.pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        totals.insert(
            row.try_get("scope_key").map_err(to_query_error)?,
            Money4::from_scaled(row.try_get("total").map_err(to_query_error)?),
        );
    }
    Ok(totals)
}

const CONTACTS: &str = "
    SELECT u.user_id, u.email,
        CASE WHEN u.status = 'active' AND m.role IN ('owner', 'admin')
             THEN m.team_id ELSE NULL END AS alert_team_id
    FROM users u LEFT JOIN team_memberships m ON m.user_id = u.user_id
";

impl PostgresStore {
    pub async fn list_budget_contacts(
        &self,
    ) -> Result<Vec<gateway_core::BudgetContact>, StoreError> {
        let rows = sqlx::query(CONTACTS)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter()
            .map(|row| {
                Ok(gateway_core::BudgetContact {
                    user_id: crate::shared::parse_uuid(
                        row.try_get("user_id").map_err(to_query_error)?,
                    )?,
                    email: row.try_get("email").map_err(to_query_error)?,
                    alert_team_id: row
                        .try_get::<Option<&str>, _>("alert_team_id")
                        .map_err(to_query_error)?
                        .map(crate::shared::parse_uuid)
                        .transpose()?,
                })
            })
            .collect()
    }
}
