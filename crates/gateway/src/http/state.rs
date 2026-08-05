use std::sync::Arc;

use gateway_core::{ProviderRegistry, SeedHumanBudgetDefaults};
use gateway_service::{GatewayService, WeightedRoutePlanner};
use gateway_store::AnyStore;

use crate::http::{
    admin_contract::{HarnessUsageView, LeaderboardView},
    response_cache::ResponseCache,
};
use crate::{config::ResolvedAdminPermissions, observability::GatewayMetrics};

pub type AppGatewayService = GatewayService<AnyStore, WeightedRoutePlanner>;

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
    pub admin_permissions: Arc<ResolvedAdminPermissions>,
    pub leaderboard_cache: Arc<ResponseCache<String, LeaderboardView>>,
    pub harness_usage_cache: Arc<ResponseCache<String, HarnessUsageView>>,
}
