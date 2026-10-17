use anyhow::{Context, bail};
use gateway_service::{
    MCP_SERVER_OAUTH_PROVIDER_KEY, McpOauthProvider, McpOauthProviderType, McpOauthRuntime,
    McpOauthTokenEndpointAuthMethod, McpOrganisationPublishing,
};
use serde::Deserialize;

use super::auth::normalize_config_auth_provider_key;
use super::references::{resolve_path_reference, resolve_secret_reference};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default)]
    pub oauth: McpOauthConfig,
    #[serde(default)]
    pub client_oauth: super::McpClientOauthConfig,
    #[serde(default)]
    pub registry: McpRegistryConfig,
}

/// Delegated server ownership. See
/// `docs/adr/2026-10-17-delegated-mcp-server-ownership.md`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct McpRegistryConfig {
    /// Who may publish a server to the organisation: `platform_admins`
    /// (default) or `team_admins`.
    #[serde(default)]
    pub organisation_publishing: McpOrganisationPublishing,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct McpOauthConfig {
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub providers: Vec<McpOauthProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpOauthProviderConfig {
    pub key: String,
    /// `google`, or `oauth2` for any authorization server with explicit endpoints.
    #[serde(default = "default_google_mcp_oauth_provider_type")]
    pub provider_type: String,
    pub client_id: String,
    /// A secret reference. Required for `google`; optional for a public `oauth2` client.
    #[serde(default)]
    pub client_secret: Option<String>,
    /// Defaults to Google's endpoint for `google`; required for `oauth2`.
    #[serde(default)]
    pub authorization_url: Option<String>,
    /// Defaults to Google's endpoint for `google`; required for `oauth2`.
    #[serde(default)]
    pub token_url: Option<String>,
    /// Requested when an MCP server using this provider sets no scopes.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// `client_secret_basic`, `client_secret_post`, or `none` (`oauth2` only).
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
}

impl McpOauthConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        let public_base_url = self.resolved_public_base_url()?;
        if public_base_url.is_none() && !self.providers.is_empty() {
            bail!("mcp.oauth.public_base_url is required when a provider is configured");
        }
        let mut keys = std::collections::BTreeSet::new();
        for provider in &self.providers {
            let key = normalize_config_auth_provider_key(&provider.key)
                .context("mcp.oauth.providers[].key")?;
            if !keys.insert(key.clone()) {
                bail!("duplicate MCP OAuth provider key `{key}`");
            }
            provider
                .resolved(&key)
                .map_err(|error| anyhow::anyhow!("MCP OAuth provider `{key}` {error:#}"))?;
        }
        Ok(())
    }

    pub fn resolved_public_base_url(&self) -> anyhow::Result<Option<String>> {
        let Some(value) = self.public_base_url.as_deref() else {
            return Ok(None);
        };
        let value = resolve_path_reference(value)?;
        normalize_https_origin(value.trim(), "mcp.oauth.public_base_url").map(Some)
    }

    pub fn runtime(&self) -> anyhow::Result<McpOauthRuntime> {
        let providers = self
            .providers
            .iter()
            .map(|provider| provider.resolved(&normalize_config_auth_provider_key(&provider.key)?))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(McpOauthRuntime::new(
            self.resolved_public_base_url()?,
            providers,
        ))
    }
}

impl McpOauthProviderConfig {
    fn resolved(&self, key: &str) -> anyhow::Result<McpOauthProvider> {
        let client_id = resolve_secret_reference(&self.client_id)?
            .trim()
            .to_string();
        if client_id.is_empty() {
            bail!("client_id cannot be empty");
        }
        let client_secret = self
            .client_secret
            .as_deref()
            .map(|value| resolve_secret_reference(value).map(|secret| secret.trim().to_string()))
            .transpose()?;
        if client_secret.as_deref().is_some_and(str::is_empty) {
            bail!("client_secret cannot be empty");
        }
        for scope in &self.scopes {
            if scope.trim().is_empty() || scope.chars().any(char::is_whitespace) {
                bail!("scopes must contain non-empty scope strings");
            }
        }
        match self.provider_type.trim() {
            "google" => {
                let client_secret = client_secret
                    .ok_or_else(|| anyhow::anyhow!("client_secret cannot be empty"))?;
                if self.token_endpoint_auth_method.is_some() {
                    bail!("token_endpoint_auth_method is only supported for `oauth2` providers");
                }
                let authorization_url = self
                    .authorization_url
                    .clone()
                    .unwrap_or_else(default_google_authorization_url);
                let token_url = self
                    .token_url
                    .clone()
                    .unwrap_or_else(default_google_token_url);
                validate_google_oauth_endpoint(
                    &authorization_url,
                    "authorization_url",
                    &default_google_authorization_url(),
                )?;
                validate_google_oauth_endpoint(
                    &token_url,
                    "token_url",
                    &default_google_token_url(),
                )?;
                Ok(McpOauthProvider {
                    key: key.to_string(),
                    provider_type: McpOauthProviderType::Google,
                    client_id,
                    client_secret: Some(client_secret),
                    authorization_url,
                    token_url,
                    token_endpoint_auth_method: McpOauthTokenEndpointAuthMethod::ClientSecretPost,
                    scopes: self.scopes.clone(),
                })
            }
            "oauth2" => {
                if key == MCP_SERVER_OAUTH_PROVIDER_KEY
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    bail!(
                        "key must contain only letters, digits, '-', or '_', and cannot be `{MCP_SERVER_OAUTH_PROVIDER_KEY}`"
                    );
                }
                let authorization_url = self
                    .authorization_url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("authorization_url is required"))?;
                let token_url = self
                    .token_url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("token_url is required"))?;
                validate_https_url(&authorization_url, "authorization_url")?;
                validate_https_url(&token_url, "token_url")?;
                let token_endpoint_auth_method = match self.token_endpoint_auth_method.as_deref() {
                    Some(value) => McpOauthTokenEndpointAuthMethod::parse(value).ok_or_else(|| {
                        anyhow::anyhow!(
                            "token_endpoint_auth_method must be client_secret_basic, client_secret_post, or none"
                        )
                    })?,
                    None if client_secret.is_some() => {
                        McpOauthTokenEndpointAuthMethod::ClientSecretBasic
                    }
                    None => McpOauthTokenEndpointAuthMethod::None,
                };
                if (token_endpoint_auth_method == McpOauthTokenEndpointAuthMethod::None)
                    != client_secret.is_none()
                {
                    bail!(
                        "client_secret is required for `{}` and not allowed for `none`",
                        token_endpoint_auth_method.as_str()
                    );
                }
                Ok(McpOauthProvider {
                    key: key.to_string(),
                    provider_type: McpOauthProviderType::Oauth2,
                    client_id,
                    client_secret,
                    authorization_url,
                    token_url,
                    token_endpoint_auth_method,
                    scopes: self.scopes.clone(),
                })
            }
            other => bail!("unsupported provider_type `{other}`; expected google or oauth2"),
        }
    }
}

fn validate_https_url(value: &str, field: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(value).with_context(|| format!("{field} is invalid"))?;
    if parsed.scheme() != "https" || parsed.host().is_none() {
        bail!("{field} must be an https URL with a host");
    }
    Ok(())
}

fn normalize_https_origin(value: &str, field: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(value).with_context(|| format!("{field} is invalid"))?;
    if parsed.scheme() != "https" || parsed.host().is_none() {
        bail!("{field} must be an https URL with a host");
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("{field} must be an origin without user information, path, query, or fragment");
    }
    Ok(parsed.origin().ascii_serialization())
}

fn validate_google_oauth_endpoint(value: &str, field: &str, expected: &str) -> anyhow::Result<()> {
    validate_https_url(value, field)?;
    if value != expected {
        bail!("{field} must be `{expected}` for the Google OAuth provider");
    }
    Ok(())
}

pub(super) fn default_google_mcp_oauth_provider_type() -> String {
    "google".to_string()
}

pub(super) fn default_google_authorization_url() -> String {
    "https://accounts.google.com/o/oauth2/v2/auth".to_string()
}

pub(super) fn default_google_token_url() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}
