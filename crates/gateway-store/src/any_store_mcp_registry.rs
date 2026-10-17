use async_trait::async_trait;
use gateway_core::{
    ExternalMcpDiscoveryRunRecord, ExternalMcpServerRecord, ExternalMcpToolRecord,
    McpRegistryRepository, McpToolGrantRecord, NewExternalMcpServerRecord, StoreError,
    TransferExternalMcpServerOwnershipRecord, UpdateExternalMcpServerRecord,
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

    async fn list_external_mcp_servers_owned_by(
        &self,
        owner_team_id: Option<Uuid>,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_external_mcp_servers_owned_by(owner_team_id, include_disabled)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_external_mcp_servers_owned_by(owner_team_id, include_disabled)
                    .await
            }
        }
    }

    async fn list_external_mcp_servers_visible_to_team(
        &self,
        team_id: Uuid,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_external_mcp_servers_visible_to_team(team_id, include_disabled)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_external_mcp_servers_visible_to_team(team_id, include_disabled)
                    .await
            }
        }
    }

    async fn list_external_mcp_servers_by_ids(
        &self,
        mcp_server_ids: &[Uuid],
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_external_mcp_servers_by_ids(mcp_server_ids).await,
            Self::Postgres(store) => store.list_external_mcp_servers_by_ids(mcp_server_ids).await,
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

    async fn disable_external_mcp_server_for_owner(
        &self,
        mcp_server_id: Uuid,
        expected_owner_team_id: Option<Uuid>,
        disabled_at: OffsetDateTime,
    ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .disable_external_mcp_server_for_owner(
                        mcp_server_id,
                        expected_owner_team_id,
                        disabled_at,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .disable_external_mcp_server_for_owner(
                        mcp_server_id,
                        expected_owner_team_id,
                        disabled_at,
                    )
                    .await
            }
        }
    }

    async fn transfer_external_mcp_server_ownership(
        &self,
        input: &TransferExternalMcpServerOwnershipRecord,
    ) -> Result<gateway_core::Updated<ExternalMcpServerRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.transfer_external_mcp_server_ownership(input).await,
            Self::Postgres(store) => store.transfer_external_mcp_server_ownership(input).await,
        }
    }

    async fn first_active_mcp_toolset_key_for_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<String>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .first_active_mcp_toolset_key_for_server(mcp_server_id)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .first_active_mcp_toolset_key_for_server(mcp_server_id)
                    .await
            }
        }
    }

    async fn count_mcp_tool_grants_outside_team(
        &self,
        mcp_server_id: Uuid,
        team_id: Uuid,
    ) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .count_mcp_tool_grants_outside_team(mcp_server_id, team_id)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .count_mcp_tool_grants_outside_team(mcp_server_id, team_id)
                    .await
            }
        }
    }

    async fn list_mcp_tool_grants_for_owner_team(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_mcp_tool_grants_for_owner_team(team_id).await,
            Self::Postgres(store) => store.list_mcp_tool_grants_for_owner_team(team_id).await,
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
}
