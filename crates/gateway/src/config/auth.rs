use anyhow::{Context, bail};
use gateway_core::{
    GlobalRole, MembershipRole, OauthJitMembership, OauthJitPolicy, OidcJitMembership,
    OidcJitPolicy, SeedOauthProvider, SeedOidcProvider,
};
use serde::Deserialize;

use super::identity::TeamConfig;
use super::normalization::normalize_config_team_key;
use super::references::{resolve_path_reference, resolve_secret_reference};

pub(super) fn normalize_config_auth_provider_key(provider_key: &str) -> anyhow::Result<String> {
    let normalized = provider_key.trim().to_string();
    if normalized.is_empty() {
        bail!("cannot be empty");
    }
    Ok(normalized)
}

fn normalize_allowed_email_domains(
    domains: &[String],
    context: &str,
) -> anyhow::Result<Vec<String>> {
    let mut normalized_domains = Vec::with_capacity(domains.len());
    let mut seen = std::collections::BTreeSet::new();

    for domain in domains {
        let normalized = normalize_allowed_email_domain(domain)
            .with_context(|| format!("{context} entry `{domain}`"))?;
        if !seen.insert(normalized.clone()) {
            bail!("{context} contains duplicate domain `{normalized}`");
        }
        normalized_domains.push(normalized);
    }

    Ok(normalized_domains)
}

fn validate_allowed_email_domains(domains: &[String], context: &str) -> anyhow::Result<()> {
    normalize_allowed_email_domains(domains, context).map(|_| ())
}

fn normalize_allowed_email_domain(domain: &str) -> anyhow::Result<String> {
    let normalized = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        bail!("cannot be empty");
    }
    if normalized.contains('@')
        || normalized.contains('/')
        || normalized.contains(':')
        || normalized.contains('*')
        || normalized.chars().any(char::is_whitespace)
    {
        bail!("must be a domain name, not an email address, URL, or wildcard");
    }
    if normalized.starts_with('.') || normalized.ends_with('.') || normalized.contains("..") {
        bail!("must be a valid domain name");
    }

    let mut label_count = 0;
    for label in normalized.split('.') {
        label_count += 1;
        if label.is_empty() || label.starts_with('-') || label.ends_with('-') {
            bail!("must be a valid domain name");
        }
        if !label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            bail!("must be a valid domain name");
        }
    }

    if label_count < 2 {
        bail!("must be a valid domain name");
    }

    Ok(normalized)
}

const fn default_enabled() -> bool {
    true
}

const fn default_request_logging_enabled() -> bool {
    true
}

fn default_oidc_scopes() -> Vec<String> {
    vec![
        "openid".to_string(),
        "email".to_string(),
        "profile".to_string(),
    ]
}

fn default_oauth_provider_type() -> String {
    "github".to_string()
}

fn default_sso_email_verification_enabled() -> bool {
    true
}

fn default_github_oauth_scopes() -> Vec<String> {
    vec!["read:user".to_string(), "user:email".to_string()]
}

const fn default_user_global_role() -> GlobalRole {
    GlobalRole::User
}

const fn default_bootstrap_admin_enabled() -> bool {
    true
}

fn default_bootstrap_admin_email() -> String {
    "admin@local".to_string()
}

fn default_bootstrap_admin_password() -> String {
    "literal.admin".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    #[serde(default)]
    pub bootstrap_admin: BootstrapAdminConfig,
    #[serde(default)]
    pub oidc: AuthOidcConfig,
    #[serde(default)]
    pub oauth: AuthOauthConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuthOidcConfig {
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub providers: Vec<OidcProviderConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuthOauthConfig {
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub providers: Vec<OauthProviderConfig>,
}

impl AuthOidcConfig {
    pub(super) fn provider_keys(&self) -> anyhow::Result<std::collections::BTreeSet<String>> {
        self.providers
            .iter()
            .map(|provider| normalize_config_auth_provider_key(&provider.key))
            .collect()
    }

    pub(super) fn validate(&self, teams: &[TeamConfig]) -> anyhow::Result<()> {
        self.resolved_public_base_url()?;
        let mut provider_keys = std::collections::BTreeSet::new();
        let team_keys = teams
            .iter()
            .map(|team| normalize_config_team_key(&team.id))
            .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()?;

        for provider in &self.providers {
            let provider_key = normalize_config_auth_provider_key(&provider.key)
                .context("auth.oidc.providers[].key")?;
            if !provider_keys.insert(provider_key.clone()) {
                bail!("duplicate oidc provider key `{provider_key}`");
            }
            if provider.label.trim().is_empty() {
                bail!("oidc provider `{provider_key}` label cannot be empty");
            }
            let parsed_issuer = url::Url::parse(provider.issuer_url.trim())
                .with_context(|| format!("oidc provider `{provider_key}` issuer_url is invalid"))?;
            match parsed_issuer.scheme() {
                "http" | "https" => {}
                scheme => bail!(
                    "oidc provider `{provider_key}` issuer_url scheme `{scheme}` is not supported"
                ),
            }
            if provider.client_id.trim().is_empty() {
                bail!("oidc provider `{provider_key}` client_id cannot be empty");
            }
            let client_secret = if provider.enabled {
                resolve_secret_reference(&provider.client_secret)
                    .with_context(|| format!("oidc provider `{provider_key}` client_secret"))?
            } else {
                provider.client_secret.clone()
            };
            if client_secret.trim().is_empty() {
                bail!("oidc provider `{provider_key}` client_secret cannot be empty");
            }
            if provider.scopes.is_empty() {
                bail!("oidc provider `{provider_key}` scopes cannot be empty");
            }
            if !provider.scopes.iter().any(|scope| scope == "openid") {
                bail!("oidc provider `{provider_key}` scopes must include `openid`");
            }
            for scope in &provider.scopes {
                if scope.trim().is_empty() || scope.chars().any(char::is_whitespace) {
                    bail!("oidc provider `{provider_key}` has invalid scope `{scope}`");
                }
            }
            if let Some(membership) = provider.jit.membership.as_ref() {
                let team_key = normalize_config_team_key(&membership.team)
                    .with_context(|| format!("oidc provider `{provider_key}` jit team"))?;
                if !team_keys.contains(&team_key) {
                    bail!(
                        "oidc provider `{provider_key}` jit references unknown team `{team_key}`"
                    );
                }
                if membership.role == MembershipRole::Owner {
                    bail!("oidc provider `{provider_key}` jit cannot assign role `owner`");
                }
            }
        }
        Ok(())
    }

    pub fn resolved_public_base_url(&self) -> anyhow::Result<Option<String>> {
        let Some(raw_url) = self.public_base_url.as_deref() else {
            return Ok(None);
        };
        let resolved_url =
            resolve_secret_reference(raw_url).context("auth.oidc.public_base_url")?;
        let trimmed = resolved_url.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            bail!("auth.oidc.public_base_url cannot be empty");
        }
        let parsed_url =
            url::Url::parse(&trimmed).context("auth.oidc.public_base_url is invalid")?;
        match parsed_url.scheme() {
            "http" | "https" => {}
            scheme => bail!("auth.oidc.public_base_url scheme `{scheme}` is not supported"),
        }
        if parsed_url.host().is_none() {
            bail!("auth.oidc.public_base_url must include a host");
        }
        Ok(Some(trimmed))
    }
}

impl AuthOauthConfig {
    pub(super) fn provider_keys(&self) -> anyhow::Result<std::collections::BTreeSet<String>> {
        self.providers
            .iter()
            .map(|provider| normalize_config_auth_provider_key(&provider.key))
            .collect()
    }

    pub(super) fn validate(&self, teams: &[TeamConfig]) -> anyhow::Result<()> {
        let public_base_url = self.resolved_public_base_url()?;
        if public_base_url.is_none() && self.providers.iter().any(|provider| provider.enabled) {
            bail!("auth.oauth.public_base_url is required when an oauth provider is enabled");
        }
        let mut provider_keys = std::collections::BTreeSet::new();
        let team_keys = teams
            .iter()
            .map(|team| normalize_config_team_key(&team.id))
            .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()?;

        for provider in &self.providers {
            let provider_key = normalize_config_auth_provider_key(&provider.key)
                .context("auth.oauth.providers[].key")?;
            if !provider_keys.insert(provider_key.clone()) {
                bail!("duplicate oauth provider key `{provider_key}`");
            }
            if provider.label.trim().is_empty() {
                bail!("oauth provider `{provider_key}` label cannot be empty");
            }
            if provider.provider_type != "github" {
                bail!(
                    "oauth provider `{provider_key}` has unsupported provider_type `{}`",
                    provider.provider_type
                );
            }
            let client_id = if provider.enabled {
                resolve_path_reference(&provider.client_id)
                    .with_context(|| format!("oauth provider `{provider_key}` client_id"))?
            } else {
                provider.client_id.clone()
            };
            if client_id.trim().is_empty() {
                bail!("oauth provider `{provider_key}` client_id cannot be empty");
            }
            let client_secret = if provider.enabled {
                resolve_secret_reference(&provider.client_secret)
                    .with_context(|| format!("oauth provider `{provider_key}` client_secret"))?
            } else {
                provider.client_secret.clone()
            };
            if client_secret.trim().is_empty() {
                bail!("oauth provider `{provider_key}` client_secret cannot be empty");
            }
            if provider.scopes.is_empty() {
                bail!("oauth provider `{provider_key}` scopes cannot be empty");
            }
            for scope in &provider.scopes {
                if scope.trim().is_empty() || scope.chars().any(char::is_whitespace) {
                    bail!("oauth provider `{provider_key}` has invalid scope `{scope}`");
                }
            }
            if !provider.scopes.iter().any(|scope| scope == "user:email") {
                bail!("oauth provider `{provider_key}` scopes must include `user:email`");
            }
            validate_allowed_email_domains(
                &provider.allowed_email_domains,
                &format!("oauth provider `{provider_key}` allowed_email_domains"),
            )?;
            if let Some(membership) = provider.jit.membership.as_ref() {
                let team_key = normalize_config_team_key(&membership.team)
                    .with_context(|| format!("oauth provider `{provider_key}` jit team"))?;
                if !team_keys.contains(&team_key) {
                    bail!(
                        "oauth provider `{provider_key}` jit references unknown team `{team_key}`"
                    );
                }
                if membership.role == MembershipRole::Owner {
                    bail!("oauth provider `{provider_key}` jit cannot assign role `owner`");
                }
            }
        }

        Ok(())
    }

    pub fn resolved_public_base_url(&self) -> anyhow::Result<Option<String>> {
        let Some(raw_url) = self.public_base_url.as_deref() else {
            return Ok(None);
        };
        let resolved_url =
            resolve_secret_reference(raw_url).context("auth.oauth.public_base_url")?;
        let trimmed = resolved_url.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            bail!("auth.oauth.public_base_url cannot be empty");
        }
        let parsed_url =
            url::Url::parse(&trimmed).context("auth.oauth.public_base_url is invalid")?;
        match parsed_url.scheme() {
            "http" | "https" => {}
            scheme => bail!("auth.oauth.public_base_url scheme `{scheme}` is not supported"),
        }
        if parsed_url.host().is_none() {
            bail!("auth.oauth.public_base_url must include a host");
        }
        Ok(Some(trimmed))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OidcProviderConfig {
    pub key: String,
    #[serde(default)]
    pub label: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_oidc_scopes")]
    pub scopes: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub jit: OidcJitConfig,
}

impl OidcProviderConfig {
    pub(super) fn seed_provider(&self) -> anyhow::Result<SeedOidcProvider> {
        let provider_key = normalize_config_auth_provider_key(&self.key)?;
        Ok(SeedOidcProvider {
            provider_type: "generic_oidc".to_string(),
            label: if self.label.trim().is_empty() {
                provider_key.clone()
            } else {
                self.label.trim().to_string()
            },
            provider_key,
            issuer_url: self.issuer_url.trim().to_string(),
            client_id: self.client_id.trim().to_string(),
            client_secret_ref: self.client_secret.clone(),
            scopes: self.scopes.clone(),
            enabled: self.enabled,
            jit: self.jit.seed_policy()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OauthProviderConfig {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_oauth_provider_type")]
    pub provider_type: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_github_oauth_scopes")]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub allowed_email_domains: Vec<String>,
    #[serde(default = "default_sso_email_verification_enabled")]
    pub sso_email_verification_enabled: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub jit: OauthJitConfig,
}

impl OauthProviderConfig {
    pub(super) fn seed_provider(&self) -> anyhow::Result<SeedOauthProvider> {
        let provider_key = normalize_config_auth_provider_key(&self.key)?;
        Ok(SeedOauthProvider {
            provider_type: self.provider_type.trim().to_string(),
            label: if self.label.trim().is_empty() {
                provider_key.clone()
            } else {
                self.label.trim().to_string()
            },
            provider_key,
            client_id: if self.enabled {
                resolve_path_reference(&self.client_id)?.trim().to_string()
            } else {
                self.client_id.trim().to_string()
            },
            client_secret_ref: self.client_secret.clone(),
            scopes: self.scopes.clone(),
            allowed_email_domains: normalize_allowed_email_domains(
                &self.allowed_email_domains,
                "auth.oauth.providers[].allowed_email_domains",
            )?,
            sso_email_verification_enabled: self.sso_email_verification_enabled,
            enabled: self.enabled,
            jit: self.jit.seed_policy()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OidcJitConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_user_global_role")]
    pub global_role: GlobalRole,
    #[serde(default)]
    pub membership: Option<OidcJitMembershipConfig>,
    #[serde(default = "default_request_logging_enabled")]
    pub request_logging_enabled: bool,
}

impl Default for OidcJitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            global_role: GlobalRole::User,
            membership: None,
            request_logging_enabled: true,
        }
    }
}

impl OidcJitConfig {
    fn seed_policy(&self) -> anyhow::Result<OidcJitPolicy> {
        Ok(OidcJitPolicy {
            enabled: self.enabled,
            global_role: self.global_role,
            membership: self
                .membership
                .as_ref()
                .map(|membership| {
                    Ok::<OidcJitMembership, anyhow::Error>(OidcJitMembership {
                        team_key: normalize_config_team_key(&membership.team)?,
                        role: membership.role,
                    })
                })
                .transpose()?,
            request_logging_enabled: self.request_logging_enabled,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OidcJitMembershipConfig {
    pub team: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OauthJitConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_user_global_role")]
    pub global_role: GlobalRole,
    #[serde(default)]
    pub membership: Option<OauthJitMembershipConfig>,
    #[serde(default = "default_request_logging_enabled")]
    pub request_logging_enabled: bool,
}

impl Default for OauthJitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            global_role: GlobalRole::User,
            membership: None,
            request_logging_enabled: true,
        }
    }
}

impl OauthJitConfig {
    fn seed_policy(&self) -> anyhow::Result<OauthJitPolicy> {
        Ok(OauthJitPolicy {
            enabled: self.enabled,
            global_role: self.global_role,
            membership: self
                .membership
                .as_ref()
                .map(|membership| {
                    Ok::<OauthJitMembership, anyhow::Error>(OauthJitMembership {
                        team_key: normalize_config_team_key(&membership.team)?,
                        role: membership.role,
                    })
                })
                .transpose()?,
            request_logging_enabled: self.request_logging_enabled,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OauthJitMembershipConfig {
    pub team: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapAdminConfig {
    #[serde(default = "default_bootstrap_admin_enabled")]
    pub enabled: bool,
    #[serde(default = "default_bootstrap_admin_email")]
    pub email: String,
    #[serde(default = "default_bootstrap_admin_password")]
    pub password: String,
    #[serde(default)]
    pub require_password_change: bool,
}

impl Default for BootstrapAdminConfig {
    fn default() -> Self {
        Self {
            enabled: default_bootstrap_admin_enabled(),
            email: default_bootstrap_admin_email(),
            password: default_bootstrap_admin_password(),
            require_password_change: false,
        }
    }
}

impl BootstrapAdminConfig {
    pub fn resolved_password(&self) -> anyhow::Result<String> {
        resolve_secret_reference(&self.password)
    }
}
