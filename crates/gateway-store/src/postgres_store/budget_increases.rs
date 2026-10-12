//! Temporary budget increases. A new increase supersedes the one still active for its budget.
use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};
use gateway_core::BudgetIncreaseRecord;

const COLUMNS: &str = "budget_increase_id, budget_id, scope_key, amount_10000, starts_at, \
                       expires_at, created_by_user_id, created_at";

pub(super) async fn get_active_budget_increase(
    store: &PostgresStore,
    budget_id: Uuid,
    at: OffsetDateTime,
) -> Result<Option<BudgetIncreaseRecord>, StoreError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM budget_increases
         WHERE budget_id = $1 AND superseded_at IS NULL AND starts_at <= $2 AND expires_at > $2
         ORDER BY created_at DESC, budget_increase_id DESC
         LIMIT 1"
    );
    let row = sqlx::query(&sql)
        .bind(budget_id.to_string())
        .bind(at.unix_timestamp())
        .fetch_optional(&store.pool)
        .await
        .map_err(to_query_error)?;
    row.as_ref().map(decode_budget_increase).transpose()
}

pub(super) async fn list_active_budget_increases(
    store: &PostgresStore,
    at: OffsetDateTime,
) -> Result<Vec<BudgetIncreaseRecord>, StoreError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM budget_increases
         WHERE superseded_at IS NULL AND starts_at <= $1 AND expires_at > $1
         ORDER BY created_at DESC, budget_increase_id DESC"
    );
    let rows = sqlx::query(&sql)
        .bind(at.unix_timestamp())
        .fetch_all(&store.pool)
        .await
        .map_err(to_query_error)?;
    let increases = rows
        .iter()
        .map(decode_budget_increase)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(crate::budget_increases::latest_per_budget(increases))
}

pub(super) async fn create_budget_increase(
    store: &PostgresStore,
    increase: &BudgetIncreaseRecord,
) -> Result<(), StoreError> {
    let mut tx = store.pool.begin().await.map_err(to_query_error)?;
    // Serialize creates per budget. Under READ COMMITTED, two concurrent supersede
    // UPDATEs cannot see each other's uncommitted insert, so both would stay active.
    sqlx::query("SELECT budget_id FROM budgets WHERE budget_id = $1 FOR UPDATE")
        .bind(increase.budget_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;
    let starts_at = increase.starts_at.unix_timestamp();
    sqlx::query(
        "UPDATE budget_increases SET superseded_at = $2
         WHERE budget_id = $1 AND superseded_at IS NULL AND expires_at > $2",
    )
    .bind(increase.budget_id.to_string())
    .bind(starts_at)
    .execute(&mut *tx)
    .await
    .map_err(to_write_error)?;
    sqlx::query(
        "INSERT INTO budget_increases (
             budget_increase_id, budget_id, scope_key, amount_10000, starts_at,
             expires_at, created_by_user_id, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(increase.budget_increase_id.to_string())
    .bind(increase.budget_id.to_string())
    .bind(increase.scope_key.as_str())
    .bind(increase.amount_usd.as_scaled_i64())
    .bind(starts_at)
    .bind(increase.expires_at.unix_timestamp())
    .bind(increase.created_by_user_id.map(|id| id.to_string()))
    .bind(increase.created_at.unix_timestamp())
    .execute(&mut *tx)
    .await
    .map_err(to_write_error)?;
    tx.commit().await.map_err(to_write_error)
}

fn decode_budget_increase(row: &sqlx::postgres::PgRow) -> Result<BudgetIncreaseRecord, StoreError> {
    let created_by: Option<String> = row.try_get(6).map_err(to_query_error)?;
    Ok(BudgetIncreaseRecord {
        budget_increase_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        budget_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        scope_key: row.try_get(2).map_err(to_query_error)?,
        amount_usd: Money4::from_scaled(row.try_get(3).map_err(to_query_error)?),
        starts_at: unix_to_datetime(row.try_get(4).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.try_get(5).map_err(to_query_error)?)?,
        created_by_user_id: created_by.as_deref().map(parse_uuid).transpose()?,
        created_at: unix_to_datetime(row.try_get(7).map_err(to_query_error)?)?,
    })
}
