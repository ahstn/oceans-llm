use async_trait::async_trait;
use gateway_core::{
    RateLimitConsumeResult, RateLimitCounterCharge, RateLimitCounterIncrement,
    RateLimitCounterObservation, RateLimitPolicyRecord, RateLimitRepository, StoreError,
};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

use super::{PostgresStore, to_query_error, to_write_error};
use crate::rate_limits::{MAX_PURGE_BATCH, POLICY_COLUMNS, RawPolicyRow, lock_order};

fn decode(row: &PgRow) -> Result<RateLimitPolicyRecord, StoreError> {
    RawPolicyRow {
        rate_limit_policy_id: row.try_get(0).map_err(to_query_error)?,
        name: row.try_get(1).map_err(to_query_error)?,
        operation: row.try_get(2).map_err(to_query_error)?,
        scope_kind: row.try_get(3).map_err(to_query_error)?,
        scope_id: row.try_get(4).map_err(to_query_error)?,
        model_key: row.try_get(5).map_err(to_query_error)?,
        mcp_server_id: row.try_get(6).map_err(to_query_error)?,
        metric: row.try_get(7).map_err(to_query_error)?,
        period: row.try_get(8).map_err(to_query_error)?,
        limit: row.try_get(9).map_err(to_query_error)?,
        enabled: row.try_get(10).map_err(to_query_error)?,
        created_at: row.try_get(11).map_err(to_query_error)?,
        updated_at: row.try_get(12).map_err(to_query_error)?,
    }
    .decode()
}

async fn read_amount(
    executor: impl sqlx::PgExecutor<'_>,
    counter_key: &str,
    window_start: i64,
) -> Result<i64, StoreError> {
    let amount: Option<i64> = sqlx::query_scalar(
        "SELECT amount FROM rate_limit_counters WHERE counter_key = $1 AND window_start = $2",
    )
    .bind(counter_key)
    .bind(window_start)
    .fetch_optional(executor)
    .await
    .map_err(to_query_error)?;
    Ok(amount.unwrap_or(0))
}

const ADD_AMOUNT: &str =
    "INSERT INTO rate_limit_counters (counter_key, window_start, amount, expires_at)
     VALUES ($1, $2, $3, $4)
     ON CONFLICT (counter_key, window_start)
     DO UPDATE SET amount = rate_limit_counters.amount + EXCLUDED.amount,
                   expires_at = GREATEST(rate_limit_counters.expires_at, EXCLUDED.expires_at)
     RETURNING amount";

#[async_trait]
impl RateLimitRepository for PostgresStore {
    async fn list_rate_limit_policies(&self) -> Result<Vec<RateLimitPolicyRecord>, StoreError> {
        let rows = sqlx::query(&format!(
            "SELECT {POLICY_COLUMNS} FROM rate_limit_policies ORDER BY name"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode).collect()
    }

    async fn list_enabled_rate_limit_policies(
        &self,
    ) -> Result<Vec<RateLimitPolicyRecord>, StoreError> {
        let rows = sqlx::query(&format!(
            "SELECT {POLICY_COLUMNS} FROM rate_limit_policies WHERE enabled = 1 ORDER BY name"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode).collect()
    }

    async fn get_rate_limit_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<RateLimitPolicyRecord>, StoreError> {
        let row = sqlx::query(&format!(
            "SELECT {POLICY_COLUMNS} FROM rate_limit_policies WHERE rate_limit_policy_id = $1"
        ))
        .bind(policy_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        row.as_ref().map(decode).transpose()
    }

    async fn insert_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(&format!(
            "INSERT INTO rate_limit_policies ({POLICY_COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"
        ))
        .bind(policy.rate_limit_policy_id.to_string())
        .bind(&policy.name)
        .bind(policy.operation.as_str())
        .bind(policy.scope_kind.as_str())
        .bind(policy.scope_id.map(|id| id.to_string()))
        .bind(policy.model_key.as_deref())
        .bind(policy.mcp_server_id.map(|id| id.to_string()))
        .bind(policy.metric.as_str())
        .bind(policy.period.as_str())
        .bind(policy.limit)
        .bind(i64::from(policy.enabled))
        .bind(policy.created_at.unix_timestamp())
        .bind(policy.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn update_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE rate_limit_policies SET
                name = $2, operation = $3, scope_kind = $4, scope_id = $5, model_key = $6,
                mcp_server_id = $7, metric = $8, period = $9, limit_value = $10, enabled = $11,
                updated_at = $12
             WHERE rate_limit_policy_id = $1",
        )
        .bind(policy.rate_limit_policy_id.to_string())
        .bind(&policy.name)
        .bind(policy.operation.as_str())
        .bind(policy.scope_kind.as_str())
        .bind(policy.scope_id.map(|id| id.to_string()))
        .bind(policy.model_key.as_deref())
        .bind(policy.mcp_server_id.map(|id| id.to_string()))
        .bind(policy.metric.as_str())
        .bind(policy.period.as_str())
        .bind(policy.limit)
        .bind(i64::from(policy.enabled))
        .bind(policy.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_rate_limit_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<RateLimitPolicyRecord>, StoreError> {
        // `RETURNING` gives the policy this call removed, as it was at deletion.
        let row = sqlx::query(&format!(
            "DELETE FROM rate_limit_policies WHERE rate_limit_policy_id = $1 RETURNING {POLICY_COLUMNS}"
        ))
        .bind(policy_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        row.as_ref().map(decode).transpose()
    }

    async fn consume_rate_limits(
        &self,
        charges: &[RateLimitCounterCharge],
    ) -> Result<RateLimitConsumeResult, StoreError> {
        let mut observations = vec![
            RateLimitCounterObservation {
                previous: 0,
                current: 0,
                admitted: true,
            };
            charges.len()
        ];
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let mut admitted = true;
        for index in lock_order(charges) {
            let charge = &charges[index];
            // A request charge upserts zero into the previous window's row
            // first. Unlike a locking read, the upsert also waits for an
            // insert of that row that has not committed yet, so a check just
            // after a window boundary sees every write to the window it reads.
            // A row created here expires when the current window ends, as the
            // previous window's row would. Rows are taken in (counter, window)
            // order: previous before current within a counter, counters in key
            // order, as every transaction does.
            let previous = match charge.window.previous_start {
                Some(start) if charge.cost > 0 => sqlx::query_scalar(ADD_AMOUNT)
                    .bind(&charge.counter_key)
                    .bind(start)
                    .bind(0_i64)
                    .bind(charge.window.end)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(to_query_error)?,
                Some(start) => read_amount(&mut *tx, &charge.counter_key, start).await?,
                None => 0,
            };
            // The upsert holds the row lock until commit, which serializes
            // concurrent checks of this counter across every replica.
            let current = if charge.cost > 0 {
                sqlx::query_scalar(ADD_AMOUNT)
                    .bind(&charge.counter_key)
                    .bind(charge.window.start)
                    .bind(charge.cost)
                    .bind(charge.window.expires_at())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(to_query_error)?
            } else {
                read_amount(&mut *tx, &charge.counter_key, charge.window.start).await?
            };
            let fits = charge.admits(previous, current);
            admitted &= fits;
            observations[index] = RateLimitCounterObservation {
                previous,
                current,
                admitted: fits,
            };
        }
        if admitted {
            tx.commit().await.map_err(to_query_error)?;
        } else {
            tx.rollback().await.map_err(to_query_error)?;
        }
        Ok(RateLimitConsumeResult {
            admitted,
            observations,
        })
    }

    async fn add_rate_limit_usage(
        &self,
        increments: &[RateLimitCounterIncrement],
    ) -> Result<(), StoreError> {
        // All or nothing, so one response is never charged to only some of
        // its policies. Rows are taken in the same order `consume_rate_limits`
        // uses, so the two cannot deadlock.
        let mut increments: Vec<_> = increments
            .iter()
            .filter(|increment| increment.amount > 0)
            .collect();
        if increments.is_empty() {
            return Ok(());
        }
        increments.sort_by(|left, right| {
            (left.counter_key.as_str(), left.window.start)
                .cmp(&(right.counter_key.as_str(), right.window.start))
        });
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        for increment in increments {
            sqlx::query(ADD_AMOUNT)
                .bind(&increment.counter_key)
                .bind(increment.window.start)
                .bind(increment.amount)
                .bind(increment.window.expires_at())
                .execute(&mut *tx)
                .await
                .map_err(to_query_error)?;
        }
        tx.commit().await.map_err(to_query_error)
    }

    async fn purge_expired_rate_limit_counters(
        &self,
        now: i64,
        limit: i64,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            "DELETE FROM rate_limit_counters WHERE (counter_key, window_start) IN (
                 SELECT counter_key, window_start FROM rate_limit_counters
                 WHERE expires_at <= $1 ORDER BY expires_at LIMIT $2
                 FOR UPDATE SKIP LOCKED
             )",
        )
        .bind(now)
        .bind(limit.clamp(1, MAX_PURGE_BATCH))
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(result.rows_affected())
    }
}
