use std::{collections::BTreeMap, fs, path::Path};

use anyhow::Context;
use gateway_guardrails::GuardrailConfig;
use gateway_service::RequestLogPayloadPolicy;
use gateway_store::StoreConnectionOptions;
use serde::Deserialize;

mod agent_analysis;
mod auth;
mod budget_alerts;
mod budgets;
mod database;
mod guardrails;
mod identity;
mod mcp;
mod models;
mod normalization;
mod permissions;
mod providers;
mod references;
mod request_logging;
mod routes;
mod runtime;
mod seeding;
mod server;

pub use agent_analysis::{
    AgentAnalysisAccessDecision, AgentAnalysisCacheProfileConfig, AgentAnalysisCacheTtlConfig,
    AgentAnalysisConfig, AgentAnalysisMetricsConfig, AgentAnalysisRuntimeCapabilities,
    LoadedAgentAnalysis,
};

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

#[cfg(test)]
use mcp::{default_google_authorization_url, default_google_token_url};

pub use permissions::{
    AdminAction, AdminPage, AdminPermissionGroup, PermissionSetConfig, PermissionsConfig,
    ResolvedAdminPermissions, ResolvedPermissionSet,
};

pub use providers::{
    AnthropicCompatAuthConfig, AnthropicCompatAuthKindConfig, AnthropicCompatProviderConfig,
    AwsBedrockAuthConfig, AwsBedrockProviderConfig, GcpCloudRunOpenAiCompatAuthConfig,
    GcpCloudRunOpenAiCompatAuthHeaderConfig, GcpCloudRunOpenAiCompatProviderConfig,
    GcpVertexAuthConfig, GcpVertexBatchConfig, GcpVertexProviderConfig, GitHubCopilotAuthConfig,
    GitHubCopilotProviderConfig, OpenAiBatchDialectConfig, OpenAiBatchProviderConfig,
    OpenAiCompatAuthConfig, OpenAiCompatProviderConfig, ProviderConfig, ProviderDisplayConfig,
    ProviderTimeouts,
};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
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
    pub agent_analysis: AgentAnalysisConfig,
    #[serde(default)]
    pub guardrails: GuardrailConfig,
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
        self.agent_analysis.validate()?;
        let model_route_keys = self.guardrail_model_route_keys();
        // MCP server references are checked against the seeded registry during startup.
        let configured_mcp_server_keys = self.guardrails.mcp_servers.keys().cloned().collect();
        self.guardrails
            .validate(&model_route_keys, &configured_mcp_server_keys)?;
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

#[cfg(test)]
mod tests;
