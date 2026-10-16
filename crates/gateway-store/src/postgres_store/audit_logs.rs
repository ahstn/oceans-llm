use async_trait::async_trait;
use gateway_core::{
    AuditActor, AuditActorKind, AuditLogPage, AuditLogQuery, AuditLogRecord, AuditLogRepository,
    NewAuditLogRecord, StoreError,
};
use sqlx::{Row, postgres::PgRow};

use super::{PostgresStore, to_query_error};
use crate::audit_logs::{
    AUDIT_LOG_COLUMNS, AuditLogBind, AuditLogFilter, AuditLogRow, finish_page, list_sql,
    parse_json, parse_optional_json,
};
use crate::shared::{parse_uuid, unix_millis_to_datetime};

fn decode(row: &PgRow) -> Result<AuditLogRecord, StoreError> {
    let actor_kind: String = row.try_get(2).map_err(to_query_error)?;
    let actor_user_id: Option<String> = row.try_get(3).map_err(to_query_error)?;
    let actor_api_key_id: Option<String> = row.try_get(4).map_err(to_query_error)?;
    let before: Option<String> = row.try_get(10).map_err(to_query_error)?;
    let after: Option<String> = row.try_get(11).map_err(to_query_error)?;
    let details: String = row.try_get(12).map_err(to_query_error)?;
    let (api_key_id, label) = crate::audit_logs::decode_api_key_actor(
        actor_api_key_id,
        row.try_get(5).map_err(to_query_error)?,
    );
    Ok(AuditLogRecord {
        id: row.try_get(0).map_err(to_query_error)?,
        occurred_at: unix_millis_to_datetime(row.try_get(1).map_err(to_query_error)?)?,
        actor: AuditActor {
            kind: AuditActorKind::from_db(&actor_kind).ok_or_else(|| {
                StoreError::Serialization(format!("unknown audit actor kind `{actor_kind}`"))
            })?,
            user_id: actor_user_id.as_deref().map(parse_uuid).transpose()?,
            api_key_id,
            label,
        },
        action: row.try_get(6).map_err(to_query_error)?,
        target_type: row.try_get(7).map_err(to_query_error)?,
        target_id: row.try_get(8).map_err(to_query_error)?,
        target_label: row.try_get(9).map_err(to_query_error)?,
        before: parse_optional_json(before.as_deref())?,
        after: parse_optional_json(after.as_deref())?,
        metadata: parse_json(&details)?,
        request_ip: row.try_get(13).map_err(to_query_error)?,
        user_agent: row.try_get(14).map_err(to_query_error)?,
    })
}

#[async_trait]
impl AuditLogRepository for PostgresStore {
    async fn insert_audit_log(&self, record: &NewAuditLogRecord) -> Result<i64, StoreError> {
        let row = AuditLogRow::from_record(record)?;
        let inserted = sqlx::query(
            r#"
            INSERT INTO audit_logs (
                ts, actor_kind, actor_user_id, actor_api_key_id, actor_label, action,
                object_type, object_id, target_label, before_json, after_json, details_json,
                request_ip, user_agent
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            RETURNING id
            "#,
        )
        .bind(row.ts)
        .bind(row.actor_kind)
        .bind(row.actor_user_id)
        .bind(row.actor_api_key_id)
        .bind(record.actor.label.as_deref())
        .bind(record.action.as_str())
        .bind(record.target_type.as_str())
        .bind(record.target_id.as_deref())
        .bind(record.target_label.as_deref())
        .bind(row.before_json)
        .bind(row.after_json)
        .bind(row.details_json)
        .bind(record.request_ip.as_deref())
        .bind(record.user_agent.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;
        inserted.try_get(0).map_err(to_query_error)
    }

    async fn list_audit_logs(&self, query: &AuditLogQuery) -> Result<AuditLogPage, StoreError> {
        let filter = AuditLogFilter::from_query(query)?;
        let (sql, binds) = list_sql(&filter, |index| format!("${index}"));
        let mut statement = sqlx::query(&sql);
        for bind in binds {
            statement = match bind {
                AuditLogBind::Text(value) => statement.bind(value),
                AuditLogBind::Int(value) => statement.bind(value),
            };
        }
        let items = statement
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?
            .iter()
            .map(decode)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(finish_page(items, filter.page_size))
    }

    async fn get_audit_log(&self, id: i64) -> Result<Option<AuditLogRecord>, StoreError> {
        sqlx::query(&format!(
            "SELECT {AUDIT_LOG_COLUMNS} FROM audit_logs WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode)
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;
    use crate::{
        StoreConnectionOptions, run_migrations_with_options,
        tests::{create_postgres_test_database, drop_postgres_test_database},
    };

    #[tokio::test]
    #[serial]
    async fn postgres_audit_logs_round_trip_filter_and_page() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping PostgreSQL audit log test because TEST_POSTGRES_URL is not set");
            return;
        };
        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");
        let store = PostgresStore::connect(&test_db.database_url, 2)
            .await
            .expect("postgres store");
        crate::audit_logs::exercise_audit_log_store(&store).await;
        drop(store);
        drop_postgres_test_database(&test_db).await;
    }
}
