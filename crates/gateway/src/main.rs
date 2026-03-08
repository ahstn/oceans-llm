mod config;
mod http;
mod observability;

use std::{env, net::SocketAddr, path::Path, sync::Arc, time::Duration};

use admin_ui::AdminUiConfig;
use anyhow::Context;
use gateway_core::ProviderRegistry;
use gateway_providers::{OpenAiCompatProvider, VertexProvider};
use gateway_service::{
    DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL, GatewayService, WeightedRoutePlanner,
    hash_gateway_key_secret,
};
use gateway_store::{LibsqlStore, run_migrations};
use http::{build_router, state::AppState};
use tokio::net::TcpListener;
use crate::config::{BootstrapAdminConfig, GatewayConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = env::var("GATEWAY_CONFIG").unwrap_or_else(|_| "./gateway.yaml".to_string());
    let config = GatewayConfig::from_path(Path::new(&config_path))
        .with_context(|| format!("failed to load gateway configuration from `{config_path}`"))?;

    observability::init_tracing(&config.server)?;

    run_migrations(&config.database.path)
        .await
        .context("failed to run database migrations")?;
    let store = Arc::new(
        LibsqlStore::new_local(&config.database.path)
            .await
            .context("failed to initialize local libsql store")?,
    );
    ensure_bootstrap_admin(&store, &config.auth.bootstrap_admin)
        .await
        .context("failed to ensure bootstrap admin access")?;

    let providers_seed = config.seed_providers()?;
    let models_seed = config.seed_models()?;
    let api_keys_seed = config.seed_api_keys()?;

    store
        .seed_from_inputs(&providers_seed, &models_seed, &api_keys_seed)
        .await
        .context("failed to seed foundational config data")?;

    let planner = Arc::new(WeightedRoutePlanner::default());
    let service = Arc::new(GatewayService::new(store, planner));
    if let Err(error) = service.refresh_pricing_catalog_if_stale().await {
        tracing::warn!(error = %error, "initial pricing catalog refresh failed");
    }
    spawn_pricing_catalog_refresh_loop(service.clone());
    let providers = build_provider_registry(&config)?;

    let bind_address: SocketAddr = config
        .server
        .bind
        .parse()
        .with_context(|| format!("invalid bind address `{}`", config.server.bind))?;

    let app = build_router(
        AppState {
            service: service.clone(),
            store: service.store().clone(),
            providers,
            identity_token_secret: Arc::new(load_identity_token_secret()),
        },
        load_admin_ui_config(),
    );

    let listener = TcpListener::bind(bind_address)
        .await
        .with_context(|| format!("failed binding gateway listener at `{bind_address}`"))?;

    tracing::info!(address = %bind_address, "gateway started");

    axum::serve(listener, app)
        .await
        .context("gateway server stopped unexpectedly")?;

    Ok(())
}

async fn ensure_bootstrap_admin(
    store: &Arc<LibsqlStore>,
    config: &BootstrapAdminConfig,
) -> anyhow::Result<()> {
    if !config.enabled {
        return Ok(());
    }

    if store
        .has_platform_admin()
        .await
        .context("failed checking for existing platform admins")?
    {
        return Ok(());
    }

    let user = store
        .upsert_bootstrap_admin_user("Admin", &config.email, config.require_password_change)
        .await
        .context("failed upserting bootstrap admin user")?;
    let password = config
        .resolved_password()
        .context("failed resolving bootstrap admin password")?;
    let password_hash =
        hash_gateway_key_secret(&password).context("failed hashing bootstrap admin password")?;
    store
        .store_user_password(user.user_id, &password_hash, time::OffsetDateTime::now_utc())
        .await
        .context("failed storing bootstrap admin password")?;

    Ok(())
}

fn build_provider_registry(config: &GatewayConfig) -> anyhow::Result<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();

    for provider_config in config.openai_compat_provider_configs()? {
        let provider = OpenAiCompatProvider::new(provider_config)
            .map_err(|error| anyhow::anyhow!("failed building openai_compat provider: {error}"))?;
        providers.register(Arc::new(provider));
    }

    for provider_config in config.vertex_provider_configs()? {
        let provider = VertexProvider::new(provider_config)
            .map_err(|error| anyhow::anyhow!("failed building gcp_vertex provider: {error}"))?;
        providers.register(Arc::new(provider));
    }

    Ok(providers)
}

fn load_admin_ui_config() -> AdminUiConfig {
    AdminUiConfig {
        base_path: env::var("ADMIN_UI_BASE_PATH").unwrap_or_else(|_| "/admin".to_string()),
        upstream: env::var("ADMIN_UI_UPSTREAM")
            .unwrap_or_else(|_| "http://localhost:3001".to_string()),
        connect_timeout_ms: env_u64("ADMIN_UI_CONNECT_TIMEOUT_MS", 750),
        request_timeout_ms: env_u64("ADMIN_UI_REQUEST_TIMEOUT_MS", 10_000),
    }
}

fn spawn_pricing_catalog_refresh_loop(
    service: Arc<GatewayService<LibsqlStore, WeightedRoutePlanner>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(pricing_catalog_refresh_interval());
        interval.tick().await;

        loop {
            interval.tick().await;
            if let Err(error) = service.refresh_pricing_catalog_if_stale().await {
                tracing::warn!(error = %error, "background pricing catalog refresh failed");
            }
        }
    });
}

fn pricing_catalog_refresh_interval() -> Duration {
    DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn load_identity_token_secret() -> String {
    env::var("GATEWAY_IDENTITY_TOKEN_SECRET")
        .unwrap_or_else(|_| "local-dev-identity-secret".to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use admin_ui::AdminUiConfig;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::{Body, Bytes, to_bytes},
        http::{Request, StatusCode},
    };
    use gateway_core::ChatCompletionsRequest;
    use gateway_core::{
        EmbeddingsRequest, ProviderCapabilities, ProviderClient, ProviderError,
        ProviderRequestContext, ProviderStream, SeedApiKey, SeedModel,
        SeedModelRoute, SeedProvider, parse_gateway_api_key,
    };
    use gateway_service::{GatewayService, WeightedRoutePlanner, hash_gateway_key_secret};
    use gateway_store::{LibsqlStore, run_migrations};
    use serde_json::{Map, Value, json};
    use serial_test::serial;
    use tempfile::tempdir;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{
        config::BootstrapAdminConfig,
        ensure_bootstrap_admin,
        http::{build_router, state::AppState},
    };

    #[derive(Clone)]
    enum MockChatResult {
        Value(Value),
        Error(MockError),
    }

    #[derive(Clone)]
    enum MockError {
        UpstreamHttp(u16, String),
    }

    impl MockError {
        fn into_provider_error(self) -> ProviderError {
            match self {
                Self::UpstreamHttp(status, body) => ProviderError::UpstreamHttp { status, body },
            }
        }
    }

    #[derive(Clone)]
    struct MockProvider {
        key: String,
        provider_type: &'static str,
        caps: ProviderCapabilities,
        chat_result: MockChatResult,
        stream_chunks: Vec<String>,
        chat_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ProviderClient for MockProvider {
        fn provider_key(&self) -> &str {
            &self.key
        }

        fn provider_type(&self) -> &str {
            self.provider_type
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.caps
        }

        async fn chat_completions(
            &self,
            _request: &ChatCompletionsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            self.chat_calls.fetch_add(1, Ordering::SeqCst);
            match self.chat_result.clone() {
                MockChatResult::Value(value) => Ok(value),
                MockChatResult::Error(error) => Err(error.into_provider_error()),
            }
        }

        async fn chat_completions_stream(
            &self,
            _request: &ChatCompletionsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            let stream = futures_util::stream::iter(
                self.stream_chunks
                    .clone()
                    .into_iter()
                    .map(|chunk| Ok(Bytes::from(chunk))),
            );
            Ok(Box::pin(stream))
        }

        async fn embeddings(
            &self,
            _request: &EmbeddingsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::NotImplemented(
                "mock embeddings not implemented".to_string(),
            ))
        }
    }

    fn make_chat_provider(
        key: &str,
        chat_result: MockChatResult,
        stream_chunks: Vec<String>,
        caps: ProviderCapabilities,
    ) -> (Arc<AtomicUsize>, MockProvider) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            calls.clone(),
            MockProvider {
                key: key.to_string(),
                provider_type: "mock",
                caps,
                chat_result,
                stream_chunks,
                chat_calls: calls,
            },
        )
    }

    #[derive(Debug)]
    struct RequestLogRow {
        request_id: String,
        user_id: Option<String>,
        team_id: Option<String>,
        model_key: String,
        provider_key: String,
        upstream_model: String,
        status_code: Option<i64>,
        latency_ms: Option<i64>,
        stream: bool,
        fallback_used: bool,
        attempt_count: i64,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        total_tokens: Option<i64>,
        payload_available: bool,
        error_code: Option<String>,
    }

    #[derive(Debug)]
    struct RequestLogPayloadRow {
        request_id: String,
        request_json: Value,
        response_json: Value,
        request_truncated: bool,
        response_truncated: bool,
    }

    async fn load_request_logs(db_path: &Path) -> Vec<RequestLogRow> {
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        let mut rows = connection
            .query(
                r#"
                SELECT request_id, user_id, team_id, model_key, provider_key, upstream_model,
                       status_code, latency_ms, stream, fallback_used, attempt_count,
                       prompt_tokens, completion_tokens, total_tokens, payload_available,
                       error_code
                FROM request_logs
                ORDER BY occurred_at ASC, rowid ASC
                "#,
                (),
            )
            .await
            .expect("request logs query");

        let mut logs = Vec::new();
        while let Some(row) = rows.next().await.expect("request logs row") {
            let stream: i64 = row.get(8).expect("stream");
            let fallback_used: i64 = row.get(9).expect("fallback_used");
            let payload_available: i64 = row.get(14).expect("payload_available");
            logs.push(RequestLogRow {
                request_id: row.get(0).expect("request_id"),
                user_id: row.get(1).expect("user_id"),
                team_id: row.get(2).expect("team_id"),
                model_key: row.get(3).expect("model_key"),
                provider_key: row.get(4).expect("provider_key"),
                upstream_model: row.get(5).expect("upstream_model"),
                status_code: row.get(6).expect("status_code"),
                latency_ms: row.get(7).expect("latency_ms"),
                stream: stream == 1,
                fallback_used: fallback_used == 1,
                attempt_count: row.get(10).expect("attempt_count"),
                prompt_tokens: row.get(11).expect("prompt_tokens"),
                completion_tokens: row.get(12).expect("completion_tokens"),
                total_tokens: row.get(13).expect("total_tokens"),
                payload_available: payload_available == 1,
                error_code: row.get(15).expect("error_code"),
            });
        }

        logs
    }

    async fn load_request_log_payloads(db_path: &Path) -> Vec<RequestLogPayloadRow> {
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        let mut rows = connection
            .query(
                r#"
                SELECT logs.request_id, payloads.request_json, payloads.response_json,
                       payloads.request_truncated, payloads.response_truncated
                FROM request_log_payloads AS payloads
                INNER JOIN request_logs AS logs
                  ON logs.request_log_id = payloads.request_log_id
                ORDER BY payloads.occurred_at ASC
                "#,
                (),
            )
            .await
            .expect("request log payloads query");

        let mut payloads = Vec::new();
        while let Some(row) = rows.next().await.expect("payload row") {
            let request_json: String = row.get(1).expect("request_json");
            let response_json: String = row.get(2).expect("response_json");
            let request_truncated: i64 = row.get(3).expect("request_truncated");
            let response_truncated: i64 = row.get(4).expect("response_truncated");
            payloads.push(RequestLogPayloadRow {
                request_id: row.get(0).expect("request_id"),
                request_json: serde_json::from_str(&request_json).expect("request json"),
                response_json: serde_json::from_str(&response_json).expect("response json"),
                request_truncated: request_truncated == 1,
                response_truncated: response_truncated == 1,
            });
        }

        payloads
    }

    async fn set_api_key_owner_to_user(
        db_path: &Path,
        raw_key: &str,
        request_logging_enabled: bool,
    ) -> Uuid {
        let parsed = parse_gateway_api_key(raw_key).expect("parse key");
        let user_id = Uuid::new_v4();
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");

        connection
            .execute(
                r#"
                INSERT INTO users (
                    user_id, name, email, email_normalized, global_role, auth_mode, status,
                    request_logging_enabled, model_access_mode, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'user', 'password', 'active', ?5, 'all', unixepoch(), unixepoch())
                "#,
                libsql::params![
                    user_id.to_string(),
                    "Request Logging Test User",
                    format!("{}@example.com", user_id.simple()),
                    format!("{}@example.com", user_id.simple()),
                    if request_logging_enabled { 1_i64 } else { 0_i64 },
                ],
            )
            .await
            .expect("insert user");

        connection
            .execute(
                r#"
                UPDATE api_keys
                SET owner_kind = 'user',
                    owner_user_id = ?1,
                    owner_team_id = NULL
                WHERE public_id = ?2
                "#,
                libsql::params![user_id.to_string(), parsed.public_id],
            )
            .await
            .expect("update api key owner");

        user_id
    }

    async fn build_test_app(
        seed_providers: Vec<SeedProvider>,
        models: Vec<SeedModel>,
        provider_registry: gateway_core::ProviderRegistry,
    ) -> (Router, String, PathBuf) {
        let tmp = tempdir().expect("tempdir");
        let tmp_path = tmp.keep();
        let db_path = tmp_path.join("gateway.db");

        run_migrations(&db_path).await.expect("migrations");

        let store = Arc::new(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        );

        let raw_key = "gwk_dev123.super-secret".to_string();
        let parsed = parse_gateway_api_key(&raw_key).expect("parse key");
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: parsed.public_id,
            secret_hash: hash_gateway_key_secret(&parsed.secret).expect("hash"),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&seed_providers, &models, &api_keys)
            .await
            .expect("seed data");

        let service = Arc::new(GatewayService::new(
            store.clone(),
            Arc::new(WeightedRoutePlanner::seeded(11)),
        ));

        let app = build_router(
            AppState {
                service,
                store,
                providers: provider_registry,
                identity_token_secret: Arc::new("local-dev-identity-secret".to_string()),
            },
            AdminUiConfig::default(),
        );

        (app, raw_key, db_path)
    }

    async fn build_default_test_app(
        providers: gateway_core::ProviderRegistry,
    ) -> (Router, String, PathBuf) {
        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({"base_url": "https://api.openai.com/v1"}),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: Some("Fast tier".to_string()),
            tags: vec!["fast".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::<String, Value>::new(),
                extra_body: Map::<String, Value>::new(),
            }],
        }];

        build_test_app(seed_providers, models, providers).await
    }

    async fn build_default_test_app_with_store(
        providers: gateway_core::ProviderRegistry,
    ) -> (Router, Arc<LibsqlStore>, PathBuf) {
        let tmp = tempdir().expect("tempdir");
        let tmp_path = tmp.keep();
        let db_path = tmp_path.join("gateway.db");

        run_migrations(&db_path).await.expect("migrations");

        let store = Arc::new(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        );

        let service = Arc::new(GatewayService::new(
            store.clone(),
            Arc::new(WeightedRoutePlanner::seeded(11)),
        ));

        let app = build_router(
            AppState {
                service,
                store: store.clone(),
                providers,
                identity_token_secret: Arc::new("local-dev-identity-secret".to_string()),
            },
            AdminUiConfig::default(),
        );

        (app, store, db_path)
    }

    fn set_cookie_header(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("set-cookie")
            .expect("set-cookie header")
            .to_str()
            .expect("set-cookie value")
            .to_string()
    }

    async fn read_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        serde_json::from_slice(&body).expect("json body")
    }

    #[tokio::test]
    #[serial]
    async fn api_routes_are_not_swallowed_by_ui_proxy() {
        let (app, _, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn readyz_returns_ok() {
        let (app, _, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn v1_models_are_auth_filtered() {
        let (app, raw_key, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["object"], "list");
        assert_eq!(json["data"][0]["id"], "fast");
    }

    #[tokio::test]
    #[serial]
    async fn chat_completions_executes_resolved_provider() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason":"stop"}],
                "usage": {"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18}
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        serde_json::json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["choices"][0]["message"]["content"], "pong");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert!(logs[0].user_id.is_none());
        assert!(logs[0].team_id.is_some());
        assert_eq!(logs[0].model_key, "fast");
        assert_eq!(logs[0].provider_key, "openai-prod");
        assert_eq!(logs[0].upstream_model, "gpt-4o-mini");
        assert_eq!(logs[0].status_code, Some(200));
        assert!(logs[0].latency_ms.is_some());
        assert!(!logs[0].stream);
        assert!(!logs[0].fallback_used);
        assert_eq!(logs[0].attempt_count, 1);
        assert_eq!(logs[0].prompt_tokens, Some(11));
        assert_eq!(logs[0].completion_tokens, Some(7));
        assert_eq!(logs[0].total_tokens, Some(18));
        assert!(logs[0].payload_available);
        assert_eq!(logs[0].error_code, None);

        let payloads = load_request_log_payloads(&db_path).await;
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].request_json["body"]["model"], "fast");
        assert_eq!(payloads[0].response_json["body"]["usage"]["total_tokens"], 18);
    }

    #[tokio::test]
    #[serial]
    async fn fallback_retries_when_idempotency_key_is_present() {
        let (primary_calls, primary_provider) = make_chat_provider(
            "primary",
            MockChatResult::Error(MockError::UpstreamHttp(503, "unavailable".to_string())),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let (fallback_calls, fallback_provider) = make_chat_provider(
            "fallback",
            MockChatResult::Value(json!({
                "id": "chatcmpl_fallback",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from-fallback"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );

        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(primary_provider));
        registry.register(Arc::new(fallback_provider));

        let seed_providers = vec![
            SeedProvider {
                provider_key: "primary".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
            SeedProvider {
                provider_key: "fallback".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
        ];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![
                SeedModelRoute {
                    provider_key: "primary".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
                SeedModelRoute {
                    provider_key: "fallback".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
            ],
        }];

        let (app, raw_key, db_path) = build_test_app(seed_providers, models, registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("idempotency-key", "idem-123")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload["choices"][0]["message"]["content"], "from-fallback");
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].provider_key, "fallback");
        assert_eq!(logs[0].status_code, Some(200));
        assert_eq!(logs[0].error_code, None);
        assert!(!logs[0].stream);
        assert!(logs[0].fallback_used);
        assert_eq!(logs[0].attempt_count, 2);
    }

    #[tokio::test]
    #[serial]
    async fn no_retry_without_idempotency_key() {
        let (primary_calls, primary_provider) = make_chat_provider(
            "primary",
            MockChatResult::Error(MockError::UpstreamHttp(503, "unavailable".to_string())),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let (fallback_calls, fallback_provider) = make_chat_provider(
            "fallback",
            MockChatResult::Value(json!({
                "id": "chatcmpl_fallback",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from-fallback"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );

        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(primary_provider));
        registry.register(Arc::new(fallback_provider));

        let seed_providers = vec![
            SeedProvider {
                provider_key: "primary".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
            SeedProvider {
                provider_key: "fallback".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
        ];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![
                SeedModelRoute {
                    provider_key: "primary".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
                SeedModelRoute {
                    provider_key: "fallback".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
            ],
        }];

        let (app, raw_key, db_path) = build_test_app(seed_providers, models, registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 0);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].provider_key, "primary");
        assert_eq!(logs[0].status_code, Some(503));
        assert_eq!(logs[0].prompt_tokens, None);
        assert_eq!(logs[0].completion_tokens, None);
        assert_eq!(logs[0].total_tokens, None);
        assert_eq!(logs[0].error_code.as_deref(), Some("upstream_http_error"));
        assert!(!logs[0].stream);
        assert!(!logs[0].fallback_used);
        assert_eq!(logs[0].attempt_count, 1);
    }

    #[tokio::test]
    #[serial]
    async fn user_owned_key_respects_request_logging_toggle() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;
        let user_id = set_api_key_owner_to_user(&db_path, &raw_key, false).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        serde_json::json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(user_id.to_string().len(), 36);
        assert!(load_request_logs(&db_path).await.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn streaming_response_emits_done_terminator() {
        let (_, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "unused"}, "finish_reason":"stop"}]
            })),
            vec![
                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n".to_string(),
                "data: [DONE]\n\n".to_string(),
            ],
            ProviderCapabilities::new(true, true, true),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "stream": true,
                            "messages": [{"role":"user","content":"ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("data: [DONE]"));

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].provider_key, "openai-prod");
        assert_eq!(logs[0].status_code, Some(200));
        assert_eq!(logs[0].prompt_tokens, None);
        assert_eq!(logs[0].completion_tokens, None);
        assert_eq!(logs[0].total_tokens, None);
        assert_eq!(logs[0].error_code, None);
        assert!(logs[0].stream);
        assert!(!logs[0].fallback_used);
        assert_eq!(logs[0].attempt_count, 1);

        let payloads = load_request_log_payloads(&db_path).await;
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].response_json["body"]["kind"], "sse_transcript");
    }

    #[tokio::test]
    #[serial]
    async fn generates_request_id_and_redacts_binary_payloads() {
        let (_, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;
        let large_image = format!("data:image/png;base64,{}", "a".repeat(9000));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{
                                "role": "user",
                                "content": [{"type": "image_url", "image_url": {"url": large_image}}],
                                "api_key": "secret-inline"
                            }]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .expect("request id header");
        assert!(!request_id.is_empty());

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].request_id, request_id);

        let payloads = load_request_log_payloads(&db_path).await;
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].request_id, request_id);
        assert_eq!(
            payloads[0].request_json["body"]["messages"][0]["api_key"],
            "[REDACTED]"
        );
        assert_eq!(
            payloads[0].request_json["body"]["messages"][0]["content"][0]["image_url"]["url"]["kind"],
            "omitted_string"
        );
        assert!(!payloads[0].request_truncated);
        assert!(!payloads[0].response_truncated);
    }

    #[tokio::test]
    #[serial]
    async fn admin_identity_routes_require_authenticated_session() {
        let (app, _, _) = build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/identity/users")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_admin_can_log_in_and_load_session() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        ensure_bootstrap_admin(
            &store,
            &BootstrapAdminConfig {
                enabled: true,
                email: "admin@local".to_string(),
                password: "literal.admin".to_string(),
                require_password_change: false,
            },
        )
        .await
        .expect("bootstrap admin");

        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login/password")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "admin@local",
                            "password": "admin"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(login_response.status(), StatusCode::OK);
        let session_cookie = set_cookie_header(&login_response);
        let login_json = read_json(login_response).await;
        assert_eq!(login_json["data"]["user"]["email"], "admin@local");
        assert_eq!(login_json["data"]["user"]["global_role"], "platform_admin");
        assert_eq!(login_json["data"]["must_change_password"], Value::Bool(false));

        let session_response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/auth/session")
                    .header("cookie", session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(session_response.status(), StatusCode::OK);
        let session_json = read_json(session_response).await;
        assert_eq!(session_json["data"]["user"]["email"], "admin@local");
        assert_eq!(session_json["data"]["must_change_password"], Value::Bool(false));
    }

    #[tokio::test]
    #[serial]
    async fn forced_password_change_can_be_completed_and_old_password_stops_working() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        ensure_bootstrap_admin(
            &store,
            &BootstrapAdminConfig {
                enabled: true,
                email: "admin@local".to_string(),
                password: "literal.admin".to_string(),
                require_password_change: true,
            },
        )
        .await
        .expect("bootstrap admin");

        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login/password")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "admin@local",
                            "password": "admin"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(login_response.status(), StatusCode::OK);
        let session_cookie = set_cookie_header(&login_response);
        let login_json = read_json(login_response).await;
        assert_eq!(login_json["data"]["must_change_password"], Value::Bool(true));

        let change_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/password/change")
                    .header("content-type", "application/json")
                    .header("cookie", session_cookie)
                    .body(Body::from(
                        json!({
                            "current_password": "admin",
                            "new_password": "s3cur3-passw0rd"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(change_response.status(), StatusCode::OK);
        let change_json = read_json(change_response).await;
        assert_eq!(change_json["data"]["must_change_password"], Value::Bool(false));

        let old_login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login/password")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "admin@local",
                            "password": "admin"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(old_login_response.status(), StatusCode::UNAUTHORIZED);

        let new_login_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login/password")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "admin@local",
                            "password": "s3cur3-passw0rd"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(new_login_response.status(), StatusCode::OK);

        let refreshed_user = store
            .get_user_by_email_normalized("admin@local")
            .await
            .expect("reload bootstrap admin")
            .expect("bootstrap admin should exist");
        assert!(!refreshed_user.must_change_password);
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_admin_is_not_reseeded_after_initial_creation() {
        let (_, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let initial_config = BootstrapAdminConfig {
            enabled: true,
            email: "admin@local".to_string(),
            password: "literal.admin".to_string(),
            require_password_change: false,
        };
        ensure_bootstrap_admin(&store, &initial_config)
            .await
            .expect("initial bootstrap admin");
        let initial_password_hash = store
            .get_user_password_auth(
                store
                    .get_user_by_email_normalized("admin@local")
                    .await
                    .expect("load bootstrap admin")
                    .expect("bootstrap admin should exist")
                    .user_id,
            )
            .await
            .expect("load bootstrap password auth")
            .expect("bootstrap password auth")
            .password_hash;

        ensure_bootstrap_admin(
            &store,
            &BootstrapAdminConfig {
                enabled: true,
                email: "admin@local".to_string(),
                password: "literal.changed".to_string(),
                require_password_change: true,
            },
        )
        .await
        .expect("second bootstrap pass");

        let bootstrap_admin = store
            .get_user_by_email_normalized("admin@local")
            .await
            .expect("reload bootstrap admin")
            .expect("bootstrap admin should exist");
        let password_hash = store
            .get_user_password_auth(bootstrap_admin.user_id)
            .await
            .expect("reload password auth")
            .expect("password auth should exist")
            .password_hash;

        assert_eq!(password_hash, initial_password_hash);
        assert!(!bootstrap_admin.must_change_password);
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_admin_is_not_created_when_another_platform_admin_exists() {
        let (_, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        store
            .create_identity_user(
                "Existing Admin",
                "owner@example.com",
                "owner@example.com",
                gateway_core::GlobalRole::PlatformAdmin,
                gateway_core::AuthMode::Password,
                "active",
            )
            .await
            .expect("existing platform admin");

        ensure_bootstrap_admin(
            &store,
            &BootstrapAdminConfig {
                enabled: true,
                email: "admin@local".to_string(),
                password: "literal.admin".to_string(),
                require_password_change: true,
            },
        )
        .await
        .expect("bootstrap should no-op");

        let bootstrap_admin = store
            .get_user_by_email_normalized("admin@local")
            .await
            .expect("lookup bootstrap admin");
        assert!(bootstrap_admin.is_none());
    }
}
