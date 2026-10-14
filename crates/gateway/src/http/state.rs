use std::{collections::HashMap, sync::Arc};

use gateway_core::{ProviderRegistry, SeedHumanBudgetDefaults, SkillObjectStore};
use gateway_guardrails::{GuardrailConfig, GuardrailEngine};
use gateway_service::{
    GatewayService, McpClientOauthSettings, McpOauthRuntime, SkillService, WeightedRoutePlanner,
};
use gateway_store::AnyStore;

use crate::http::{
    admin_contract::{HarnessUsageView, LeaderboardView},
    handlers::CountTokensLimiter,
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
    pub mcp_http_client: reqwest::Client,
    pub mcp_oauth_runtime: Arc<McpOauthRuntime>,
    /// The gateway as an OAuth authorization server for MCP clients; `None` when disabled.
    pub mcp_client_oauth: Option<Arc<McpClientOauthSettings>>,
    pub identity_token_secret: Arc<String>,
    pub oidc_public_base_url: Arc<Option<String>>,
    pub oauth_public_base_url: Arc<Option<String>>,
    pub client_config_gateway_base_url: Arc<Option<String>>,
    pub benchmark_model_ids: Arc<HashMap<String, String>>,
    pub budget_defaults: Arc<SeedHumanBudgetDefaults>,
    pub agent_analysis: AgentAnalysisRuntimeCapabilities,
    pub admin_permissions: Arc<ResolvedAdminPermissions>,
    pub leaderboard_cache: Arc<ResponseCache<String, LeaderboardView>>,
    pub harness_usage_cache: Arc<ResponseCache<String, HarnessUsageView>>,
    pub count_tokens_limiter: Arc<CountTokensLimiter>,
}
