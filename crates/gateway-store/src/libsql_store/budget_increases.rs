//! Temporary budget increases. A new increase supersedes the one still active for its budget.
use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};
use gateway_core::BudgetIncreaseRecord;

const COLUMNS: &str = "budget_increase_id, budget_id, scope_key, amount_10000, starts_at, \
                       expires_at, created_by_user_id, created_at";
// An increase applies from `starts_at` until it expires or is superseded. Supersession
// is not retroactive: usage that occurred before it still sees the replaced increase.
const APPLIES_AT_1: &str = "starts_at <= ?1 AND expires_at > ?1 \
                            AND (superseded_at IS NULL OR superseded_at > ?1)";
const APPLIES_AT_2: &str = "starts_at <= ?2 AND expires_at > ?2 \
                            AND (superseded_at IS NULL OR superseded_at > ?2)";

pub(super) async fn get_active_budget_increase(
    store: &LibsqlStore,
    budget_id: Uuid,
    at: OffsetDateTime,
) -> Result<Option<BudgetIncreaseRecord>, StoreError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM budget_increases
         WHERE budget_id = ?1 AND {APPLIES_AT_2}
         ORDER BY created_at DESC, budget_increase_id DESC
         LIMIT 1"
    );
    let mut rows = store
        .connection
        .query(
            &sql,
            libsql::params![budget_id.to_string(), at.unix_timestamp()],
        )
        .await
        .map_err(to_query_error)?;
    match rows.next().await.map_err(to_query_error)? {
        Some(row) => decode_budget_increase(&row).map(Some),
        None => Ok(None),
    }
}

pub(super) async fn list_active_budget_increases(
    store: &LibsqlStore,
    at: OffsetDateTime,
) -> Result<Vec<BudgetIncreaseRecord>, StoreError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM budget_increases
         WHERE {APPLIES_AT_1}
         ORDER BY created_at DESC, budget_increase_id DESC"
    );
    let mut rows = store
        .connection
        .query(&sql, [at.unix_timestamp()])
        .await
        .map_err(to_query_error)?;
    let mut increases = Vec::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        increases.push(decode_budget_increase(&row)?);
    }
    Ok(crate::budget_increases::latest_per_budget(increases))
}

pub(super) async fn create_budget_increase(
    store: &LibsqlStore,
    increase: &BudgetIncreaseRecord,
    expected_active: Option<Uuid>,
) -> Result<(), StoreError> {
    // IMMEDIATE takes the write lock up front, so the active-increase check below and
    // the supersede-and-insert see the same state.
    let tx = store
        .connection
        .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
        .await
        .map_err(to_query_error)?;
    let starts_at = increase.starts_at.unix_timestamp();
    let mut rows = tx
        .query(
            &format!(
                "SELECT budget_increase_id FROM budget_increases
                 WHERE budget_id = ?1 AND {APPLIES_AT_2}
                 ORDER BY created_at DESC, budget_increase_id DESC
                 LIMIT 1"
            ),
            libsql::params![increase.budget_id.to_string(), starts_at],
        )
        .await
        .map_err(to_query_error)?;
    let active = match rows.next().await.map_err(to_query_error)? {
        Some(row) => Some(parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?),
        None => None,
    };
    drop(rows);
    if active != expected_active {
        return Err(crate::budget_increases::active_increase_changed());
    }
    tx.execute(
        "UPDATE budget_increases SET superseded_at = ?2
         WHERE budget_id = ?1 AND superseded_at IS NULL AND expires_at > ?2",
        libsql::params![increase.budget_id.to_string(), starts_at],
    )
    .await
    .map_err(to_write_error)?;
    tx.execute(
        "INSERT INTO budget_increases (
             budget_increase_id, budget_id, scope_key, amount_10000, starts_at,
             expires_at, created_by_user_id, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        libsql::params![
            increase.budget_increase_id.to_string(),
            increase.budget_id.to_string(),
            increase.scope_key.as_str(),
            increase.amount_usd.as_scaled_i64(),
            starts_at,
            increase.expires_at.unix_timestamp(),
            increase.created_by_user_id.map(|id| id.to_string()),
            increase.created_at.unix_timestamp(),
        ],
    )
    .await
    .map_err(to_write_error)?;
    tx.commit().await.map_err(to_write_error)
}

fn decode_budget_increase(row: &libsql::Row) -> Result<BudgetIncreaseRecord, StoreError> {
    let created_by: Option<String> = row.get(6).map_err(to_query_error)?;
    Ok(BudgetIncreaseRecord {
        budget_increase_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        budget_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
        scope_key: row.get(2).map_err(to_query_error)?,
        amount_usd: Money4::from_scaled(row.get(3).map_err(to_query_error)?),
        starts_at: unix_to_datetime(row.get(4).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.get(5).map_err(to_query_error)?)?,
        created_by_user_id: created_by.as_deref().map(parse_uuid).transpose()?,
        created_at: unix_to_datetime(row.get(7).map_err(to_query_error)?)?,
    })
}
