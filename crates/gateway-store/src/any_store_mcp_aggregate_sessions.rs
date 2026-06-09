use async_trait::async_trait;
use gateway_core::{
    McpAggregateSessionRecord, McpAggregateSessionRepository, NewMcpAggregateSessionRecord,
    StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl McpAggregateSessionRepository for AnyStore {
    async fn create_mcp_aggregate_session(
        &self,
        session: &NewMcpAggregateSessionRecord,
    ) -> Result<McpAggregateSessionRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.create_mcp_aggregate_session(session).await,
            Self::Postgres(store) => store.create_mcp_aggregate_session(session).await,
        }
    }

    async fn get_mcp_aggregate_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_mcp_aggregate_session_by_token_hash(token_hash)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_mcp_aggregate_session_by_token_hash(token_hash)
                    .await
            }
        }
    }

    async fn update_mcp_aggregate_session_initialized(
        &self,
        session_id: Uuid,
        token_hash: &str,
        initialized_at: OffsetDateTime,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .update_mcp_aggregate_session_initialized(
                        session_id,
                        token_hash,
                        initialized_at,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .update_mcp_aggregate_session_initialized(
                        session_id,
                        token_hash,
                        initialized_at,
                    )
                    .await
            }
        }
    }

    async fn touch_mcp_aggregate_session(
        &self,
        session_id: Uuid,
        token_hash: &str,
        touched_at: OffsetDateTime,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .touch_mcp_aggregate_session(session_id, token_hash, touched_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .touch_mcp_aggregate_session(session_id, token_hash, touched_at)
                    .await
            }
        }
    }

    async fn revoke_mcp_aggregate_session(
        &self,
        session_id: Uuid,
        token_hash: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .revoke_mcp_aggregate_session(session_id, token_hash, revoked_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .revoke_mcp_aggregate_session(session_id, token_hash, revoked_at)
                    .await
            }
        }
    }
}
