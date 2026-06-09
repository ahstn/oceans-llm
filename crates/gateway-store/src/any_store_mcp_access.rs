use async_trait::async_trait;
use gateway_core::{
    ExternalMcpToolRecord, McpAccessRepository, McpAccessResolution, McpCatalogAccessResolution,
    McpGrantSubject, McpToolGrantRecord, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpToolsetRecord, McpToolsetToolRecord, NewMcpToolsetRecord, StoreError,
    UpdateMcpToolsetRecord, UpsertMcpToolGrantRecord,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl McpAccessRepository for AnyStore {
    async fn list_mcp_toolsets(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<McpToolsetRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_mcp_toolsets(include_disabled).await,
            Self::Postgres(store) => store.list_mcp_toolsets(include_disabled).await,
        }
    }

    async fn create_mcp_toolset(
        &self,
        input: &NewMcpToolsetRecord,
    ) -> Result<McpToolsetRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.create_mcp_toolset(input).await,
            Self::Postgres(store) => store.create_mcp_toolset(input).await,
        }
    }

    async fn update_mcp_toolset(
        &self,
        input: &UpdateMcpToolsetRecord,
    ) -> Result<McpToolsetRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.update_mcp_toolset(input).await,
            Self::Postgres(store) => store.update_mcp_toolset(input).await,
        }
    }

    async fn disable_mcp_toolset(
        &self,
        toolset_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<McpToolsetRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.disable_mcp_toolset(toolset_id, disabled_at).await,
            Self::Postgres(store) => store.disable_mcp_toolset(toolset_id, disabled_at).await,
        }
    }

    async fn replace_mcp_toolset_tools(
        &self,
        toolset_id: Uuid,
        tool_ids: &[Uuid],
        updated_at: OffsetDateTime,
    ) -> Result<Vec<McpToolsetToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .replace_mcp_toolset_tools(toolset_id, tool_ids, updated_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .replace_mcp_toolset_tools(toolset_id, tool_ids, updated_at)
                    .await
            }
        }
    }

    async fn list_mcp_toolset_tools(
        &self,
        toolset_id: Uuid,
    ) -> Result<Vec<McpToolsetToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_mcp_toolset_tools(toolset_id).await,
            Self::Postgres(store) => store.list_mcp_toolset_tools(toolset_id).await,
        }
    }

    async fn upsert_mcp_tool_grant(
        &self,
        grant: &UpsertMcpToolGrantRecord,
    ) -> Result<McpToolGrantRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_mcp_tool_grant(grant).await,
            Self::Postgres(store) => store.upsert_mcp_tool_grant(grant).await,
        }
    }

    async fn revoke_mcp_tool_grant(
        &self,
        subject_kind: McpToolGrantSubjectKind,
        subject_id: Uuid,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .revoke_mcp_tool_grant(
                        subject_kind,
                        subject_id,
                        target_kind,
                        target_id,
                        revoked_at,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .revoke_mcp_tool_grant(
                        subject_kind,
                        subject_id,
                        target_kind,
                        target_id,
                        revoked_at,
                    )
                    .await
            }
        }
    }

    async fn list_mcp_tool_grants(
        &self,
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_mcp_tool_grants(subject_kind, subject_id).await,
            Self::Postgres(store) => store.list_mcp_tool_grants(subject_kind, subject_id).await,
        }
    }

    async fn resolve_mcp_access_for_subjects(
        &self,
        subjects: &[McpGrantSubject],
        mcp_server_id: Option<Uuid>,
    ) -> Result<McpAccessResolution, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .resolve_mcp_access_for_subjects(subjects, mcp_server_id)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .resolve_mcp_access_for_subjects(subjects, mcp_server_id)
                    .await
            }
        }
    }

    async fn resolve_mcp_catalog_access_for_subjects(
        &self,
        subjects: &[McpGrantSubject],
        server_key: Option<&str>,
    ) -> Result<McpCatalogAccessResolution, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .resolve_mcp_catalog_access_for_subjects(subjects, server_key)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .resolve_mcp_catalog_access_for_subjects(subjects, server_key)
                    .await
            }
        }
    }

    async fn get_active_mcp_tool_by_name(
        &self,
        mcp_server_id: Uuid,
        upstream_name: &str,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_active_mcp_tool_by_name(mcp_server_id, upstream_name)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_active_mcp_tool_by_name(mcp_server_id, upstream_name)
                    .await
            }
        }
    }
}
