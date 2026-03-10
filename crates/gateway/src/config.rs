use std::{collections::BTreeMap, env, fs, path::Path};

use anyhow::{Context, bail};
use gateway_core::{SeedApiKey, SeedModel, SeedModelRoute, SeedProvider, parse_gateway_api_key};
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
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
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

        let provider_by_id = self
            .providers
            .iter()
            .map(|provider| (provider.id().to_string(), provider))
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
            for route in &model.routes {
                if let Some(provider) = provider_by_id.get(route.provider.as_str())
                    && matches!(provider, ProviderConfig::GcpVertex(_))
                {
                    validate_vertex_upstream_model_format(&route.upstream_model)?;
                }
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            log_format: default_log_format(),
            otel_endpoint: None,
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
                let path = self
                    .path
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(default_db_path);
                Ok(StoreConnectionOptions::Libsql { path: path.into() })
            }
            "postgres" => {
                let raw_url = self
                    .url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("database.url is required when database.kind=postgres"))?;
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
}

fn resolve_env_reference(value: &str) -> anyhow::Result<String> {
    let env_var_name = value
        .strip_prefix("env.")
        .ok_or_else(|| anyhow::anyhow!("expected env.* secret reference, got `{value}`"))?;

    let resolved = env::var(env_var_name)
        .with_context(|| format!("required environment variable `{env_var_name}` is not set"))?;

    Ok(resolved)
}

fn resolve_secret_reference(value: &str) -> anyhow::Result<String> {
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

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
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

const fn default_bootstrap_admin_enabled() -> bool {
    true
}

fn default_bootstrap_admin_email() -> String {
    "admin@local".to_string()
}

fn default_bootstrap_admin_password() -> String {
    "literal.admin".to_string()
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
    fn production_config_requires_bootstrap_password_change() {
        let config_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gateway.prod.yaml");
        unsafe {
            env::set_var("POSTGRES_URL", "postgres://postgres:postgres@localhost/test");
        }

        let config = GatewayConfig::from_path(&config_path).expect("prod config should parse");

        assert!(config.auth.bootstrap_admin.enabled);
        assert_eq!(config.auth.bootstrap_admin.email, "admin@local");
        assert!(config.auth.bootstrap_admin.require_password_change);
    }
}
