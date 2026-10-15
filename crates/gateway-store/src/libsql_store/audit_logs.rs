use async_trait::async_trait;
use gateway_core::{
    AuditActor, AuditActorKind, AuditLogPage, AuditLogQuery, AuditLogRecord, AuditLogRepository,
    NewAuditLogRecord, StoreError,
};

use super::{LibsqlStore, to_query_error};
use crate::audit_logs::{
    AuditLogFilter, AuditLogRow, finish_page, parse_json, parse_optional_json,
};
use crate::shared::{parse_uuid, unix_millis_to_datetime};

const AUDIT_LOG_COLUMNS: &str = "id, ts, actor_kind, actor_user_id, actor_api_key_id, \
    actor_label, action, object_type, object_id, target_label, before_json, after_json, \
    details_json, request_ip, user_agent";

fn decode(row: &libsql::Row) -> Result<AuditLogRecord, StoreError> {
    let actor_kind: String = row.get(2).map_err(to_query_error)?;
    let actor_user_id: Option<String> = row.get(3).map_err(to_query_error)?;
    let actor_api_key_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let before: Option<String> = row.get(10).map_err(to_query_error)?;
    let after: Option<String> = row.get(11).map_err(to_query_error)?;
    let details: String = row.get(12).map_err(to_query_error)?;
    Ok(AuditLogRecord {
        id: row.get(0).map_err(to_query_error)?,
        occurred_at: unix_millis_to_datetime(row.get(1).map_err(to_query_error)?)?,
        actor: AuditActor {
            kind: AuditActorKind::from_db(&actor_kind).ok_or_else(|| {
                StoreError::Serialization(format!("unknown audit actor kind `{actor_kind}`"))
            })?,
            user_id: actor_user_id.as_deref().map(parse_uuid).transpose()?,
            api_key_id: actor_api_key_id.as_deref().map(parse_uuid).transpose()?,
            label: row.get(5).map_err(to_query_error)?,
        },
        action: row.get(6).map_err(to_query_error)?,
        target_type: row.get(7).map_err(to_query_error)?,
        target_id: row.get(8).map_err(to_query_error)?,
        target_label: row.get(9).map_err(to_query_error)?,
        before: parse_optional_json(before.as_deref())?,
        after: parse_optional_json(after.as_deref())?,
        metadata: parse_json(&details)?,
        request_ip: row.get(13).map_err(to_query_error)?,
        user_agent: row.get(14).map_err(to_query_error)?,
    })
}

#[async_trait]
impl AuditLogRepository for LibsqlStore {
    async fn insert_audit_log(&self, record: &NewAuditLogRecord) -> Result<i64, StoreError> {
        let row = AuditLogRow::from_record(record)?;
        let mut rows = self
            .connection
            .query(
                r#"
                INSERT INTO audit_logs (
                    ts, actor_kind, actor_user_id, actor_api_key_id, actor_label, action,
                    object_type, object_id, target_label, before_json, after_json, details_json,
                    request_ip, user_agent
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                RETURNING id
                "#,
                libsql::params![
                    row.ts,
                    row.actor_kind,
                    row.actor_user_id,
                    row.actor_api_key_id,
                    record.actor.label.as_deref(),
                    record.action.as_str(),
                    record.target_type.as_str(),
                    record.target_id.as_deref(),
                    record.target_label.as_deref(),
                    row.before_json,
                    row.after_json,
                    row.details_json,
                    record.request_ip.as_deref(),
                    record.user_agent.as_deref(),
                ],
            )
            .await
            .map_err(to_query_error)?;
        let row = rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::Query("audit log insert returned no id".to_string()))?;
        row.get(0).map_err(to_query_error)
    }

    async fn list_audit_logs(&self, query: &AuditLogQuery) -> Result<AuditLogPage, StoreError> {
        let filter = AuditLogFilter::from_query(query)?;
        let sql = format!(
            r#"
            SELECT {AUDIT_LOG_COLUMNS}
            FROM audit_logs
            WHERE (?1 IS NULL OR actor_user_id = ?1)
              AND (?2 IS NULL OR action = ?2)
              AND (?3 IS NULL OR object_type = ?3)
              AND (?4 IS NULL OR object_id = ?4)
              AND (?5 IS NULL OR ts >= ?5)
              AND (?6 IS NULL OR ts <= ?6)
              AND (?7 IS NULL OR ts < ?7 OR (ts = ?7 AND id < ?8))
            ORDER BY ts DESC, id DESC
            LIMIT ?9
            "#
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    filter.actor_user_id,
                    filter.action,
                    filter.target_type,
                    filter.target_id,
                    filter.start_ms,
                    filter.end_ms,
                    filter.cursor_ts,
                    filter.cursor_id,
                    filter.fetch_limit,
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            items.push(decode(&row)?);
        }
        Ok(finish_page(items, filter.page_size))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::run_migrations;

    #[tokio::test]
    async fn audit_logs_round_trip_filter_and_page() {
        let temporary = tempdir().expect("tempdir");
        let database_path = temporary.path().join("gateway.db");
        run_migrations(&database_path).await.expect("migrations");
        let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
            .await
            .expect("store");
        crate::audit_logs::exercise_audit_log_store(&store).await;
    }
}
