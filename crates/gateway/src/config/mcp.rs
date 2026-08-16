use anyhow::{Context, bail};
use gateway_service::{McpOauthProvider, McpOauthRuntime};
use serde::Deserialize;

use super::auth::normalize_config_oauth_provider_key;
use super::references::{resolve_path_reference, resolve_secret_reference};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default)]
    pub oauth: McpOauthConfig,
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
    #[serde(default = "default_google_mcp_oauth_provider_type")]
    pub provider_type: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_google_authorization_url")]
    pub authorization_url: String,
    #[serde(default = "default_google_token_url")]
    pub token_url: String,
}

impl McpOauthConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        let public_base_url = self.resolved_public_base_url()?;
        if public_base_url.is_none() && !self.providers.is_empty() {
            bail!("mcp.oauth.public_base_url is required when a provider is configured");
        }
        let mut keys = std::collections::BTreeSet::new();
        for provider in &self.providers {
            let key = normalize_config_oauth_provider_key(&provider.key)
                .context("mcp.oauth.providers[].key")?;
            if !keys.insert(key.clone()) {
                bail!("duplicate MCP OAuth provider key `{key}`");
            }
            if provider.provider_type.trim() != "google" {
                bail!(
                    "MCP OAuth provider `{key}` has unsupported provider_type `{}`",
                    provider.provider_type
                );
            }
            if resolve_secret_reference(&provider.client_id)?
                .trim()
                .is_empty()
            {
                bail!("MCP OAuth provider `{key}` client_id cannot be empty");
            }
            if resolve_secret_reference(&provider.client_secret)?
                .trim()
                .is_empty()
            {
                bail!("MCP OAuth provider `{key}` client_secret cannot be empty");
            }
            validate_google_oauth_endpoint(
                &provider.authorization_url,
                &format!("MCP OAuth provider `{key}` authorization_url"),
                &default_google_authorization_url(),
            )?;
            validate_google_oauth_endpoint(
                &provider.token_url,
                &format!("MCP OAuth provider `{key}` token_url"),
                &default_google_token_url(),
            )?;
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
            .map(|provider| {
                Ok(McpOauthProvider {
                    key: normalize_config_oauth_provider_key(&provider.key)?,
                    client_id: resolve_secret_reference(&provider.client_id)?
                        .trim()
                        .to_string(),
                    client_secret: resolve_secret_reference(&provider.client_secret)?
                        .trim()
                        .to_string(),
                    authorization_url: provider.authorization_url.trim().to_string(),
                    token_url: provider.token_url.trim().to_string(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(McpOauthRuntime::new(
            self.resolved_public_base_url()?,
            providers,
        ))
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
