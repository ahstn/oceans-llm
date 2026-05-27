use std::{collections::BTreeMap, sync::Arc};

use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpServerRecord, ExternalMcpServerStatus, GatewayError,
    McpRegistryRepository, StoreError,
};

use crate::mcp_upstream_auth::gateway_mcp_upstream_headers;

#[derive(Debug, Clone)]
pub struct McpGatewayUpstream {
    pub server: ExternalMcpServerRecord,
    pub headers: Option<BTreeMap<String, String>>,
}

#[derive(Clone)]
pub struct McpGatewayService<R> {
    repo: Arc<R>,
}

impl<R> McpGatewayService<R>
where
    R: McpRegistryRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn prepare_upstream(
        &self,
        server_key: &str,
    ) -> Result<McpGatewayUpstream, GatewayError> {
        let server_key = normalize_server_key(server_key)?;
        let server = self
            .repo
            .get_external_mcp_server_by_key(&server_key)
            .await?
            .filter(|server| server.status == ExternalMcpServerStatus::Active)
            .ok_or_else(|| {
                GatewayError::Store(StoreError::NotFound(format!(
                    "external MCP server `{server_key}` not found"
                )))
            })?;

        if matches!(
            server.auth_mode,
            ExternalMcpAuthMode::UserPassthrough | ExternalMcpAuthMode::OauthObo
        ) {
            return Err(GatewayError::McpUpstreamAuthRequired {
                server_key: server.server_key,
            });
        }

        let headers = gateway_mcp_upstream_headers(&server)?;
        Ok(McpGatewayUpstream { server, headers })
    }
}

fn normalize_server_key(value: &str) -> Result<String, GatewayError> {
    let key = value.trim().to_ascii_lowercase();
    if !(3..=64).contains(&key.len()) {
        return Err(GatewayError::InvalidRequest(
            "server_key must be 3-64 characters".to_string(),
        ));
    }
    if !key.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(GatewayError::InvalidRequest(
            "server_key may only contain lowercase letters, digits, hyphen, and underscore"
                .to_string(),
        ));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gateway_core::{
        ExternalMcpDiscoveryRunRecord, ExternalMcpToolRecord, ExternalMcpTransport,
        NewExternalMcpServerRecord, UpdateExternalMcpServerRecord, UpsertExternalMcpToolRecord,
    };
    use serde_json::Map;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[tokio::test]
    async fn personal_oauth_modes_require_user_scoped_grants() {
        let repo = Arc::new(SingleServerRepo {
            server: server_record(
                ExternalMcpAuthMode::OauthObo,
                ExternalMcpServerStatus::Active,
            ),
        });
        let service = McpGatewayService::new(repo);

        let error = service
            .prepare_upstream("github")
            .await
            .expect_err("requires grants");
        assert_eq!(error.http_status_code(), 403);
        assert_eq!(error.error_code(), "mcp_upstream_auth_required");
    }

    #[tokio::test]
    async fn disabled_servers_are_not_found() {
        let repo = Arc::new(SingleServerRepo {
            server: server_record(ExternalMcpAuthMode::None, ExternalMcpServerStatus::Disabled),
        });
        let service = McpGatewayService::new(repo);

        let error = service
            .prepare_upstream("github")
            .await
            .expect_err("disabled is hidden");
        assert_eq!(error.http_status_code(), 404);
    }

    struct SingleServerRepo {
        server: ExternalMcpServerRecord,
    }

    #[async_trait]
    impl McpRegistryRepository for SingleServerRepo {
        async fn list_external_mcp_servers(
            &self,
            _include_disabled: bool,
        ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
            unimplemented!()
        }

        async fn get_external_mcp_server(
            &self,
            _mcp_server_id: Uuid,
        ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
            unimplemented!()
        }

        async fn get_external_mcp_server_by_key(
            &self,
            server_key: &str,
        ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
            Ok((self.server.server_key == server_key).then(|| self.server.clone()))
        }

        async fn create_external_mcp_server(
            &self,
            _input: &NewExternalMcpServerRecord,
        ) -> Result<ExternalMcpServerRecord, StoreError> {
            unimplemented!()
        }

        async fn update_external_mcp_server(
            &self,
            _input: &UpdateExternalMcpServerRecord,
        ) -> Result<ExternalMcpServerRecord, StoreError> {
            unimplemented!()
        }

        async fn disable_external_mcp_server(
            &self,
            _mcp_server_id: Uuid,
            _disabled_at: OffsetDateTime,
        ) -> Result<ExternalMcpServerRecord, StoreError> {
            unimplemented!()
        }

        async fn list_external_mcp_tools(
            &self,
            _mcp_server_id: Uuid,
            _include_inactive: bool,
        ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
            unimplemented!()
        }

        async fn record_external_mcp_discovery_success(
            &self,
            _run: &ExternalMcpDiscoveryRunRecord,
            _tools: &[UpsertExternalMcpToolRecord],
        ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
            unimplemented!()
        }

        async fn record_external_mcp_discovery_failure(
            &self,
            _run: &ExternalMcpDiscoveryRunRecord,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
    }

    fn server_record(
        auth_mode: ExternalMcpAuthMode,
        status: ExternalMcpServerStatus,
    ) -> ExternalMcpServerRecord {
        let now = OffsetDateTime::now_utc();
        ExternalMcpServerRecord {
            mcp_server_id: Uuid::new_v4(),
            server_key: "github".to_string(),
            display_name: "GitHub".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://example.test/mcp".to_string(),
            auth_mode,
            auth_config: Map::new(),
            timeout_ms: 30_000,
            status,
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: now,
            updated_at: now,
            disabled_at: None,
        }
    }
}
