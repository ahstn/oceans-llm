use std::{collections::BTreeMap, env, fs, net::SocketAddr, path::Path};

use anyhow::{Context, bail};
use gateway_core::{
    ApiKeySecretStorageKind, AuthMode, AwsBedrockApiStyle, AwsBedrockRouteCompatibility,
    BudgetCadence, GitHubCopilotRouteCompatibility, GlobalRole, ManagedApiKeySource,
    MembershipRole, ModelAllowlistPolicy, Money4, OauthJitMembership, OauthJitPolicy,
    OidcJitMembership, OidcJitPolicy, OpenAiCompatDeveloperRole, OpenAiCompatEmptyTools,
    OpenAiCompatMaxTokensField, OpenAiCompatReasoningEffort, OpenAiCompatRouteCompatibility,
    OpenRouterMaxPrice, OpenRouterPercentileCutoffs, OpenRouterPercentilePreference,
    OpenRouterProviderRouting, OpenRouterRouteCompatibility, ProviderCapabilities,
    RequestLogRetentionWindow, RequestTag, RouteCompatibility, RoutePricingOverride,
    SeedApiKeySecretMaterial, SeedBudget, SeedHumanBudgetDefaults, SeedManagedServiceAccountApiKey,
    SeedModel, SeedModelRoute, SeedOauthProvider, SeedOidcProvider, SeedProvider,
    SeedServiceAccount, SeedTeam, SeedUser, SeedUserMembership, SeedUserModelBudgetDefault,
    hash_gateway_key_secret, parse_gateway_api_key, validate_entity_tags,
};
use gateway_providers::{
    BedrockAuthConfig, BedrockEndpointKind, BedrockProviderConfig, CloudRunOpenAiCompatAuth,
    CopilotAuthConfig, CopilotProviderConfig, OpenAiCompatConfig, VertexAuthConfig,
    VertexProviderConfig,
};
use gateway_service::{
    McpOauthProvider, McpOauthRuntime, PayloadPath, RequestLogPayloadCaptureMode,
    RequestLogPayloadPolicy, encrypt_gateway_api_key_secret, parse_payload_path,
};
use gateway_store::StoreConnectionOptions;
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value, json};
use uuid::Uuid;

mod auth;
mod budget_alerts;
mod budgets;
mod database;
mod identity;
mod mcp;
mod models;
mod permissions;
mod providers;
mod references;
mod request_logging;
mod routes;
mod runtime;
mod seeding;
mod server;

pub use auth::{
    AuthConfig, AuthOauthConfig, AuthOidcConfig, BootstrapAdminConfig, OauthJitConfig,
    OauthJitMembershipConfig, OauthProviderConfig, OidcJitConfig, OidcJitMembershipConfig,
    OidcProviderConfig,
};
pub use budget_alerts::{
    BudgetAlertConfig, BudgetAlertEmailConfig, BudgetAlertEmailTransportConfig,
    SmtpBudgetAlertEmailTransportConfig,
};
pub use budgets::{
    BudgetConfig, BudgetsConfig, UserBudgetDefaultsConfig, UserModelBudgetDefaultConfig,
};
pub use database::DatabaseConfig;
pub use identity::{
    ServiceAccountConfig, ServiceAccountKeyConfig, TeamConfig, UserConfig, UserMembershipConfig,
};
pub use mcp::{McpConfig, McpOauthConfig, McpOauthProviderConfig};
pub use models::{ModelAllowlistConfig, ModelConfig};
pub use request_logging::{
    RequestLogPayloadCaptureModeConfig, RequestLogPayloadConfig, RequestLogPurgeConfig,
    RequestLoggingConfig,
};
pub use routes::{
    AwsBedrockRouteCompatibilityConfig, ModelRouteConfig, OpenAiCompatRouteCompatibilityConfig,
    RouteCapabilitiesConfig, RouteCompatibilityConfig, RoutePricingOverrideConfig,
};
pub use server::ServerConfig;

pub(crate) use references::resolve_secret_reference;

use references::{
    ResolvedCopilotPrivateKey, resolve_copilot_private_key, resolve_path_reference,
    validate_env_reference_if_needed,
};

#[cfg(test)]
use mcp::{default_google_authorization_url, default_google_token_url};

pub use permissions::{
    AdminAction, AdminPage, AdminPermissionGroup, PermissionSetConfig, PermissionsConfig,
    ResolvedAdminPermissions, ResolvedPermissionSet,
};

pub use providers::{
    AwsBedrockAuthConfig, AwsBedrockProviderConfig, GcpCloudRunOpenAiCompatAuthConfig,
    GcpCloudRunOpenAiCompatAuthHeaderConfig, GcpCloudRunOpenAiCompatProviderConfig,
    GcpVertexAuthConfig, GcpVertexProviderConfig, GitHubCopilotAuthConfig,
    GitHubCopilotProviderConfig, OpenAiCompatAuthConfig, OpenAiCompatProviderConfig,
    ProviderConfig, ProviderDisplayConfig, ProviderTimeouts,
};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GatewayConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub budget_alerts: BudgetAlertConfig,
    #[serde(default)]
    pub budgets: BudgetsConfig,
    #[serde(default)]
    pub request_logging: RequestLoggingConfig,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    #[serde(default)]
    pub teams: Vec<TeamConfig>,
    #[serde(default)]
    pub service_accounts: Vec<ServiceAccountConfig>,
    #[serde(default)]
    pub users: Vec<UserConfig>,
}

impl GatewayConfig {
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed reading config file `{}`", path.display()))?;
        let parsed: Self = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed parsing yaml config `{}`", path.display()))?;

        parsed
            .validate()
            .with_context(|| format!("invalid gateway configuration `{}`", path.display()))?;

        Ok(parsed)
    }

    fn validate(&self) -> anyhow::Result<()> {
        self.server.validate()?;
        self.database.connection_options()?;
        self.budget_alerts.validate()?;
        self.request_logging.validate()?;
        self.permissions.resolve()?;
        self.auth.oidc.validate(&self.teams)?;
        self.auth.oauth.validate(&self.teams)?;
        self.mcp.oauth.validate()?;

        let provider_by_id = self
            .providers
            .iter()
            .map(|provider| (provider.id().to_string(), provider))
            .collect::<BTreeMap<_, _>>();
        let model_by_id = self
            .models
            .iter()
            .map(|model| (model.id.as_str(), model))
            .collect::<BTreeMap<_, _>>();

        providers::validate_providers(&self.providers)?;

        models::validate_models(&self.models, &model_by_id, &provider_by_id)?;

        budgets::validate_user_defaults(&self.budgets, &model_by_id)?;

        let team_keys = identity::validate_teams(&self.teams)?;
        identity::validate_service_accounts(&self.service_accounts, &team_keys, &model_by_id)?;
        identity::validate_users(&self.users, &self.auth, &team_keys)?;

        Ok(())
    }

    pub fn resolved_admin_permissions(&self) -> anyhow::Result<ResolvedAdminPermissions> {
        self.permissions.resolve()
    }

    pub fn database_options(&self) -> anyhow::Result<StoreConnectionOptions> {
        self.database.connection_options()
    }

    pub fn request_log_payload_policy(&self) -> anyhow::Result<RequestLogPayloadPolicy> {
        self.request_logging.payloads.to_policy()
    }
}

fn normalize_model_allowlist(
    model_id: &str,
    allowlist: &ModelAllowlistConfig,
) -> anyhow::Result<ModelAllowlistPolicy> {
    let users = allowlist
        .users
        .iter()
        .map(|email| normalize_config_email(email))
        .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let teams = allowlist
        .teams
        .iter()
        .map(|team_key| normalize_config_team_key(team_key))
        .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()?
        .into_iter()
        .collect::<Vec<_>>();

    if users.is_empty() && teams.is_empty() {
        bail!("model `{model_id}` allowlist must include at least one user or team");
    }

    Ok(ModelAllowlistPolicy { users, teams })
}

fn normalize_config_email(email: &str) -> anyhow::Result<String> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.is_empty() || !normalized.contains('@') {
        bail!("email must be a valid email address");
    }
    Ok(normalized)
}

fn normalize_config_entity_tags(
    tags: &[RequestTag],
    context: &str,
) -> anyhow::Result<Vec<RequestTag>> {
    validate_entity_tags(tags, context).map_err(|error| anyhow::anyhow!("{error}"))
}

fn normalize_optional_config_entity_tags(
    tags: Option<&[RequestTag]>,
    context: &str,
) -> anyhow::Result<Option<Vec<RequestTag>>> {
    tags.map(|tags| normalize_config_entity_tags(tags, context))
        .transpose()
}

fn normalize_config_team_key(team_key: &str) -> anyhow::Result<String> {
    let normalized = team_key.trim().to_string();
    if normalized.is_empty() {
        bail!("team key cannot be empty");
    }
    Ok(normalized)
}

fn normalize_config_model_key(model_key: &str) -> anyhow::Result<String> {
    let normalized = model_key.trim().to_string();
    if normalized.is_empty() {
        bail!("model key cannot be empty");
    }
    Ok(normalized)
}

fn config_model_uuid(model_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("model:{model_key}").as_bytes(),
    )
}

fn normalize_config_service_account_key(service_account_key: &str) -> anyhow::Result<String> {
    let normalized = service_account_key.trim().to_string();
    if normalized.is_empty() {
        bail!("service account key cannot be empty");
    }
    Ok(normalized)
}

fn normalize_config_managed_api_key(config_key: &str) -> anyhow::Result<String> {
    let normalized = config_key.trim().to_string();
    if normalized.is_empty() {
        bail!("managed api key id cannot be empty");
    }
    Ok(normalized)
}

fn normalize_config_oidc_provider_key(provider_key: &str) -> anyhow::Result<String> {
    let normalized = provider_key.trim().to_string();
    if normalized.is_empty() {
        bail!("cannot be empty");
    }
    Ok(normalized)
}

fn normalize_config_oauth_provider_key(provider_key: &str) -> anyhow::Result<String> {
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
const fn default_model_rank() -> i32 {
    100
}

const fn default_route_priority() -> i32 {
    100
}

const fn default_route_weight() -> f64 {
    1.0
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

const fn default_request_log_request_max_bytes() -> usize {
    64 * 1024
}

const fn default_request_log_response_max_bytes() -> usize {
    64 * 1024
}

const fn default_request_log_stream_max_events() -> usize {
    128
}

const fn default_user_global_role() -> GlobalRole {
    GlobalRole::User
}

const fn default_membership_role() -> MembershipRole {
    MembershipRole::Member
}

fn default_budget_timezone() -> String {
    "UTC".to_string()
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

fn default_budget_alert_from_email() -> String {
    "alerts@local".to_string()
}

const fn default_budget_alert_poll_interval_secs() -> u64 {
    30
}

const fn default_budget_alert_batch_size() -> u32 {
    25
}

const fn default_request_log_purge_retention() -> RequestLogRetentionWindow {
    RequestLogRetentionWindow::SevenDays
}

fn default_request_log_purge_schedule() -> String {
    "0 0 * * *".to_string()
}

fn validate_daily_cron_schedule(field_name: &str, schedule: &str) -> anyhow::Result<()> {
    let schedule = schedule.trim();
    let fields = schedule.split_whitespace().count();
    if fields != 5 {
        bail!("{field_name} must use standard 5-field cron syntax");
    }

    let parsed: cron::Schedule = format!("0 {schedule}")
        .parse()
        .with_context(|| format!("{field_name} `{schedule}` is invalid"))?;
    let mut upcoming = parsed.upcoming(chrono::Utc);
    let first = upcoming
        .next()
        .ok_or_else(|| anyhow::anyhow!("{field_name} `{schedule}` has no upcoming run"))?;
    let second = upcoming
        .next()
        .ok_or_else(|| anyhow::anyhow!("{field_name} `{schedule}` has fewer than two runs"))?;

    if second - first < chrono::Duration::days(1) {
        bail!("{field_name} must not run more frequently than once per day");
    }

    Ok(())
}

const fn default_budget_alert_smtp_port() -> u16 {
    587
}

const fn default_budget_alert_smtp_starttls() -> bool {
    true
}

#[cfg(test)]
mod tests;
