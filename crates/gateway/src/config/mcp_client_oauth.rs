use anyhow::{Context, bail};
use gateway_service::{McpClientOauthSettings, mcp_client_oauth};
use serde::Deserialize;

use super::GatewayConfig;
use super::references::resolve_path_reference;

/// `mcp.client_oauth`: the gateway as an OAuth authorization server for MCP clients.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpClientOauthConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Public origin used as the issuer. Falls back to the other configured
    /// public base URLs when unset.
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default = "default_access_token_ttl_seconds")]
    pub access_token_ttl_seconds: i64,
    #[serde(default = "default_refresh_token_ttl_seconds")]
    pub refresh_token_ttl_seconds: i64,
    #[serde(default = "default_true")]
    pub dynamic_client_registration: bool,
    /// Days a dynamically registered client may go without a grant before
    /// the hourly cleanup deletes it.
    #[serde(default = "default_unused_client_retention_days")]
    pub unused_client_retention_days: i64,
}

impl Default for McpClientOauthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            public_base_url: None,
            access_token_ttl_seconds: default_access_token_ttl_seconds(),
            refresh_token_ttl_seconds: default_refresh_token_ttl_seconds(),
            dynamic_client_registration: true,
            unused_client_retention_days: default_unused_client_retention_days(),
        }
    }
}

fn default_access_token_ttl_seconds() -> i64 {
    mcp_client_oauth::DEFAULT_ACCESS_TOKEN_TTL_SECONDS
}

fn default_refresh_token_ttl_seconds() -> i64 {
    mcp_client_oauth::DEFAULT_REFRESH_TOKEN_TTL_SECONDS
}

fn default_unused_client_retention_days() -> i64 {
    mcp_client_oauth::DEFAULT_UNUSED_CLIENT_RETENTION_DAYS
}

fn default_true() -> bool {
    true
}

impl GatewayConfig {
    /// Resolves `mcp.client_oauth`, or `None` when the feature is disabled.
    ///
    /// The issuer is the first of `mcp.client_oauth.public_base_url`,
    /// `mcp.oauth.public_base_url`, `auth.oidc.public_base_url`, and
    /// `auth.oauth.public_base_url` that is set.
    pub fn mcp_client_oauth_settings(&self) -> anyhow::Result<Option<McpClientOauthSettings>> {
        let config = &self.mcp.client_oauth;
        if !config.enabled {
            return Ok(None);
        }
        let (issuer, field) = if let Some(raw) = config.public_base_url.as_deref() {
            (
                resolve_path_reference(raw).context("mcp.client_oauth.public_base_url")?,
                "mcp.client_oauth.public_base_url",
            )
        } else if let Some(issuer) = self.mcp.oauth.resolved_public_base_url()? {
            (issuer, "mcp.oauth.public_base_url")
        } else if let Some(issuer) = self.auth.oidc.resolved_public_base_url()? {
            (issuer, "auth.oidc.public_base_url")
        } else if let Some(issuer) = self.auth.oauth.resolved_public_base_url()? {
            (issuer, "auth.oauth.public_base_url")
        } else {
            bail!(
                "mcp.client_oauth.public_base_url is required when mcp.client_oauth is enabled \
                 and no other public base URL is configured"
            );
        };
        McpClientOauthSettings::new(
            &issuer,
            config.access_token_ttl_seconds,
            config.refresh_token_ttl_seconds,
            config.dynamic_client_registration,
        )
        .with_context(|| format!("{field} cannot be used as the MCP OAuth issuer"))?
        .with_unused_client_retention_days(config.unused_client_retention_days)
        .context("mcp.client_oauth.unused_client_retention_days")
        .map(Some)
    }
}
