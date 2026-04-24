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
use gateway_core::{
    AdminApiKeyRepository, ApiKeyOwnerKind, ApiKeyRepository, ApiKeyStatus, BudgetRepository,
    IdentityRepository, ModelRepository, Money4, NewApiKeyRecord, ProviderRegistry,
    RequestLogPayloadRecord, RequestLogRecord, RequestLogRepository, RequestTag, RequestTags,
    UsageLedgerRecord, UsagePricingStatus, UserStatus,
};
use gateway_providers::{OpenAiCompatProvider, VertexProvider};
use gateway_service::{
    DEFAULT_PRICING_CATALOG_REFRESH_INTERVAL, GatewayService, WeightedRoutePlanner,
    hash_gateway_key_secret,
};
use gateway_store::{
    AnyStore, GatewayStore, MigrationStatus, check_migrations_with_options,
    run_migrations_with_options, status_migrations_with_options,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tokio::net::TcpListener;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;

    let observability = observability::init_observability(&config.server)?;

    let result = match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => run_serve(&config, observability.metrics.clone(), args).await,
        Command::Migrate(args) => run_migrate(&config, args.action()?).await,
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
    let teams_seed = config.seed_teams()?;
    let users_seed = config.seed_users()?;

    store
        .seed_from_inputs(
            &providers_seed,
            &models_seed,
            &api_keys_seed,
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

    let planner = Arc::new(WeightedRoutePlanner::default());
    let budget_alert_sender = build_budget_alert_sender(&config.budget_alerts.email)
        .context("failed to build budget alert email sender")?;
    let service = Arc::new(GatewayService::new_with_budget_alert_sender(
        store,
        planner,
        budget_alert_sender,
    ));
    if let Err(error) = service.refresh_pricing_catalog_if_stale().await {
        tracing::warn!(error = %error, "initial pricing catalog refresh failed");
    }
    spawn_pricing_catalog_refresh_loop(service.clone());
    spawn_budget_alert_delivery_loop(service.clone(), &config.budget_alerts.email);
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

#[derive(Debug, Clone, Copy)]
struct LocalDemoUserFixture {
    email: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum LocalDemoOwnerFixture {
    User(&'static str),
    Team(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct LocalDemoApiKeyFixture {
    name: &'static str,
    public_id: &'static str,
    secret: &'static str,
    owner: LocalDemoOwnerFixture,
    model_keys: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct LocalDemoRequestFixture {
    request_id: &'static str,
    api_key_public_id: &'static str,
    days_ago: i64,
    hours_ago: i64,
    model_key: &'static str,
    resolved_model_key: &'static str,
    provider_key: &'static str,
    upstream_model: &'static str,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cost_scaled: i64,
    status_code: i64,
    latency_ms: i64,
    service: &'static str,
    component: &'static str,
    env: &'static str,
    bespoke_key: &'static str,
    bespoke_value: &'static str,
    prompt: &'static str,
    completion: &'static str,
    error_code: Option<&'static str>,
}

const LOCAL_DEMO_USER_PASSWORD: &str = "localdemo123";

const LOCAL_DEMO_USERS: &[LocalDemoUserFixture] = &[
    LocalDemoUserFixture {
        email: "alice@platform.local",
    },
    LocalDemoUserFixture {
        email: "ben@platform.local",
    },
    LocalDemoUserFixture {
        email: "cara@research.local",
    },
    LocalDemoUserFixture {
        email: "diego@research.local",
    },
    LocalDemoUserFixture {
        email: "erin@research.local",
    },
];

const LOCAL_DEMO_API_KEYS: &[LocalDemoApiKeyFixture] = &[
    LocalDemoApiKeyFixture {
        name: "Alice Personal Key",
        public_id: "locdemoalice1",
        secret: "alice-demo-secret",
        owner: LocalDemoOwnerFixture::User("alice@platform.local"),
        model_keys: &["openai-fast", "openai-fast-v2"],
    },
    LocalDemoApiKeyFixture {
        name: "Cara Research Key",
        public_id: "locdemocara1",
        secret: "cara-demo-secret",
        owner: LocalDemoOwnerFixture::User("cara@research.local"),
        model_keys: &["gemini-fast", "claude-sonnet"],
    },
    LocalDemoApiKeyFixture {
        name: "Diego Daily Budget Key",
        public_id: "locdemodiego1",
        secret: "diego-demo-secret",
        owner: LocalDemoOwnerFixture::User("diego@research.local"),
        model_keys: &["openai-fast", "claude-sonnet"],
    },
    LocalDemoApiKeyFixture {
        name: "Platform Team Key",
        public_id: "locdemoplatform1",
        secret: "platform-demo-secret",
        owner: LocalDemoOwnerFixture::Team("platform"),
        model_keys: &["openai-fast-v2", "gemini-fast"],
    },
];

const LOCAL_DEMO_REQUESTS: &[LocalDemoRequestFixture] = &[
    LocalDemoRequestFixture {
        request_id: "demo-req-001",
        api_key_public_id: "locdemoalice1",
        days_ago: 0,
        hours_ago: 2,
        model_key: "openai-fast",
        resolved_model_key: "openai-fast-v2",
        provider_key: "openai-prod",
        upstream_model: "gpt-5",
        prompt_tokens: Some(620),
        completion_tokens: Some(280),
        cost_scaled: 1_245,
        status_code: 200,
        latency_ms: 482,
        service: "admin-ui",
        component: "dashboard",
        env: "local",
        bespoke_key: "workflow",
        bespoke_value: "summary",
        prompt: "Summarize this week’s API key usage.",
        completion: "Here is a compact weekly usage summary for the platform team.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-002",
        api_key_public_id: "locdemoplatform1",
        days_ago: 0,
        hours_ago: 4,
        model_key: "openai-fast-v2",
        resolved_model_key: "openai-fast-v2",
        provider_key: "openai-secondary",
        upstream_model: "gpt-5",
        prompt_tokens: Some(840),
        completion_tokens: Some(410),
        cost_scaled: 2_180,
        status_code: 200,
        latency_ms: 611,
        service: "batch-jobs",
        component: "nightly-rollup",
        env: "local",
        bespoke_key: "job",
        bespoke_value: "cost-rollup",
        prompt: "Generate a nightly rollup of usage and spend anomalies.",
        completion: "Nightly rollup completed with two notable spend anomalies.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-003",
        api_key_public_id: "locdemocara1",
        days_ago: 1,
        hours_ago: 3,
        model_key: "gemini-fast",
        resolved_model_key: "gemini-fast",
        provider_key: "vertex-adc",
        upstream_model: "google/gemini-2.0-flash",
        prompt_tokens: Some(510),
        completion_tokens: Some(320),
        cost_scaled: 1_610,
        status_code: 200,
        latency_ms: 729,
        service: "research-lab",
        component: "experiment-runner",
        env: "local",
        bespoke_key: "experiment",
        bespoke_value: "safety-eval",
        prompt: "Draft notes for the latest safety evaluation batch.",
        completion: "The latest safety evaluation batch shows stable refusal behavior.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-004",
        api_key_public_id: "locdemodiego1",
        days_ago: 0,
        hours_ago: 1,
        model_key: "claude-sonnet",
        resolved_model_key: "claude-sonnet",
        provider_key: "vertex-claude",
        upstream_model: "anthropic/claude-sonnet-4-6",
        prompt_tokens: Some(430),
        completion_tokens: Some(690),
        cost_scaled: 3_420,
        status_code: 200,
        latency_ms: 938,
        service: "research-lab",
        component: "daily-brief",
        env: "local",
        bespoke_key: "brief",
        bespoke_value: "morning",
        prompt: "Write the daily research brief for active experiments.",
        completion: "Daily research brief prepared for the active experiment queue.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-005",
        api_key_public_id: "locdemoalice1",
        days_ago: 2,
        hours_ago: 6,
        model_key: "openai-fast-v2",
        resolved_model_key: "openai-fast-v2",
        provider_key: "openai-prod",
        upstream_model: "gpt-5",
        prompt_tokens: Some(710),
        completion_tokens: Some(360),
        cost_scaled: 1_980,
        status_code: 200,
        latency_ms: 564,
        service: "admin-ui",
        component: "request-log-detail",
        env: "local",
        bespoke_key: "panel",
        bespoke_value: "detail",
        prompt: "Explain the request log payload delta between retries.",
        completion: "The second attempt used the fallback provider and a shorter system prompt.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-006",
        api_key_public_id: "locdemoplatform1",
        days_ago: 3,
        hours_ago: 5,
        model_key: "gemini-fast",
        resolved_model_key: "gemini-fast",
        provider_key: "vertex-adc",
        upstream_model: "google/gemini-2.0-flash",
        prompt_tokens: Some(390),
        completion_tokens: Some(240),
        cost_scaled: 1_140,
        status_code: 200,
        latency_ms: 688,
        service: "batch-jobs",
        component: "owner-sync",
        env: "local",
        bespoke_key: "job",
        bespoke_value: "owner-sync",
        prompt: "Normalize owner metadata for request-log exports.",
        completion: "Owner metadata normalized for the latest export window.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-007",
        api_key_public_id: "locdemocara1",
        days_ago: 4,
        hours_ago: 2,
        model_key: "claude-sonnet",
        resolved_model_key: "claude-sonnet",
        provider_key: "vertex-claude",
        upstream_model: "anthropic/claude-sonnet-4-6",
        prompt_tokens: Some(560),
        completion_tokens: Some(470),
        cost_scaled: 2_860,
        status_code: 200,
        latency_ms: 1_024,
        service: "research-lab",
        component: "retrospective",
        env: "local",
        bespoke_key: "artifact",
        bespoke_value: "retrospective",
        prompt: "Write a concise retrospective for the last experiment iteration.",
        completion: "The retrospective highlights routing stability and lower-than-expected cost.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-008",
        api_key_public_id: "locdemodiego1",
        days_ago: 1,
        hours_ago: 7,
        model_key: "openai-fast",
        resolved_model_key: "openai-fast-v2",
        provider_key: "openai-prod",
        upstream_model: "gpt-5",
        prompt_tokens: Some(280),
        completion_tokens: Some(150),
        cost_scaled: 790,
        status_code: 200,
        latency_ms: 431,
        service: "research-lab",
        component: "token-audit",
        env: "local",
        bespoke_key: "audit",
        bespoke_value: "token-usage",
        prompt: "Audit the latest token usage sample for anomalies.",
        completion: "Token usage stayed within the expected variance band.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-009",
        api_key_public_id: "locdemoplatform1",
        days_ago: 5,
        hours_ago: 4,
        model_key: "openai-fast-v2",
        resolved_model_key: "openai-fast-v2",
        provider_key: "openai-prod",
        upstream_model: "gpt-5",
        prompt_tokens: Some(930),
        completion_tokens: Some(510),
        cost_scaled: 2_640,
        status_code: 200,
        latency_ms: 702,
        service: "batch-jobs",
        component: "forecasting",
        env: "local",
        bespoke_key: "job",
        bespoke_value: "forecasting",
        prompt: "Forecast next week’s spend trend from the last 14 days.",
        completion: "The next week trends slightly upward with stable model mix.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-010",
        api_key_public_id: "locdemoalice1",
        days_ago: 6,
        hours_ago: 3,
        model_key: "openai-fast",
        resolved_model_key: "openai-fast-v2",
        provider_key: "openai-secondary",
        upstream_model: "gpt-5",
        prompt_tokens: Some(470),
        completion_tokens: Some(250),
        cost_scaled: 1_120,
        status_code: 200,
        latency_ms: 547,
        service: "admin-ui",
        component: "breadcrumbs",
        env: "local",
        bespoke_key: "screen",
        bespoke_value: "overview",
        prompt: "Summarize top navigation usage patterns from the admin UI.",
        completion: "The overview screen remains the most active admin entry point.",
        error_code: None,
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-011",
        api_key_public_id: "locdemocara1",
        days_ago: 2,
        hours_ago: 8,
        model_key: "gemini-fast",
        resolved_model_key: "gemini-fast",
        provider_key: "vertex-adc",
        upstream_model: "google/gemini-2.0-flash",
        prompt_tokens: None,
        completion_tokens: None,
        cost_scaled: 0,
        status_code: 429,
        latency_ms: 214,
        service: "research-lab",
        component: "burst-eval",
        env: "local",
        bespoke_key: "failure",
        bespoke_value: "rate-limit",
        prompt: "Run the burst evaluation batch.",
        completion: "rate limit exceeded by upstream",
        error_code: Some("rate_limited"),
    },
    LocalDemoRequestFixture {
        request_id: "demo-req-012",
        api_key_public_id: "locdemodiego1",
        days_ago: 3,
        hours_ago: 9,
        model_key: "claude-sonnet",
        resolved_model_key: "claude-sonnet",
        provider_key: "vertex-claude",
        upstream_model: "anthropic/claude-sonnet-4-6",
        prompt_tokens: None,
        completion_tokens: None,
        cost_scaled: 0,
        status_code: 502,
        latency_ms: 301,
        service: "research-lab",
        component: "provider-fallback",
        env: "local",
        bespoke_key: "failure",
        bespoke_value: "upstream-error",
        prompt: "Generate a fallback plan for the next routing attempt.",
        completion: "upstream gateway timeout",
        error_code: Some("upstream_http_502"),
    },
];

async fn seed_local_demo_data(store: &AnyStore) -> anyhow::Result<Vec<(&'static str, String)>> {
    let password_hash = hash_gateway_key_secret(LOCAL_DEMO_USER_PASSWORD)
        .context("failed hashing local demo user password")?;
    let now = OffsetDateTime::now_utc();

    let mut user_ids = std::collections::HashMap::new();
    let mut user_team_ids = std::collections::HashMap::new();
    for fixture in LOCAL_DEMO_USERS {
        let user = store
            .get_user_by_email_normalized(&normalize_demo_email(fixture.email))
            .await
            .with_context(|| format!("failed loading demo user `{}`", fixture.email))?
            .ok_or_else(|| {
                anyhow::anyhow!("demo user `{}` is missing from config seed", fixture.email)
            })?;
        store
            .store_user_password(user.user_id, &password_hash, now)
            .await
            .with_context(|| format!("failed storing password for `{}`", fixture.email))?;
        store
            .update_user_status(user.user_id, UserStatus::Active, now)
            .await
            .with_context(|| format!("failed activating `{}`", fixture.email))?;
        store
            .update_user_must_change_password(user.user_id, false, now)
            .await
            .with_context(|| {
                format!("failed clearing password rotation for `{}`", fixture.email)
            })?;

        let team_id = store
            .get_team_membership_for_user(user.user_id)
            .await
            .with_context(|| format!("failed loading team membership for `{}`", fixture.email))?
            .map(|membership| membership.team_id);
        user_ids.insert(fixture.email, user.user_id);
        user_team_ids.insert(fixture.email, team_id);
    }

    let mut team_ids = std::collections::HashMap::new();
    for team_key in ["platform", "research"] {
        let team = store
            .get_team_by_key(team_key)
            .await
            .with_context(|| format!("failed loading demo team `{team_key}`"))?
            .ok_or_else(|| anyhow::anyhow!("demo team `{team_key}` is missing from config seed"))?;
        team_ids.insert(team_key, team.team_id);
    }

    let mut model_ids = std::collections::HashMap::new();
    for model_key in [
        "openai-fast",
        "openai-fast-v2",
        "gemini-fast",
        "claude-sonnet",
    ] {
        let model = store
            .get_model_by_key(model_key)
            .await
            .with_context(|| format!("failed loading demo model `{model_key}`"))?
            .ok_or_else(|| {
                anyhow::anyhow!("demo model `{model_key}` is missing from config seed")
            })?;
        model_ids.insert(model_key, model.id);
    }

    let mut api_keys = std::collections::HashMap::new();
    let mut raw_keys = Vec::new();
    for fixture in LOCAL_DEMO_API_KEYS {
        let owner = match fixture.owner {
            LocalDemoOwnerFixture::User(email) => (
                ApiKeyOwnerKind::User,
                Some(
                    *user_ids
                        .get(email)
                        .ok_or_else(|| anyhow::anyhow!("missing demo user `{email}`"))?,
                ),
                None,
            ),
            LocalDemoOwnerFixture::Team(team_key) => (
                ApiKeyOwnerKind::Team,
                None,
                Some(
                    *team_ids
                        .get(team_key)
                        .ok_or_else(|| anyhow::anyhow!("missing demo team `{team_key}`"))?,
                ),
            ),
        };
        let model_grants = fixture
            .model_keys
            .iter()
            .map(|model_key| {
                model_ids
                    .get(model_key)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("missing demo model `{model_key}`"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let raw_key = format!("gwk_{}.{}", fixture.public_id, fixture.secret);
        let api_key = match store
            .get_api_key_by_public_id(fixture.public_id)
            .await
            .with_context(|| format!("failed loading demo api key `{}`", fixture.public_id))?
        {
            Some(existing) => {
                if existing.status != ApiKeyStatus::Active {
                    anyhow::bail!(
                        "demo api key `{}` already exists but is not active; reset the local database and reseed",
                        fixture.public_id
                    );
                }
                if existing.owner_kind != owner.0
                    || existing.owner_user_id != owner.1
                    || existing.owner_team_id != owner.2
                {
                    anyhow::bail!(
                        "demo api key `{}` already exists with a different owner; reset the local database and reseed",
                        fixture.public_id
                    );
                }
                store
                    .replace_api_key_model_grants(existing.id, &model_grants)
                    .await
                    .with_context(|| {
                        format!("failed refreshing grants for `{}`", fixture.public_id)
                    })?;
                existing
            }
            None => {
                let secret_hash = hash_gateway_key_secret(fixture.secret)
                    .with_context(|| format!("failed hashing api key `{}`", fixture.public_id))?;
                let created = store
                    .create_api_key(&NewApiKeyRecord {
                        name: fixture.name.to_string(),
                        public_id: fixture.public_id.to_string(),
                        secret_hash,
                        owner_kind: owner.0,
                        owner_user_id: owner.1,
                        owner_team_id: owner.2,
                        created_at: now,
                    })
                    .await
                    .with_context(|| {
                        format!("failed creating demo api key `{}`", fixture.public_id)
                    })?;
                store
                    .replace_api_key_model_grants(created.id, &model_grants)
                    .await
                    .with_context(|| {
                        format!("failed storing grants for `{}`", fixture.public_id)
                    })?;
                created
            }
        };
        api_keys.insert(fixture.public_id, api_key);
        raw_keys.push((fixture.name, raw_key));
    }

    for fixture in LOCAL_DEMO_REQUESTS {
        let api_key = api_keys.get(fixture.api_key_public_id).ok_or_else(|| {
            anyhow::anyhow!("missing demo api key `{}`", fixture.api_key_public_id)
        })?;
        let occurred_at =
            now - time::Duration::days(fixture.days_ago) - time::Duration::hours(fixture.hours_ago);
        let ownership_scope_key = match api_key.owner_kind {
            ApiKeyOwnerKind::User => format!(
                "user:{}",
                api_key
                    .owner_user_id
                    .ok_or_else(|| anyhow::anyhow!("user-owned demo key missing owner_user_id"))?
            ),
            ApiKeyOwnerKind::Team => format!(
                "team:{}:actor:none",
                api_key
                    .owner_team_id
                    .ok_or_else(|| anyhow::anyhow!("team-owned demo key missing owner_team_id"))?
            ),
        };
        if store
            .get_usage_ledger_by_request_and_scope(fixture.request_id, &ownership_scope_key)
            .await
            .with_context(|| format!("failed loading usage ledger for `{}`", fixture.request_id))?
            .is_some()
        {
            continue;
        }

        let user_id = api_key.owner_user_id;
        let team_id = match api_key.owner_kind {
            ApiKeyOwnerKind::User => {
                let owner_email = LOCAL_DEMO_API_KEYS
                    .iter()
                    .find(|candidate| candidate.public_id == fixture.api_key_public_id)
                    .and_then(|candidate| match candidate.owner {
                        LocalDemoOwnerFixture::User(email) => Some(email),
                        LocalDemoOwnerFixture::Team(_) => None,
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "demo key `{}` is missing a user owner fixture",
                            fixture.api_key_public_id
                        )
                    })?;
                user_team_ids.get(owner_email).copied().flatten()
            }
            ApiKeyOwnerKind::Team => api_key.owner_team_id,
        };

        let total_tokens = fixture
            .prompt_tokens
            .zip(fixture.completion_tokens)
            .map(|(prompt, completion)| prompt + completion);
        let priced = fixture.error_code.is_none();
        let request_log_id = Uuid::new_v4();
        let request_tags = RequestTags {
            service: Some(fixture.service.to_string()),
            component: Some(fixture.component.to_string()),
            env: Some(fixture.env.to_string()),
            bespoke: vec![RequestTag {
                key: fixture.bespoke_key.to_string(),
                value: fixture.bespoke_value.to_string(),
            }],
        };
        let metadata = Map::from_iter([
            (
                "seed_source".to_string(),
                Value::String("local_demo_seed".to_string()),
            ),
            (
                "api_key_public_id".to_string(),
                Value::String(fixture.api_key_public_id.to_string()),
            ),
        ]);
        let log = RequestLogRecord {
            request_log_id,
            request_id: fixture.request_id.to_string(),
            api_key_id: api_key.id,
            user_id,
            team_id,
            model_key: fixture.model_key.to_string(),
            resolved_model_key: fixture.resolved_model_key.to_string(),
            provider_key: fixture.provider_key.to_string(),
            status_code: Some(fixture.status_code),
            latency_ms: Some(fixture.latency_ms),
            prompt_tokens: fixture.prompt_tokens,
            completion_tokens: fixture.completion_tokens,
            total_tokens,
            error_code: fixture.error_code.map(str::to_string),
            has_payload: true,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags,
            metadata,
            occurred_at,
        };
        let payload = RequestLogPayloadRecord {
            request_log_id,
            request_json: json!({
                "model": fixture.model_key,
                "messages": [
                    {"role": "system", "content": "You are a local demo assistant."},
                    {"role": "user", "content": fixture.prompt}
                ],
                "stream": false,
                "temperature": 0.2,
            }),
            response_json: if priced {
                json!({
                    "id": format!("chatcmpl_{}", fixture.request_id),
                    "object": "chat.completion",
                    "model": fixture.resolved_model_key,
                    "choices": [
                        {
                            "index": 0,
                            "finish_reason": "stop",
                            "message": {"role": "assistant", "content": fixture.completion}
                        }
                    ],
                    "usage": {
                        "prompt_tokens": fixture.prompt_tokens,
                        "completion_tokens": fixture.completion_tokens,
                        "total_tokens": total_tokens,
                    }
                })
            } else {
                json!({
                    "error": {
                        "code": fixture.error_code,
                        "message": fixture.completion,
                        "type": "upstream_error",
                    }
                })
            },
        };
        store
            .insert_request_log(&log, Some(&payload))
            .await
            .with_context(|| format!("failed inserting request log `{}`", fixture.request_id))?;

        let model_id = model_ids
            .get(fixture.resolved_model_key)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!("missing demo model `{}`", fixture.resolved_model_key)
            })?;
        let ledger = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: fixture.request_id.to_string(),
            ownership_scope_key,
            api_key_id: api_key.id,
            user_id,
            team_id,
            actor_user_id: None,
            model_id: Some(model_id),
            provider_key: fixture.provider_key.to_string(),
            upstream_model: fixture.upstream_model.to_string(),
            prompt_tokens: fixture.prompt_tokens,
            completion_tokens: fixture.completion_tokens,
            total_tokens,
            provider_usage: if priced {
                json!({
                    "prompt_tokens": fixture.prompt_tokens,
                    "completion_tokens": fixture.completion_tokens,
                    "total_tokens": total_tokens,
                })
            } else {
                json!({"status_code": fixture.status_code, "error_code": fixture.error_code})
            },
            pricing_status: if priced {
                UsagePricingStatus::Priced
            } else {
                UsagePricingStatus::UsageMissing
            },
            unpriced_reason: if priced {
                None
            } else {
                Some("upstream_error".to_string())
            },
            pricing_row_id: None,
            pricing_provider_id: pricing_provider_id_for_demo_provider(fixture.provider_key)
                .map(str::to_string),
            pricing_model_id: Some(fixture.upstream_model.to_string()),
            pricing_source: if priced {
                Some("local_demo_seed".to_string())
            } else {
                None
            },
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: if priced {
                Some(occurred_at.date().to_string())
            } else {
                None
            },
            input_cost_per_million_tokens: if priced {
                Some(Money4::from_scaled(1_000))
            } else {
                None
            },
            output_cost_per_million_tokens: if priced {
                Some(Money4::from_scaled(2_000))
            } else {
                None
            },
            computed_cost_usd: Money4::from_scaled(fixture.cost_scaled),
            occurred_at,
        };
        store
            .insert_usage_ledger_if_absent(&ledger)
            .await
            .with_context(|| format!("failed inserting usage ledger `{}`", fixture.request_id))?;
        store
            .touch_api_key_last_used(api_key.id)
            .await
            .with_context(|| {
                format!(
                    "failed updating last-used for `{}`",
                    fixture.api_key_public_id
                )
            })?;
    }

    Ok(raw_keys)
}

fn normalize_demo_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn pricing_provider_id_for_demo_provider(provider_key: &str) -> Option<&'static str> {
    match provider_key {
        "openai-prod" | "openai-secondary" => Some("openai"),
        "vertex-adc" => Some("google"),
        "vertex-claude" => Some("anthropic"),
        _ => None,
    }
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
        collections::BTreeMap,
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
        response::Response,
        routing::post,
    };
    use gateway_core::{
        ApiKeyRepository, AuthMode, BudgetAlertChannel, BudgetAlertDeliveryRecord,
        BudgetAlertDeliveryStatus, BudgetAlertRepository, BudgetCadence, BudgetRepository,
        CoreChatRequest, CoreEmbeddingsRequest, GlobalRole, IdentityRepository, MembershipRole,
        ModelRepository, Money4, ProviderCapabilities, ProviderClient, ProviderError,
        ProviderRequestContext, ProviderStream, SeedApiKey, SeedModel, SeedModelRoute,
        SeedProvider, UsageLedgerRecord, UsagePricingStatus, parse_gateway_api_key,
    };
    use gateway_providers::{OpenAiCompatConfig, OpenAiCompatProvider};
    use gateway_service::{GatewayService, WeightedRoutePlanner, hash_gateway_key_secret};
    use gateway_store::{
        AnyStore, GatewayStore, LibsqlStore, StoreConnectionOptions, run_migrations,
        run_migrations_with_options,
    };
    use serde_json::{Map, Value, json};
    use serial_test::serial;
    use sqlx::Row;
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use tower::ServiceExt;
    use url::Url;
    use uuid::Uuid;

    use crate::{ensure_bootstrap_admin, ensure_seed_local_demo_targets_local_database};
    use gateway::{
        config::{BootstrapAdminConfig, GatewayConfig},
        http::{build_router, state::AppState},
    };

    #[derive(Clone)]
    enum MockChatResult {
        Value(Value),
        Error(MockError),
    }

    #[derive(Clone)]
    enum MockEmbeddingsResult {
        Value(Value),
        Error(MockError),
    }

    #[derive(Clone)]
    enum MockError {
        UpstreamHttp(u16, String),
        InvalidRequest(String),
    }

    impl MockError {
        fn into_provider_error(self) -> ProviderError {
            match self {
                Self::UpstreamHttp(status, body) => ProviderError::UpstreamHttp { status, body },
                Self::InvalidRequest(message) => ProviderError::InvalidRequest(message),
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
            _request: &CoreChatRequest,
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
            _request: &CoreChatRequest,
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
            _request: &CoreEmbeddingsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "mock embeddings unsupported for this provider".to_string(),
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

    #[derive(Clone)]
    struct MockEmbeddingsProvider {
        key: String,
        provider_type: &'static str,
        caps: ProviderCapabilities,
        embeddings_result: MockEmbeddingsResult,
        embeddings_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ProviderClient for MockEmbeddingsProvider {
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
            _request: &CoreChatRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "mock chat completions unsupported for this provider".to_string(),
            ))
        }

        async fn chat_completions_stream(
            &self,
            _request: &CoreChatRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "mock chat stream unsupported for this provider".to_string(),
            ))
        }

        async fn embeddings(
            &self,
            _request: &CoreEmbeddingsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            self.embeddings_calls.fetch_add(1, Ordering::SeqCst);
            match self.embeddings_result.clone() {
                MockEmbeddingsResult::Value(value) => Ok(value),
                MockEmbeddingsResult::Error(error) => Err(error.into_provider_error()),
            }
        }
    }

    fn make_embeddings_provider(
        key: &str,
        embeddings_result: MockEmbeddingsResult,
        caps: ProviderCapabilities,
    ) -> (Arc<AtomicUsize>, MockEmbeddingsProvider) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            calls.clone(),
            MockEmbeddingsProvider {
                key: key.to_string(),
                provider_type: "mock_embeddings",
                caps,
                embeddings_result,
                embeddings_calls: calls,
            },
        )
    }

    #[derive(Debug)]
    struct RequestLogRow {
        user_id: Option<String>,
        team_id: Option<String>,
        model_key: String,
        resolved_model_key: Option<String>,
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

    #[derive(Debug)]
    struct RequestLogPayloadRow {
        response_json: Value,
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
                SELECT user_id, team_id, model_key, resolved_model_key, provider_key, status_code,
                       latency_ms, prompt_tokens, completion_tokens, total_tokens, error_code, metadata_json
                FROM request_logs
                ORDER BY occurred_at ASC, rowid ASC
                "#,
                (),
            )
            .await
            .expect("request logs query");

        let mut logs = Vec::new();
        while let Some(row) = rows.next().await.expect("request logs row") {
            let metadata_json: String = row.get(11).expect("metadata json");
            logs.push(RequestLogRow {
                user_id: row.get(0).expect("user_id"),
                team_id: row.get(1).expect("team_id"),
                model_key: row.get(2).expect("model_key"),
                resolved_model_key: row.get(3).expect("resolved_model_key"),
                provider_key: row.get(4).expect("provider_key"),
                status_code: row.get(5).expect("status_code"),
                latency_ms: row.get(6).expect("latency_ms"),
                prompt_tokens: row.get(7).expect("prompt_tokens"),
                completion_tokens: row.get(8).expect("completion_tokens"),
                total_tokens: row.get(9).expect("total_tokens"),
                error_code: row.get(10).expect("error_code"),
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

    async fn load_request_log_payloads(db_path: &Path) -> Vec<RequestLogPayloadRow> {
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        let mut rows = connection
            .query(
                r#"
                SELECT response_json
                FROM request_log_payloads
                ORDER BY rowid ASC
                "#,
                (),
            )
            .await
            .expect("request log payloads query");

        let mut payloads = Vec::new();
        while let Some(row) = rows.next().await.expect("request log payload row") {
            let response_json: String = row.get(0).expect("response json");
            payloads.push(RequestLogPayloadRow {
                response_json: serde_json::from_str(&response_json).expect("response json value"),
            });
        }

        payloads
    }

    async fn drop_model_pricing_table(db_path: &Path) {
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        connection
            .execute("DROP TABLE model_pricing", ())
            .await
            .expect("drop model_pricing table");
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

    async fn insert_enabled_oidc_provider(db_path: &Path, provider_key: &str) -> String {
        let oidc_provider_id = format!("oidc-{provider_key}");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");

        connection
            .execute(
                r#"
                INSERT INTO oidc_providers (
                    oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                    client_secret_ref, scopes_json, enabled, created_at, updated_at
                ) VALUES (?1, ?2, 'generic_oidc', ?3, ?4, ?5, '["openid","email"]', 1, unixepoch(), unixepoch())
                "#,
                libsql::params![
                    oidc_provider_id,
                    provider_key,
                    format!("https://{provider_key}.example.com"),
                    format!("{provider_key}-client"),
                    format!("env.{}_CLIENT_SECRET", provider_key.to_ascii_uppercase()),
                ],
            )
            .await
            .expect("insert oidc provider");

        format!("oidc-{provider_key}")
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
            .seed_from_inputs(&seed_providers, &models, &api_keys, &[], &[])
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
                metrics: Arc::new(gateway::observability::GatewayMetrics::default()),
                identity_token_secret: Arc::new("local-dev-identity-secret".to_string()),
            },
            AdminUiConfig::default(),
        );

        (app, raw_key, db_path)
    }

    async fn build_test_app_with_metrics(
        seed_providers: Vec<SeedProvider>,
        models: Vec<SeedModel>,
        provider_registry: gateway_core::ProviderRegistry,
    ) -> (
        Router,
        String,
        PathBuf,
        Arc<crate::observability::GatewayMetrics>,
    ) {
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
            .seed_from_inputs(&seed_providers, &models, &api_keys, &[], &[])
            .await
            .expect("seed data");

        let service = Arc::new(GatewayService::new(
            store.clone(),
            Arc::new(WeightedRoutePlanner::seeded(11)),
        ));
        let metrics = Arc::new(crate::observability::GatewayMetrics::default());

        let app = build_router(
            AppState {
                service,
                store,
                providers: provider_registry,
                metrics: metrics.clone(),
                identity_token_secret: Arc::new("local-dev-identity-secret".to_string()),
            },
            AdminUiConfig::default(),
        );

        (app, raw_key, db_path, metrics)
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
            alias_target_model_key: None,
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
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];

        build_test_app(seed_providers, models, providers).await
    }

    async fn build_default_test_app_with_metrics(
        providers: gateway_core::ProviderRegistry,
    ) -> (
        Router,
        String,
        PathBuf,
        Arc<crate::observability::GatewayMetrics>,
    ) {
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
            alias_target_model_key: None,
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
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];

        build_test_app_with_metrics(seed_providers, models, providers).await
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
                metrics: Arc::new(gateway::observability::GatewayMetrics::default()),
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
        run_migrations_with_options(&options)
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
            alias_target_model_key: None,
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
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
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
            .seed_from_inputs(&seed_providers, &models, &api_keys, &[], &[])
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
                metrics: Arc::new(gateway::observability::GatewayMetrics::default()),
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

    async fn bootstrap_admin_session_cookie(app: &Router, store: &Arc<AnyStore>) -> String {
        ensure_bootstrap_admin(
            store,
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
            .expect("login response");
        assert_eq!(login_response.status(), StatusCode::OK);
        set_cookie_header(&login_response)
    }

    #[allow(clippy::too_many_arguments)]
    fn usage_ledger_record(
        request_id: &str,
        ownership_scope_key: String,
        api_key_id: Uuid,
        user_id: Option<Uuid>,
        team_id: Option<Uuid>,
        model_id: Option<Uuid>,
        upstream_model: &str,
        pricing_status: UsagePricingStatus,
        computed_cost_10000: i64,
        occurred_at: time::OffsetDateTime,
    ) -> UsageLedgerRecord {
        UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key,
            api_key_id,
            user_id,
            team_id,
            actor_user_id: None,
            model_id,
            provider_key: "openai-prod".to_string(),
            upstream_model: upstream_model.to_string(),
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            provider_usage: json!({
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }),
            pricing_status,
            unpriced_reason: match pricing_status {
                UsagePricingStatus::Unpriced => Some("missing_pricing".to_string()),
                _ => None,
            },
            pricing_row_id: None,
            pricing_provider_id: Some("openai".to_string()),
            pricing_model_id: Some(upstream_model.to_string()),
            pricing_source: Some("test".to_string()),
            pricing_source_etag: Some("etag-1".to_string()),
            pricing_source_fetched_at: Some(occurred_at),
            pricing_last_updated: Some("2026-03-15".to_string()),
            input_cost_per_million_tokens: Some(Money4::from_scaled(1_250)),
            output_cost_per_million_tokens: Some(Money4::from_scaled(10_000)),
            computed_cost_usd: Money4::from_scaled(computed_cost_10000),
            occurred_at,
        }
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
        assert_eq!(logs[0].metadata["operation"], "chat_completions");
        assert!(logs[0].metadata.get("fallback_used").is_none());
        assert!(logs[0].metadata.get("attempt_count").is_none());
        assert_eq!(logs[0].resolved_model_key.as_deref(), Some("fast"));

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
    async fn non_stream_success_remains_200_when_post_success_accounting_fails() {
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
        drop_model_pricing_table(&db_path).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("x-request-id", "req-accounting-fail-open")
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
        let payload = read_json(response).await;
        assert_eq!(payload["choices"][0]["message"]["content"], "pong");

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status_code, Some(200));
        assert_eq!(logs[0].provider_key, "openai-prod");

        assert!(load_usage_ledger(&db_path).await.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn stream_success_remains_200_when_post_success_accounting_fails() {
        let (_, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_unused",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "unused"}, "finish_reason":"stop"}]
            })),
            vec![
                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}\n\n".to_string(),
                "data: [DONE]\n\n".to_string(),
            ],
            ProviderCapabilities::new(true, true, false),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;
        drop_model_pricing_table(&db_path).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("x-request-id", "req-stream-accounting-fail-open")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "stream": true,
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
        let transcript = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(transcript.contains("\"content\":\"hi\""));
        assert!(transcript.contains("data: [DONE]"));

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status_code, Some(200));
        assert_eq!(logs[0].metadata["stream"], Value::Bool(true));

        assert!(load_usage_ledger(&db_path).await.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn stream_request_logging_reassembles_split_sse_and_keeps_latest_usage() {
        let (_, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_unused_split",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "unused"}, "finish_reason":"stop"}]
            })),
            vec![
                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"he".to_string(),
                "llo\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\ndata:{\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}\n\n".to_string(),
                "data: [DONE]\n\n".to_string(),
            ],
            ProviderCapabilities::new(true, true, false),
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
                    .header("x-request-id", "req-stream-split-usage")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "stream": true,
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
        let transcript = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(transcript.contains("\"content\":\"hello\""));
        assert!(transcript.contains("data: [DONE]"));

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status_code, Some(200));
        assert_eq!(logs[0].prompt_tokens, Some(11));
        assert_eq!(logs[0].completion_tokens, Some(7));
        assert_eq!(logs[0].total_tokens, Some(18));

        let payloads = load_request_log_payloads(&db_path).await;
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].response_json["usage"]["prompt_tokens"], 11);
        assert_eq!(payloads[0].response_json["usage"]["completion_tokens"], 7);
        assert_eq!(payloads[0].response_json["usage"]["total_tokens"], 18);
        assert_eq!(
            payloads[0].response_json["events"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(
            payloads[0].response_json["events"][0]["choices"][0]["delta"]["content"],
            "hello"
        );

        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].request_id, "req-stream-split-usage");
        assert_eq!(ledgers[0].prompt_tokens, Some(11));
        assert_eq!(ledgers[0].completion_tokens, Some(7));
        assert_eq!(ledgers[0].total_tokens, Some(18));
    }

    #[tokio::test]
    #[serial]
    async fn team_hard_limit_blocks_before_provider_call() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_blocked_team",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "should-not-run"}, "finish_reason":"stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 5, "total_tokens": 10}
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;
        let parsed = parse_gateway_api_key(&raw_key).expect("parse key");
        let store = AnyStore::Libsql(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        );
        let api_key = store
            .get_api_key_by_public_id(&parsed.public_id)
            .await
            .expect("load api key")
            .expect("api key");
        let team_id = api_key.owner_team_id.expect("team owner");
        let model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("model");
        let now = time::OffsetDateTime::now_utc();

        store
            .upsert_active_budget_for_team(
                team_id,
                BudgetCadence::Daily,
                Money4::from_scaled(10_000),
                true,
                "UTC",
                now,
            )
            .await
            .expect("upsert team budget");
        assert!(
            store
                .insert_usage_ledger_if_absent(&usage_ledger_record(
                    "seed-team-over-limit",
                    format!("team:{team_id}:actor:none"),
                    api_key.id,
                    None,
                    Some(team_id),
                    Some(model.id),
                    "gpt-5",
                    UsagePricingStatus::Priced,
                    10_000,
                    now - time::Duration::minutes(1),
                ))
                .await
                .expect("insert seed spend")
        );

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

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let payload: Value = read_json(response).await;
        assert_eq!(payload["error"]["code"], "budget_exceeded");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(load_usage_ledger(&db_path).await.len(), 1);
    }

    #[tokio::test]
    #[serial]
    async fn team_hard_limit_records_budget_error_request_outcome() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_blocked_team_metrics",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "should-not-run"}, "finish_reason":"stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 5, "total_tokens": 10}
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path, metrics) = build_default_test_app_with_metrics(registry).await;
        let parsed = parse_gateway_api_key(&raw_key).expect("parse key");
        let store = AnyStore::Libsql(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        );
        let api_key = store
            .get_api_key_by_public_id(&parsed.public_id)
            .await
            .expect("load api key")
            .expect("api key");
        let team_id = api_key.owner_team_id.expect("team owner");
        let model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("model");
        let now = time::OffsetDateTime::now_utc();

        store
            .upsert_active_budget_for_team(
                team_id,
                BudgetCadence::Daily,
                Money4::from_scaled(10_000),
                true,
                "UTC",
                now,
            )
            .await
            .expect("upsert team budget");
        assert!(
            store
                .insert_usage_ledger_if_absent(&usage_ledger_record(
                    "seed-team-over-limit-metrics",
                    format!("team:{team_id}:actor:none"),
                    api_key.id,
                    None,
                    Some(team_id),
                    Some(model.id),
                    "gpt-5",
                    UsagePricingStatus::Priced,
                    10_000,
                    now - time::Duration::minutes(1),
                ))
                .await
                .expect("insert seed spend")
        );

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

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let payload: Value = read_json(response).await;
        assert_eq!(payload["error"]["code"], "budget_exceeded");
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let snapshot = metrics.test_snapshot();
        assert_eq!(snapshot.requests, 1);
        assert_eq!(snapshot.request_outcomes.get("budget_error"), Some(&1));
    }

    #[tokio::test]
    #[serial]
    async fn user_hard_limit_blocks_before_provider_call() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_blocked_user",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "should-not-run"}, "finish_reason":"stop"}],
                "usage": {"prompt_tokens": 5, "completion_tokens": 5, "total_tokens": 10}
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;
        let user_id = set_api_key_owner_to_user(&db_path, &raw_key, false).await;
        let parsed = parse_gateway_api_key(&raw_key).expect("parse key");
        let store = AnyStore::Libsql(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        );
        let api_key = store
            .get_api_key_by_public_id(&parsed.public_id)
            .await
            .expect("load api key")
            .expect("api key");
        let model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("model");
        let now = time::OffsetDateTime::now_utc();

        store
            .upsert_active_budget_for_user(
                user_id,
                BudgetCadence::Daily,
                Money4::from_scaled(8_000),
                true,
                "UTC",
                now,
            )
            .await
            .expect("upsert user budget");
        assert!(
            store
                .insert_usage_ledger_if_absent(&usage_ledger_record(
                    "seed-user-over-limit",
                    format!("user:{user_id}"),
                    api_key.id,
                    Some(user_id),
                    None,
                    Some(model.id),
                    "gpt-5",
                    UsagePricingStatus::Priced,
                    8_000,
                    now - time::Duration::minutes(1),
                ))
                .await
                .expect("insert seed spend")
        );

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

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let payload: Value = read_json(response).await;
        assert_eq!(payload["error"]["code"], "budget_exceeded");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(load_usage_ledger(&db_path).await.len(), 1);
    }

    #[tokio::test]
    #[serial]
    async fn chat_completions_resolve_alias_models_without_changing_public_model_key() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "model": "gpt-5",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason":"stop"}],
                "usage": {"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18}
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
            secrets: None,
        }];
        let models = vec![
            SeedModel {
                model_key: "fast-v2".to_string(),
                alias_target_model_key: None,
                description: Some("Fast v2".to_string()),
                tags: vec!["fast".to_string()],
                rank: 5,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: Some("fast-v2".to_string()),
                description: Some("Fast alias".to_string()),
                tags: vec!["fast".to_string()],
                rank: 10,
                routes: Vec::new(),
            },
        ];
        let (app, raw_key, db_path) = build_test_app(seed_providers, models, registry).await;

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
        assert_eq!(json["model"], "fast");
        assert_eq!(json["choices"][0]["message"]["content"], "pong");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].model_key, "fast");
        assert_eq!(logs[0].provider_key, "openai-prod");
        assert_eq!(logs[0].resolved_model_key.as_deref(), Some("fast-v2"));
    }

    #[tokio::test]
    #[serial]
    async fn idempotency_header_does_not_enable_retry_fallback() {
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
            alias_target_model_key: None,
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
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                },
                SeedModelRoute {
                    provider_key: "fallback".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
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

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 0);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].provider_key, "primary");
        assert_eq!(logs[0].status_code, Some(503));
        assert_eq!(logs[0].error_code.as_deref(), Some("upstream_http_error"));
        assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
        assert_eq!(logs[0].metadata["operation"], "chat_completions");
        assert!(logs[0].metadata.get("fallback_used").is_none());
        assert!(logs[0].metadata.get("attempt_count").is_none());
        assert_eq!(logs[0].resolved_model_key.as_deref(), Some("fast"));
        assert!(load_usage_ledger(&db_path).await.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn single_route_execution_without_idempotency_key() {
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
            alias_target_model_key: None,
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
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                },
                SeedModelRoute {
                    provider_key: "fallback".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
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
        assert_eq!(logs[0].metadata["operation"], "chat_completions");
        assert!(logs[0].metadata.get("fallback_used").is_none());
        assert!(logs[0].metadata.get("attempt_count").is_none());
        assert!(load_usage_ledger(&db_path).await.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn rejects_requests_when_routes_lack_required_capabilities() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_vision",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "should-not-run"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({
                "base_url": "https://example.invalid/v1",
                "pricing_provider_id": "openai"
            }),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::with_dimensions(
                    true, true, true, true, false, true, true,
                ),
                compatibility: Default::default(),
            }],
        }];

        let (app, raw_key, _) = build_test_app(seed_providers, models, registry).await;

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
                                "content": [
                                    {"type": "text", "text": "describe this"},
                                    {"type": "image_url", "image_url": {"url": "https://example.com/cat.png"}}
                                ]
                            }]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["error"]["code"], "invalid_request");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    #[serial]
    async fn route_selection_prefers_capability_compatible_target() {
        let (primary_calls, primary_provider) = make_chat_provider(
            "primary",
            MockChatResult::Value(json!({
                "id": "chatcmpl_primary",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from-primary"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let (tools_calls, tools_provider) = make_chat_provider(
            "tools",
            MockChatResult::Value(json!({
                "id": "chatcmpl_tools",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from-tools"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );

        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(primary_provider));
        registry.register(Arc::new(tools_provider));

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
                provider_key: "tools".to_string(),
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
            alias_target_model_key: None,
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
                    capabilities: ProviderCapabilities::with_dimensions(
                        true, true, true, false, true, true, true,
                    ),
                    compatibility: Default::default(),
                },
                SeedModelRoute {
                    provider_key: "tools".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                },
            ],
        }];

        let (app, raw_key, _) = build_test_app(seed_providers, models, registry).await;

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
                            "messages": [{"role": "user", "content": "ping"}],
                            "tools": [{
                                "type": "function",
                                "function": {"name": "ping", "parameters": {"type": "object"}}
                            }]
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
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["choices"][0]["message"]["content"], "from-tools");
        assert_eq!(primary_calls.load(Ordering::SeqCst), 0);
        assert_eq!(tools_calls.load(Ordering::SeqCst), 1);
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
        assert_eq!(logs[0].metadata["operation"], "chat_completions");

        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].request_id, request_id);
        assert_eq!(ledgers[0].pricing_status, "usage_missing");
        assert_eq!(ledgers[0].computed_cost_10000, 0);
    }

    #[tokio::test]
    #[serial]
    async fn embeddings_executes_and_records_usage() {
        let (calls, provider) = make_embeddings_provider(
            "openai-prod",
            MockEmbeddingsResult::Value(json!({
                "object": "list",
                "data": [{
                    "object": "embedding",
                    "index": 0,
                    "embedding": [0.1, 0.2, 0.3]
                }],
                "model": "text-embedding-3-small",
                "usage": {"prompt_tokens": 4, "total_tokens": 4}
            })),
            ProviderCapabilities::with_dimensions(false, false, true, false, false, false, false),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "input": "hello"
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

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["model"], "fast");
        assert_eq!(payload["data"][0]["object"], "embedding");

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].provider_key, "openai-prod");
        assert_eq!(logs[0].status_code, Some(200));
        assert_eq!(logs[0].prompt_tokens, Some(4));
        assert_eq!(logs[0].completion_tokens, None);
        assert_eq!(logs[0].total_tokens, Some(4));
        assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
        assert_eq!(logs[0].metadata["operation"], "embeddings");

        let ledgers = load_usage_ledger(&db_path).await;
        assert_eq!(ledgers.len(), 1);
        assert_eq!(ledgers[0].request_id, request_id);
        assert_eq!(ledgers[0].provider_key, "openai-prod");
        assert_eq!(ledgers[0].prompt_tokens, Some(4));
        assert_eq!(ledgers[0].completion_tokens, None);
        assert_eq!(ledgers[0].total_tokens, Some(4));
    }

    #[tokio::test]
    #[serial]
    async fn embeddings_rejects_when_no_route_supports_embeddings() {
        let (calls, provider) = make_embeddings_provider(
            "openai-prod",
            MockEmbeddingsResult::Value(json!({
                "object": "list",
                "data": [],
                "model": "text-embedding-3-small"
            })),
            ProviderCapabilities::all_enabled(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({
                "base_url":"https://example.invalid/v1",
                "pricing_provider_id":"openai"
            }),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "text-embedding-3-small".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::with_dimensions(
                    true, true, false, true, true, true, true,
                ),
                compatibility: Default::default(),
            }],
        }];
        let (app, raw_key, _) = build_test_app(seed_providers, models, registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "input": "hello"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["error"]["code"], "invalid_request");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    #[serial]
    async fn embeddings_provider_invalid_request_is_deterministic() {
        let (calls, provider) = make_embeddings_provider(
            "openai-prod",
            MockEmbeddingsResult::Error(MockError::InvalidRequest(
                "embeddings input was invalid".to_string(),
            )),
            ProviderCapabilities::with_dimensions(false, false, true, false, false, false, false),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path) = build_default_test_app(registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "input": "hello"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["error"]["code"], "invalid_request");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status_code, Some(400));
        assert_eq!(logs[0].metadata["operation"], "embeddings");
    }

    #[tokio::test]
    #[serial]
    async fn embeddings_requires_authorization_header() {
        let (app, _, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "input": "hello"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["error"]["code"], "missing_authorization_header");
    }

    #[tokio::test]
    #[serial]
    async fn embeddings_respects_model_access_controls() {
        let (_, provider) = make_embeddings_provider(
            "openai-prod",
            MockEmbeddingsResult::Value(json!({
                "object": "list",
                "data": [],
                "model": "text-embedding-3-small"
            })),
            ProviderCapabilities::all_enabled(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({
                "base_url":"https://example.invalid/v1",
                "pricing_provider_id":"openai"
            }),
            secrets: None,
        }];
        let models = vec![
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: vec![],
                rank: 10,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "text-embedding-3-small".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
            SeedModel {
                model_key: "restricted".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: vec![],
                rank: 20,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "text-embedding-3-small".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
        ];

        let (app, raw_key, _) = build_test_app(seed_providers, models, registry).await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/embeddings")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "restricted",
                            "input": "hello"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(payload["error"]["code"], "model_not_granted");
    }

    #[tokio::test]
    #[serial]
    async fn openai_compat_streaming_works_through_gateway() {
        let upstream = Router::new().route(
            "/v1/chat/completions",
            post(|| async move {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
                    ))
                    .expect("response")
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            axum::serve(listener, upstream)
                .await
                .expect("serve upstream");
        });

        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
            provider_key: "openai-prod".to_string(),
            base_url: format!("http://{addr}/v1"),
            bearer_token: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 10_000,
        })
        .expect("provider");
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
            .expect("body bytes");
        let transcript = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(transcript.contains("\"content\":\"hello\""));
        assert!(transcript.contains("data: [DONE]"));

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].metadata["stream"], Value::Bool(true));
        assert_eq!(logs[0].metadata["operation"], "chat_completions");
    }

    #[tokio::test]
    #[serial]
    async fn truncated_terminal_stream_frame_records_failure_without_usage_accounting() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "unused-for-stream",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "unused"}, "finish_reason":"stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            })),
            vec![
                "data: {\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":5,\"total_tokens\":9}}\n\n"
                    .to_string(),
                "data: {\"choices\":[{\"delta\":{\"content\":\"oops\"}".to_string(),
            ],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key, db_path, metrics) = build_default_test_app_with_metrics(registry).await;

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
            .expect("body bytes");
        let transcript = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(transcript.contains("\"prompt_tokens\":4"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let logs = load_request_logs(&db_path).await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].provider_key, "openai-prod");
        assert_eq!(logs[0].status_code, Some(502));
        assert_eq!(logs[0].error_code.as_deref(), Some("stream_parse_error"));
        assert_eq!(logs[0].metadata["stream"], Value::Bool(true));
        assert_eq!(logs[0].metadata["operation"], "chat_completions");
        assert!(logs[0].prompt_tokens.is_none());
        assert!(logs[0].completion_tokens.is_none());
        assert!(logs[0].total_tokens.is_none());
        assert!(load_usage_ledger(&db_path).await.is_empty());

        let snapshot = metrics.test_snapshot();
        assert_eq!(snapshot.requests, 1);
        assert_eq!(snapshot.request_outcomes.get("upstream_error"), Some(&1));
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
    async fn admin_api_key_routes_create_update_list_and_revoke_live_gateway_keys() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({
                "base_url": "https://api.openai.com/v1",
                "pricing_provider_id": "openai"
            }),
            secrets: None,
        }];
        let models = vec![
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: None,
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
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
            SeedModel {
                model_key: "reasoning".to_string(),
                alias_target_model_key: None,
                description: Some("Reasoning tier".to_string()),
                tags: vec!["reasoning".to_string()],
                rank: 20,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::<String, Value>::new(),
                    extra_body: Map::<String, Value>::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
        ];
        store
            .seed_from_inputs(&providers, &models, &[], &[], &[])
            .await
            .expect("seed models");

        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("create user");

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/api-keys")
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Production Web",
                            "owner_kind": "user",
                            "owner_user_id": user.user_id,
                            "owner_team_id": null,
                            "model_keys": ["fast"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_body = read_json(create_response).await;
        assert_eq!(create_body["data"]["api_key"]["name"], "Production Web");
        assert_eq!(create_body["data"]["api_key"]["owner_kind"], "user");
        assert_eq!(
            create_body["data"]["api_key"]["owner_id"],
            user.user_id.to_string()
        );
        assert_eq!(create_body["data"]["api_key"]["model_keys"][0], "fast");
        let raw_key = create_body["data"]["raw_key"]
            .as_str()
            .expect("raw key")
            .to_string();
        let api_key_id = create_body["data"]["api_key"]["id"]
            .as_str()
            .expect("api key id")
            .to_string();
        assert!(raw_key.starts_with("gwk_"));

        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/api-keys")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body = read_json(list_response).await;
        assert!(
            list_body["data"]["items"]
                .as_array()
                .expect("api key items")
                .iter()
                .any(|item| {
                    item["id"] == api_key_id
                        && item["owner_name"] == "Member"
                        && item["model_keys"] == json!(["fast"])
                })
        );

        let models_response = app
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
            .expect("response");
        assert_eq!(models_response.status(), StatusCode::OK);
        assert_eq!(
            read_json(models_response).await["data"][0]["id"],
            Value::String("fast".to_string())
        );

        let update_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/admin/api-keys/{api_key_id}"))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "model_keys": ["reasoning"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(update_response.status(), StatusCode::OK);
        let update_body = read_json(update_response).await;
        assert_eq!(
            update_body["data"]["api_key"]["model_keys"],
            json!(["reasoning"])
        );

        let updated_models_response = app
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
            .expect("response");
        assert_eq!(updated_models_response.status(), StatusCode::OK);
        let updated_models_body = read_json(updated_models_response).await;
        let updated_models = updated_models_body["data"]
            .as_array()
            .expect("updated models");
        assert_eq!(updated_models.len(), 1);
        assert_eq!(
            updated_models[0]["id"],
            Value::String("reasoning".to_string())
        );

        let revoke_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/api-keys/{api_key_id}/revoke"))
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(revoke_response.status(), StatusCode::OK);
        let revoke_body = read_json(revoke_response).await;
        assert_eq!(revoke_body["data"]["api_key"]["status"], "revoked");
        assert!(revoke_body["data"]["api_key"]["revoked_at"].is_string());

        let revoked_update = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/admin/api-keys/{api_key_id}"))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "model_keys": ["fast"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(revoked_update.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            read_json(revoked_update).await["error"]["code"],
            "invalid_request"
        );

        let rejected_response = app
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
        assert_eq!(rejected_response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            read_json(rejected_response).await["error"]["code"],
            "api_key_revoked"
        );
    }

    #[tokio::test]
    #[serial]
    async fn admin_api_key_routes_reject_invalid_create_requests() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let providers = vec![SeedProvider {
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
            alias_target_model_key: None,
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
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];
        store
            .seed_from_inputs(&providers, &models, &[], &[], &[])
            .await
            .expect("seed models");

        let invalid_owner_kind = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/api-keys")
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Invalid Owner",
                            "owner_kind": "system",
                            "owner_user_id": null,
                            "owner_team_id": null,
                            "model_keys": ["fast"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_owner_kind.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            read_json(invalid_owner_kind).await["error"]["code"],
            "invalid_request"
        );

        let unknown_model = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/api-keys")
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Unknown Model",
                            "owner_kind": "team",
                            "owner_user_id": null,
                            "owner_team_id": gateway_core::SYSTEM_LEGACY_TEAM_ID,
                            "model_keys": ["missing"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unknown_model.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            read_json(unknown_model).await["error"]["code"],
            "invalid_request"
        );
    }

    #[tokio::test]
    #[serial]
    async fn admin_api_key_routes_return_not_found_for_missing_owner_and_key() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let providers = vec![SeedProvider {
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
            alias_target_model_key: None,
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
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];
        store
            .seed_from_inputs(&providers, &models, &[], &[], &[])
            .await
            .expect("seed models");

        let missing_owner = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/api-keys")
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Missing Owner",
                            "owner_kind": "user",
                            "owner_user_id": Uuid::new_v4(),
                            "owner_team_id": null,
                            "model_keys": ["fast"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_owner.status(), StatusCode::NOT_FOUND);
        assert_eq!(read_json(missing_owner).await["error"]["code"], "not_found");

        let missing_key = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/api-keys/{}/revoke", Uuid::new_v4()))
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_key.status(), StatusCode::NOT_FOUND);
        assert_eq!(read_json(missing_key).await["error"]["code"], "not_found");

        let missing_update_key = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/admin/api-keys/{}", Uuid::new_v4()))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "model_keys": ["fast"]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_update_key.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            read_json(missing_update_key).await["error"]["code"],
            "not_found"
        );
    }

    #[tokio::test]
    #[serial]
    async fn admin_identity_lifecycle_endpoints_enforce_transitions_and_revoke_sessions() {
        let (app, store, db_path) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let bootstrap_cookie = bootstrap_admin_session_cookie(&app, &store).await;
        let oidc_provider_id = insert_enabled_oidc_provider(&db_path, "corp").await;
        let now = time::OffsetDateTime::now_utc();

        let active_user = store
            .create_identity_user(
                "Active User",
                "active@example.com",
                "active@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("active user");
        let other_admin = store
            .create_identity_user(
                "Other Admin",
                "other-admin@example.com",
                "other-admin@example.com",
                GlobalRole::PlatformAdmin,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("other admin");
        let target_admin = store
            .create_identity_user(
                "Target Admin",
                "target-admin@example.com",
                "target-admin@example.com",
                GlobalRole::PlatformAdmin,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("target admin");

        let password_hash = hash_gateway_key_secret("admin-pass").expect("hash password");
        store
            .store_user_password(target_admin.user_id, &password_hash, now)
            .await
            .expect("store admin password");

        let invalid_auth_mode_switch = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!(
                        "/api/v1/admin/identity/users/{}",
                        active_user.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "global_role": "user",
                            "auth_mode": "oidc",
                            "oidc_provider_key": "corp",
                            "team_id": null,
                            "team_role": null
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_auth_mode_switch.status(), StatusCode::BAD_REQUEST);

        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login/password")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "target-admin@example.com",
                            "password": "admin-pass"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("login response");
        assert_eq!(login_response.status(), StatusCode::OK);
        let target_admin_cookie = set_cookie_header(&login_response);

        let deactivate = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/users/{}/deactivate",
                        target_admin.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(deactivate.status(), StatusCode::OK);
        assert_eq!(read_json(deactivate).await["data"]["status"], "ok");
        assert_eq!(
            store
                .get_user_by_id(target_admin.user_id)
                .await
                .expect("load target admin")
                .expect("target admin exists")
                .status,
            gateway_core::UserStatus::Disabled
        );

        let stale_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/auth/session")
                    .header("cookie", &target_admin_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(stale_session.status(), StatusCode::OK);
        assert_eq!(read_json(stale_session).await["data"], Value::Null);

        let reactivate = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/users/{}/reactivate",
                        target_admin.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(reactivate.status(), StatusCode::OK);
        assert_eq!(read_json(reactivate).await["data"]["status"], "ok");
        assert_eq!(
            store
                .get_user_by_id(target_admin.user_id)
                .await
                .expect("reload target admin")
                .expect("target admin")
                .status,
            gateway_core::UserStatus::Active
        );

        let invited_user = store
            .create_identity_user(
                "Invited User",
                "invited@example.com",
                "invited@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Invited,
            )
            .await
            .expect("invited user");
        let reset_password = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/users/{}/reset-onboarding",
                        invited_user.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(reset_password.status(), StatusCode::OK);
        let reset_password_body = read_json(reset_password).await;
        assert_eq!(reset_password_body["data"]["kind"], "password_invite");
        assert!(
            reset_password_body["data"]["invite_url"]
                .as_str()
                .expect("invite url")
                .contains("/admin/invite/")
        );

        let oidc_user = store
            .create_identity_user(
                "OIDC User",
                "oidc@example.com",
                "oidc@example.com",
                GlobalRole::User,
                AuthMode::Oidc,
                gateway_core::UserStatus::Disabled,
            )
            .await
            .expect("oidc user");
        store
            .set_user_oidc_link(oidc_user.user_id, &oidc_provider_id, now)
            .await
            .expect("set oidc link");
        store
            .create_user_oidc_auth(
                oidc_user.user_id,
                &oidc_provider_id,
                "mock:corp:oidc@example.com",
                Some("oidc@example.com"),
                now,
            )
            .await
            .expect("create oidc auth");

        let reset_oidc = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/users/{}/reset-onboarding",
                        oidc_user.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(reset_oidc.status(), StatusCode::OK);
        let reset_oidc_body = read_json(reset_oidc).await;
        assert_eq!(reset_oidc_body["data"]["kind"], "oidc_sign_in");
        assert!(
            reset_oidc_body["data"]["sign_in_url"]
                .as_str()
                .expect("sign in url")
                .contains("provider_key=corp")
        );
        assert!(
            store
                .get_user_oidc_auth_by_user(oidc_user.user_id, &oidc_provider_id)
                .await
                .expect("oidc auth lookup")
                .is_none()
        );
        assert_eq!(
            store
                .get_user_by_id(oidc_user.user_id)
                .await
                .expect("reload oidc user")
                .expect("oidc user")
                .status,
            gateway_core::UserStatus::Invited
        );

        let disabled_user = store
            .create_identity_user(
                "Disabled User",
                "disabled@example.com",
                "disabled@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Disabled,
            )
            .await
            .expect("disabled user");
        let reactivate_missing_credentials = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/users/{}/reactivate",
                        disabled_user.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(reactivate_missing_credentials.status(), StatusCode::OK);
        assert_eq!(
            store
                .get_user_by_id(disabled_user.user_id)
                .await
                .expect("reload disabled user")
                .expect("disabled user")
                .status,
            gateway_core::UserStatus::Invited
        );

        let _ = other_admin;
    }

    #[tokio::test]
    #[serial]
    async fn admin_identity_team_member_workflows_transfer_remove_and_block_owner() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let bootstrap_cookie = bootstrap_admin_session_cookie(&app, &store).await;
        let source_team = store
            .create_team("source", "Source")
            .await
            .expect("source team");
        let destination_team = store
            .create_team("destination", "Destination")
            .await
            .expect("destination team");
        let member = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("member");
        store
            .assign_team_membership(member.user_id, source_team.team_id, MembershipRole::Member)
            .await
            .expect("assign member");

        let transfer = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/teams/{}/members/{}/transfer",
                        source_team.team_id, member.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "destination_team_id": destination_team.team_id,
                            "destination_role": "admin"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(transfer.status(), StatusCode::OK);
        assert_eq!(read_json(transfer).await["data"]["status"], "ok");
        let transferred_membership = store
            .get_team_membership_for_user(member.user_id)
            .await
            .expect("membership lookup")
            .expect("membership exists");
        assert_eq!(transferred_membership.team_id, destination_team.team_id);
        assert_eq!(transferred_membership.role, MembershipRole::Admin);

        let remove = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/admin/identity/teams/{}/members/{}",
                        destination_team.team_id, member.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(remove.status(), StatusCode::OK);
        assert_eq!(read_json(remove).await["data"]["status"], "ok");
        assert!(
            store
                .get_team_membership_for_user(member.user_id)
                .await
                .expect("membership lookup")
                .is_none()
        );

        let owner = store
            .create_identity_user(
                "Owner",
                "owner@example.com",
                "owner@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("owner");
        store
            .assign_team_membership(owner.user_id, source_team.team_id, MembershipRole::Owner)
            .await
            .expect("assign owner");

        let blocked_transfer = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/admin/identity/teams/{}/members/{}/transfer",
                        source_team.team_id, owner.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "destination_team_id": destination_team.team_id,
                            "destination_role": "member"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(blocked_transfer.status(), StatusCode::BAD_REQUEST);

        let blocked_remove = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/admin/identity/teams/{}/members/{}",
                        source_team.team_id, owner.user_id
                    ))
                    .header("cookie", &bootstrap_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(blocked_remove.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[serial]
    async fn admin_spend_routes_require_authenticated_session() {
        let (app, _, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let owner_id = Uuid::new_v4();
        let payload = json!({
            "cadence": "daily",
            "amount_usd": "10.0000",
            "hard_limit": true,
            "timezone": "UTC"
        })
        .to_string();

        let report = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/report")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(report.status(), StatusCode::UNAUTHORIZED);

        let budgets = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/budgets")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(budgets.status(), StatusCode::UNAUTHORIZED);

        let alert_history = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/budget-alerts")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(alert_history.status(), StatusCode::UNAUTHORIZED);

        let user_upsert = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/admin/spend/budgets/users/{owner_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(payload.clone()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(user_upsert.status(), StatusCode::UNAUTHORIZED);

        let user_deactivate = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/admin/spend/budgets/users/{owner_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(user_deactivate.status(), StatusCode::UNAUTHORIZED);

        let team_upsert = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/admin/spend/budgets/teams/{owner_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(team_upsert.status(), StatusCode::UNAUTHORIZED);

        let leaderboard = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/observability/leaderboard")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(leaderboard.status(), StatusCode::UNAUTHORIZED);

        let team_deactivate = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/admin/spend/budgets/teams/{owner_id}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(team_deactivate.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn admin_request_log_detail_returns_404_for_unknown_id() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/admin/observability/request-logs/{}",
                        Uuid::new_v4()
                    ))
                    .header("cookie", session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = read_json(response).await;
        assert_eq!(body["error"]["code"], "not_found");
    }

    #[tokio::test]
    #[serial]
    async fn admin_spend_report_returns_live_ledger_aggregates_and_honors_filters() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let providers = vec![SeedProvider {
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
            alias_target_model_key: None,
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
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];
        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("load api key")
            .expect("api key");
        let model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("model");
        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("create user");
        let team = store
            .create_team("platform", "Platform")
            .await
            .expect("create team");

        let now = time::OffsetDateTime::now_utc();
        for event in [
            usage_ledger_record(
                "req-user-priced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
                None,
                Some(model.id),
                "gpt-5",
                UsagePricingStatus::Priced,
                10_000,
                now - time::Duration::hours(2),
            ),
            usage_ledger_record(
                "req-user-unpriced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
                None,
                Some(model.id),
                "gpt-5",
                UsagePricingStatus::Unpriced,
                0,
                now - time::Duration::hours(2),
            ),
            usage_ledger_record(
                "req-team-legacy",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::LegacyEstimated,
                20_000,
                now - time::Duration::hours(26),
            ),
            usage_ledger_record(
                "req-team-usage-missing",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::UsageMissing,
                0,
                now - time::Duration::hours(26),
            ),
        ] {
            assert!(
                store
                    .insert_usage_ledger_if_absent(&event)
                    .await
                    .expect("insert usage ledger")
            );
        }

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/report?days=7&owner_kind=all")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = read_json(response).await;
        assert_eq!(body["data"]["window_days"], 7);
        assert_eq!(body["data"]["owner_kind"], "all");
        assert_eq!(body["data"]["daily"].as_array().expect("daily").len(), 7);
        assert_eq!(body["data"]["totals"]["priced_cost_usd_10000"], 30_000);
        assert_eq!(body["data"]["totals"]["priced_request_count"], 2);
        assert_eq!(body["data"]["totals"]["unpriced_request_count"], 1);
        assert_eq!(body["data"]["totals"]["usage_missing_request_count"], 1);
        let owners = body["data"]["owners"].as_array().expect("owners");
        assert_eq!(owners.len(), 2);
        assert!(owners.iter().any(|owner| owner["owner_kind"] == "user"));
        assert!(owners.iter().any(|owner| owner["owner_kind"] == "team"));
        let models = body["data"]["models"].as_array().expect("models");
        assert!(
            models
                .iter()
                .any(|item| item["model_key"] == "fast" && item["unpriced_request_count"] == 1)
        );
        assert!(
            models
                .iter()
                .any(|item| item["model_key"] == "claude-3-5-sonnet"
                    && item["usage_missing_request_count"] == 1)
        );

        let user_only = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/report?days=7&owner_kind=user")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(user_only.status(), StatusCode::OK);
        let user_only_body = read_json(user_only).await;
        assert_eq!(user_only_body["data"]["owner_kind"], "user");
        assert_eq!(
            user_only_body["data"]["totals"]["priced_cost_usd_10000"],
            10_000
        );
        assert_eq!(
            user_only_body["data"]["owners"]
                .as_array()
                .expect("owners")
                .len(),
            1
        );
        assert_eq!(
            user_only_body["data"]["owners"][0]["owner_kind"],
            Value::String("user".to_string())
        );

        let invalid_days = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/report?days=14")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_days.status(), StatusCode::BAD_REQUEST);

        let invalid_owner_kind = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/report?owner_kind=provider")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_owner_kind.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[serial]
    async fn admin_usage_leaderboard_returns_ranked_users_and_bucketed_series() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({
                "base_url": "https://api.openai.com/v1",
                "pricing_provider_id": "openai"
            }),
            secrets: None,
        }];
        let models = vec![
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: None,
                description: Some("Fast tier".to_string()),
                tags: vec!["fast".to_string()],
                rank: 10,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5-mini".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::<String, Value>::new(),
                    extra_body: Map::<String, Value>::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
            SeedModel {
                model_key: "reasoning".to_string(),
                alias_target_model_key: None,
                description: Some("Reasoning tier".to_string()),
                tags: vec!["reasoning".to_string()],
                rank: 20,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::<String, Value>::new(),
                    extra_body: Map::<String, Value>::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
        ];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string(), "reasoning".to_string()],
        }];
        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("load api key")
            .expect("api key");
        let fast_model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("fast model");
        let reasoning_model = store
            .get_model_by_key("reasoning")
            .await
            .expect("load model")
            .expect("reasoning model");

        let users = [
            store
                .create_identity_user(
                    "Ada",
                    "ada@example.com",
                    "ada@example.com",
                    GlobalRole::User,
                    AuthMode::Password,
                    gateway_core::UserStatus::Active,
                )
                .await
                .expect("create ada"),
            store
                .create_identity_user(
                    "Ben",
                    "ben@example.com",
                    "ben@example.com",
                    GlobalRole::User,
                    AuthMode::Password,
                    gateway_core::UserStatus::Active,
                )
                .await
                .expect("create ben"),
            store
                .create_identity_user(
                    "Cleo",
                    "cleo@example.com",
                    "cleo@example.com",
                    GlobalRole::User,
                    AuthMode::Password,
                    gateway_core::UserStatus::Active,
                )
                .await
                .expect("create cleo"),
            store
                .create_identity_user(
                    "Dina",
                    "dina@example.com",
                    "dina@example.com",
                    GlobalRole::User,
                    AuthMode::Password,
                    gateway_core::UserStatus::Active,
                )
                .await
                .expect("create dina"),
            store
                .create_identity_user(
                    "Eli",
                    "eli@example.com",
                    "eli@example.com",
                    GlobalRole::User,
                    AuthMode::Password,
                    gateway_core::UserStatus::Active,
                )
                .await
                .expect("create eli"),
            store
                .create_identity_user(
                    "Fay",
                    "fay@example.com",
                    "fay@example.com",
                    GlobalRole::User,
                    AuthMode::Password,
                    gateway_core::UserStatus::Active,
                )
                .await
                .expect("create fay"),
        ];

        let now = time::OffsetDateTime::now_utc();
        let user_costs = [70_000, 61_000, 52_000, 43_000, 34_000, 25_000];
        for (index, user) in users.iter().enumerate() {
            assert!(
                store
                    .insert_usage_ledger_if_absent(&usage_ledger_record(
                        &format!("leaderboard-fast-{index}"),
                        format!("user:{}", user.user_id),
                        api_key.id,
                        Some(user.user_id),
                        None,
                        Some(fast_model.id),
                        "gpt-5-mini",
                        UsagePricingStatus::Priced,
                        user_costs[index],
                        now - time::Duration::hours((index as i64 + 1) * 6),
                    ))
                    .await
                    .expect("insert fast ledger")
            );
            assert!(
                store
                    .insert_usage_ledger_if_absent(&usage_ledger_record(
                        &format!("leaderboard-reasoning-{index}"),
                        format!("user:{}", user.user_id),
                        api_key.id,
                        Some(user.user_id),
                        None,
                        Some(reasoning_model.id),
                        "gpt-5",
                        UsagePricingStatus::Priced,
                        1_000,
                        now - time::Duration::hours((index as i64 + 1) * 6 + 1),
                    ))
                    .await
                    .expect("insert reasoning ledger")
            );
            assert!(
                store
                    .insert_usage_ledger_if_absent(&usage_ledger_record(
                        &format!("leaderboard-unpriced-{index}"),
                        format!("user:{}", user.user_id),
                        api_key.id,
                        Some(user.user_id),
                        None,
                        Some(fast_model.id),
                        "gpt-5-mini",
                        UsagePricingStatus::Unpriced,
                        0,
                        now - time::Duration::hours((index as i64 + 1) * 6 + 2),
                    ))
                    .await
                    .expect("insert unpriced ledger")
            );
        }

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/observability/leaderboard?range=7d")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = read_json(response).await;
        assert_eq!(body["data"]["range"], "7d");
        assert_eq!(body["data"]["bucket_hours"], 12);
        assert_eq!(
            body["data"]["chart_users"]
                .as_array()
                .expect("chart users")
                .len(),
            5
        );
        assert_eq!(body["data"]["series"].as_array().expect("series").len(), 14);
        assert_eq!(
            body["data"]["leaders"].as_array().expect("leaders").len(),
            6
        );
        assert_eq!(body["data"]["leaders"][0]["user_name"], "Ada");
        assert_eq!(body["data"]["leaders"][0]["total_spend_usd_10000"], 71_000);
        assert_eq!(body["data"]["leaders"][0]["most_used_model"], "fast");
        assert_eq!(body["data"]["leaders"][0]["total_requests"], 3);
        assert_eq!(body["data"]["leaders"][5]["user_name"], "Fay");
        assert_eq!(
            body["data"]["series"][0]["values"]
                .as_array()
                .expect("series values")
                .len(),
            5
        );

        let longer_range = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/observability/leaderboard?range=31d")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(longer_range.status(), StatusCode::OK);
        let longer_body = read_json(longer_range).await;
        assert_eq!(
            longer_body["data"]["series"]
                .as_array()
                .expect("series")
                .len(),
            62
        );

        let invalid_range = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/observability/leaderboard?range=14d")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_range.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[serial]
    async fn admin_spend_budget_endpoints_validate_and_support_upsert_and_deactivate() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("create user");
        let team = store
            .create_team("platform", "Platform")
            .await
            .expect("create team");

        let invalid_cadence = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/admin/spend/budgets/users/{}",
                        user.user_id
                    ))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "cadence": "quarterly",
                            "amount_usd": "25.0000",
                            "hard_limit": true,
                            "timezone": "UTC"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_cadence.status(), StatusCode::BAD_REQUEST);

        let upsert_user = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/admin/spend/budgets/users/{}",
                        user.user_id
                    ))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "cadence": "monthly",
                            "amount_usd": "25.0000",
                            "hard_limit": true,
                            "timezone": "UTC"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(upsert_user.status(), StatusCode::OK);
        let upsert_user_body = read_json(upsert_user).await;
        assert_eq!(upsert_user_body["data"]["owner_kind"], "user");
        assert_eq!(
            upsert_user_body["data"]["owner_id"],
            user.user_id.to_string()
        );
        assert_eq!(upsert_user_body["data"]["budget"]["amount_usd"], "25.0000");
        assert_eq!(upsert_user_body["data"]["budget"]["cadence"], "monthly");
        assert_eq!(upsert_user_body["data"]["budget"]["hard_limit"], true);

        let upsert_team = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/admin/spend/budgets/teams/{}",
                        team.team_id
                    ))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "cadence": "weekly",
                            "amount_usd": "100.0000",
                            "hard_limit": false,
                            "timezone": "UTC"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(upsert_team.status(), StatusCode::OK);
        let upsert_team_body = read_json(upsert_team).await;
        assert_eq!(upsert_team_body["data"]["owner_kind"], "team");
        assert_eq!(
            upsert_team_body["data"]["owner_id"],
            team.team_id.to_string()
        );
        assert_eq!(upsert_team_body["data"]["budget"]["cadence"], "weekly");
        assert_eq!(upsert_team_body["data"]["budget"]["hard_limit"], false);

        let list_budgets = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/budgets")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(list_budgets.status(), StatusCode::OK);
        let list_body = read_json(list_budgets).await;
        let user_rows = list_body["data"]["users"].as_array().expect("users");
        assert!(user_rows.iter().any(|row| {
            row["user_id"] == user.user_id.to_string()
                && row["budget"]["amount_usd_10000"] == 250_000
        }));
        let team_rows = list_body["data"]["teams"].as_array().expect("teams");
        assert!(team_rows.iter().any(|row| {
            row["team_id"] == team.team_id.to_string()
                && row["budget"]["amount_usd_10000"] == 1_000_000
        }));

        let remove_user = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/admin/spend/budgets/users/{}",
                        user.user_id
                    ))
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(remove_user.status(), StatusCode::OK);
        assert_eq!(read_json(remove_user).await["data"]["deactivated"], true);

        let remove_user_again = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/admin/spend/budgets/users/{}",
                        user.user_id
                    ))
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(remove_user_again.status(), StatusCode::OK);
        assert_eq!(
            read_json(remove_user_again).await["data"]["deactivated"],
            false
        );

        let unknown_team = Uuid::new_v4();
        let upsert_unknown_team = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/admin/spend/budgets/teams/{unknown_team}"))
                    .header("cookie", &session_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "cadence": "daily",
                            "amount_usd": "10.0000",
                            "hard_limit": true,
                            "timezone": "UTC"
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(upsert_unknown_team.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[serial]
    async fn admin_budget_alert_history_lists_authenticated_alerts_with_filters() {
        let (app, store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;
        let session_cookie = bootstrap_admin_session_cookie(&app, &store).await;

        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                gateway_core::UserStatus::Active,
            )
            .await
            .expect("create user");
        let now = time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .expect("zero nanos");
        let alert = gateway_core::BudgetAlertRecord {
            budget_alert_id: Uuid::new_v4(),
            ownership_scope_key: format!("user:{}", user.user_id),
            owner_kind: gateway_core::ApiKeyOwnerKind::User,
            owner_id: user.user_id,
            owner_name: user.name.clone(),
            budget_id: Uuid::new_v4(),
            cadence: BudgetCadence::Monthly,
            threshold_bps: 2_000,
            window_start: now - time::Duration::days(17),
            window_end: now + time::Duration::days(14),
            spend_before_usd: Money4::from_scaled(7_500_000),
            spend_after_usd: Money4::from_scaled(8_200_000),
            remaining_budget_usd: Money4::from_scaled(1_800_000),
            created_at: now,
            updated_at: now,
        };
        let delivery = BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: Uuid::new_v4(),
            budget_alert_id: alert.budget_alert_id,
            channel: BudgetAlertChannel::Email,
            delivery_status: BudgetAlertDeliveryStatus::Pending,
            recipient: Some(user.email.clone()),
            provider_message_id: None,
            failure_reason: None,
            queued_at: now,
            last_attempted_at: None,
            sent_at: None,
            updated_at: now,
        };
        assert!(
            store
                .create_budget_alert_with_deliveries(&alert, &[delivery])
                .await
                .expect("insert alert history row")
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/spend/budget-alerts?owner_kind=user&channel=email&status=pending")
                    .header("cookie", &session_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let body = read_json(response).await;
        assert_eq!(body["data"]["page"], 1);
        assert_eq!(body["data"]["page_size"], 25);
        assert_eq!(body["data"]["total"], 1);
        assert_eq!(
            body["data"]["items"][0]["budget_alert_id"],
            alert.budget_alert_id.to_string()
        );
        assert_eq!(body["data"]["items"][0]["owner_kind"], "user");
        assert_eq!(
            body["data"]["items"][0]["owner_id"],
            user.user_id.to_string()
        );
        assert_eq!(body["data"]["items"][0]["owner_name"], "Member");
        assert_eq!(body["data"]["items"][0]["cadence"], "monthly");
        assert_eq!(body["data"]["items"][0]["threshold_bps"], 2_000);
        assert_eq!(
            body["data"]["items"][0]["recipient_summary"],
            "member@example.com"
        );
        assert_eq!(body["data"]["items"][0]["delivery_status"], "pending");
        assert_eq!(body["data"]["items"][0]["channel"], "email");
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
    async fn logout_revokes_only_current_session_and_clears_cookie() {
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

        let login = |app: Router| async move {
            app.oneshot(
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
            .expect("login response")
        };

        let first_login = login(app.clone()).await;
        assert_eq!(first_login.status(), StatusCode::OK);
        let first_cookie = set_cookie_header(&first_login);
        let second_login = login(app.clone()).await;
        assert_eq!(second_login.status(), StatusCode::OK);
        let second_cookie = set_cookie_header(&second_login);

        let logout = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header("cookie", &first_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("logout response");
        assert_eq!(logout.status(), StatusCode::OK);
        let clear_cookie = set_cookie_header(&logout);
        assert!(clear_cookie.starts_with("ogw_session=;"));
        assert!(clear_cookie.contains("Max-Age=0"));
        assert_eq!(read_json(logout).await["data"]["status"], "ok");

        let stale_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/auth/session")
                    .header("cookie", &first_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("stale session response");
        assert_eq!(stale_session.status(), StatusCode::OK);
        assert_eq!(read_json(stale_session).await["data"], Value::Null);

        let protected_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/api-keys")
                    .header("cookie", &first_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("protected response");
        assert_eq!(protected_response.status(), StatusCode::UNAUTHORIZED);

        let active_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/auth/session")
                    .header("cookie", &second_cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("active session response");
        assert_eq!(active_session.status(), StatusCode::OK);
        assert_eq!(
            read_json(active_session).await["data"]["user"]["email"],
            "admin@local"
        );
    }

    #[tokio::test]
    #[serial]
    async fn logout_is_idempotent_for_missing_and_invalid_session_cookies() {
        let (app, _store, _) =
            build_default_test_app_with_store(gateway_core::ProviderRegistry::new()).await;

        for cookie in [None, Some("ogw_session=not-a-signed-token")] {
            let mut request = Request::builder().method("POST").uri("/api/v1/auth/logout");
            if let Some(cookie) = cookie {
                request = request.header("cookie", cookie);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).expect("request"))
                .await
                .expect("response");

            assert_eq!(response.status(), StatusCode::OK);
            let clear_cookie = set_cookie_header(&response);
            assert!(clear_cookie.starts_with("ogw_session=;"));
            assert!(clear_cookie.contains("Max-Age=0"));
            assert_eq!(read_json(response).await["data"]["status"], "ok");
        }
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
                gateway_core::UserStatus::Active,
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

    #[test]
    fn seed_local_demo_accepts_libsql_and_loopback_postgres_only() {
        assert!(
            ensure_seed_local_demo_targets_local_database(&StoreConnectionOptions::Libsql {
                path: PathBuf::from("./gateway.db"),
            })
            .is_ok()
        );

        assert!(
            ensure_seed_local_demo_targets_local_database(&StoreConnectionOptions::Postgres {
                url: "postgres://postgres:postgres@localhost/gateway".to_string(),
                max_connections: 4,
            })
            .is_ok()
        );

        assert!(
            ensure_seed_local_demo_targets_local_database(&StoreConnectionOptions::Postgres {
                url: "postgres://postgres:postgres@127.0.0.1/gateway".to_string(),
                max_connections: 4,
            })
            .is_ok()
        );

        assert!(
            ensure_seed_local_demo_targets_local_database(&StoreConnectionOptions::Postgres {
                url: "postgres://postgres:postgres@[::1]/gateway".to_string(),
                max_connections: 4,
            })
            .is_ok()
        );

        let error =
            ensure_seed_local_demo_targets_local_database(&StoreConnectionOptions::Postgres {
                url: "postgres://postgres:postgres@db.internal/gateway".to_string(),
                max_connections: 4,
            })
            .expect_err("non-local postgres should be rejected");
        assert!(
            error
                .to_string()
                .contains("`seed-local-demo` only supports local databases")
        );
    }

    #[test]
    fn local_gateway_config_does_not_seed_platform_admin_users() {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gateway.yaml");
        let config = GatewayConfig::from_path(&config_path).expect("gateway.yaml should parse");
        let seeded_users = config.seed_users().expect("seed users");

        assert!(
            seeded_users
                .iter()
                .all(|user| user.global_role != GlobalRole::PlatformAdmin)
        );
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
            ProviderCapabilities::new(true, false, false),
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
