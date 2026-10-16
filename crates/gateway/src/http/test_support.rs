use std::{sync::Arc, time::Duration};

use gateway_core::{
    BudgetCadence, Money4, ProviderCapabilities, RouteCompatibility, SeedApiKey, SeedBudget,
    SeedModel, SeedModelRoute, SeedProvider, SeedServiceAccount, SeedTeam, hash_gateway_key_secret,
};
use gateway_guardrails::{GuardrailConfig, GuardrailEngine};
use gateway_service::{GatewayService, McpOauthRuntime, WeightedRoutePlanner};
use gateway_store::{AnyStore, GatewayStore, StoreConnectionOptions, run_migrations_with_options};
use serde_json::json;

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
        skills: None,
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
        benchmark_model_ids: Arc::default(),
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
        count_tokens_limiter: Arc::default(),
        trusted_proxies: crate::http::client_ip::TrustedProxies::default(),
    };
    (directory, state)
}

/// Seeds the `fast` model on a `vertex` route, reachable with the
/// `gwk_streamtest.cancel-secret` API key.
pub async fn seed_stream_cancellation_test(store: &AnyStore) {
    store
        .seed_from_inputs(
            &[SeedProvider {
                provider_key: "vertex".to_string(),
                provider_type: "openai_compat".to_string(),
                config: json!({}),
                secrets: None,
            }],
            &[SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: None,
                max_reasoning_effort: None,
                routing: None,
                description: None,
                tags: Vec::new(),
                rank: 0,
                routes: vec![SeedModelRoute {
                    route_key: None,
                    provider_key: "vertex".to_string(),
                    upstream_model: "fast-upstream".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    context_window_tokens: None,
                    pricing_override: None,
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: RouteCompatibility::default(),
                }],
                allowlist: None,
            }],
            &[SeedApiKey {
                name: "Stream Test Key".to_string(),
                public_id: "streamtest".to_string(),
                secret_hash: hash_gateway_key_secret("cancel-secret").expect("hash test key"),
                service_account_key: "stream-test".to_string(),
                allowed_models: vec!["fast".to_string()],
            }],
            &[SeedServiceAccount {
                service_account_key: "stream-test".to_string(),
                service_account_name: "Stream Test".to_string(),
                team_key: "stream-test".to_string(),
                tags: None,
                budget: SeedBudget {
                    cadence: BudgetCadence::Daily,
                    amount_usd: Money4::from_scaled(250_000),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                },
                managed_api_keys: Vec::new(),
            }],
            &[],
            &[],
            &[SeedTeam {
                team_key: "stream-test".to_string(),
                team_name: "Stream Test".to_string(),
                tags: None,
            }],
            &[],
        )
        .await
        .expect("seed cancellation test");
}

/// A clock the rate limiter reads, so tests can step across windows.
#[derive(Clone)]
pub struct TestClock(Arc<std::sync::Mutex<time::OffsetDateTime>>);

impl TestClock {
    pub fn advance(&self, seconds: i64) {
        *self.0.lock().unwrap() += time::Duration::seconds(seconds);
    }
}

/// Replaces the service with one that enforces `policies`, keeping the store.
/// The clock starts ten seconds into the current minute.
pub fn install_rate_limits(
    state: &mut AppState,
    policies: Vec<gateway_service::ConfiguredRateLimitPolicy>,
) -> TestClock {
    let now = time::OffsetDateTime::now_utc();
    let start = now
        - time::Duration::seconds(now.unix_timestamp().rem_euclid(60) - 10)
        - time::Duration::nanoseconds(i64::from(now.nanosecond()));
    let clock = TestClock(Arc::new(std::sync::Mutex::new(start)));
    let reader = clock.clone();
    let limiter = gateway_service::RateLimiter::new(
        state.store.clone(),
        gateway_service::RateLimitSettings {
            config_policies: policies,
            ..Default::default()
        },
    )
    .with_clock(move || *reader.0.lock().unwrap());
    state.service = Arc::new(
        GatewayService::new(
            state.store.clone(),
            Arc::new(WeightedRoutePlanner::default()),
        )
        .with_rate_limiter(Arc::new(limiter)),
    );
    clock
}

/// A rate-limit store that is down.
pub struct DownRateLimitStore;

#[async_trait::async_trait]
impl gateway_core::RateLimitRepository for DownRateLimitStore {
    async fn list_rate_limit_policies(
        &self,
    ) -> Result<Vec<gateway_core::RateLimitPolicyRecord>, gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn get_rate_limit_policy(
        &self,
        _: uuid::Uuid,
    ) -> Result<Option<gateway_core::RateLimitPolicyRecord>, gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn insert_rate_limit_policy(
        &self,
        _: &gateway_core::RateLimitPolicyRecord,
    ) -> Result<(), gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn update_rate_limit_policy(
        &self,
        _: &gateway_core::RateLimitPolicyRecord,
    ) -> Result<
        Option<gateway_core::Updated<gateway_core::RateLimitPolicyRecord>>,
        gateway_core::StoreError,
    > {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn delete_rate_limit_policy(
        &self,
        _: uuid::Uuid,
    ) -> Result<Option<gateway_core::RateLimitPolicyRecord>, gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn consume_rate_limits(
        &self,
        _: &[gateway_core::RateLimitCounterCharge],
    ) -> Result<gateway_core::RateLimitConsumeResult, gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn add_rate_limit_usage(
        &self,
        _: &[gateway_core::RateLimitCounterIncrement],
    ) -> Result<(), gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
    async fn purge_expired_rate_limit_counters(
        &self,
        _: i64,
        _: i64,
    ) -> Result<u64, gateway_core::StoreError> {
        Err(gateway_core::StoreError::Unavailable("down".to_string()))
    }
}

/// Replaces the service with one whose rate limiter cannot reach its store
/// and fails closed, so every rate-limit check errors with 503.
pub fn install_down_rate_limiter(state: &mut AppState) {
    let limiter = gateway_service::RateLimiter::new(
        Arc::new(DownRateLimitStore),
        gateway_service::RateLimitSettings {
            failure_mode: gateway_service::RateLimitFailureMode::Closed,
            ..Default::default()
        },
    );
    state.service = Arc::new(
        GatewayService::new(
            state.store.clone(),
            Arc::new(WeightedRoutePlanner::default()),
        )
        .with_rate_limiter(Arc::new(limiter)),
    );
}
