use std::sync::Arc;

use gateway_core::{ProviderRegistry, SeedHumanBudgetDefaults};
use gateway_service::{GatewayService, WeightedRoutePlanner};
use gateway_store::AnyStore;

use crate::observability::GatewayMetrics;

pub type AppGatewayService = GatewayService<AnyStore, WeightedRoutePlanner>;

#[derive(Debug, Clone, Copy)]
pub struct AgentAnalysisRuntimeCapabilities {
    pub passive_analysis_enabled: bool,
    pub shadow_diagnostics_visible: bool,
    pub calibrated_score_visible: bool,
    pub team_admin_analytics_enabled: bool,
    pub aggregate_monitoring_enabled: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<AppGatewayService>,
    pub store: Arc<AnyStore>,
    pub providers: ProviderRegistry,
    pub metrics: Arc<GatewayMetrics>,
    pub mcp_http_client: reqwest::Client,
    pub identity_token_secret: Arc<String>,
    pub oidc_public_base_url: Arc<Option<String>>,
    pub oauth_public_base_url: Arc<Option<String>>,
    pub client_config_gateway_base_url: Arc<Option<String>>,
    pub budget_defaults: Arc<SeedHumanBudgetDefaults>,
    pub agent_analysis: AgentAnalysisRuntimeCapabilities,
}
