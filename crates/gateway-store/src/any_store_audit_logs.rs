use async_trait::async_trait;
use gateway_core::{
    AuditLogPage, AuditLogQuery, AuditLogRecord, AuditLogRepository, NewAuditLogRecord, StoreError,
};

use crate::store::AnyStore;

#[async_trait]
impl AuditLogRepository for AnyStore {
    async fn insert_audit_log(&self, record: &NewAuditLogRecord) -> Result<i64, StoreError> {
        match self {
            Self::Libsql(store) => store.insert_audit_log(record).await,
            Self::Postgres(store) => store.insert_audit_log(record).await,
        }
    }

    async fn list_audit_logs(&self, query: &AuditLogQuery) -> Result<AuditLogPage, StoreError> {
        match self {
            Self::Libsql(store) => store.list_audit_logs(query).await,
            Self::Postgres(store) => store.list_audit_logs(query).await,
        }
    }

    async fn get_audit_log(&self, id: i64) -> Result<Option<AuditLogRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_audit_log(id).await,
            Self::Postgres(store) => store.get_audit_log(id).await,
        }
    }
}
