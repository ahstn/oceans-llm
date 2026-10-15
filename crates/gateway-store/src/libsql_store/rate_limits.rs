use async_trait::async_trait;
use gateway_core::{
    RateLimitConsumeResult, RateLimitCounterCharge, RateLimitCounterIncrement,
    RateLimitCounterObservation, RateLimitPolicyRecord, RateLimitRepository, StoreError,
};
use uuid::Uuid;

use super::{LibsqlStore, to_query_error, to_write_error};
use crate::rate_limits::{MAX_PURGE_BATCH, POLICY_COLUMNS, RawPolicyRow, lock_order};
use gateway_core::Updated;

fn decode(row: &libsql::Row) -> Result<RateLimitPolicyRecord, StoreError> {
    RawPolicyRow {
        rate_limit_policy_id: row.get(0).map_err(to_query_error)?,
        name: row.get(1).map_err(to_query_error)?,
        operation: row.get(2).map_err(to_query_error)?,
        scope_kind: row.get(3).map_err(to_query_error)?,
        scope_id: row.get(4).map_err(to_query_error)?,
        model_key: row.get(5).map_err(to_query_error)?,
        mcp_server_id: row.get(6).map_err(to_query_error)?,
        metric: row.get(7).map_err(to_query_error)?,
        period: row.get(8).map_err(to_query_error)?,
        limit: row.get(9).map_err(to_query_error)?,
        enabled: row.get(10).map_err(to_query_error)?,
        created_at: row.get(11).map_err(to_query_error)?,
        updated_at: row.get(12).map_err(to_query_error)?,
    }
    .decode()
}

async fn read_amount(
    connection: &libsql::Connection,
    counter_key: &str,
    window_start: i64,
) -> Result<i64, StoreError> {
    let mut rows = connection
        .query(
            "SELECT amount FROM rate_limit_counters WHERE counter_key = ?1 AND window_start = ?2",
            libsql::params![counter_key, window_start],
        )
        .await
        .map_err(to_query_error)?;
    match rows.next().await.map_err(to_query_error)? {
        Some(row) => row.get(0).map_err(to_query_error),
        None => Ok(0),
    }
}

async fn add_amount(
    connection: &libsql::Connection,
    counter_key: &str,
    window_start: i64,
    amount: i64,
    expires_at: i64,
) -> Result<i64, StoreError> {
    let mut rows = connection
        .query(
            "INSERT INTO rate_limit_counters (counter_key, window_start, amount, expires_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (counter_key, window_start)
             DO UPDATE SET amount = rate_limit_counters.amount + excluded.amount,
                           expires_at = MAX(rate_limit_counters.expires_at, excluded.expires_at)
             RETURNING amount",
            libsql::params![counter_key, window_start, amount, expires_at],
        )
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::Unexpected("rate limit counter upsert returned no row".into()))?
        .get(0)
        .map_err(to_query_error)
}

#[async_trait]
impl RateLimitRepository for LibsqlStore {
    async fn list_rate_limit_policies(&self) -> Result<Vec<RateLimitPolicyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                &format!("SELECT {POLICY_COLUMNS} FROM rate_limit_policies ORDER BY name"),
                (),
            )
            .await
            .map_err(to_query_error)?;
        let mut policies = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            policies.push(decode(&row)?);
        }
        Ok(policies)
    }

    async fn list_enabled_rate_limit_policies(
        &self,
    ) -> Result<Vec<RateLimitPolicyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                &format!(
                    "SELECT {POLICY_COLUMNS} FROM rate_limit_policies WHERE enabled = 1 ORDER BY name"
                ),
                (),
            )
            .await
            .map_err(to_query_error)?;
        let mut policies = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            policies.push(decode(&row)?);
        }
        Ok(policies)
    }

    async fn get_rate_limit_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<RateLimitPolicyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                &format!(
                    "SELECT {POLICY_COLUMNS} FROM rate_limit_policies WHERE rate_limit_policy_id = ?1"
                ),
                [policy_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode)
            .transpose()
    }

    async fn insert_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                &format!(
                    "INSERT INTO rate_limit_policies ({POLICY_COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
                ),
                libsql::params![
                    policy.rate_limit_policy_id.to_string(),
                    policy.name.as_str(),
                    policy.operation.as_str(),
                    policy.scope_kind.as_str(),
                    policy.scope_id.map(|id| id.to_string()),
                    policy.model_key.as_deref(),
                    policy.mcp_server_id.map(|id| id.to_string()),
                    policy.metric.as_str(),
                    policy.period.as_str(),
                    policy.limit,
                    i64::from(policy.enabled),
                    policy.created_at.unix_timestamp(),
                    policy.updated_at.unix_timestamp()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn update_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<Option<Updated<RateLimitPolicyRecord>>, StoreError> {
        // An immediate transaction holds the write lock, so the policy read here is
        // exactly the one this update replaces.
        let connection = self.transaction_connection().await?;
        let tx = connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_write_error)?;
        let mut rows = tx
            .query(
                &format!(
                    "SELECT {POLICY_COLUMNS} FROM rate_limit_policies WHERE rate_limit_policy_id = ?1"
                ),
                [policy.rate_limit_policy_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let existing = rows
            .next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode)
            .transpose()?;
        drop(rows);
        let Some(existing) = existing else {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(None);
        };
        if existing.same_settings(policy) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Some(Updated::unchanged(existing)));
        }
        tx.execute(
            "UPDATE rate_limit_policies SET
                    name = ?2, operation = ?3, scope_kind = ?4, scope_id = ?5, model_key = ?6,
                    mcp_server_id = ?7, metric = ?8, period = ?9, limit_value = ?10,
                    enabled = ?11, updated_at = ?12
                 WHERE rate_limit_policy_id = ?1",
            libsql::params![
                policy.rate_limit_policy_id.to_string(),
                policy.name.as_str(),
                policy.operation.as_str(),
                policy.scope_kind.as_str(),
                policy.scope_id.map(|id| id.to_string()),
                policy.model_key.as_deref(),
                policy.mcp_server_id.map(|id| id.to_string()),
                policy.metric.as_str(),
                policy.period.as_str(),
                policy.limit,
                i64::from(policy.enabled),
                policy.updated_at.unix_timestamp()
            ],
        )
        .await
        .map_err(to_write_error)?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Some(Updated {
            current: policy.clone(),
            previous: Some(existing),
        }))
    }

    async fn delete_rate_limit_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<RateLimitPolicyRecord>, StoreError> {
        // `RETURNING` gives the policy this call removed, as it was at deletion.
        let mut rows = self
            .connection
            .query(
                &format!(
                    "DELETE FROM rate_limit_policies WHERE rate_limit_policy_id = ?1 RETURNING {POLICY_COLUMNS}"
                ),
                [policy_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode)
            .transpose()
    }

    async fn consume_rate_limits(
        &self,
        charges: &[RateLimitCounterCharge],
    ) -> Result<RateLimitConsumeResult, StoreError> {
        let charges = charges.to_vec();
        // Only request charges write. A check made only of token charges reads,
        // so it takes a deferred transaction and no writer lock.
        let behavior = if charges.iter().any(|charge| charge.cost > 0) {
            libsql::TransactionBehavior::Immediate
        } else {
            libsql::TransactionBehavior::Deferred
        };
        self.run_routing_operation(move |connection| async move {
            let transaction = connection
                .transaction_with_behavior(behavior)
                .await
                .map_err(to_query_error)?;
            let mut observations = vec![
                RateLimitCounterObservation {
                    previous: 0,
                    current: 0,
                    admitted: true,
                };
                charges.len()
            ];
            let mut admitted = true;
            for index in lock_order(&charges) {
                let charge = &charges[index];
                let current = if charge.cost > 0 {
                    add_amount(
                        &transaction,
                        &charge.counter_key,
                        charge.window.start,
                        charge.cost,
                        charge.window.expires_at(),
                    )
                    .await?
                } else {
                    read_amount(&transaction, &charge.counter_key, charge.window.start).await?
                };
                let previous = match charge.window.previous_start {
                    Some(start) => read_amount(&transaction, &charge.counter_key, start).await?,
                    None => 0,
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
                transaction.commit().await.map_err(to_query_error)?;
            } else {
                transaction.rollback().await.map_err(to_query_error)?;
            }
            Ok(RateLimitConsumeResult {
                admitted,
                observations,
            })
        })
        .await
    }

    async fn add_rate_limit_usage(
        &self,
        increments: &[RateLimitCounterIncrement],
    ) -> Result<(), StoreError> {
        let mut increments: Vec<_> = increments
            .iter()
            .filter(|increment| increment.amount > 0)
            .cloned()
            .collect();
        if increments.is_empty() {
            return Ok(());
        }
        increments.sort_by(|left, right| {
            (left.counter_key.as_str(), left.window.start)
                .cmp(&(right.counter_key.as_str(), right.window.start))
        });
        self.run_routing_operation(move |connection| async move {
            // All or nothing, so one response is never charged to only some
            // of its policies.
            let transaction = connection
                .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
                .await
                .map_err(to_query_error)?;
            for increment in &increments {
                add_amount(
                    &transaction,
                    &increment.counter_key,
                    increment.window.start,
                    increment.amount,
                    increment.window.expires_at(),
                )
                .await?;
            }
            transaction.commit().await.map_err(to_query_error)
        })
        .await
    }

    async fn purge_expired_rate_limit_counters(
        &self,
        now: i64,
        limit: i64,
    ) -> Result<u64, StoreError> {
        let limit = limit.clamp(1, MAX_PURGE_BATCH);
        self.run_routing_operation(move |connection| async move {
            connection
                .execute(
                    "DELETE FROM rate_limit_counters WHERE rowid IN (
                         SELECT rowid FROM rate_limit_counters
                         WHERE expires_at <= ?1 ORDER BY expires_at LIMIT ?2
                     )",
                    libsql::params![now, limit],
                )
                .await
                .map_err(to_query_error)
        })
        .await
    }
}
