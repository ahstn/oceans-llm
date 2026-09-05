use std::collections::BTreeMap;

use anyhow::{Context, bail};
use gateway_core::{AuthMode, GlobalRole, MembershipRole, RequestTag};
use serde::Deserialize;

use super::{
    auth::{AuthConfig, normalize_config_auth_provider_key},
    budgets::BudgetConfig,
    models::ModelConfig,
    normalization::{
        normalize_config_email, normalize_config_team_key, normalize_optional_config_entity_tags,
    },
};

pub(super) fn normalize_config_service_account_key(
    service_account_key: &str,
) -> anyhow::Result<String> {
    let normalized = service_account_key.trim().to_string();
    if normalized.is_empty() {
        bail!("service account key cannot be empty");
    }
    Ok(normalized)
}

pub(super) fn normalize_config_managed_api_key(config_key: &str) -> anyhow::Result<String> {
    let normalized = config_key.trim().to_string();
    if normalized.is_empty() {
        bail!("managed api key id cannot be empty");
    }
    Ok(normalized)
}

const fn default_enabled() -> bool {
    true
}

const fn default_request_logging_enabled() -> bool {
    true
}

const fn default_user_global_role() -> GlobalRole {
    GlobalRole::User
}

const fn default_membership_role() -> MembershipRole {
    MembershipRole::Member
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tags: Option<Vec<RequestTag>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceAccountConfig {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub team: String,
    #[serde(default)]
    pub tags: Option<Vec<RequestTag>>,
    pub budget: BudgetConfig,
    #[serde(default)]
    pub keys: Vec<ServiceAccountKeyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceAccountKeyConfig {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_enabled")]
    pub auto_create: bool,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub allowed_models: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserConfig {
    pub name: String,
    pub email: String,
    pub auth_mode: AuthMode,
    #[serde(default = "default_user_global_role")]
    pub global_role: GlobalRole,
    #[serde(default = "default_request_logging_enabled")]
    pub request_logging_enabled: bool,
    #[serde(default)]
    pub tags: Option<Vec<RequestTag>>,
    #[serde(default)]
    pub oidc_provider_key: Option<String>,
    #[serde(default)]
    pub oauth_provider_key: Option<String>,
    #[serde(default)]
    pub membership: Option<UserMembershipConfig>,
    #[serde(default)]
    pub budget: Option<BudgetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserMembershipConfig {
    pub team: String,
    #[serde(default = "default_membership_role")]
    pub role: MembershipRole,
}

pub(super) fn validate_teams(
    teams: &[TeamConfig],
) -> anyhow::Result<std::collections::BTreeSet<String>> {
    let mut team_keys = std::collections::BTreeSet::new();
    for team in teams {
        let team_key = normalize_config_team_key(&team.id)?;
        if team.name.trim().is_empty() {
            bail!("team `{team_key}` name cannot be empty");
        }
        normalize_optional_config_entity_tags(
            team.tags.as_deref(),
            &format!("team `{team_key}` tags"),
        )?;
        if !team_keys.insert(team_key.clone()) {
            bail!("duplicate team id `{team_key}`");
        }
    }
    Ok(team_keys)
}

pub(super) fn validate_service_accounts(
    service_accounts: &[ServiceAccountConfig],
    team_keys: &std::collections::BTreeSet<String>,
    model_by_id: &BTreeMap<&str, &ModelConfig>,
) -> anyhow::Result<()> {
    let mut service_account_keys = std::collections::BTreeSet::new();
    for service_account in service_accounts {
        let service_account_key = normalize_config_service_account_key(&service_account.id)?;
        if !service_account_keys.insert(service_account_key.clone()) {
            bail!("duplicate service account id `{service_account_key}`");
        }
        if let Some(name) = &service_account.name
            && name.trim().is_empty()
        {
            bail!("service account `{service_account_key}` name cannot be empty");
        }
        normalize_optional_config_entity_tags(
            service_account.tags.as_deref(),
            &format!("service account `{service_account_key}` tags"),
        )?;
        let team_key = normalize_config_team_key(&service_account.team)
            .with_context(|| format!("service account `{service_account_key}` team"))?;
        if !team_keys.contains(&team_key) {
            bail!("service account `{service_account_key}` references unknown team `{team_key}`");
        }
        service_account
            .budget
            .validate(&format!("service account `{service_account_key}` budget"))?;

        let mut managed_key_ids = std::collections::BTreeSet::new();
        for key in &service_account.keys {
            let config_key = normalize_config_managed_api_key(&key.id)
                .with_context(|| format!("service account `{service_account_key}` key id"))?;
            if !managed_key_ids.insert(config_key.clone()) {
                bail!(
                    "service account `{service_account_key}` has duplicate key id `{config_key}`"
                );
            }
            if let Some(name) = &key.name
                && name.trim().is_empty()
            {
                bail!(
                    "service account `{service_account_key}` key `{config_key}` name cannot be empty"
                );
            }
            if !key.auto_create && key.value.is_none() {
                bail!(
                    "service account `{service_account_key}` key `{config_key}` must set value when auto_create is false"
                );
            }
            for model_key in &key.allowed_models {
                if !model_by_id.contains_key(model_key.as_str()) {
                    bail!(
                        "service account `{service_account_key}` key `{config_key}` references unknown model `{model_key}`"
                    );
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate_users(
    users: &[UserConfig],
    auth: &AuthConfig,
    team_keys: &std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    let reserved_bootstrap_admin_email = normalize_config_email(&auth.bootstrap_admin.email)
        .context("bootstrap_admin.email must be a valid email address")?;
    let oidc_provider_keys = auth.oidc.provider_keys()?;
    let oauth_provider_keys = auth.oauth.provider_keys()?;
    let mut user_emails = std::collections::BTreeSet::new();

    for user in users {
        if user.name.trim().is_empty() {
            bail!("user name cannot be empty");
        }
        let email_normalized = normalize_config_email(&user.email)?;
        if email_normalized == reserved_bootstrap_admin_email {
            bail!("user email `{reserved_bootstrap_admin_email}` is reserved for bootstrap admin");
        }
        if !user_emails.insert(email_normalized.clone()) {
            bail!("duplicate user email `{email_normalized}`");
        }
        normalize_optional_config_entity_tags(
            user.tags.as_deref(),
            &format!("user `{}` tags", user.email),
        )?;
        validate_user_auth(user, &oidc_provider_keys, &oauth_provider_keys)?;

        if let Some(membership) = &user.membership {
            let membership_team = normalize_config_team_key(&membership.team)
                .with_context(|| format!("user `{}` membership team", user.email))?;
            if !team_keys.contains(&membership_team) {
                bail!(
                    "user `{}` references unknown team `{}`",
                    user.email,
                    membership_team
                );
            }
            if membership.role == MembershipRole::Owner {
                bail!("user `{}` cannot seed membership role `owner`", user.email);
            }
        }
        if let Some(budget) = &user.budget {
            budget.validate(&format!("user `{}` budget", user.email))?;
        }
    }
    Ok(())
}

fn validate_user_auth(
    user: &UserConfig,
    oidc_provider_keys: &std::collections::BTreeSet<String>,
    oauth_provider_keys: &std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    match user.auth_mode {
        AuthMode::Oidc => {
            let Some(provider_key) = user.oidc_provider_key.as_deref() else {
                bail!(
                    "user `{}` with auth_mode `oidc` requires oidc_provider_key",
                    user.email
                );
            };
            let provider_key = normalize_config_auth_provider_key(provider_key)
                .with_context(|| format!("user `{}` oidc_provider_key", user.email))?;
            if !oidc_provider_keys.contains(&provider_key) {
                bail!(
                    "user `{}` references unknown oidc provider `{provider_key}`",
                    user.email
                );
            }
            if user.oauth_provider_key.is_some() {
                bail!(
                    "user `{}` cannot set oauth_provider_key unless auth_mode is `oauth`",
                    user.email
                );
            }
        }
        AuthMode::Password => {
            if user.oidc_provider_key.is_some() {
                bail!(
                    "user `{}` cannot set oidc_provider_key unless auth_mode is `oidc`",
                    user.email
                );
            }
            if user.oauth_provider_key.is_some() {
                bail!(
                    "user `{}` cannot set oauth_provider_key unless auth_mode is `oauth`",
                    user.email
                );
            }
        }
        AuthMode::Oauth => {
            let Some(provider_key) = user.oauth_provider_key.as_deref() else {
                bail!(
                    "user `{}` with auth_mode `oauth` requires oauth_provider_key",
                    user.email
                );
            };
            let provider_key = normalize_config_auth_provider_key(provider_key)
                .with_context(|| format!("user `{}` oauth_provider_key", user.email))?;
            if !oauth_provider_keys.contains(&provider_key) {
                bail!(
                    "user `{}` references unknown oauth provider `{provider_key}`",
                    user.email
                );
            }
            if user.oidc_provider_key.is_some() {
                bail!(
                    "user `{}` cannot set oidc_provider_key unless auth_mode is `oidc`",
                    user.email
                );
            }
        }
    }
    Ok(())
}
