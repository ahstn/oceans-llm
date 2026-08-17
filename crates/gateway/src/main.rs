use std::{env, path::Path, sync::Arc, time::Duration};

use admin_ui::AdminUiConfig;
use anyhow::Context;
use clap::Parser;
use gateway::{
    cli::{Cli, Command, ConfigCommand, MigrateAction, ServeArgs},
    config::{BootstrapAdminConfig, BudgetAlertEmailConfig, GatewayConfig},
    email::build_budget_alert_sender,
    http::{build_router, response_cache::ResponseCache, state::AppState},
    observability,
};
use gateway_core::{ProviderRegistry, SeedHumanBudgetDefaults};
use gateway_providers::{BedrockProvider, CopilotProvider, OpenAiCompatProvider, VertexProvider};
use gateway_service::{
    AnalysisPolicy, DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL, GatewayService, McpCredentialService,
    WeightedRoutePlanner, hash_gateway_key_secret,
};
use gateway_store::{
    AnyStore, GatewayStore, MigrationStatus, check_migrations_with_options,
    run_migrations_with_options, status_migrations_with_options,
};
use tokio::net::TcpListener;

mod agent_analysis_recompute;
mod local_demo_seed;
mod request_log_purge;

use local_demo_seed::{LOCAL_DEMO_USER_PASSWORD, seed_local_demo_data};

const ADMIN_VIEW_CACHE_TTL: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Serve(ServeArgs::default()));

    if matches!(&command, Command::Config(ConfigCommand::Validate)) {
        validate_config_file(&cli.config)?;
        println!("gateway configuration `{}` is valid", cli.config);
        return Ok(());
    }

    let config = load_config(&cli.config)?;
    let observability = observability::init_observability(&config.server)?;

    let result = match command {
        Command::Config(ConfigCommand::Validate) => unreachable!("handled before runtime startup"),
        Command::Serve(args) => run_serve(&config, observability.metrics.clone(), args).await,
        Command::Migrate(args) => run_migrate(&config, args.action()?).await,
        Command::PurgeRequestLogs(args) => request_log_purge::run_command(&config, args).await,
        Command::RecomputeAgentAnalysis(args) => {
            agent_analysis_recompute::run_command(&config, args).await
        }
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

fn validate_config_file(config_path: &str) -> anyhow::Result<()> {
    if !Path::new(config_path).exists() {
        anyhow::bail!("gateway configuration `{config_path}` does not exist");
    }
    let _ = load_config(config_path)?;
    Ok(())
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
    let service_accounts_seed = config.seed_service_accounts()?;
    let oidc_providers_seed = config.seed_oidc_providers()?;
    let oauth_providers_seed = config.seed_oauth_providers()?;
    let teams_seed = config.seed_teams()?;
    let users_seed = config.seed_users()?;
    let human_budget_defaults = config.seed_human_budget_defaults()?;

    store
        .seed_from_inputs(
            &providers_seed,
            &models_seed,
            &[],
            &service_accounts_seed,
            &oidc_providers_seed,
            &oauth_providers_seed,
            &teams_seed,
            &users_seed,
        )
        .await
        .context("failed to seed foundational config data")?;
    store
        .reconcile_human_budget_defaults(&human_budget_defaults, time::OffsetDateTime::now_utc())
        .await
        .context("failed to reconcile human budget defaults")
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
    let human_budget_defaults = Arc::new(config.seed_human_budget_defaults()?);
    let admin_permissions = Arc::new(config.resolved_admin_permissions()?);

    if args.seed_config {
        seed_config(store.as_ref(), config).await?;
    }

    if args.bootstrap_admin {
        ensure_bootstrap_admin(
            &store,
            &config.auth.bootstrap_admin,
            human_budget_defaults.as_ref(),
        )
        .await
        .context("failed to ensure bootstrap admin access")?;
    }

    let agent_analysis = config.agent_analysis.resolve()?;
    let service = build_gateway_service(
        config,
        store,
        agent_analysis.capabilities.passive_analysis_enabled,
        agent_analysis.report_retention,
        agent_analysis.queue_retention,
        agent_analysis.policy.clone(),
    )?;
    service
        .refresh_pricing_catalog_if_stale()
        .await
        .context("failed to initialize pricing catalog")?;
    service
        .validate_route_context_overrides()
        .await
        .context("invalid route context-window override")?;
    spawn_pricing_catalog_refresh_loop(service.clone());
    spawn_budget_alert_delivery_loop(service.clone(), &config.budget_alerts.email);
    if agent_analysis.capabilities.passive_analysis_enabled {
        spawn_agent_analysis_loop(service.clone());
    }
    spawn_agent_analysis_retention_loop(service.clone());
    request_log_purge::spawn_loop(service.clone(), &config.request_logging.purge);
    let providers = build_provider_registry(config)?;
    gateway::batch_worker::spawn(service.clone(), providers.clone());
    McpCredentialService::<AnyStore>::validate_runtime_configuration(
        !config.mcp.oauth.providers.is_empty(),
    )
    .context("invalid MCP credential runtime configuration")?;

    let bind_address = config.server.bind_address()?;

    let app = build_router(
        AppState {
            service: service.clone(),
            store: service.store().clone(),
            providers,
            metrics,
            mcp_http_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("MCP HTTP client configuration must be valid"),
            mcp_oauth_runtime: Arc::new(
                config
                    .mcp
                    .oauth
                    .runtime()
                    .context("failed resolving MCP OAuth configuration")?,
            ),
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
            client_config_gateway_base_url: Arc::new(
                load_client_config_gateway_base_url()
                    .context("failed resolving client config gateway base URL")?,
            ),
            budget_defaults: human_budget_defaults,
            agent_analysis: agent_analysis.capabilities,
            admin_permissions,
            leaderboard_cache: Arc::new(ResponseCache::new(ADMIN_VIEW_CACHE_TTL)),
            harness_usage_cache: Arc::new(ResponseCache::new(ADMIN_VIEW_CACHE_TTL)),
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
    let human_budget_defaults = config.seed_human_budget_defaults()?;
    ensure_bootstrap_admin(&store, &config.auth.bootstrap_admin, &human_budget_defaults).await
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
    let human_budget_defaults = config.seed_human_budget_defaults()?;
    seed_config(store.as_ref(), config).await?;
    ensure_bootstrap_admin(&store, &config.auth.bootstrap_admin, &human_budget_defaults).await?;
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
    human_budget_defaults: &SeedHumanBudgetDefaults,
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
    let now = time::OffsetDateTime::now_utc();
    store
        .store_user_password(user.user_id, &password_hash, now)
        .await
        .context("failed storing bootstrap admin password")?;
    store
        .apply_human_budget_defaults_for_user(human_budget_defaults, user.user_id, now)
        .await
        .context("failed applying bootstrap admin budget defaults")?;

    Ok(())
}

fn build_provider_registry(config: &GatewayConfig) -> anyhow::Result<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();

    for provider_config in config.openai_compatible_provider_configs()? {
        let provider_type = provider_config.provider_type.clone();
        let provider = OpenAiCompatProvider::new(provider_config).map_err(|error| {
            anyhow::anyhow!("failed building {provider_type} provider: {error}")
        })?;
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

    for provider_config in config.copilot_provider_configs()? {
        let provider = CopilotProvider::new(provider_config)
            .map_err(|error| anyhow::anyhow!("failed building github_copilot provider: {error}"))?;
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

fn load_client_config_gateway_base_url() -> anyhow::Result<Option<String>> {
    let raw_url = match env::var("GATEWAY_CLIENT_CONFIG_BASE_URL") {
        Ok(raw_url) => raw_url,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            anyhow::bail!("GATEWAY_CLIENT_CONFIG_BASE_URL must be valid Unicode")
        }
    };
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        anyhow::bail!("GATEWAY_CLIENT_CONFIG_BASE_URL cannot be empty");
    }
    if trimmed.len() != raw_url.len() {
        anyhow::bail!(
            "GATEWAY_CLIENT_CONFIG_BASE_URL cannot include leading or trailing whitespace"
        );
    }

    let parsed = url::Url::parse(trimmed)
        .context("GATEWAY_CLIENT_CONFIG_BASE_URL must be an absolute URL")?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            anyhow::bail!("GATEWAY_CLIENT_CONFIG_BASE_URL scheme `{scheme}` is not supported")
        }
    }
    if parsed.host().is_none() {
        anyhow::bail!("GATEWAY_CLIENT_CONFIG_BASE_URL must include a host");
    }

    Ok(Some(trimmed.trim_end_matches('/').to_string()))
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

fn spawn_agent_analysis_loop(service: Arc<GatewayService<AnyStore, WeightedRoutePlanner>>) {
    let lease_owner = format!("gateway-{}-{}", std::process::id(), uuid::Uuid::new_v4());
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let now = time::OffsetDateTime::now_utc();
            if let Err(error) = service.finalize_idle_agent_sessions(now).await {
                tracing::warn!(error = %error, "finalizing idle agent sessions failed");
            }
            loop {
                match service
                    .process_next_agent_analysis(&lease_owner, time::OffsetDateTime::now_utc())
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        tracing::warn!(error = %error, "background agent analysis failed");
                        break;
                    }
                }
            }
        }
    });
}
fn spawn_agent_analysis_retention_loop(
    service: Arc<GatewayService<AnyStore, WeightedRoutePlanner>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
        loop {
            interval.tick().await;
            if let Err(error) = service
                .purge_expired_agent_analysis(time::OffsetDateTime::now_utc())
                .await
            {
                tracing::warn!(error = %error, "agent analysis retention purge failed");
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
    agent_analysis_enabled: bool,
    analysis_report_retention: time::Duration,
    analysis_queue_retention: time::Duration,
    analysis_policy: AnalysisPolicy,
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
        )
        .with_agent_analysis_enabled(agent_analysis_enabled)
        .with_agent_analysis_retention(analysis_report_retention, analysis_queue_retention)
        .with_agent_analysis_policy(analysis_policy),
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::validate_config_file;

    #[test]
    fn config_validation_requires_an_existing_file() {
        let tmp = tempdir().expect("tempdir");
        let missing_path = tmp.path().join("missing.yaml");

        let error = validate_config_file(missing_path.to_str().expect("utf-8 path"))
            .expect_err("missing config should fail");

        assert!(format!("{error:#}").contains("does not exist"));
    }
}
