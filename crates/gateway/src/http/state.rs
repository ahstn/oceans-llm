use std::sync::Arc;

use gateway_core::ProviderRegistry;
use gateway_service::{CodeExecutor, CodeModeLimits, GatewayService, WeightedRoutePlanner};
use gateway_store::AnyStore;

use crate::observability::GatewayMetrics;

pub type AppGatewayService = GatewayService<AnyStore, WeightedRoutePlanner>;

/// Runtime state for the `/code-mode-mcp` surface. When `enabled` is false
/// the route returns 404 and the executor is never invoked.
#[derive(Clone)]
pub struct CodeModeState {
    pub enabled: bool,
    pub executor: Arc<dyn CodeExecutor>,
    pub limits: CodeModeLimits,
}

impl CodeModeState {
    /// Disabled state with an executor that must never run.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            executor: Arc::new(UnreachableExecutor),
            limits: CodeModeLimits::default(),
        }
    }
}

/// Placeholder executor used while Code Mode is disabled. The route guards
/// on `enabled`, so executing through this is an infrastructure bug.
struct UnreachableExecutor;

#[async_trait::async_trait]
impl CodeExecutor for UnreachableExecutor {
    async fn execute(
        &self,
        _code: &str,
        _dispatcher: Arc<dyn gateway_service::HostDispatcher>,
        _limits: &CodeModeLimits,
    ) -> Result<gateway_service::ExecutionOutcome, gateway_service::ExecutorError> {
        Err(gateway_service::ExecutorError::Infrastructure(
            "code mode is disabled; no executor is configured".to_string(),
        ))
    }
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
    pub code_mode: CodeModeState,
}
