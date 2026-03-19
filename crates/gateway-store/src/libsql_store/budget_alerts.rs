use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

const ALERT_STATUS_SQL: &str = r#"
CASE
    WHEN SUM(CASE WHEN d.delivery_status = 'failed' THEN 1 ELSE 0 END) > 0 THEN 'failed'
    WHEN SUM(CASE WHEN d.delivery_status = 'pending' THEN 1 ELSE 0 END) > 0 THEN 'pending'
    ELSE 'sent'
END
"#;

fn normalize_query(query: &BudgetAlertHistoryQuery) -> (i64, i64, i64) {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let offset = i64::from(page.saturating_sub(1) * page_size);
    (i64::from(page), i64::from(page_size), offset)
}

fn decode_history_row(row: &libsql::Row) -> Result<BudgetAlertHistoryRecord, StoreError> {
    let budget_alert_id: String = row.get(0).map_err(to_query_error)?;
    let owner_kind: String = row.get(1).map_err(to_query_error)?;
    let owner_id: String = row.get(2).map_err(to_query_error)?;
    let channel: String = row.get(4).map_err(to_query_error)?;
    let delivery_status: String = row.get(5).map_err(to_query_error)?;
    let cadence: String = row.get(8).map_err(to_query_error)?;
    let window_start: i64 = row.get(9).map_err(to_query_error)?;
    let window_end: i64 = row.get(10).map_err(to_query_error)?;
    let spend_before_10000: i64 = row.get(11).map_err(to_query_error)?;
    let spend_after_10000: i64 = row.get(12).map_err(to_query_error)?;
    let remaining_budget_10000: i64 = row.get(13).map_err(to_query_error)?;
    let created_at: i64 = row.get(14).map_err(to_query_error)?;
    let last_attempted_at: Option<i64> = row.get(15).map_err(to_query_error)?;
    let sent_at: Option<i64> = row.get(16).map_err(to_query_error)?;

    Ok(BudgetAlertHistoryRecord {
        budget_alert_id: parse_uuid(&budget_alert_id)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
        })?,
        owner_id: parse_uuid(&owner_id)?,
        owner_name: row.get(3).map_err(to_query_error)?,
        channel: BudgetAlertChannel::from_db(&channel)
            .ok_or_else(|| StoreError::Serialization(format!("unknown channel `{channel}`")))?,
        delivery_status: BudgetAlertDeliveryStatus::from_db(&delivery_status).ok_or_else(
            || StoreError::Serialization(format!("unknown delivery status `{delivery_status}`")),
        )?,
        recipient_summary: row.get(6).map_err(to_query_error)?,
        threshold_bps: row.get(7).map_err(to_query_error)?,
        cadence: BudgetCadence::from_db(&cadence)
            .ok_or_else(|| StoreError::Serialization(format!("unknown cadence `{cadence}`")))?,
        window_start: unix_to_datetime(window_start)?,
        window_end: unix_to_datetime(window_end)?,
        spend_before_usd: Money4::from_scaled(spend_before_10000),
        spend_after_usd: Money4::from_scaled(spend_after_10000),
        remaining_budget_usd: Money4::from_scaled(remaining_budget_10000),
        created_at: unix_to_datetime(created_at)?,
        last_attempted_at: last_attempted_at.map(unix_to_datetime).transpose()?,
        sent_at: sent_at.map(unix_to_datetime).transpose()?,
        failure_reason: row.get(17).map_err(to_query_error)?,
    })
}

fn decode_dispatch_row(row: &libsql::Row) -> Result<BudgetAlertDispatchTask, StoreError> {
    let budget_alert_id: String = row.get(0).map_err(to_query_error)?;
    let owner_kind: String = row.get(2).map_err(to_query_error)?;
    let owner_id: String = row.get(3).map_err(to_query_error)?;
    let budget_id: String = row.get(5).map_err(to_query_error)?;
    let cadence: String = row.get(6).map_err(to_query_error)?;
    let window_start: i64 = row.get(8).map_err(to_query_error)?;
    let window_end: i64 = row.get(9).map_err(to_query_error)?;
    let created_at: i64 = row.get(13).map_err(to_query_error)?;
    let updated_at: i64 = row.get(14).map_err(to_query_error)?;
    let delivery_id: String = row.get(15).map_err(to_query_error)?;
    let channel: String = row.get(17).map_err(to_query_error)?;
    let delivery_status: String = row.get(18).map_err(to_query_error)?;
    let queued_at: i64 = row.get(22).map_err(to_query_error)?;
    let last_attempted_at: Option<i64> = row.get(23).map_err(to_query_error)?;
    let sent_at: Option<i64> = row.get(24).map_err(to_query_error)?;
    let delivery_updated_at: i64 = row.get(25).map_err(to_query_error)?;

    Ok(BudgetAlertDispatchTask {
        alert: BudgetAlertRecord {
            budget_alert_id: parse_uuid(&budget_alert_id)?,
            ownership_scope_key: row.get(1).map_err(to_query_error)?,
            owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
                StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
            })?,
            owner_id: parse_uuid(&owner_id)?,
            owner_name: row.get(4).map_err(to_query_error)?,
            budget_id: parse_uuid(&budget_id)?,
            cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
                StoreError::Serialization(format!("unknown cadence `{cadence}`"))
            })?,
            threshold_bps: row.get(7).map_err(to_query_error)?,
            window_start: unix_to_datetime(window_start)?,
            window_end: unix_to_datetime(window_end)?,
            spend_before_usd: Money4::from_scaled(row.get(10).map_err(to_query_error)?),
            spend_after_usd: Money4::from_scaled(row.get(11).map_err(to_query_error)?),
            remaining_budget_usd: Money4::from_scaled(row.get(12).map_err(to_query_error)?),
            created_at: unix_to_datetime(created_at)?,
            updated_at: unix_to_datetime(updated_at)?,
        },
        delivery: BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: parse_uuid(&delivery_id)?,
            budget_alert_id: parse_uuid(&budget_alert_id)?,
            channel: BudgetAlertChannel::from_db(&channel)
                .ok_or_else(|| StoreError::Serialization(format!("unknown channel `{channel}`")))?,
            delivery_status: BudgetAlertDeliveryStatus::from_db(&delivery_status).ok_or_else(
                || StoreError::Serialization(format!("unknown delivery status `{delivery_status}`")),
            )?,
            recipient: row.get(19).map_err(to_query_error)?,
            provider_message_id: row.get(20).map_err(to_query_error)?,
            failure_reason: row.get(21).map_err(to_query_error)?,
            queued_at: unix_to_datetime(queued_at)?,
            last_attempted_at: last_attempted_at.map(unix_to_datetime).transpose()?,
            sent_at: sent_at.map(unix_to_datetime).transpose()?,
            updated_at: unix_to_datetime(delivery_updated_at)?,
        },
    })
}

fn is_unique_violation(error: &libsql::Error) -> bool {
    let message = error.to_string();
    message.contains("UNIQUE constraint failed") || message.contains("unique constraint failed")
}

#[async_trait]
impl BudgetAlertRepository for LibsqlStore {
    async fn create_budget_alert_with_deliveries(
        &self,
        alert: &BudgetAlertRecord,
        deliveries: &[BudgetAlertDeliveryRecord],
    ) -> Result<bool, StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let inserted = tx
            .execute(
                r#"
                INSERT INTO budget_alerts (
                    budget_alert_id, ownership_scope_key, owner_kind, owner_id, owner_name,
                    budget_id, cadence, threshold_bps, window_start, window_end,
                    spend_before_10000, spend_after_10000, remaining_budget_10000,
                    created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
                libsql::params![
                    alert.budget_alert_id.to_string(),
                    alert.ownership_scope_key.as_str(),
                    alert.owner_kind.as_str(),
                    alert.owner_id.to_string(),
                    alert.owner_name.as_str(),
                    alert.budget_id.to_string(),
                    alert.cadence.as_str(),
                    alert.threshold_bps,
                    alert.window_start.unix_timestamp(),
                    alert.window_end.unix_timestamp(),
                    alert.spend_before_usd.as_scaled_i64(),
                    alert.spend_after_usd.as_scaled_i64(),
                    alert.remaining_budget_usd.as_scaled_i64(),
                    alert.created_at.unix_timestamp(),
                    alert.updated_at.unix_timestamp(),
                ],
            )
            .await;

        if let Err(error) = inserted {
            if is_unique_violation(&error) {
                tx.rollback()
                    .await
                    .map_err(|rollback| StoreError::Query(rollback.to_string()))?;
                return Ok(false);
            }
            return Err(StoreError::Query(error.to_string()));
        }

        for delivery in deliveries {
            tx.execute(
                r#"
                INSERT INTO budget_alert_deliveries (
                    budget_alert_delivery_id, budget_alert_id, channel, delivery_status, recipient,
                    provider_message_id, failure_reason, queued_at, last_attempted_at, sent_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                libsql::params![
                    delivery.budget_alert_delivery_id.to_string(),
                    delivery.budget_alert_id.to_string(),
                    delivery.channel.as_str(),
                    delivery.delivery_status.as_str(),
                    delivery.recipient.as_deref(),
                    delivery.provider_message_id.as_deref(),
                    delivery.failure_reason.as_deref(),
                    delivery.queued_at.unix_timestamp(),
                    delivery.last_attempted_at.map(|value| value.unix_timestamp()),
                    delivery.sent_at.map(|value| value.unix_timestamp()),
                    delivery.updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
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
                    COALESCE(REPLACE(GROUP_CONCAT(DISTINCT d.recipient), ',', ', '), '(no recipient)') AS recipient_summary,
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
                WHERE (?1 IS NULL OR a.owner_kind = ?1)
                  AND (?2 IS NULL OR d.channel = ?2)
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

        let count_sql =
            format!("{summary_cte} SELECT COUNT(*) FROM summary WHERE (?3 IS NULL OR delivery_status = ?3)");
        let mut count_rows = self
            .connection
            .query(
                &count_sql,
                libsql::params![owner_kind.as_deref(), channel.as_deref(), delivery_status.as_deref()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        let total_row = count_rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
            .ok_or_else(|| StoreError::Unexpected("missing alert history count row".to_string()))?;
        let total: i64 = total_row.get(0).map_err(to_query_error)?;

        let page_sql = format!(
            "{summary_cte}
            SELECT *
            FROM summary
            WHERE (?3 IS NULL OR delivery_status = ?3)
            ORDER BY created_at DESC, budget_alert_id DESC
            LIMIT ?4 OFFSET ?5"
        );
        let mut rows = self
            .connection
            .query(
                &page_sql,
                libsql::params![
                    owner_kind.as_deref(),
                    channel.as_deref(),
                    delivery_status.as_deref(),
                    page_size,
                    offset
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut items = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            items.push(decode_history_row(&row)?);
        }

        Ok(BudgetAlertHistoryPage {
            items,
            page: page as u32,
            page_size: page_size as u32,
            total: total as u64,
        })
    }

    async fn claim_pending_budget_alert_delivery_tasks(
        &self,
        limit: u32,
        claimed_at: OffsetDateTime,
    ) -> Result<Vec<BudgetAlertDispatchTask>, StoreError> {
        let limit = i64::from(limit.clamp(1, 200));
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut rows = tx
            .query(
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
                LIMIT ?1
                "#,
                [limit],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut candidates = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            candidates.push(decode_dispatch_row(&row)?);
        }
        drop(rows);

        let mut tasks = Vec::new();
        for mut task in candidates {
            let claimed = tx
                .execute(
                r#"
                UPDATE budget_alert_deliveries
                SET last_attempted_at = ?1,
                    updated_at = ?2
                WHERE budget_alert_delivery_id = ?3
                  AND delivery_status = 'pending'
                  AND last_attempted_at IS NULL
                "#,
                libsql::params![
                    claimed_at.unix_timestamp(),
                    claimed_at.unix_timestamp(),
                    task.delivery.budget_alert_delivery_id.to_string()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
            if claimed > 0 {
                task.delivery.last_attempted_at = Some(claimed_at);
                tasks.push(task);
            }
        }

        tx.commit()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(tasks)
    }

    async fn mark_budget_alert_delivery_sent(
        &self,
        delivery_id: Uuid,
        provider_message_id: Option<&str>,
        sent_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE budget_alert_deliveries
                SET delivery_status = 'sent',
                    provider_message_id = ?1,
                    sent_at = ?2,
                    last_attempted_at = COALESCE(last_attempted_at, ?2),
                    updated_at = ?2
                WHERE budget_alert_delivery_id = ?3
                "#,
                libsql::params![
                    provider_message_id,
                    sent_at.unix_timestamp(),
                    delivery_id.to_string()
                ],
            )
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
        self.connection
            .execute(
                r#"
                UPDATE budget_alert_deliveries
                SET delivery_status = 'failed',
                    failure_reason = ?1,
                    last_attempted_at = COALESCE(last_attempted_at, ?2),
                    updated_at = ?2
                WHERE budget_alert_delivery_id = ?3
                "#,
                libsql::params![
                    failure_reason,
                    failed_at.unix_timestamp(),
                    delivery_id.to_string()
                ],
            )
            .await
            .map_err(to_query_error)?;
        Ok(())
    }
}
