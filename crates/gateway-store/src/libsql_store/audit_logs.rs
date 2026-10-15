use async_trait::async_trait;
use gateway_core::{
    AuditActor, AuditActorKind, AuditLogPage, AuditLogQuery, AuditLogRecord, AuditLogRepository,
    NewAuditLogRecord, StoreError,
};

use super::{LibsqlStore, to_query_error};
use crate::audit_logs::{
    AUDIT_LOG_COLUMNS, AuditLogBind, AuditLogFilter, AuditLogRow, finish_page, list_sql,
    parse_json, parse_optional_json,
};
use crate::shared::{parse_uuid, unix_millis_to_datetime};

fn decode(row: &libsql::Row) -> Result<AuditLogRecord, StoreError> {
    let actor_kind: String = row.get(2).map_err(to_query_error)?;
    let actor_user_id: Option<String> = row.get(3).map_err(to_query_error)?;
    let actor_api_key_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let before: Option<String> = row.get(10).map_err(to_query_error)?;
    let after: Option<String> = row.get(11).map_err(to_query_error)?;
    let details: String = row.get(12).map_err(to_query_error)?;
    let (api_key_id, label) = crate::audit_logs::decode_api_key_actor(
        actor_api_key_id,
        row.get(5).map_err(to_query_error)?,
    );
    Ok(AuditLogRecord {
        id: row.get(0).map_err(to_query_error)?,
        occurred_at: unix_millis_to_datetime(row.get(1).map_err(to_query_error)?)?,
        actor: AuditActor {
            kind: AuditActorKind::from_db(&actor_kind).ok_or_else(|| {
                StoreError::Serialization(format!("unknown audit actor kind `{actor_kind}`"))
            })?,
            user_id: actor_user_id.as_deref().map(parse_uuid).transpose()?,
            api_key_id,
            label,
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
        let (sql, binds) = list_sql(&filter, |index| format!("?{index}"));
        let params = binds.into_iter().map(|bind| match bind {
            AuditLogBind::Text(value) => libsql::Value::Text(value),
            AuditLogBind::Int(value) => libsql::Value::Integer(value),
        });
        let mut rows = self
            .connection
            .query(&sql, libsql::params_from_iter(params))
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            items.push(decode(&row)?);
        }
        Ok(finish_page(items, filter.page_size))
    }

    async fn get_audit_log(&self, id: i64) -> Result<Option<AuditLogRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                &format!("SELECT {AUDIT_LOG_COLUMNS} FROM audit_logs WHERE id = ?1"),
                [id],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode(&row))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::run_migrations;

    #[tokio::test]
    async fn the_migration_keeps_legacy_actors_and_converts_timestamps() {
        use crate::migration_registry::{MIGRATION_REGISTRY, MigrationBackend};

        let temporary = tempdir().expect("tempdir");
        let database_path = temporary.path().join("gateway.db");
        let database = libsql::Builder::new_local(database_path.to_str().expect("database path"))
            .build()
            .await
            .expect("database");
        let connection = database.connect().expect("connection");
        let apply = |below: i64, at_least: i64| {
            MIGRATION_REGISTRY
                .iter()
                .filter(move |migration| {
                    i64::from(migration.version) >= at_least && i64::from(migration.version) < below
                })
                .map(|migration| migration.sql_for(MigrationBackend::Libsql))
        };
        for sql in apply(61, 0) {
            connection
                .execute_batch(sql)
                .await
                .expect("earlier migration");
        }
        // The legacy row only needs a key id, not a key.
        connection
            .execute("PRAGMA foreign_keys = OFF", ())
            .await
            .expect("pragma");
        connection
            .execute(
                "INSERT INTO audit_logs (ts, actor_api_key_id, action, object_type, details_json) \
                 VALUES (1700000000, 'legacy-key', 'legacy.action', 'api_key', '{}'), \
                        (1700000001, NULL, 'legacy.system', 'api_key', '{}')",
                (),
            )
            .await
            .expect("legacy rows");
        for sql in apply(62, 61) {
            connection
                .execute_batch(sql)
                .await
                .expect("audit migration");
        }

        let mut rows = connection
            .query("SELECT actor_kind, ts FROM audit_logs ORDER BY ts", ())
            .await
            .expect("query");
        let mut kinds = Vec::new();
        let mut timestamps = Vec::new();
        while let Some(row) = rows.next().await.expect("row") {
            kinds.push(row.get::<String>(0).expect("actor kind"));
            timestamps.push(row.get::<i64>(1).expect("ts"));
        }
        assert_eq!(kinds, ["api_key", "system"]);
        // Legacy seconds become milliseconds.
        assert_eq!(timestamps, [1_700_000_000_000, 1_700_000_001_000]);
        drop(rows);
        drop(connection);
        drop(database);

        // The legacy rows list and open through the store, even with a key id
        // that is not a UUID.
        let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
            .await
            .expect("store");
        let page = store
            .list_audit_logs(&AuditLogQuery::default())
            .await
            .expect("list legacy rows");
        assert_eq!(page.items.len(), 2);
        let legacy = page
            .items
            .iter()
            .find(|item| item.action == "legacy.action")
            .expect("legacy key row");
        assert_eq!(legacy.actor.kind, AuditActorKind::ApiKey);
        assert_eq!(legacy.actor.api_key_id, None);
        assert_eq!(legacy.actor.label.as_deref(), Some("API key legacy-key"));
        let detail = store
            .get_audit_log(legacy.id)
            .await
            .expect("open legacy row")
            .expect("legacy row exists");
        assert_eq!(detail.actor.label.as_deref(), Some("API key legacy-key"));
    }

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

    #[tokio::test]
    async fn filtered_pages_read_the_matching_index_in_page_order() {
        let temporary = tempdir().expect("tempdir");
        let database_path = temporary.path().join("gateway.db");
        run_migrations(&database_path).await.expect("migrations");
        let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
            .await
            .expect("store");
        let cases = [
            (
                AuditLogQuery {
                    action: Some("api_key.created".to_string()),
                    ..Default::default()
                },
                "audit_logs_action_idx",
            ),
            (
                AuditLogQuery {
                    target_type: Some("team".to_string()),
                    target_id: Some("t-1".to_string()),
                    cursor: Some(gateway_core::AuditLogCursor {
                        occurred_at_ms: 1,
                        id: 1,
                    }),
                    ..Default::default()
                },
                "audit_logs_target_idx",
            ),
            (
                AuditLogQuery {
                    actor_user_id: Some(uuid::Uuid::nil()),
                    action: Some("team.updated".to_string()),
                    ..Default::default()
                },
                "audit_logs_actor_action_idx",
            ),
            (
                AuditLogQuery {
                    target_id: Some("t-1".to_string()),
                    ..Default::default()
                },
                "audit_logs_object_id_idx",
            ),
            (
                AuditLogQuery {
                    target_type: Some("team".to_string()),
                    ..Default::default()
                },
                "audit_logs_object_type_idx",
            ),
            (AuditLogQuery::default(), "audit_logs_ts_idx"),
        ];
        for (query, index) in cases {
            let filter = AuditLogFilter::from_query(&query).expect("filter");
            let (sql, binds) = list_sql(&filter, |position| format!("?{position}"));
            let params = binds.into_iter().map(|bind| match bind {
                AuditLogBind::Text(value) => libsql::Value::Text(value),
                AuditLogBind::Int(value) => libsql::Value::Integer(value),
            });
            let mut rows = store
                .connection
                .query(
                    &format!("EXPLAIN QUERY PLAN {sql}"),
                    libsql::params_from_iter(params),
                )
                .await
                .expect("query plan");
            let mut plan = String::new();
            while let Some(row) = rows.next().await.expect("plan row") {
                let detail: String = row.get(3).expect("plan detail");
                plan.push_str(&detail);
                plan.push('\n');
            }
            assert!(plan.contains(index), "expected {index} in plan:\n{plan}");
            assert!(
                !plan.contains("TEMP B-TREE"),
                "page order needs no sort:\n{plan}"
            );
        }
    }
}
