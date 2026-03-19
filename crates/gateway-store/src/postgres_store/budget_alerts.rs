use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

const ALERT_STATUS_SQL: &str = r#"
CASE
    WHEN SUM(CASE WHEN d.delivery_status = 'failed' THEN 1 ELSE 0 END) > 0 THEN 'failed'
    WHEN SUM(CASE WHEN d.delivery_status = 'pending' THEN 1 ELSE 0 END) > 0 THEN 'pending'
    ELSE 'sent'
END
"#;

fn normalize_query(query: &BudgetAlertHistoryQuery) -> (u32, u32, i64) {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let offset = i64::from(page.saturating_sub(1) * page_size);
    (page, page_size, offset)
}

fn decode_history_row(row: &PgRow) -> Result<BudgetAlertHistoryRecord, StoreError> {
    let owner_kind: String = row.try_get(1).map_err(to_query_error)?;
    let channel: String = row.try_get(4).map_err(to_query_error)?;
    let delivery_status: String = row.try_get(5).map_err(to_query_error)?;
    let cadence: String = row.try_get(8).map_err(to_query_error)?;
    let window_start: i64 = row.try_get(9).map_err(to_query_error)?;
    let window_end: i64 = row.try_get(10).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(14).map_err(to_query_error)?;
    let last_attempted_at: Option<i64> = row.try_get(15).map_err(to_query_error)?;
    let sent_at: Option<i64> = row.try_get(16).map_err(to_query_error)?;

    Ok(BudgetAlertHistoryRecord {
        budget_alert_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
        })?,
        owner_id: parse_uuid(&row.try_get::<String, _>(2).map_err(to_query_error)?)?,
        owner_name: row.try_get(3).map_err(to_query_error)?,
        channel: BudgetAlertChannel::from_db(&channel)
            .ok_or_else(|| StoreError::Serialization(format!("unknown channel `{channel}`")))?,
        delivery_status: BudgetAlertDeliveryStatus::from_db(&delivery_status).ok_or_else(
            || StoreError::Serialization(format!("unknown delivery status `{delivery_status}`")),
        )?,
        recipient_summary: row.try_get(6).map_err(to_query_error)?,
        threshold_bps: row.try_get(7).map_err(to_query_error)?,
        cadence: BudgetCadence::from_db(&cadence)
            .ok_or_else(|| StoreError::Serialization(format!("unknown cadence `{cadence}`")))?,
        window_start: unix_to_datetime(window_start)?,
        window_end: unix_to_datetime(window_end)?,
        spend_before_usd: Money4::from_scaled(row.try_get(11).map_err(to_query_error)?),
        spend_after_usd: Money4::from_scaled(row.try_get(12).map_err(to_query_error)?),
        remaining_budget_usd: Money4::from_scaled(row.try_get(13).map_err(to_query_error)?),
        created_at: unix_to_datetime(created_at)?,
        last_attempted_at: last_attempted_at.map(unix_to_datetime).transpose()?,
        sent_at: sent_at.map(unix_to_datetime).transpose()?,
        failure_reason: row.try_get(17).map_err(to_query_error)?,
    })
}

fn decode_dispatch_row(row: &PgRow) -> Result<BudgetAlertDispatchTask, StoreError> {
    let owner_kind: String = row.try_get(2).map_err(to_query_error)?;
    let cadence: String = row.try_get(6).map_err(to_query_error)?;
    let window_start: i64 = row.try_get(8).map_err(to_query_error)?;
    let window_end: i64 = row.try_get(9).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(13).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(14).map_err(to_query_error)?;
    let channel: String = row.try_get(17).map_err(to_query_error)?;
    let delivery_status: String = row.try_get(18).map_err(to_query_error)?;
    let queued_at: i64 = row.try_get(22).map_err(to_query_error)?;
    let last_attempted_at: Option<i64> = row.try_get(23).map_err(to_query_error)?;
    let sent_at: Option<i64> = row.try_get(24).map_err(to_query_error)?;
    let delivery_updated_at: i64 = row.try_get(25).map_err(to_query_error)?;

    Ok(BudgetAlertDispatchTask {
        alert: BudgetAlertRecord {
            budget_alert_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
            ownership_scope_key: row.try_get(1).map_err(to_query_error)?,
            owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
                StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
            })?,
            owner_id: parse_uuid(&row.try_get::<String, _>(3).map_err(to_query_error)?)?,
            owner_name: row.try_get(4).map_err(to_query_error)?,
            budget_id: parse_uuid(&row.try_get::<String, _>(5).map_err(to_query_error)?)?,
            cadence: BudgetCadence::from_db(&cadence)
                .ok_or_else(|| StoreError::Serialization(format!("unknown cadence `{cadence}`")))?,
            threshold_bps: row.try_get(7).map_err(to_query_error)?,
            window_start: unix_to_datetime(window_start)?,
            window_end: unix_to_datetime(window_end)?,
            spend_before_usd: Money4::from_scaled(row.try_get(10).map_err(to_query_error)?),
            spend_after_usd: Money4::from_scaled(row.try_get(11).map_err(to_query_error)?),
            remaining_budget_usd: Money4::from_scaled(row.try_get(12).map_err(to_query_error)?),
            created_at: unix_to_datetime(created_at)?,
            updated_at: unix_to_datetime(updated_at)?,
        },
        delivery: BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: parse_uuid(&row.try_get::<String, _>(15).map_err(to_query_error)?)?,
            budget_alert_id: parse_uuid(&row.try_get::<String, _>(16).map_err(to_query_error)?)?,
            channel: BudgetAlertChannel::from_db(&channel)
                .ok_or_else(|| StoreError::Serialization(format!("unknown channel `{channel}`")))?,
            delivery_status: BudgetAlertDeliveryStatus::from_db(&delivery_status).ok_or_else(
                || StoreError::Serialization(format!("unknown delivery status `{delivery_status}`")),
            )?,
            recipient: row.try_get(19).map_err(to_query_error)?,
            provider_message_id: row.try_get(20).map_err(to_query_error)?,
            failure_reason: row.try_get(21).map_err(to_query_error)?,
            queued_at: unix_to_datetime(queued_at)?,
            last_attempted_at: last_attempted_at.map(unix_to_datetime).transpose()?,
            sent_at: sent_at.map(unix_to_datetime).transpose()?,
            updated_at: unix_to_datetime(delivery_updated_at)?,
        },
    })
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db_error| db_error.code())
        .is_some_and(|code| code == "23505")
}

#[async_trait]
impl BudgetAlertRepository for PostgresStore {
    async fn create_budget_alert_with_deliveries(
        &self,
        alert: &BudgetAlertRecord,
        deliveries: &[BudgetAlertDeliveryRecord],
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;

        let inserted = sqlx::query(
            r#"
            INSERT INTO budget_alerts (
                budget_alert_id, ownership_scope_key, owner_kind, owner_id, owner_name,
                budget_id, cadence, threshold_bps, window_start, window_end,
                spend_before_10000, spend_after_10000, remaining_budget_10000,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15
            )
            "#,
        )
        .bind(alert.budget_alert_id.to_string())
        .bind(alert.ownership_scope_key.as_str())
        .bind(alert.owner_kind.as_str())
        .bind(alert.owner_id.to_string())
        .bind(alert.owner_name.as_str())
        .bind(alert.budget_id.to_string())
        .bind(alert.cadence.as_str())
        .bind(alert.threshold_bps)
        .bind(alert.window_start.unix_timestamp())
        .bind(alert.window_end.unix_timestamp())
        .bind(alert.spend_before_usd.as_scaled_i64())
        .bind(alert.spend_after_usd.as_scaled_i64())
        .bind(alert.remaining_budget_usd.as_scaled_i64())
        .bind(alert.created_at.unix_timestamp())
        .bind(alert.updated_at.unix_timestamp())
        .execute(&mut *tx)
        .await;

        if let Err(error) = inserted {
            if is_unique_violation(&error) {
                tx.rollback().await.map_err(to_query_error)?;
                return Ok(false);
            }
            return Err(to_query_error(error));
        }

        for delivery in deliveries {
            sqlx::query(
                r#"
                INSERT INTO budget_alert_deliveries (
                    budget_alert_delivery_id, budget_alert_id, channel, delivery_status, recipient,
                    provider_message_id, failure_reason, queued_at, last_attempted_at, sent_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                "#,
            )
            .bind(delivery.budget_alert_delivery_id.to_string())
            .bind(delivery.budget_alert_id.to_string())
            .bind(delivery.channel.as_str())
            .bind(delivery.delivery_status.as_str())
            .bind(delivery.recipient.as_deref())
            .bind(delivery.provider_message_id.as_deref())
            .bind(delivery.failure_reason.as_deref())
            .bind(delivery.queued_at.unix_timestamp())
            .bind(delivery.last_attempted_at.map(|value| value.unix_timestamp()))
            .bind(delivery.sent_at.map(|value| value.unix_timestamp()))
            .bind(delivery.updated_at.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
        }

        tx.commit().await.map_err(to_query_error)?;
        Ok(true)
    }

    async fn list_budget_alert_history(
        &self,
        query: &BudgetAlertHistoryQuery,
    ) -> Result<BudgetAlertHistoryPage, StoreError> {
        let (page, page_size, offset) = normalize_query(query);
        let owner_kind = query.owner_kind.map(|value| value.as_str().to_string());
        let channel = query.channel.map(|value| value.as_str().to_string());
        let delivery_status = query.delivery_status.map(|value| value.as_str().to_string());
        let summary_cte = format!(
            r#"
            WITH summary AS (
                SELECT
                    a.budget_alert_id,
                    a.owner_kind,
                    a.owner_id,
                    a.owner_name,
                    COALESCE(MIN(d.channel), 'email') AS channel,
                    {status_sql} AS delivery_status,
                    COALESCE(string_agg(DISTINCT d.recipient, ', ') FILTER (WHERE d.recipient IS NOT NULL), '(no recipient)') AS recipient_summary,
                    a.threshold_bps,
                    a.cadence,
                    a.window_start,
                    a.window_end,
                    a.spend_before_10000,
                    a.spend_after_10000,
                    a.remaining_budget_10000,
                    a.created_at,
                    MAX(d.last_attempted_at) AS last_attempted_at,
                    MAX(d.sent_at) AS sent_at,
                    MAX(CASE WHEN d.delivery_status = 'failed' THEN d.failure_reason END) AS failure_reason
                FROM budget_alerts a
                LEFT JOIN budget_alert_deliveries d
                  ON d.budget_alert_id = a.budget_alert_id
                WHERE ($1::TEXT IS NULL OR a.owner_kind = $1)
                  AND ($2::TEXT IS NULL OR d.channel = $2)
                GROUP BY
                    a.budget_alert_id,
                    a.owner_kind,
                    a.owner_id,
                    a.owner_name,
                    a.threshold_bps,
                    a.cadence,
                    a.window_start,
                    a.window_end,
                    a.spend_before_10000,
                    a.spend_after_10000,
                    a.remaining_budget_10000,
                    a.created_at
            )
            "#,
            status_sql = ALERT_STATUS_SQL.trim()
        );

        let count_row = sqlx::query(&format!(
            "{summary_cte} SELECT COUNT(*) FROM summary WHERE ($3::TEXT IS NULL OR delivery_status = $3)"
        ))
        .bind(owner_kind.as_deref())
        .bind(channel.as_deref())
        .bind(delivery_status.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;
        let total: i64 = count_row.try_get(0).map_err(to_query_error)?;

        let rows = sqlx::query(&format!(
            "{summary_cte}
            SELECT *
            FROM summary
            WHERE ($3::TEXT IS NULL OR delivery_status = $3)
            ORDER BY created_at DESC, budget_alert_id DESC
            LIMIT $4 OFFSET $5"
        ))
        .bind(owner_kind.as_deref())
        .bind(channel.as_deref())
        .bind(delivery_status.as_deref())
        .bind(i64::from(page_size))
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        let items = rows
            .iter()
            .map(decode_history_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(BudgetAlertHistoryPage {
            items,
            page,
            page_size,
            total: total as u64,
        })
    }

    async fn claim_pending_budget_alert_delivery_tasks(
        &self,
        limit: u32,
        claimed_at: OffsetDateTime,
    ) -> Result<Vec<BudgetAlertDispatchTask>, StoreError> {
        let limit = limit.clamp(1, 200) as i64;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;

        let rows = sqlx::query(
            r#"
            SELECT
                a.budget_alert_id,
                a.ownership_scope_key,
                a.owner_kind,
                a.owner_id,
                a.owner_name,
                a.budget_id,
                a.cadence,
                a.threshold_bps,
                a.window_start,
                a.window_end,
                a.spend_before_10000,
                a.spend_after_10000,
                a.remaining_budget_10000,
                a.created_at,
                a.updated_at,
                d.budget_alert_delivery_id,
                d.budget_alert_id,
                d.channel,
                d.delivery_status,
                d.recipient,
                d.provider_message_id,
                d.failure_reason,
                d.queued_at,
                d.last_attempted_at,
                d.sent_at,
                d.updated_at
            FROM budget_alert_deliveries d
            INNER JOIN budget_alerts a
              ON a.budget_alert_id = d.budget_alert_id
            WHERE d.delivery_status = 'pending'
              AND d.last_attempted_at IS NULL
            ORDER BY d.queued_at ASC, d.budget_alert_delivery_id ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(to_query_error)?;

        let mut tasks = rows
            .iter()
            .map(decode_dispatch_row)
            .collect::<Result<Vec<_>, _>>()?;

        for task in &mut tasks {
            sqlx::query(
                r#"
                UPDATE budget_alert_deliveries
                SET last_attempted_at = $1,
                    updated_at = $1
                WHERE budget_alert_delivery_id = $2
                "#,
            )
            .bind(claimed_at.unix_timestamp())
            .bind(task.delivery.budget_alert_delivery_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
            task.delivery.last_attempted_at = Some(claimed_at);
        }

        tx.commit().await.map_err(to_query_error)?;
        Ok(tasks)
    }

    async fn mark_budget_alert_delivery_sent(
        &self,
        delivery_id: Uuid,
        provider_message_id: Option<&str>,
        sent_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE budget_alert_deliveries
            SET delivery_status = 'sent',
                provider_message_id = $1,
                sent_at = $2,
                last_attempted_at = COALESCE(last_attempted_at, $2),
                updated_at = $2
            WHERE budget_alert_delivery_id = $3
            "#,
        )
        .bind(provider_message_id)
        .bind(sent_at.unix_timestamp())
        .bind(delivery_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }

    async fn mark_budget_alert_delivery_failed(
        &self,
        delivery_id: Uuid,
        failure_reason: &str,
        failed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE budget_alert_deliveries
            SET delivery_status = 'failed',
                failure_reason = $1,
                last_attempted_at = COALESCE(last_attempted_at, $2),
                updated_at = $2
            WHERE budget_alert_delivery_id = $3
            "#,
        )
        .bind(failure_reason)
        .bind(failed_at.unix_timestamp())
        .bind(delivery_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}
