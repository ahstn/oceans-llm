use std::{collections::HashMap, sync::Arc};

use gateway_core::{ProviderRegistry, SeedHumanBudgetDefaults, SkillObjectStore};
use gateway_guardrails::{GuardrailConfig, GuardrailEngine};
use gateway_service::{
    GatewayService, McpClientOauthSettings, McpOauthRuntime, McpUpstreamClients, SkillService,
    WeightedRoutePlanner,
};
use gateway_store::AnyStore;

use crate::http::{
    admin_contract::{HarnessUsageView, LeaderboardView},
    client_ip::TrustedProxies,
    handlers::{CountTokensLimiter, McpTelemetrySnapshot},
    mcp_client_oauth::OauthStreamWatcher,
    response_cache::ResponseCache,
};
use crate::{
    config::{AgentAnalysisRuntimeCapabilities, ResolvedAdminPermissions},
    observability::GatewayMetrics,
};

pub type AppGatewayService = GatewayService<AnyStore, WeightedRoutePlanner>;
pub type AppSkillService = SkillService<AnyStore, dyn SkillObjectStore>;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<AppGatewayService>,
    pub skills: Option<Arc<AppSkillService>>,
    pub store: Arc<AnyStore>,
    pub providers: ProviderRegistry,
    pub copilot_user_provider_keys: Arc<Vec<String>>,
    pub guardrail_engine: Arc<GuardrailEngine>,
    pub guardrail_config: Arc<GuardrailConfig>,
    pub metrics: Arc<GatewayMetrics>,
    /// Clients for upstream MCP traffic; a team-owned server's reach only the
    /// addresses its URL allows.
    pub mcp_http_clients: McpUpstreamClients,
    pub mcp_oauth_runtime: Arc<McpOauthRuntime>,
    /// The gateway as an OAuth authorization server for MCP clients; `None` when disabled.
    pub mcp_client_oauth: Option<Arc<McpClientOauthSettings>>,
    /// Ends OAuth callers' MCP event streams when their access stops.
    pub oauth_streams: Arc<OauthStreamWatcher>,
    /// Ends MCP event streams when their caller can no longer open them.
    pub mcp_stream_reach: Arc<crate::http::mcp_gateway::stream_reach::McpStreamReachWatcher>,
    pub identity_token_secret: Arc<String>,
    pub oidc_public_base_url: Arc<Option<String>>,
    pub oauth_public_base_url: Arc<Option<String>>,
    pub client_config_gateway_base_url: Arc<Option<String>>,
    pub benchmark_model_ids: Arc<HashMap<String, String>>,
    pub budget_defaults: Arc<SeedHumanBudgetDefaults>,
    pub agent_analysis: AgentAnalysisRuntimeCapabilities,
    pub admin_permissions: Arc<ResolvedAdminPermissions>,
    /// Who may publish an MCP server to the organisation catalog.
    pub mcp_organisation_publishing: gateway_service::McpOrganisationPublishing,
    pub leaderboard_cache: Arc<ResponseCache<String, LeaderboardView>>,
    pub harness_usage_cache: Arc<ResponseCache<String, HarnessUsageView>>,
    pub count_tokens_limiter: Arc<CountTokensLimiter>,
    /// Each credential's MCP tool set for inference request telemetry, by
    /// API key or OAuth grant id, kept briefly so requests skip re-resolving it.
    pub mcp_telemetry_cache: Arc<ResponseCache<uuid::Uuid, Arc<McpTelemetrySnapshot>>>,
    pub trusted_proxies: TrustedProxies,
}
