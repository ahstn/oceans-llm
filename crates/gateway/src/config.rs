use std::{collections::BTreeMap, env, fs, path::Path};

use anyhow::{Context, bail};
use gateway_core::{
    AuthMode, BudgetCadence, GlobalRole, MembershipRole, Money4, ProviderCapabilities,
    SYSTEM_LEGACY_TEAM_KEY, SeedApiKey, SeedBudget, SeedModel, SeedModelRoute, SeedProvider,
    SeedTeam, SeedUser, SeedUserMembership, parse_gateway_api_key,
};
use gateway_providers::{OpenAiCompatConfig, VertexAuthConfig, VertexProviderConfig};
use gateway_service::{hash_gateway_key_secret, is_supported_pricing_provider_id};
use gateway_store::StoreConnectionOptions;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GatewayConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub budget_alerts: BudgetAlertConfig,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    #[serde(default)]
    pub teams: Vec<TeamConfig>,
    #[serde(default)]
    pub users: Vec<UserConfig>,
}

impl GatewayConfig {
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed reading config file `{}`", path.display()))?;
        let parsed: Self = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed parsing yaml config `{}`", path.display()))?;

        parsed
            .validate()
            .with_context(|| format!("invalid gateway configuration `{}`", path.display()))?;

        Ok(parsed)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let _ = self.database.connection_options()?;
        self.budget_alerts.validate()?;

        let provider_by_id = self
            .providers
            .iter()
            .map(|provider| (provider.id().to_string(), provider))
            .collect::<BTreeMap<_, _>>();
        let model_by_id = self
            .models
            .iter()
            .map(|model| (model.id.as_str(), model))
            .collect::<BTreeMap<_, _>>();

        for provider in &self.providers {
            match provider {
                ProviderConfig::OpenAiCompat(provider) => {
                    if provider.id.trim().is_empty() {
                        bail!("openai_compat provider id cannot be empty");
                    }
                    if provider.base_url.trim().is_empty() {
                        bail!(
                            "openai_compat provider `{}` base_url cannot be empty",
                            provider.id
                        );
                    }
                    if provider.pricing_provider_id.trim().is_empty() {
                        bail!(
                            "openai_compat provider `{}` pricing_provider_id cannot be empty",
                            provider.id
                        );
                    }
                    if !is_supported_pricing_provider_id(&provider.pricing_provider_id) {
                        bail!(
                            "openai_compat provider `{}` pricing_provider_id `{}` is not supported",
                            provider.id,
                            provider.pricing_provider_id
                        );
                    }
                }
                ProviderConfig::GcpVertex(provider) => {
                    if provider.id.trim().is_empty() {
                        bail!("gcp_vertex provider id cannot be empty");
                    }
                    if provider.project_id.trim().is_empty() {
                        bail!(
                            "gcp_vertex provider `{}` project_id cannot be empty",
                            provider.id
                        );
                    }
                    if provider.location.trim().is_empty() {
                        bail!(
                            "gcp_vertex provider `{}` location cannot be empty",
                            provider.id
                        );
                    }
                    if provider.api_host.trim().is_empty() {
                        bail!(
                            "gcp_vertex provider `{}` api_host cannot be empty",
                            provider.id
                        );
                    }

                    match &provider.auth {
                        GcpVertexAuthConfig::Adc => {}
                        GcpVertexAuthConfig::ServiceAccount { credentials_path } => {
                            if credentials_path.trim().is_empty() {
                                bail!(
                                    "gcp_vertex provider `{}` service_account.credentials_path cannot be empty",
                                    provider.id
                                );
                            }
                        }
                        GcpVertexAuthConfig::Bearer { token } => {
                            if token.trim().is_empty() {
                                bail!(
                                    "gcp_vertex provider `{}` bearer.token cannot be empty",
                                    provider.id
                                );
                            }
                        }
                    }
                }
            }
        }

        for model in &self.models {
            let has_alias = model.alias_of.is_some();
            let has_routes = !model.routes.is_empty();

            match (has_alias, has_routes) {
                (true, true) => bail!(
                    "model `{}` cannot define both alias_of and routes",
                    model.id
                ),
                (false, false) => bail!(
                    "model `{}` must define either alias_of or at least one route",
                    model.id
                ),
                _ => {}
            }

            if let Some(alias_target) = model.alias_of.as_deref() {
                if alias_target == model.id {
                    bail!("model `{}` cannot alias itself", model.id);
                }
                if !model_by_id.contains_key(alias_target) {
                    bail!(
                        "model `{}` aliases unknown model `{alias_target}`",
                        model.id
                    );
                }
            }

            for route in &model.routes {
                if let Some(provider) = provider_by_id.get(route.provider.as_str())
                    && matches!(provider, ProviderConfig::GcpVertex(_))
                {
                    validate_vertex_upstream_model_format(&route.upstream_model)?;
                }
            }
        }

        for model in &self.models {
            let mut seen = std::collections::BTreeSet::new();
            let mut current = model;

            while let Some(alias_target) = current.alias_of.as_deref() {
                if !seen.insert(current.id.as_str()) {
                    bail!("model alias cycle detected starting at `{}`", model.id);
                }

                current = model_by_id.get(alias_target).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "model `{}` aliases unknown model `{alias_target}`",
                        model.id
                    )
                })?;
            }
        }

        let mut team_keys = std::collections::BTreeSet::new();
        for team in &self.teams {
            let team_key = normalize_config_team_key(&team.key)?;
            if team.name.trim().is_empty() {
                bail!("team `{team_key}` name cannot be empty");
            }
            if team_key == SYSTEM_LEGACY_TEAM_KEY {
                bail!("team key `{SYSTEM_LEGACY_TEAM_KEY}` is reserved");
            }
            if !team_keys.insert(team_key.clone()) {
                bail!("duplicate team key `{team_key}`");
            }
            if let Some(budget) = &team.budget {
                budget.validate(&format!("team `{team_key}` budget"))?;
            }
        }

        let reserved_bootstrap_admin_email =
            normalize_config_email(&self.auth.bootstrap_admin.email)
                .context("bootstrap_admin.email must be a valid email address")?;

        let mut user_emails = std::collections::BTreeSet::new();
        for user in &self.users {
            if user.name.trim().is_empty() {
                bail!("user name cannot be empty");
            }
            let email_normalized = normalize_config_email(&user.email)?;
            if email_normalized == reserved_bootstrap_admin_email {
                bail!(
                    "user email `{reserved_bootstrap_admin_email}` is reserved for bootstrap admin"
                );
            }
            if !user_emails.insert(email_normalized.clone()) {
                bail!("duplicate user email `{email_normalized}`");
            }
            if user.auth_mode == AuthMode::Oauth {
                bail!("users config does not support auth_mode `oauth`");
            }
            match user.auth_mode {
                AuthMode::Oidc => {
                    let Some(provider_key) = user.oidc_provider_key.as_deref() else {
                        bail!(
                            "user `{}` with auth_mode `oidc` requires oidc_provider_key",
                            user.email
                        );
                    };
                    normalize_config_oidc_provider_key(provider_key)
                        .with_context(|| format!("user `{}` oidc_provider_key", user.email))?;
                }
                AuthMode::Password => {
                    if user.oidc_provider_key.is_some() {
                        bail!(
                            "user `{}` cannot set oidc_provider_key unless auth_mode is `oidc`",
                            user.email
                        );
                    }
                }
                AuthMode::Oauth => unreachable!(),
            }
            if let Some(membership) = &user.membership {
                let membership_team = normalize_config_team_key(&membership.team)
                    .with_context(|| format!("user `{}` membership team", user.email))?;
                if !team_keys.contains(&membership_team) {
                    bail!(
                        "user `{}` references unknown team `{}`",
                        user.email,
                        membership_team
                    );
                }
                if membership.role == MembershipRole::Owner {
                    bail!("user `{}` cannot seed membership role `owner`", user.email);
                }
            }
            if let Some(budget) = &user.budget {
                budget.validate(&format!("user `{}` budget", user.email))?;
            }
        }

        Ok(())
    }

    pub fn seed_providers(&self) -> anyhow::Result<Vec<SeedProvider>> {
        let mut providers = Vec::new();

        for provider in &self.providers {
            match provider {
                ProviderConfig::OpenAiCompat(provider) => {
                    if let Some(auth) = &provider.auth
                        && let Some(token) = &auth.token
                    {
                        validate_env_reference_if_needed(token)?;
                    }

                    let config = json!({
                        "base_url": provider.base_url,
                        "pricing_provider_id": provider.pricing_provider_id,
                        "default_headers": provider.default_headers,
                        "timeouts": provider.timeouts,
                    });

                    let secrets = provider.auth.as_ref().map(|auth| {
                        json!({
                            "kind": auth.kind,
                            "token": auth.token,
                        })
                    });

                    providers.push(SeedProvider {
                        provider_key: provider.id.clone(),
                        provider_type: "openai_compat".to_string(),
                        config,
                        secrets,
                    });
                }
                ProviderConfig::GcpVertex(provider) => {
                    if let GcpVertexAuthConfig::Bearer { token } = &provider.auth {
                        validate_env_reference_if_needed(token)?;
                    }
                    if let GcpVertexAuthConfig::ServiceAccount { credentials_path } = &provider.auth
                    {
                        validate_env_reference_if_needed(credentials_path)?;
                    }

                    let config = json!({
                        "project_id": provider.project_id,
                        "location": provider.location,
                        "api_host": provider.api_host,
                        "default_headers": provider.default_headers,
                        "timeouts": provider.timeouts,
                    });

                    let secrets = Some(match &provider.auth {
                        GcpVertexAuthConfig::Adc => json!({"mode": "adc"}),
                        GcpVertexAuthConfig::ServiceAccount { credentials_path } => {
                            json!({"mode": "service_account", "credentials_path": credentials_path})
                        }
                        GcpVertexAuthConfig::Bearer { token } => {
                            json!({"mode": "bearer", "token": token})
                        }
                    });

                    providers.push(SeedProvider {
                        provider_key: provider.id.clone(),
                        provider_type: "gcp_vertex".to_string(),
                        config,
                        secrets,
                    });
                }
            }
        }

        Ok(providers)
    }

    pub fn seed_models(&self) -> anyhow::Result<Vec<SeedModel>> {
        let models = self
            .models
            .iter()
            .map(|model| SeedModel {
                model_key: model.id.clone(),
                alias_target_model_key: model.alias_of.clone(),
                description: model.description.clone(),
                tags: model.tags.clone(),
                rank: model.rank,
                routes: model
                    .routes
                    .iter()
                    .map(|route| SeedModelRoute {
                        provider_key: route.provider.clone(),
                        upstream_model: route.upstream_model.clone(),
                        priority: route.priority,
                        weight: route.weight,
                        enabled: route.enabled,
                        extra_headers: route
                            .extra_headers
                            .iter()
                            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                            .collect::<Map<String, Value>>(),
                        extra_body: route.extra_body.clone(),
                        capabilities: route.capabilities.clone().into_capabilities(),
                    })
                    .collect(),
            })
            .collect();

        Ok(models)
    }

    pub fn seed_api_keys(&self) -> anyhow::Result<Vec<SeedApiKey>> {
        let mut api_keys = Vec::new();

        for seed_key in &self.auth.seed_api_keys {
            let raw_value = resolve_env_reference(&seed_key.value)?;
            let parsed = parse_gateway_api_key(&raw_value).with_context(|| {
                format!("invalid gateway key configured for `{}`", seed_key.name)
            })?;

            let secret_hash = hash_gateway_key_secret(&parsed.secret).with_context(|| {
                format!(
                    "failed hashing configured gateway key for `{}`",
                    seed_key.name
                )
            })?;

            api_keys.push(SeedApiKey {
                name: seed_key.name.clone(),
                public_id: parsed.public_id,
                secret_hash,
                allowed_models: seed_key.allowed_models.clone(),
            });
        }

        Ok(api_keys)
    }

    pub fn seed_teams(&self) -> anyhow::Result<Vec<SeedTeam>> {
        self.teams
            .iter()
            .map(|team| {
                Ok(SeedTeam {
                    team_key: normalize_config_team_key(&team.key)?,
                    team_name: team.name.trim().to_string(),
                    budget: team
                        .budget
                        .as_ref()
                        .map(BudgetConfig::seed_budget)
                        .transpose()?,
                })
            })
            .collect()
    }

    pub fn seed_users(&self) -> anyhow::Result<Vec<SeedUser>> {
        self.users
            .iter()
            .map(|user| {
                Ok(SeedUser {
                    name: user.name.trim().to_string(),
                    email: user.email.trim().to_string(),
                    email_normalized: normalize_config_email(&user.email)?,
                    global_role: user.global_role,
                    auth_mode: user.auth_mode,
                    request_logging_enabled: user.request_logging_enabled,
                    oidc_provider_key: user
                        .oidc_provider_key
                        .as_deref()
                        .map(normalize_config_oidc_provider_key)
                        .transpose()?,
                    membership: match user.membership.as_ref() {
                        Some(membership) => Some(SeedUserMembership {
                            team_key: normalize_config_team_key(&membership.team)?,
                            role: membership.role,
                        }),
                        None => None,
                    },
                    budget: user
                        .budget
                        .as_ref()
                        .map(BudgetConfig::seed_budget)
                        .transpose()?,
                })
            })
            .collect()
    }

    pub fn openai_compat_provider_configs(&self) -> anyhow::Result<Vec<OpenAiCompatConfig>> {
        let mut configs = Vec::new();

        for provider in &self.providers {
            let ProviderConfig::OpenAiCompat(provider) = provider else {
                continue;
            };

            let mut config =
                OpenAiCompatConfig::new(provider.id.clone(), provider.base_url.clone());
            config.default_headers = provider.default_headers.clone();
            config.request_timeout_ms = provider
                .timeouts
                .as_ref()
                .map(|timeouts| timeouts.total_ms)
                .unwrap_or(120_000);

            if let Some(auth) = &provider.auth
                && let Some(token) = &auth.token
            {
                config.bearer_token = Some(resolve_secret_reference(token)?);
            }

            configs.push(config);
        }

        Ok(configs)
    }

    pub fn vertex_provider_configs(&self) -> anyhow::Result<Vec<VertexProviderConfig>> {
        let mut configs = Vec::new();

        for provider in &self.providers {
            let ProviderConfig::GcpVertex(provider) = provider else {
                continue;
            };

            let auth = match &provider.auth {
                GcpVertexAuthConfig::Adc => VertexAuthConfig::Adc,
                GcpVertexAuthConfig::ServiceAccount { credentials_path } => {
                    VertexAuthConfig::ServiceAccount {
                        credentials_path: resolve_path_reference(credentials_path)?.into(),
                    }
                }
                GcpVertexAuthConfig::Bearer { token } => VertexAuthConfig::Bearer {
                    token: resolve_secret_reference(token)?,
                },
            };

            configs.push(VertexProviderConfig {
                provider_key: provider.id.clone(),
                project_id: provider.project_id.clone(),
                location: provider.location.clone(),
                api_host: provider.api_host.clone(),
                auth,
                default_headers: provider.default_headers.clone(),
                request_timeout_ms: provider
                    .timeouts
                    .as_ref()
                    .map(|timeouts| timeouts.total_ms)
                    .unwrap_or(120_000),
            });
        }

        Ok(configs)
    }

    pub fn database_options(&self) -> anyhow::Result<StoreConnectionOptions> {
        self.database.connection_options()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub otel_endpoint: Option<String>,
    #[serde(default)]
    pub otel_metrics_endpoint: Option<String>,
    #[serde(default = "default_otel_export_interval_secs")]
    pub otel_export_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            log_format: default_log_format(),
            otel_endpoint: None,
            otel_metrics_endpoint: None,
            otel_export_interval_secs: default_otel_export_interval_secs(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_postgres_max_connections")]
    pub max_connections: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            kind: Some(default_db_kind()),
            path: Some(default_db_path()),
            url: None,
            max_connections: default_postgres_max_connections(),
        }
    }
}

impl DatabaseConfig {
    pub fn connection_options(&self) -> anyhow::Result<StoreConnectionOptions> {
        let kind = self.kind.as_deref().unwrap_or_else(|| {
            if self.url.is_some() {
                "postgres"
            } else {
                "libsql"
            }
        });

        match kind {
            "libsql" => {
                let path = self.path.as_ref().cloned().unwrap_or_else(default_db_path);
                Ok(StoreConnectionOptions::Libsql { path: path.into() })
            }
            "postgres" => {
                let raw_url = self.url.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("database.url is required when database.kind=postgres")
                })?;
                let url = resolve_secret_reference(raw_url)?;
                Ok(StoreConnectionOptions::Postgres {
                    url,
                    max_connections: self.max_connections,
                })
            }
            other => bail!("unsupported database.kind `{other}`; use libsql or postgres"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub seed_api_keys: Vec<SeedApiKeyConfig>,
    #[serde(default)]
    pub bootstrap_admin: BootstrapAdminConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BudgetAlertConfig {
    #[serde(default)]
    pub email: BudgetAlertEmailConfig,
}

impl BudgetAlertConfig {
    fn validate(&self) -> anyhow::Result<()> {
        self.email.validate()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BudgetAlertEmailConfig {
    #[serde(default = "default_budget_alert_from_email")]
    pub from_email: String,
    #[serde(default)]
    pub from_name: Option<String>,
    #[serde(default = "default_budget_alert_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_budget_alert_batch_size")]
    pub batch_size: u32,
    #[serde(default)]
    pub transport: BudgetAlertEmailTransportConfig,
}

impl Default for BudgetAlertEmailConfig {
    fn default() -> Self {
        Self {
            from_email: default_budget_alert_from_email(),
            from_name: None,
            poll_interval_secs: default_budget_alert_poll_interval_secs(),
            batch_size: default_budget_alert_batch_size(),
            transport: BudgetAlertEmailTransportConfig::default(),
        }
    }
}

impl BudgetAlertEmailConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.from_email.trim().is_empty() {
            bail!("budget_alerts.email.from_email cannot be empty");
        }
        if self.poll_interval_secs == 0 {
            bail!("budget_alerts.email.poll_interval_secs must be > 0");
        }
        if self.batch_size == 0 {
            bail!("budget_alerts.email.batch_size must be > 0");
        }
        match &self.transport {
            BudgetAlertEmailTransportConfig::Sink => {}
            BudgetAlertEmailTransportConfig::Smtp(smtp) => {
                if smtp.host.trim().is_empty() {
                    bail!("budget_alerts.email.transport.smtp.host cannot be empty");
                }
                if smtp.username.is_some() != smtp.password.is_some() {
                    bail!(
                        "budget_alerts.email.transport.smtp.username and password must be set together"
                    );
                }
                if let Some(username) = &smtp.username {
                    validate_env_reference_if_needed(username)?;
                }
                if let Some(password) = &smtp.password {
                    let _ = resolve_secret_reference(password)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetAlertEmailTransportConfig {
    #[default]
    Sink,
    Smtp(SmtpBudgetAlertEmailTransportConfig),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpBudgetAlertEmailTransportConfig {
    pub host: String,
    #[serde(default = "default_budget_alert_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_budget_alert_smtp_starttls")]
    pub starttls: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapAdminConfig {
    #[serde(default = "default_bootstrap_admin_enabled")]
    pub enabled: bool,
    #[serde(default = "default_bootstrap_admin_email")]
    pub email: String,
    #[serde(default = "default_bootstrap_admin_password")]
    pub password: String,
    #[serde(default)]
    pub require_password_change: bool,
}

impl Default for BootstrapAdminConfig {
    fn default() -> Self {
        Self {
            enabled: default_bootstrap_admin_enabled(),
            email: default_bootstrap_admin_email(),
            password: default_bootstrap_admin_password(),
            require_password_change: false,
        }
    }
}

impl BootstrapAdminConfig {
    pub fn resolved_password(&self) -> anyhow::Result<String> {
        resolve_secret_reference(&self.password)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedApiKeyConfig {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub allowed_models: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamConfig {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub budget: Option<BudgetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserConfig {
    pub name: String,
    pub email: String,
    pub auth_mode: AuthMode,
    #[serde(default = "default_user_global_role")]
    pub global_role: GlobalRole,
    #[serde(default = "default_request_logging_enabled")]
    pub request_logging_enabled: bool,
    #[serde(default)]
    pub oidc_provider_key: Option<String>,
    #[serde(default)]
    pub membership: Option<UserMembershipConfig>,
    #[serde(default)]
    pub budget: Option<BudgetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserMembershipConfig {
    pub team: String,
    #[serde(default = "default_membership_role")]
    pub role: MembershipRole,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BudgetConfig {
    pub cadence: BudgetCadence,
    pub amount_usd: String,
    #[serde(default = "default_enabled")]
    pub hard_limit: bool,
    #[serde(default = "default_budget_timezone")]
    pub timezone: String,
}

impl BudgetConfig {
    fn validate(&self, label: &str) -> anyhow::Result<()> {
        if self.timezone.trim().is_empty() {
            bail!("{label} timezone cannot be empty");
        }
        let amount = Money4::from_decimal_str(&self.amount_usd)
            .map_err(|error| anyhow::anyhow!("{label} amount_usd is invalid: {error}"))?;
        if amount.is_negative() {
            bail!("{label} amount_usd cannot be negative");
        }
        Ok(())
    }

    fn seed_budget(&self) -> anyhow::Result<SeedBudget> {
        let amount_usd = Money4::from_decimal_str(&self.amount_usd).map_err(|error| {
            anyhow::anyhow!("invalid amount_usd `{}`: {error}", self.amount_usd)
        })?;
        Ok(SeedBudget {
            cadence: self.cadence,
            amount_usd,
            hard_limit: self.hard_limit,
            timezone: self.timezone.trim().to_string(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    #[serde(rename = "openai_compat")]
    OpenAiCompat(OpenAiCompatProviderConfig),
    GcpVertex(GcpVertexProviderConfig),
}

impl ProviderConfig {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::OpenAiCompat(provider) => &provider.id,
            Self::GcpVertex(provider) => &provider.id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiCompatProviderConfig {
    pub id: String,
    pub base_url: String,
    #[serde(default)]
    pub pricing_provider_id: String,
    #[serde(default)]
    pub auth: Option<OpenAiCompatAuthConfig>,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiCompatAuthConfig {
    pub kind: String,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GcpVertexProviderConfig {
    pub id: String,
    pub project_id: String,
    #[serde(default = "default_vertex_location")]
    pub location: String,
    #[serde(default = "default_vertex_api_host")]
    pub api_host: String,
    pub auth: GcpVertexAuthConfig,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum GcpVertexAuthConfig {
    Adc,
    ServiceAccount { credentials_path: String },
    Bearer { token: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderTimeouts {
    #[serde(default = "default_provider_timeout_ms")]
    pub total_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    #[serde(default)]
    pub alias_of: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_model_rank")]
    pub rank: i32,
    #[serde(default)]
    pub routes: Vec<ModelRouteConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelRouteConfig {
    pub provider: String,
    pub upstream_model: String,
    #[serde(default = "default_route_priority")]
    pub priority: i32,
    #[serde(default = "default_route_weight")]
    pub weight: f64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    #[serde(default)]
    pub capabilities: RouteCapabilitiesConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteCapabilitiesConfig {
    #[serde(default = "default_enabled")]
    pub chat_completions: bool,
    #[serde(default = "default_enabled")]
    pub stream: bool,
    #[serde(default = "default_enabled")]
    pub embeddings: bool,
    #[serde(default = "default_enabled")]
    pub tools: bool,
    #[serde(default = "default_enabled")]
    pub vision: bool,
    #[serde(default = "default_enabled")]
    pub json_schema: bool,
    #[serde(default = "default_enabled")]
    pub developer_role: bool,
}

impl RouteCapabilitiesConfig {
    fn into_capabilities(self) -> ProviderCapabilities {
        ProviderCapabilities::with_dimensions(
            self.chat_completions,
            self.stream,
            self.embeddings,
            self.tools,
            self.vision,
            self.json_schema,
            self.developer_role,
        )
    }
}

impl Default for RouteCapabilitiesConfig {
    fn default() -> Self {
        Self {
            chat_completions: true,
            stream: true,
            embeddings: true,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        }
    }
}

fn resolve_env_reference(value: &str) -> anyhow::Result<String> {
    let env_var_name = value
        .strip_prefix("env.")
        .ok_or_else(|| anyhow::anyhow!("expected env.* secret reference, got `{value}`"))?;

    let resolved = env::var(env_var_name)
        .with_context(|| format!("required environment variable `{env_var_name}` is not set"))?;

    Ok(resolved)
}

pub(crate) fn resolve_secret_reference(value: &str) -> anyhow::Result<String> {
    if value.starts_with("env.") {
        resolve_env_reference(value)
    } else if let Some(literal) = value.strip_prefix("literal.") {
        Ok(literal.to_string())
    } else {
        bail!("unsupported secret reference `{value}`; use env.* or literal.* for this phase")
    }
}

fn resolve_path_reference(value: &str) -> anyhow::Result<String> {
    if value.starts_with("env.") {
        resolve_env_reference(value)
    } else if let Some(literal) = value.strip_prefix("literal.") {
        Ok(literal.to_string())
    } else {
        Ok(value.to_string())
    }
}

fn validate_env_reference_if_needed(value: &str) -> anyhow::Result<()> {
    if value.starts_with("env.") {
        let _ = resolve_env_reference(value)?;
    }
    Ok(())
}

fn validate_vertex_upstream_model_format(value: &str) -> anyhow::Result<()> {
    let mut parts = value.splitn(2, '/');
    let publisher = parts.next().unwrap_or_default();
    let model_id = parts.next().unwrap_or_default();
    if publisher.is_empty() || model_id.is_empty() {
        bail!(
            "gcp_vertex routes require upstream_model in <publisher>/<model_id> format, got `{value}`"
        );
    }
    Ok(())
}

fn normalize_config_email(email: &str) -> anyhow::Result<String> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.is_empty() || !normalized.contains('@') {
        bail!("email must be a valid email address");
    }
    Ok(normalized)
}

fn normalize_config_team_key(team_key: &str) -> anyhow::Result<String> {
    let normalized = team_key.trim().to_string();
    if normalized.is_empty() {
        bail!("team key cannot be empty");
    }
    Ok(normalized)
}

fn normalize_config_oidc_provider_key(provider_key: &str) -> anyhow::Result<String> {
    let normalized = provider_key.trim().to_string();
    if normalized.is_empty() {
        bail!("cannot be empty");
    }
    Ok(normalized)
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

const fn default_otel_export_interval_secs() -> u64 {
    30
}

fn default_db_path() -> String {
    "./gateway.db".to_string()
}

fn default_db_kind() -> String {
    "libsql".to_string()
}

const fn default_postgres_max_connections() -> u32 {
    10
}

const fn default_provider_timeout_ms() -> u64 {
    120_000
}

const fn default_model_rank() -> i32 {
    100
}

const fn default_route_priority() -> i32 {
    100
}

const fn default_route_weight() -> f64 {
    1.0
}

const fn default_enabled() -> bool {
    true
}

const fn default_request_logging_enabled() -> bool {
    true
}

const fn default_user_global_role() -> GlobalRole {
    GlobalRole::User
}

const fn default_membership_role() -> MembershipRole {
    MembershipRole::Member
}

fn default_budget_timezone() -> String {
    "UTC".to_string()
}

const fn default_bootstrap_admin_enabled() -> bool {
    true
}

fn default_bootstrap_admin_email() -> String {
    "admin@local".to_string()
}

fn default_bootstrap_admin_password() -> String {
    "literal.admin".to_string()
}

fn default_budget_alert_from_email() -> String {
    "alerts@local".to_string()
}

const fn default_budget_alert_poll_interval_secs() -> u64 {
    30
}

const fn default_budget_alert_batch_size() -> u32 {
    25
}

const fn default_budget_alert_smtp_port() -> u16 {
    587
}

const fn default_budget_alert_smtp_starttls() -> bool {
    true
}

fn default_vertex_location() -> String {
    "global".to_string()
}

fn default_vertex_api_host() -> String {
    "aiplatform.googleapis.com".to_string()
}

#[cfg(test)]
mod tests {
    use std::{env, path::Path};

    use gateway_core::{AuthMode, BudgetCadence, GlobalRole, MembershipRole, Money4};
    use tempfile::tempdir;

    use super::GatewayConfig;

    fn write_config(path: &Path, yaml: &str) {
        std::fs::write(path, yaml).expect("write config");
    }

    #[test]
    fn accepts_valid_vertex_auth_modes() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: vertex-adc
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: adc
  - id: vertex-sa
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: service_account
      credentials_path: /tmp/sa.json
  - id: vertex-bearer
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: bearer
      token: literal.test-token
models:
  - id: fast
    routes:
      - provider: vertex-adc
        upstream_model: google/gemini-2.0-flash
"#,
        );

        GatewayConfig::from_path(&config_path).expect("config should parse");
    }

    #[test]
    fn accepts_alias_backed_model_config() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: fast
    alias_of: fast-v2
"#,
        );

        GatewayConfig::from_path(&config_path).expect("config should parse");
    }

    #[test]
    fn rejects_model_with_alias_and_routes() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast
    alias_of: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("cannot define both alias_of and routes"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_model_without_alias_or_routes() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
models:
  - id: fast
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("must define either alias_of or at least one route"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_alias_to_unknown_model() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
models:
  - id: fast
    alias_of: missing
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("aliases unknown model `missing`"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_self_alias() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
models:
  - id: fast
    alias_of: fast
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("cannot alias itself"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_alias_cycles() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
models:
  - id: fast
    alias_of: fast-v2
  - id: fast-v2
    alias_of: fast
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("model alias cycle detected"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_missing_vertex_service_account_path() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: vertex-sa
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: service_account
"#,
        );

        GatewayConfig::from_path(&config_path).expect_err("config should fail");
    }

    #[test]
    fn rejects_missing_vertex_bearer_token() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: vertex-bearer
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: bearer
"#,
        );

        GatewayConfig::from_path(&config_path).expect_err("config should fail");
    }

    #[test]
    fn rejects_invalid_vertex_upstream_model_route_format() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: vertex
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: adc
models:
  - id: fast
    routes:
      - provider: vertex
        upstream_model: gemini-2.0-flash
"#,
        );

        GatewayConfig::from_path(&config_path).expect_err("config should fail");
    }

    #[test]
    fn accepts_openai_compat_with_supported_pricing_provider() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
"#,
        );

        GatewayConfig::from_path(&config_path).expect("config should parse");
    }

    #[test]
    fn rejects_openai_compat_without_pricing_provider_id() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("pricing_provider_id cannot be empty"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_openai_compat_with_unsupported_pricing_provider_id() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: azure
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("pricing_provider_id `azure` is not supported"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn parses_route_capability_metadata_into_seed_models() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
        capabilities:
          stream: false
          tools: false
          vision: false
"#,
        );

        let config = GatewayConfig::from_path(&config_path).expect("config should parse");
        let seeded = config.seed_models().expect("seed models");

        let route = &seeded[0].routes[0];
        assert!(route.capabilities.chat_completions);
        assert!(!route.capabilities.stream);
        assert!(route.capabilities.embeddings);
        assert!(!route.capabilities.tools);
        assert!(!route.capabilities.vision);
        assert!(route.capabilities.json_schema);
        assert!(route.capabilities.developer_role);
    }

    #[test]
    fn production_config_requires_bootstrap_password_change() {
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gateway.prod.yaml");
        unsafe {
            env::set_var(
                "POSTGRES_URL",
                "postgres://postgres:postgres@localhost/test",
            );
        }

        let config = GatewayConfig::from_path(&config_path).expect("prod config should parse");

        assert!(config.auth.bootstrap_admin.enabled);
        assert_eq!(config.auth.bootstrap_admin.email, "admin@local");
        assert!(config.auth.bootstrap_admin.require_password_change);
    }

    #[test]
    fn parses_declarative_teams_and_users_into_seed_inputs() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
teams:
  - key: " platform "
    name: Platform
    budget:
      cadence: monthly
      amount_usd: "250.0000"
      hard_limit: true
      timezone: UTC
users:
  - name: Member
    email: " Member@Example.com "
    auth_mode: oidc
    global_role: platform_admin
    request_logging_enabled: false
    oidc_provider_key: " okta "
    membership:
      team: " platform "
      role: admin
    budget:
      cadence: weekly
      amount_usd: "75.0000"
      hard_limit: false
      timezone: Europe/London
"#,
        );

        let config = GatewayConfig::from_path(&config_path).expect("config should parse");
        let teams = config.seed_teams().expect("seed teams");
        let users = config.seed_users().expect("seed users");

        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].team_key, "platform");
        assert_eq!(teams[0].team_name, "Platform");
        let team_budget = teams[0].budget.as_ref().expect("team budget");
        assert_eq!(team_budget.cadence, BudgetCadence::Monthly);
        assert_eq!(team_budget.amount_usd, Money4::from_scaled(2_500_000));
        assert!(team_budget.hard_limit);
        assert_eq!(team_budget.timezone, "UTC");

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].email_normalized, "member@example.com");
        assert_eq!(users[0].auth_mode, AuthMode::Oidc);
        assert_eq!(users[0].global_role, GlobalRole::PlatformAdmin);
        assert!(!users[0].request_logging_enabled);
        assert_eq!(users[0].oidc_provider_key.as_deref(), Some("okta"));
        let membership = users[0].membership.as_ref().expect("membership");
        assert_eq!(membership.team_key, "platform");
        assert_eq!(membership.role, MembershipRole::Admin);
        let user_budget = users[0].budget.as_ref().expect("user budget");
        assert_eq!(user_budget.cadence, BudgetCadence::Weekly);
        assert_eq!(user_budget.amount_usd, Money4::from_scaled(750_000));
        assert!(!user_budget.hard_limit);
        assert_eq!(user_budget.timezone, "Europe/London");
    }

    #[test]
    fn rejects_reserved_declarative_team_keys() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
teams:
  - key: " system-legacy "
    name: Reserved
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("team key `system-legacy` is reserved"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_duplicate_declarative_team_keys_after_normalization() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
teams:
  - key: platform
    name: Platform
  - key: " platform "
    name: Duplicate
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("duplicate team key `platform`"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_invalid_declarative_user_memberships() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
teams:
  - key: platform
    name: Platform
users:
  - name: Member
    email: member@example.com
    auth_mode: password
    membership:
      team: platform
      role: owner
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("cannot seed membership role `owner`"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn rejects_user_email_matching_configured_bootstrap_admin_email() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            r#"
auth:
  bootstrap_admin:
    enabled: true
    email: "ops-admin@example.com"
    password: "literal.secret"
users:
  - name: Ops Admin
    email: " ops-admin@example.com "
    auth_mode: password
"#,
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text
                .contains("user email `ops-admin@example.com` is reserved for bootstrap admin"),
            "unexpected error: {error_text}"
        );
    }
}
