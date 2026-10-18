//! Capability scopes and MCP server lists on API keys.
//! See `docs/adr/2026-10-17-scoped-api-keys.md`.

use gateway_core::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyAccess, ApiKeyStatus, BudgetRepository,
    GatewayError, IdentityRepository, McpRegistryRepository, ModelRepository, StoreError,
};
use uuid::Uuid;

use super::{AdminApiKeyService, AdminApiKeySummary, build_api_key_summary};
use crate::admin_audit::AuditProgress;

#[derive(Debug, Clone)]
pub struct AdminApiKeyMcpServerOption {
    pub key: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct UpdateAdminApiKeyScopesInput {
    /// `None` restores full access.
    pub scopes: Option<Vec<String>>,
    /// `None` restores every server the owner's grants reach.
    pub mcp_server_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct UpdateAdminApiKeyScopesResult {
    pub api_key: AdminApiKeySummary,
    /// The access this update replaced, or `None` when it changed nothing.
    pub replaced_access: Option<ApiKeyAccess>,
}

impl<R> AdminApiKeyService<R>
where
    R: AdminApiKeyRepository
        + AdminIdentityRepository
        + IdentityRepository
        + ModelRepository
        + BudgetRepository
        + McpRegistryRepository
        + Send
        + Sync
        + 'static,
{
    /// Replaces a key's scopes and MCP server list. `progress` is marked only
    /// when the store wrote a change.
    pub async fn update_api_key_scopes(
        &self,
        api_key_id: Uuid,
        request: UpdateAdminApiKeyScopesInput,
        progress: &AuditProgress,
    ) -> Result<UpdateAdminApiKeyScopesResult, GatewayError> {
        let api_key = self
            .repo
            .get_api_key_by_id(api_key_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("api key `{api_key_id}`")))?;
        if api_key.status != ApiKeyStatus::Active {
            return Err(GatewayError::InvalidRequest(
                "revoked api keys cannot be updated".to_string(),
            ));
        }
        let access = self
            .validated_access(request.scopes, request.mcp_server_keys)
            .await?;
        let replaced_access = self
            .repo
            .replace_api_key_access(api_key_id, &access)
            .await?;
        if replaced_access.is_some() {
            progress.landed_on(api_key_id);
        }

        let users = self.repo.list_identity_users().await?;
        let teams = self.repo.list_teams().await?;
        let service_accounts = self.repo.list_service_accounts().await?;
        let granted_models = self.repo.list_models_for_api_key(api_key_id).await?;
        // The summary describes this update's write, not a later read.
        let mut api_key = api_key;
        api_key.access = access;
        Ok(UpdateAdminApiKeyScopesResult {
            api_key: build_api_key_summary(
                &api_key,
                &users,
                &teams,
                &service_accounts,
                &granted_models,
            )?,
            replaced_access,
        })
    }

    /// Validates requested access. Every listed server must be registered.
    pub(super) async fn validated_access(
        &self,
        scopes: Option<Vec<String>>,
        mcp_server_keys: Option<Vec<String>>,
    ) -> Result<ApiKeyAccess, GatewayError> {
        let access =
            ApiKeyAccess::new(scopes, mcp_server_keys).map_err(GatewayError::InvalidRequest)?;
        if let Some(keys) = &access.mcp_server_keys {
            let servers = self.repo.list_external_mcp_servers(true).await?;
            if let Some(unknown) = keys
                .iter()
                .find(|key| !servers.iter().any(|server| &server.server_key == *key))
            {
                return Err(GatewayError::InvalidRequest(format!(
                    "MCP server `{unknown}` is not registered"
                )));
            }
        }
        Ok(access)
    }

    pub(super) async fn mcp_server_options(
        &self,
    ) -> Result<Vec<AdminApiKeyMcpServerOption>, GatewayError> {
        let mut servers = self.repo.list_external_mcp_servers(false).await?;
        servers.sort_by(|left, right| left.server_key.cmp(&right.server_key));
        Ok(servers
            .into_iter()
            .map(|server| AdminApiKeyMcpServerOption {
                key: server.server_key,
                display_name: server.display_name,
            })
            .collect())
    }
}
