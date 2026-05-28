use std::{env, net::SocketAddr, path::Path, sync::Arc, time::Duration};

use admin_ui::AdminUiConfig;
use anyhow::Context;
use clap::Parser;
use gateway::{
    cli::{Cli, Command, MigrateAction, ServeArgs},
    config::{BootstrapAdminConfig, BudgetAlertEmailConfig, GatewayConfig},
    email::build_budget_alert_sender,
    http::{build_router, state::AppState},
    observability,
};
use gateway_core::ProviderRegistry;
use gateway_providers::{BedrockProvider, OpenAiCompatProvider, VertexProvider};
use gateway_service::{
    DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL, GatewayService, WeightedRoutePlanner,
    hash_gateway_key_secret,
};
use gateway_store::{
    AnyStore, GatewayStore, MigrationStatus, check_migrations_with_options,
    run_migrations_with_options, status_migrations_with_options,
};
use tokio::net::TcpListener;

mod local_demo_seed;
mod request_log_purge;

use local_demo_seed::{LOCAL_DEMO_USER_PASSWORD, seed_local_demo_data};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;

    let observability = observability::init_observability(&config.server)?;

    let result = match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => run_serve(&config, observability.metrics.clone(), args).await,
        Command::Migrate(args) => run_migrate(&config, args.action()?).await,
        Command::PurgeRequestLogs(args) => request_log_purge::run_command(&config, args).await,
        Command::BootstrapAdmin => run_bootstrap_admin_command(&config).await,
        Command::SeedConfig => run_seed_config_command(&config).await,
        Command::SeedLocalDemo => run_seed_local_demo_command(&config).await,
    };
    observability.shutdown()?;
    result
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

fn ensure_seed_local_demo_targets_local_database(
    database_options: &gateway_store::StoreConnectionOptions,
) -> anyhow::Result<()> {
    match database_options {
        gateway_store::StoreConnectionOptions::Libsql { .. } => Ok(()),
        gateway_store::StoreConnectionOptions::Postgres { url, .. } => {
            let parsed = url::Url::parse(url)
                .context("failed parsing postgres url for `seed-local-demo`")?;
            let host = parsed.host().ok_or_else(|| {
                anyhow::anyhow!(
                    "`seed-local-demo` requires a postgres url with an explicit local host"
                )
            })?;
            let is_local = match host {
                url::Host::Domain(domain) => {
                    domain.eq_ignore_ascii_case("localhost")
                        || domain
                            .parse::<std::net::IpAddr>()
                            .map(|address| address.is_loopback())
                            .unwrap_or(false)
                }
                url::Host::Ipv4(address) => address.is_loopback(),
                url::Host::Ipv6(address) => address.is_loopback(),
            };

            if is_local {
                Ok(())
            } else {
                anyhow::bail!(
                    "`seed-local-demo` only supports local databases; postgres host `{host}` is not local"
                )
            }
        }
    }
}

async fn maybe_run_migrations(
    database_options: &gateway_store::StoreConnectionOptions,
    enabled: bool,
) -> anyhow::Result<()> {
    if !enabled {
        return Ok(());
    }

    run_migrations_with_options(database_options)
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
    let oidc_providers_seed = config.seed_oidc_providers()?;
    let oauth_providers_seed = config.seed_oauth_providers()?;
    let teams_seed = config.seed_teams()?;
    let users_seed = config.seed_users()?;

    store
        .seed_from_inputs(
            &providers_seed,
            &models_seed,
            &api_keys_seed,
            &oidc_providers_seed,
            &oauth_providers_seed,
            &teams_seed,
            &users_seed,
        )
        .await
        .context("failed to seed foundational config data")
}

async fn run_serve(
    config: &GatewayConfig,
    metrics: Arc<observability::GatewayMetrics>,
    args: ServeArgs,
) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    maybe_run_migrations(&database_options, args.run_migrations).await?;
    let store = Arc::new(
        AnyStore::connect(&database_options)
            .await
            .context("failed to initialize gateway store")?,
    );
    run_serve_with_store(config, store, metrics, args).await
}

async fn run_serve_with_store(
    config: &GatewayConfig,
    store: Arc<AnyStore>,
    metrics: Arc<observability::GatewayMetrics>,
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

    let service = build_gateway_service(config, store)?;
    if let Err(error) = service.refresh_pricing_catalog_if_stale().await {
        tracing::warn!(error = %error, "initial pricing catalog refresh failed");
    }
    spawn_pricing_catalog_refresh_loop(service.clone());
    spawn_budget_alert_delivery_loop(service.clone(), &config.budget_alerts.email);
    request_log_purge::spawn_loop(service.clone(), &config.request_logging.purge);
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
            metrics,
            mcp_http_client: reqwest::Client::new(),
            identity_token_secret: Arc::new(load_identity_token_secret()),
            oidc_public_base_url: Arc::new(
                config
                    .auth
                    .oidc
                    .resolved_public_base_url()
                    .context("failed resolving OIDC public base URL")?,
            ),
            oauth_public_base_url: Arc::new(
                config
                    .auth
                    .oauth
                    .resolved_public_base_url()
                    .context("failed resolving OAuth public base URL")?,
            ),
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
            run_migrations_with_options(&database_options)
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

async fn run_seed_local_demo_command(config: &GatewayConfig) -> anyhow::Result<()> {
    let database_options = database_options(config)?;
    ensure_seed_local_demo_targets_local_database(&database_options)?;
    maybe_run_migrations(&database_options, true).await?;
    let store = Arc::new(
        AnyStore::connect(&database_options)
            .await
            .context("failed to initialize gateway store")?,
    );
    ensure_bootstrap_admin(&store, &config.auth.bootstrap_admin).await?;
    seed_config(store.as_ref(), config).await?;
    let raw_keys = seed_local_demo_data(store.as_ref()).await?;

    println!("seeded local demo dataset");
    println!("sample user password: {}", LOCAL_DEMO_USER_PASSWORD);
    for (name, raw_key) in raw_keys {
        println!("{name}: {raw_key}");
    }

    Ok(())
}

fn print_migration_status(status: &MigrationStatus) {
    println!("backend: {}", status.backend);
    for entry in &status.entries {
        let state = if entry.applied { "applied" } else { "pending" };
        println!("v{} {} [{}]", entry.version, entry.name, state);
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

    for provider_config in config.bedrock_provider_configs()? {
        let provider = BedrockProvider::new(provider_config)
            .map_err(|error| anyhow::anyhow!("failed building aws_bedrock provider: {error}"))?;
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

fn spawn_budget_alert_delivery_loop(
    service: Arc<GatewayService<AnyStore, WeightedRoutePlanner>>,
    config: &BudgetAlertEmailConfig,
) {
    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let batch_size = config.batch_size;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        interval.tick().await;

        loop {
            interval.tick().await;
            if let Err(error) = service
                .dispatch_pending_budget_alert_deliveries(batch_size)
                .await
            {
                tracing::warn!(error = %error, "background budget alert delivery failed");
            }
        }
    });
}

fn build_gateway_service(
    config: &GatewayConfig,
    store: Arc<AnyStore>,
) -> anyhow::Result<Arc<GatewayService<AnyStore, WeightedRoutePlanner>>> {
    let planner = Arc::new(WeightedRoutePlanner::default());
    let budget_alert_sender = build_budget_alert_sender(&config.budget_alerts.email)
        .context("failed to build budget alert email sender")?;
    let payload_policy = config
        .request_log_payload_policy()
        .context("failed to build request log payload policy")?;
    Ok(Arc::new(
        GatewayService::new_with_budget_alert_sender_and_payload_policy(
            store,
            planner,
            budget_alert_sender,
            payload_policy,
        ),
    ))
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
