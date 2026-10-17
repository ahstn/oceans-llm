use async_trait::async_trait;
use gateway_core::{
    ExternalMcpDiscoveryRunRecord, ExternalMcpServerRecord, ExternalMcpToolRecord,
    McpRegistryRepository, NewExternalMcpServerRecord, StoreError, UpdateExternalMcpServerRecord,
    UpsertExternalMcpToolRecord,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl McpRegistryRepository for AnyStore {
    async fn list_external_mcp_servers(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_external_mcp_servers(include_disabled).await,
            Self::Postgres(store) => store.list_external_mcp_servers(include_disabled).await,
        }
    }

    async fn get_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_external_mcp_server(mcp_server_id).await,
            Self::Postgres(store) => store.get_external_mcp_server(mcp_server_id).await,
        }
    }

    async fn get_external_mcp_server_by_key(
        &self,
        server_key: &str,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_external_mcp_server_by_key(server_key).await,
            Self::Postgres(store) => store.get_external_mcp_server_by_key(server_key).await,
        }
    }

    async fn create_external_mcp_server(
        &self,
        input: &NewExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.create_external_mcp_server(input).await,
            Self::Postgres(store) => store.create_external_mcp_server(input).await,
        }
    }

    async fn update_external_mcp_server(
        &self,
        input: &UpdateExternalMcpServerRecord,
    ) -> Result<gateway_core::Updated<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.update_external_mcp_server(input).await,
            Self::Postgres(store) => store.update_external_mcp_server(input).await,
        }
    }

    async fn disable_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .disable_external_mcp_server(mcp_server_id, disabled_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .disable_external_mcp_server(mcp_server_id, disabled_at)
                    .await
            }
        }
    }

    async fn list_external_mcp_tools(
        &self,
        mcp_server_id: Uuid,
        include_inactive: bool,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_external_mcp_tools(mcp_server_id, include_inactive)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_external_mcp_tools(mcp_server_id, include_inactive)
                    .await
            }
        }
    }

    async fn record_external_mcp_discovery_success(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
        tools: &[UpsertExternalMcpToolRecord],
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .record_external_mcp_discovery_success(run, tools)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .record_external_mcp_discovery_success(run, tools)
                    .await
            }
        }
    }

    async fn record_external_mcp_discovery_failure(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.record_external_mcp_discovery_failure(run).await,
            Self::Postgres(store) => store.record_external_mcp_discovery_failure(run).await,
        }
    }

    async fn get_external_mcp_tool(
        &self,
        mcp_tool_id: Uuid,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_external_mcp_tool(mcp_tool_id).await,
            Self::Postgres(store) => store.get_external_mcp_tool(mcp_tool_id).await,
        }
    }

    async fn set_mcp_tool_tags(
        &self,
        mcp_tool_id: Uuid,
        tags: &[String],
    ) -> Result<gateway_core::Updated<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.set_mcp_tool_tags(mcp_tool_id, tags).await,
            Self::Postgres(store) => store.set_mcp_tool_tags(mcp_tool_id, tags).await,
        }
    }

    async fn set_mcp_tool_annotation_overrides(
        &self,
        mcp_tool_id: Uuid,
        overrides: &gateway_core::McpAnnotationOverrides,
    ) -> Result<gateway_core::Updated<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .set_mcp_tool_annotation_overrides(mcp_tool_id, overrides)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .set_mcp_tool_annotation_overrides(mcp_tool_id, overrides)
                    .await
            }
        }
    }

    async fn list_mcp_tool_versions(
        &self,
        mcp_tool_id: Uuid,
    ) -> Result<Vec<gateway_core::McpToolVersionRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_mcp_tool_versions(mcp_tool_id).await,
            Self::Postgres(store) => store.list_mcp_tool_versions(mcp_tool_id).await,
        }
    }

    async fn review_mcp_tool_version(
        &self,
        mcp_tool_id: Uuid,
        schema_version: i64,
        decision: gateway_core::McpToolReviewDecision,
        reviewed_by_user_id: Option<Uuid>,
        reviewed_at: OffsetDateTime,
    ) -> Result<gateway_core::Updated<ExternalMcpToolRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .review_mcp_tool_version(
                        mcp_tool_id,
                        schema_version,
                        decision,
                        reviewed_by_user_id,
                        reviewed_at,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .review_mcp_tool_version(
                        mcp_tool_id,
                        schema_version,
                        decision,
                        reviewed_by_user_id,
                        reviewed_at,
                    )
                    .await
            }
        }
    }
}
