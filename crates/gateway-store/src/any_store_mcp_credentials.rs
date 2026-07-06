use async_trait::async_trait;
use gateway_core::{
    McpUpstreamCredentialBindingRecord, McpUpstreamCredentialOwnerScopeKind,
    McpUpstreamCredentialRepository, StoreError, UpsertMcpUpstreamCredentialBindingRecord,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl McpUpstreamCredentialRepository for AnyStore {
    async fn upsert_mcp_upstream_credential_binding(
        &self,
        input: &UpsertMcpUpstreamCredentialBindingRecord,
    ) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_mcp_upstream_credential_binding(input).await,
            Self::Postgres(store) => store.upsert_mcp_upstream_credential_binding(input).await,
        }
    }

    async fn get_active_mcp_upstream_credential_binding(
        &self,
        mcp_server_id: Uuid,
        owner_scope_key: &str,
    ) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_active_mcp_upstream_credential_binding(mcp_server_id, owner_scope_key)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_active_mcp_upstream_credential_binding(mcp_server_id, owner_scope_key)
                    .await
            }
        }
    }

    async fn list_mcp_upstream_credential_bindings(
        &self,
        mcp_server_id: Option<Uuid>,
        owner_scope_kind: Option<McpUpstreamCredentialOwnerScopeKind>,
        owner_scope_id: Option<Uuid>,
        include_revoked: bool,
    ) -> Result<Vec<McpUpstreamCredentialBindingRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_mcp_upstream_credential_bindings(
                        mcp_server_id,
                        owner_scope_kind,
                        owner_scope_id,
                        include_revoked,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_mcp_upstream_credential_bindings(
                        mcp_server_id,
                        owner_scope_kind,
                        owner_scope_id,
                        include_revoked,
                    )
                    .await
            }
        }
    }

    async fn revoke_mcp_upstream_credential_binding(
        &self,
        credential_binding_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .revoke_mcp_upstream_credential_binding(credential_binding_id, revoked_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .revoke_mcp_upstream_credential_binding(credential_binding_id, revoked_at)
                    .await
            }
        }
    }

    async fn touch_mcp_upstream_credential_binding_last_used(
        &self,
        credential_binding_id: Uuid,
        last_used_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .touch_mcp_upstream_credential_binding_last_used(
                        credential_binding_id,
                        last_used_at,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .touch_mcp_upstream_credential_binding_last_used(
                        credential_binding_id,
                        last_used_at,
                    )
                    .await
            }
        }
    }
}
