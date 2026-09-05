use std::{sync::Arc, time::Duration};

use gateway_guardrails::{GuardrailConfig, GuardrailEngine};
use gateway_service::{GatewayService, McpOauthRuntime, WeightedRoutePlanner};
use gateway_store::{AnyStore, StoreConnectionOptions, run_migrations_with_options};

use crate::{
    config::{AgentAnalysisRuntimeCapabilities, PermissionsConfig},
    http::{response_cache::ResponseCache, state::AppState},
    observability::GatewayMetrics,
};

pub async fn app_state() -> (tempfile::TempDir, AppState) {
    let directory = tempfile::tempdir().expect("test database directory");
    let options = StoreConnectionOptions::Libsql {
        path: directory.path().join("gateway.db"),
    };
    run_migrations_with_options(&options)
        .await
        .expect("migrations");
    let store = Arc::new(AnyStore::connect(&options).await.expect("test store"));
    let state = AppState {
        service: Arc::new(GatewayService::new(
            store.clone(),
            Arc::new(WeightedRoutePlanner::default()),
        )),
        store,
        providers: Default::default(),
        copilot_user_provider_keys: Arc::new(Vec::new()),
        guardrail_engine: Arc::new(GuardrailEngine::new(Vec::new(), Default::default())),
        guardrail_config: Arc::new(GuardrailConfig::default()),
        metrics: Arc::new(GatewayMetrics::new()),
        mcp_http_client: reqwest::Client::new(),
        mcp_oauth_runtime: Arc::new(McpOauthRuntime::new(None, Vec::new())),
        identity_token_secret: Arc::new("test-secret".to_string()),
        oidc_public_base_url: Arc::new(None),
        oauth_public_base_url: Arc::new(None),
        client_config_gateway_base_url: Arc::new(None),
        budget_defaults: Arc::new(Default::default()),
        agent_analysis: AgentAnalysisRuntimeCapabilities {
            passive_analysis_enabled: false,
            shadow_diagnostics_visible: false,
            calibrated_score_visible: false,
            team_admin_analytics_enabled: false,
        },
        admin_permissions: Arc::new(PermissionsConfig::default().resolve().expect("permissions")),
        leaderboard_cache: Arc::new(ResponseCache::new(Duration::from_secs(1))),
        harness_usage_cache: Arc::new(ResponseCache::new(Duration::from_secs(1))),
    };
    (directory, state)
}
