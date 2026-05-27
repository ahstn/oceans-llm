use std::sync::Arc;

use gateway_core::ProviderRegistry;
use gateway_service::{GatewayService, WeightedRoutePlanner};
use gateway_store::AnyStore;

use crate::observability::GatewayMetrics;

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
}
