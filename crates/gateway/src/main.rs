mod cli;
mod config;
mod http;
mod observability;

use std::{env, net::SocketAddr, path::Path, sync::Arc, time::Duration};

use crate::{
    cli::{Cli, Command, MigrateAction, ServeArgs},
    config::{BootstrapAdminConfig, GatewayConfig},
};
use admin_ui::AdminUiConfig;
use anyhow::Context;
use clap::Parser;
use gateway_core::ProviderRegistry;
use gateway_providers::{OpenAiCompatProvider, VertexProvider};
use gateway_service::{
    DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL, GatewayService, WeightedRoutePlanner,
    hash_gateway_key_secret,
};
use gateway_store::{
    AnyStore, GatewayStore, MigrationStatus, MigrationTestHook, check_migrations_with_options,
    run_migrations_with_options, status_migrations_with_options,
};
use http::{build_router, state::AppState};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;

    observability::init_tracing(&config.server)?;

    match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => run_serve(&config, args).await,
        Command::Migrate(args) => run_migrate(&config, args.action()?).await,
        Command::BootstrapAdmin => run_bootstrap_admin_command(&config).await,
        Command::SeedConfig => run_seed_config_command(&config).await,
    }
}

fn load_config(config_path: &str) -> anyhow::Result<GatewayConfig> {
    GatewayConfig::from_path(Path::new(config_path))
        .with_context(|| format!("failed to load gateway configuration from `{config_path}`"))
}

fn database_options(
    config: &GatewayConfig,
) -> anyhow::Result<gateway_store::StoreConnectionOptions> {
    config
        .database_options()
        .context("failed resolving database configuration")
}

async fn maybe_run_migrations(
    database_options: &gateway_store::StoreConnectionOptions,
    enabled: bool,
) -> anyhow::Result<()> {
    if !enabled {
        return Ok(());
    }

    run_migrations_with_options(database_options, MigrationTestHook::default())
        .await
        .context("failed to run database migrations")
}

async fn seed_config<S>(store: &S, config: &GatewayConfig) -> anyhow::Result<()>
where
    S: GatewayStore + ?Sized,
{
    let providers_seed = config.seed_providers()?;
    let models_seed = config.seed_models()?;
    let api_keys_seed = config.seed_api_keys()?;

    store
        .seed_from_inputs(&providers_seed, &models_seed, &api_keys_seed)
        .await
        .context("failed to seed foundational config data")
}

async fn run_serve(config: &GatewayConfig, args: ServeArgs) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    maybe_run_migrations(&database_options, args.run_migrations).await?;
    let store = Arc::new(
        AnyStore::connect(&database_options)
            .await
            .context("failed to initialize gateway store")?,
    );
    run_serve_with_store(config, store, args).await
}

async fn run_serve_with_store(
    config: &GatewayConfig,
    store: Arc<AnyStore>,
    args: ServeArgs,
) -> anyhow::Result<()> {
    if args.bootstrap_admin {
        ensure_bootstrap_admin(&store, &config.auth.bootstrap_admin)
            .await
            .context("failed to ensure bootstrap admin access")?;
    }

    if args.seed_config {
        seed_config(store.as_ref(), config).await?;
    }

    let planner = Arc::new(WeightedRoutePlanner::default());
    let service = Arc::new(GatewayService::new(store, planner));
    if let Err(error) = service.refresh_pricing_catalog_if_stale().await {
        tracing::warn!(error = %error, "initial pricing catalog refresh failed");
    }
    spawn_pricing_catalog_refresh_loop(service.clone());
    let providers = build_provider_registry(config)?;

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

async fn run_migrate(config: &GatewayConfig, action: MigrateAction) -> anyhow::Result<()> {
    let database_options = database_options(config)?;

    match action {
        MigrateAction::Apply => {
            run_migrations_with_options(&database_options, MigrationTestHook::default())
                .await
                .context("failed to apply database migrations")?;
            let status = status_migrations_with_options(&database_options).await?;
            print_migration_status(&status);
            Ok(())
        }
        MigrateAction::Check => {
            let status = check_migrations_with_options(&database_options)
                .await
                .context("database migration check failed")?;
            print_migration_status(&status);
            Ok(())
        }
        MigrateAction::Status => {
            let status = status_migrations_with_options(&database_options).await?;
            print_migration_status(&status);
            Ok(())
        }
    }
}

async fn run_bootstrap_admin_command(config: &GatewayConfig) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    maybe_run_migrations(&database_options, true).await?;
    let store = Arc::new(
        AnyStore::connect(&database_options)
            .await
            .context("failed to initialize gateway store")?,
    );
    ensure_bootstrap_admin(&store, &config.auth.bootstrap_admin).await
}

async fn run_seed_config_command(config: &GatewayConfig) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    maybe_run_migrations(&database_options, true).await?;
    let store = Arc::new(
        AnyStore::connect(&database_options)
            .await
            .context("failed to initialize gateway store")?,
    );
    seed_config(store.as_ref(), config).await
}

fn print_migration_status(status: &MigrationStatus) {
    println!("backend: {}", status.backend);
    for entry in &status.entries {
        let state = if entry.applied { "applied" } else { "pending" };
        match entry.backend_note {
            Some(note) => println!("v{} {} [{}] ({})", entry.version, entry.name, state, note),
            None => println!("v{} {} [{}]", entry.version, entry.name, state),
        }
    }
}

async fn ensure_bootstrap_admin(
    store: &Arc<AnyStore>,
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
        .store_user_password(
            user.user_id,
            &password_hash,
            time::OffsetDateTime::now_utc(),
        )
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
    service: Arc<GatewayService<AnyStore, WeightedRoutePlanner>>,
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
        env,
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
        ProviderRequestContext, ProviderStream, SeedApiKey, SeedModel, SeedModelRoute,
        SeedProvider, parse_gateway_api_key,
    };
    use gateway_service::{GatewayService, WeightedRoutePlanner, hash_gateway_key_secret};
    use gateway_store::{
        AnyStore, GatewayStore, LibsqlStore, MigrationTestHook, StoreConnectionOptions,
        run_migrations, run_migrations_with_options,
    };
    use serde_json::{Map, Value, json};
    use serial_test::serial;
    use sqlx::Row;
    use tempfile::tempdir;
    use tower::ServiceExt;
    use url::Url;
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
        user_id: Option<String>,
        team_id: Option<String>,
        model_key: String,
        provider_key: String,
        status_code: Option<i64>,
        latency_ms: Option<i64>,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        total_tokens: Option<i64>,
        error_code: Option<String>,
        metadata: Value,
    }

    #[derive(Debug)]
    struct UsageLedgerRow {
        request_id: String,
        ownership_scope_key: String,
        provider_key: String,
        upstream_model: String,
        pricing_status: String,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        total_tokens: Option<i64>,
        pricing_provider_id: Option<String>,
        pricing_model_id: Option<String>,
        computed_cost_10000: i64,
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
                SELECT user_id, team_id, model_key, provider_key, status_code, latency_ms,
                       prompt_tokens, completion_tokens, total_tokens, error_code, metadata_json
                FROM request_logs
                ORDER BY occurred_at ASC, rowid ASC
                "#,
                (),
            )
            .await
            .expect("request logs query");

        let mut logs = Vec::new();
        while let Some(row) = rows.next().await.expect("request logs row") {
            let metadata_json: String = row.get(10).expect("metadata json");
            logs.push(RequestLogRow {
                user_id: row.get(0).expect("user_id"),
                team_id: row.get(1).expect("team_id"),
                model_key: row.get(2).expect("model_key"),
                provider_key: row.get(3).expect("provider_key"),
                status_code: row.get(4).expect("status_code"),
                latency_ms: row.get(5).expect("latency_ms"),
                prompt_tokens: row.get(6).expect("prompt_tokens"),
                completion_tokens: row.get(7).expect("completion_tokens"),
                total_tokens: row.get(8).expect("total_tokens"),
                error_code: row.get(9).expect("error_code"),
                metadata: serde_json::from_str(&metadata_json).expect("metadata value"),
            });
        }

        logs
    }

    async fn load_usage_ledger(db_path: &Path) -> Vec<UsageLedgerRow> {
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        let mut rows = connection
            .query(
                r#"
                SELECT request_id, ownership_scope_key, provider_key, upstream_model,
                       pricing_status, prompt_tokens, completion_tokens, total_tokens,
                       pricing_provider_id, pricing_model_id, computed_cost_10000
                FROM usage_cost_events
                ORDER BY occurred_at ASC, rowid ASC
                "#,
                (),
            )
            .await
            .expect("usage ledger query");

        let mut ledgers = Vec::new();
        while let Some(row) = rows.next().await.expect("usage ledger row") {
            ledgers.push(UsageLedgerRow {
                request_id: row.get(0).expect("request_id"),
                ownership_scope_key: row.get(1).expect("ownership_scope_key"),
                provider_key: row.get(2).expect("provider_key"),
                upstream_model: row.get(3).expect("upstream_model"),
                pricing_status: row.get(4).expect("pricing_status"),
                prompt_tokens: row.get(5).expect("prompt_tokens"),
                completion_tokens: row.get(6).expect("completion_tokens"),
                total_tokens: row.get(7).expect("total_tokens"),
                pricing_provider_id: row.get(8).expect("pricing_provider_id"),
                pricing_model_id: row.get(9).expect("pricing_model_id"),
                computed_cost_10000: row.get(10).expect("computed_cost_10000"),
            });
        }

        ledgers
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

        let store = Arc::new(AnyStore::Libsql(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        ));

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
            config: serde_json::json!({
                "base_url": "https://api.openai.com/v1",
                "pricing_provider_id": "openai"
            }),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: Some("Fast tier".to_string()),
            tags: vec!["fast".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-5".to_string(),
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
    ) -> (Router, Arc<AnyStore>, PathBuf) {
        let tmp = tempdir().expect("tempdir");
        let tmp_path = tmp.keep();
        let db_path = tmp_path.join("gateway.db");

        run_migrations(&db_path).await.expect("migrations");

        let store = Arc::new(AnyStore::Libsql(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        ));

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

    async fn build_postgres_test_app(
        database_url: &str,
        provider_registry: gateway_core::ProviderRegistry,
    ) -> (Router, String) {
        let options = StoreConnectionOptions::Postgres {
            url: database_url.to_string(),
            max_connections: 4,
        };
        run_migrations_with_options(&options, MigrationTestHook::default())
            .await
            .expect("postgres migrations");

        let store = Arc::new(AnyStore::connect(&options).await.expect("postgres store"));

        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({
                "base_url": "https://api.openai.com/v1",
                "pricing_provider_id": "openai"
            }),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: Some("Fast tier".to_string()),
            tags: vec!["fast".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-5".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::<String, Value>::new(),
                extra_body: Map::<String, Value>::new(),
            }],
        }];

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
            .expect("seed");

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

        (app, raw_key)
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

    struct PostgresTestDatabase {
        admin_url: String,
        database_url: String,
        database_name: String,
    }

    async fn create_postgres_test_database() -> Option<PostgresTestDatabase> {
        let base_url = env::var("TEST_POSTGRES_URL").ok()?;
        let mut admin_url = Url::parse(&base_url).expect("valid postgres url");
        admin_url.set_path("/postgres");

        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(admin_url.as_str())
            .await
            .expect("admin postgres pool");

        let database_name = format!("gateway_test_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {database_name}"))
            .execute(&admin_pool)
            .await
            .expect("create test database");
        admin_pool.close().await;

        let mut database_url = Url::parse(&base_url).expect("valid postgres url");
        database_url.set_path(&format!("/{database_name}"));

        Some(PostgresTestDatabase {
            admin_url: admin_url.to_string(),
            database_url: database_url.to_string(),
            database_name,
        })
    }

    async fn drop_postgres_test_database(database: &PostgresTestDatabase) {
        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database.admin_url)
            .await
            .expect("admin postgres pool");

        sqlx::query(
            r#"
            SELECT pg_terminate_backend(pid)
            FROM pg_stat_activity
            WHERE datname = $1
              AND pid <> pg_backend_pid()
            "#,
        )
        .bind(database.database_name.as_str())
        .execute(&admin_pool)
        .await
        .expect("terminate sessions");

        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {}",
            database.database_name
        ))
        .execute(&admin_pool)
        .await
        .expect("drop test database");
        admin_pool.close().await;
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

        let request_id = response
            .headers()
            .get("x-request-id")
            .expect("x-request-id header")
            .to_str()
            .expect("request id value")
            .to_string();
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
        assert_eq!(logs[0].status_code, Some(200));
        assert!(logs[0].latency_ms.is_some());
        assert_eq!(logs[0].prompt_tokens, Some(11));
        assert_eq!(logs[0].completion_tokens, Some(7));
        assert_eq!(logs[0].total_tokens, Some(18));
        assert_eq!(logs[0].error_code, None);
        assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
        assert_eq!(logs[0].metadata["fallback_used"], Value::Bool(false));
        assert_eq!(logs[0].metadata["attempt_count"], json!(1));

        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].request_id, request_id);
        assert_eq!(ledgers[0].provider_key, "openai-prod");
        assert_eq!(ledgers[0].upstream_model, "gpt-5");
        assert_eq!(ledgers[0].pricing_status, "priced");
        assert_eq!(ledgers[0].prompt_tokens, Some(11));
        assert_eq!(ledgers[0].completion_tokens, Some(7));
        assert_eq!(ledgers[0].total_tokens, Some(18));
        assert_eq!(ledgers[0].pricing_provider_id.as_deref(), Some("openai"));
        assert_eq!(ledgers[0].pricing_model_id.as_deref(), Some("gpt-5"));
        assert!(ledgers[0].computed_cost_10000 >= 0);
    }

    #[tokio::test]
    #[serial]
    async fn repeated_request_id_is_accounted_once_per_scope() {
        let (_, provider) = make_chat_provider(
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

        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/chat/completions")
                        .header("content-type", "application/json")
                        .header("authorization", format!("Bearer {raw_key}"))
                        .header("x-request-id", "req-dedupe")
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
        }

        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].request_id, "req-dedupe");
        assert!(ledgers[0].ownership_scope_key.starts_with("team:"));
        assert_eq!(ledgers[0].pricing_status, "priced");
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
                config: serde_json::json!({
                    "base_url":"https://example.invalid/v1",
                    "pricing_provider_id":"openai"
                }),
                secrets: None,
            },
            SeedProvider {
                provider_key: "fallback".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({
                    "base_url":"https://example.invalid/v1",
                    "pricing_provider_id":"openai"
                }),
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
        assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
        assert_eq!(logs[0].metadata["fallback_used"], Value::Bool(true));
        assert_eq!(logs[0].metadata["attempt_count"], json!(2));
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
                config: serde_json::json!({
                    "base_url":"https://example.invalid/v1",
                    "pricing_provider_id":"openai"
                }),
                secrets: None,
            },
            SeedProvider {
                provider_key: "fallback".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({
                    "base_url":"https://example.invalid/v1",
                    "pricing_provider_id":"openai"
                }),
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
        assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
        assert_eq!(logs[0].metadata["fallback_used"], Value::Bool(false));
        assert_eq!(logs[0].metadata["attempt_count"], json!(1));
        assert!(load_usage_ledger(&db_path).await.is_empty());
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

        let request_id = response
            .headers()
            .get("x-request-id")
            .expect("x-request-id header")
            .to_str()
            .expect("request id value")
            .to_string();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(user_id.to_string().len(), 36);
        assert!(load_request_logs(&db_path).await.is_empty());
        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].pricing_status, "usage_missing");
        assert_eq!(ledgers[0].request_id, request_id);
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

        let request_id = response
            .headers()
            .get("x-request-id")
            .expect("x-request-id header")
            .to_str()
            .expect("request id value")
            .to_string();
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
        assert_eq!(logs[0].metadata["stream"], Value::Bool(true));
        assert_eq!(logs[0].metadata["fallback_used"], Value::Bool(false));
        assert_eq!(logs[0].metadata["attempt_count"], json!(1));

        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].request_id, request_id);
        assert_eq!(ledgers[0].pricing_status, "usage_missing");
        assert_eq!(ledgers[0].computed_cost_10000, 0);
    }

    #[tokio::test]
    #[serial]
    async fn admin_identity_routes_require_authenticated_session() {
        let (app, _, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;

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
        assert_eq!(
            login_json["data"]["must_change_password"],
            Value::Bool(false)
        );

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
        assert_eq!(
            session_json["data"]["must_change_password"],
            Value::Bool(false)
        );
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
        assert_eq!(
            login_json["data"]["must_change_password"],
            Value::Bool(true)
        );

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
        assert_eq!(
            change_json["data"]["must_change_password"],
            Value::Bool(false)
        );

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

    #[tokio::test]
    #[serial]
    async fn postgres_runtime_serves_and_logs_requests() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres gateway smoke test because TEST_POSTGRES_URL is not set");
            return;
        };

        let mut providers = gateway_core::ProviderRegistry::new();
        let (_, mock_provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4o-mini",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hello"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 2,
                    "total_tokens": 5
                }
            })),
            vec![],
            ProviderCapabilities {
                chat_completions: true,
                chat_completions_stream: false,
                embeddings: false,
            },
        );
        providers.register(Arc::new(mock_provider));

        let (app, raw_key) = build_postgres_test_app(&test_db.database_url, providers).await;

        let ready = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("ready response");
        assert_eq!(ready.status(), StatusCode::OK);

        let models = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("models response");
        assert_eq!(models.status(), StatusCode::OK);

        let chat = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("content-type", "application/json")
                    .header("x-request-id", "req-postgres-ledger")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "hello"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("chat response");
        assert_eq!(chat.status(), StatusCode::OK);
        let request_id = chat
            .headers()
            .get("x-request-id")
            .expect("x-request-id header")
            .to_str()
            .expect("request id value");
        assert_eq!(request_id, "req-postgres-ledger");

        let replay = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("content-type", "application/json")
                    .header("x-request-id", "req-postgres-ledger")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "hello"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("replay response");
        assert_eq!(replay.status(), StatusCode::OK);

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let row = sqlx::query("SELECT COUNT(*) FROM request_logs")
            .fetch_one(&pool)
            .await
            .expect("request log count");
        let count: i64 = row.try_get(0).expect("count");
        assert_eq!(count, 2);
        let ledger_row = sqlx::query(
            r#"
            SELECT request_id, pricing_status, provider_key, upstream_model, computed_cost_10000
            FROM usage_cost_events
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("usage ledger rows");
        assert_eq!(ledger_row.len(), 1);
        assert_eq!(
            ledger_row[0].try_get::<String, _>(0).expect("request id"),
            "req-postgres-ledger"
        );
        assert_eq!(
            ledger_row[0]
                .try_get::<String, _>(1)
                .expect("pricing status"),
            "priced"
        );
        assert_eq!(
            ledger_row[0].try_get::<String, _>(2).expect("provider key"),
            "openai-prod"
        );
        assert_eq!(
            ledger_row[0]
                .try_get::<String, _>(3)
                .expect("upstream model"),
            "gpt-5"
        );
        assert!(ledger_row[0].try_get::<i64, _>(4).expect("computed cost") >= 0);
        let model_pricing_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM model_pricing")
            .fetch_one(&pool)
            .await
            .expect("model pricing count");
        assert!(model_pricing_count >= 1);
        pool.close().await;

        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_stream_without_usage_records_usage_missing() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres gateway stream smoke test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let mut providers = gateway_core::ProviderRegistry::new();
        let (_, mock_provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "unused"},
                    "finish_reason": "stop"
                }]
            })),
            vec![
                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n".to_string(),
                "data: [DONE]\n\n".to_string(),
            ],
            ProviderCapabilities::new(true, true, false),
        );
        providers.register(Arc::new(mock_provider));

        let (app, raw_key) = build_postgres_test_app(&test_db.database_url, providers).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("content-type", "application/json")
                    .header("x-request-id", "req-postgres-stream")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "stream": true,
                            "messages": [{"role": "user", "content": "hello"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("stream response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(
            String::from_utf8(body.to_vec())
                .expect("utf8")
                .contains("data: [DONE]")
        );

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let row = sqlx::query(
            "SELECT request_id, pricing_status, computed_cost_10000 FROM usage_cost_events",
        )
        .fetch_all(&pool)
        .await
        .expect("usage ledger rows");
        assert_eq!(row.len(), 1);
        assert_eq!(
            row[0].try_get::<String, _>(0).expect("request id"),
            "req-postgres-stream"
        );
        assert_eq!(
            row[0].try_get::<String, _>(1).expect("pricing status"),
            "usage_missing"
        );
        assert_eq!(row[0].try_get::<i64, _>(2).expect("computed cost"), 0);
        pool.close().await;

        drop_postgres_test_database(&test_db).await;
    }
}
