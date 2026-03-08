use std::sync::Arc;

use gateway_core::ProviderRegistry;
use gateway_service::{GatewayService, WeightedRoutePlanner};
use gateway_store::LibsqlStore;

pub type AppGatewayService = GatewayService<LibsqlStore, WeightedRoutePlanner>;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<AppGatewayService>,
    pub store: Arc<LibsqlStore>,
    pub providers: ProviderRegistry,
    pub identity_token_secret: Arc<String>,
}
