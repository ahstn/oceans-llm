use std::collections::BTreeMap;

use anyhow::{Context, bail};
use gateway_providers::{BearerAuthHeader, BedrockEndpointKind, BedrockProviderConfig};
use gateway_service::{ProviderIconKey, is_supported_pricing_provider_id};
use serde::{Deserialize, Serialize};

use super::references::{resolve_copilot_private_key, resolve_secret_reference};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    #[serde(rename = "openai_compat")]
    OpenAiCompat(OpenAiCompatProviderConfig),
    #[serde(rename = "gcp_cloud_run_openai_compat")]
    GcpCloudRunOpenAiCompat(GcpCloudRunOpenAiCompatProviderConfig),
    #[serde(rename = "anthropic_compat")]
    AnthropicCompat(AnthropicCompatProviderConfig),
    GcpVertex(GcpVertexProviderConfig),
    AwsBedrock(AwsBedrockProviderConfig),
    #[serde(rename = "github_copilot")]
    GitHubCopilot(GitHubCopilotProviderConfig),
}
impl ProviderConfig {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::OpenAiCompat(provider) => &provider.id,
            Self::GcpCloudRunOpenAiCompat(provider) => &provider.id,
            Self::AnthropicCompat(provider) => &provider.id,
            Self::GcpVertex(provider) => &provider.id,
            Self::AwsBedrock(provider) => &provider.id,
            Self::GitHubCopilot(provider) => &provider.id,
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
    #[serde(default)]
    pub display: Option<ProviderDisplayConfig>,
    #[serde(default)]
    pub batch: Option<OpenAiBatchProviderConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAiBatchProviderConfig {
    pub dialect: OpenAiBatchDialectConfig,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiBatchDialectConfig {
    OpenAi,
    OpenRouter,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiCompatAuthConfig {
    pub kind: String,
    #[serde(default)]
    pub token: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicCompatProviderConfig {
    pub id: String,
    pub base_url: String,
    #[serde(default)]
    pub pricing_provider_id: String,
    #[serde(default)]
    pub auth: Option<AnthropicCompatAuthConfig>,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
    #[serde(default)]
    pub display: Option<ProviderDisplayConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicCompatAuthConfig {
    #[serde(default)]
    pub kind: AnthropicCompatAuthKindConfig,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicCompatAuthKindConfig {
    #[default]
    XApiKey,
    Bearer,
}

impl AnthropicCompatAuthKindConfig {
    pub(super) const fn into_provider_kind(self) -> gateway_providers::AnthropicCompatAuthKind {
        match self {
            Self::XApiKey => gateway_providers::AnthropicCompatAuthKind::XApiKey,
            Self::Bearer => gateway_providers::AnthropicCompatAuthKind::Bearer,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GcpCloudRunOpenAiCompatProviderConfig {
    pub id: String,
    pub base_url: String,
    #[serde(default)]
    pub audience: Option<String>,
    pub pricing_provider_id: String,
    pub auth: GcpCloudRunOpenAiCompatAuthConfig,
    #[serde(default)]
    pub auth_header: GcpCloudRunOpenAiCompatAuthHeaderConfig,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
    #[serde(default)]
    pub display: Option<ProviderDisplayConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GcpCloudRunOpenAiCompatAuthConfig {
    Adc,
    ServiceAccount { credentials_path: String },
    Bearer { token: String },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GcpCloudRunOpenAiCompatAuthHeaderConfig {
    #[default]
    Authorization,
    XServerlessAuthorization,
}

impl GcpCloudRunOpenAiCompatAuthHeaderConfig {
    pub(super) const fn into_provider_header(self) -> BearerAuthHeader {
        match self {
            Self::Authorization => BearerAuthHeader::Authorization,
            Self::XServerlessAuthorization => BearerAuthHeader::XServerlessAuthorization,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderDisplayConfig {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub icon_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GcpVertexProviderConfig {
    pub id: String,
    pub project_id: String,
    #[serde(default = "default_vertex_location")]
    pub location: String,
    /// Vertex API host. Defaults to the host that serves `location`
    /// (`aiplatform.googleapis.com` for `global`, `{region}-aiplatform.googleapis.com` otherwise).
    #[serde(default)]
    pub api_host: Option<String>,
    pub auth: GcpVertexAuthConfig,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
    #[serde(default)]
    pub display: Option<ProviderDisplayConfig>,
    #[serde(default)]
    pub batch: Option<GcpVertexBatchConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GcpVertexBatchConfig {
    #[serde(default)]
    pub bigquery_project_id: Option<String>,
    pub dataset: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum GcpVertexAuthConfig {
    Adc,
    ServiceAccount { credentials_path: String },
    Bearer { token: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AwsBedrockProviderConfig {
    pub id: String,
    pub region: String,
    pub endpoint_kind: BedrockEndpointKind,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default)]
    pub auth: AwsBedrockAuthConfig,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
    #[serde(default)]
    pub display: Option<ProviderDisplayConfig>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubCopilotProviderConfig {
    pub id: String,
    #[serde(default = "default_copilot_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub github_api_url: Option<String>,
    #[serde(default)]
    pub pricing_provider_id: Option<String>,
    pub auth: GitHubCopilotAuthConfig,
    #[serde(default = "default_copilot_editor_version")]
    pub editor_version: String,
    #[serde(default = "default_copilot_integration_id")]
    pub integration_id: String,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub timeouts: Option<ProviderTimeouts>,
    #[serde(default)]
    pub display: Option<ProviderDisplayConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitHubCopilotAuthConfig {
    #[serde(rename = "github_app")]
    GitHubApp {
        app_id: u64,
        private_key: String,
        installation_id: u64,
        repository_id: u64,
    },
    #[serde(rename = "github_user")]
    GitHubUser,
    Bearer {
        token: String,
    },
}

fn default_copilot_base_url() -> String {
    gateway_providers::DEFAULT_COPILOT_API_URL.to_string()
}

fn default_copilot_editor_version() -> String {
    gateway_providers::DEFAULT_COPILOT_EDITOR_VERSION.to_string()
}

fn default_copilot_integration_id() -> String {
    gateway_providers::DEFAULT_COPILOT_INTEGRATION_ID.to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum AwsBedrockAuthConfig {
    #[default]
    DefaultChain,
    Bearer {
        token: String,
    },
    StaticCredentials {
        access_key_id: String,
        secret_access_key: String,
        #[serde(default)]
        session_token: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderTimeouts {
    #[serde(default = "default_provider_timeout_ms")]
    pub total_ms: u64,
}

const fn default_provider_timeout_ms() -> u64 {
    120_000
}

fn default_vertex_location() -> String {
    "global".to_string()
}

impl GcpVertexProviderConfig {
    pub fn resolved_api_host(&self) -> String {
        self.api_host
            .clone()
            .unwrap_or_else(|| gateway_providers::vertex_api_host_for_location(&self.location))
    }
}

pub(super) fn validate_providers(providers: &[ProviderConfig]) -> anyhow::Result<()> {
    for provider in providers {
        match provider {
            ProviderConfig::OpenAiCompat(provider) => provider.validate()?,
            ProviderConfig::AnthropicCompat(provider) => provider.validate()?,
            ProviderConfig::GcpCloudRunOpenAiCompat(provider) => provider.validate()?,
            ProviderConfig::GcpVertex(provider) => provider.validate()?,
            ProviderConfig::AwsBedrock(provider) => provider.validate()?,
            ProviderConfig::GitHubCopilot(provider) => provider.validate()?,
        }
    }
    Ok(())
}

impl OpenAiCompatProviderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("openai_compat provider id cannot be empty");
        }
        if self.base_url.trim().is_empty() {
            bail!(
                "openai_compat provider `{}` base_url cannot be empty",
                self.id
            );
        }
        if self.pricing_provider_id.trim().is_empty() {
            bail!(
                "openai_compat provider `{}` pricing_provider_id cannot be empty",
                self.id
            );
        }
        if !is_supported_pricing_provider_id(&self.pricing_provider_id) {
            bail!(
                "openai_compat provider `{}` pricing_provider_id `{}` is not supported",
                self.id,
                self.pricing_provider_id
            );
        }
        if let Some(batch) = &self.batch {
            if let Some(base_url) = batch.base_url.as_deref() {
                if base_url.trim().is_empty() {
                    bail!(
                        "openai_compat provider `{}` batch.base_url cannot be empty",
                        self.id
                    );
                }
                let parsed = url::Url::parse(base_url).with_context(|| {
                    format!(
                        "openai_compat provider `{}` batch.base_url is invalid",
                        self.id
                    )
                })?;
                if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                    bail!(
                        "openai_compat provider `{}` batch.base_url must be an HTTP URL with a host",
                        self.id
                    );
                }
            }
            if batch.dialect == OpenAiBatchDialectConfig::OpenRouter && batch.base_url.is_none() {
                bail!(
                    "openai_compat provider `{}` OpenRouter batch mode requires batch.base_url",
                    self.id
                );
            }
        }
        validate_provider_display_config(self.id.as_str(), self.display.as_ref())?;
        Ok(())
    }
}

impl AnthropicCompatProviderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("anthropic_compat provider id cannot be empty");
        }
        if self.base_url.trim().is_empty() {
            bail!(
                "anthropic_compat provider `{}` base_url cannot be empty",
                self.id
            );
        }
        let parsed = url::Url::parse(&self.base_url).with_context(|| {
            format!(
                "anthropic_compat provider `{}` base_url is invalid",
                self.id
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            bail!(
                "anthropic_compat provider `{}` base_url must be an HTTP URL with a host",
                self.id
            );
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            bail!(
                "anthropic_compat provider `{}` base_url cannot include query parameters or fragments",
                self.id
            );
        }
        if self.pricing_provider_id.trim().is_empty() {
            bail!(
                "anthropic_compat provider `{}` pricing_provider_id cannot be empty",
                self.id
            );
        }
        if !is_supported_pricing_provider_id(&self.pricing_provider_id) {
            bail!(
                "anthropic_compat provider `{}` pricing_provider_id `{}` is not supported",
                self.id,
                self.pricing_provider_id
            );
        }
        validate_provider_display_config(self.id.as_str(), self.display.as_ref())?;
        Ok(())
    }
}

impl GcpCloudRunOpenAiCompatProviderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("gcp_cloud_run_openai_compat provider id cannot be empty");
        }
        validate_cloud_run_base_url(&self.id, &self.base_url)?;
        if self.pricing_provider_id.trim().is_empty() {
            bail!(
                "gcp_cloud_run_openai_compat provider `{}` pricing_provider_id cannot be empty",
                self.id
            );
        }
        if !is_supported_pricing_provider_id(&self.pricing_provider_id) {
            bail!(
                "gcp_cloud_run_openai_compat provider `{}` pricing_provider_id `{}` is not supported",
                self.id,
                self.pricing_provider_id
            );
        }
        if let Some(audience) = self.audience.as_deref()
            && audience.trim().is_empty()
        {
            bail!(
                "gcp_cloud_run_openai_compat provider `{}` audience cannot be empty",
                self.id
            );
        }
        match &self.auth {
            GcpCloudRunOpenAiCompatAuthConfig::Adc => {}
            GcpCloudRunOpenAiCompatAuthConfig::ServiceAccount { credentials_path } => {
                if credentials_path.trim().is_empty() {
                    bail!(
                        "gcp_cloud_run_openai_compat provider `{}` service_account.credentials_path cannot be empty",
                        self.id
                    );
                }
            }
            GcpCloudRunOpenAiCompatAuthConfig::Bearer { token } => {
                if token.trim().is_empty() {
                    bail!(
                        "gcp_cloud_run_openai_compat provider `{}` bearer.token cannot be empty",
                        self.id
                    );
                }
            }
        }
        validate_provider_display_config(self.id.as_str(), self.display.as_ref())?;
        Ok(())
    }
}

impl GcpVertexProviderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("gcp_vertex provider id cannot be empty");
        }
        if self.project_id.trim().is_empty() {
            bail!(
                "gcp_vertex provider `{}` project_id cannot be empty",
                self.id
            );
        }
        if self.location.trim().is_empty() {
            bail!("gcp_vertex provider `{}` location cannot be empty", self.id);
        }
        if self
            .api_host
            .as_deref()
            .is_some_and(|host| host.trim().is_empty())
        {
            bail!("gcp_vertex provider `{}` api_host cannot be empty", self.id);
        }
        if let Some(batch) = &self.batch {
            if batch.dataset.trim().is_empty() {
                bail!(
                    "gcp_vertex provider `{}` batch.dataset cannot be empty",
                    self.id
                );
            }
            if batch
                .bigquery_project_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                bail!(
                    "gcp_vertex provider `{}` batch.bigquery_project_id cannot be empty",
                    self.id
                );
            }
        }

        match &self.auth {
            GcpVertexAuthConfig::Adc => {}
            GcpVertexAuthConfig::ServiceAccount { credentials_path } => {
                if credentials_path.trim().is_empty() {
                    bail!(
                        "gcp_vertex provider `{}` service_account.credentials_path cannot be empty",
                        self.id
                    );
                }
            }
            GcpVertexAuthConfig::Bearer { token } => {
                if token.trim().is_empty() {
                    bail!(
                        "gcp_vertex provider `{}` bearer.token cannot be empty",
                        self.id
                    );
                }
            }
        }

        validate_provider_display_config(self.id.as_str(), self.display.as_ref())?;
        Ok(())
    }
}

impl AwsBedrockProviderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("aws_bedrock provider id cannot be empty");
        }
        if self.region.trim().is_empty() {
            bail!("aws_bedrock provider `{}` region cannot be empty", self.id);
        }
        if let Some(endpoint_url) = self.endpoint_url.as_deref() {
            validate_bedrock_endpoint_url(&self.id, endpoint_url)?;
        }
        let _ = BedrockProviderConfig::resolved_endpoint_url(
            self.endpoint_kind,
            self.region.trim(),
            self.endpoint_url.as_deref(),
        )
        .with_context(|| format!("aws_bedrock provider `{}` endpoint_url is invalid", self.id))?;
        match &self.auth {
            AwsBedrockAuthConfig::DefaultChain => {}
            AwsBedrockAuthConfig::Bearer { token } => {
                if token.trim().is_empty() {
                    bail!(
                        "aws_bedrock provider `{}` bearer.token cannot be empty",
                        self.id
                    );
                }
                let _ = resolve_secret_reference(token)
                    .with_context(|| format!("aws_bedrock provider `{}` bearer.token", self.id))?;
            }
            AwsBedrockAuthConfig::StaticCredentials {
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                let access_key_id = resolve_secret_reference(access_key_id).with_context(|| {
                    format!(
                        "aws_bedrock provider `{}` static_credentials.access_key_id",
                        self.id
                    )
                })?;
                if access_key_id.trim().is_empty() {
                    bail!(
                        "aws_bedrock provider `{}` static_credentials.access_key_id cannot be empty",
                        self.id
                    );
                }
                let secret_access_key =
                    resolve_secret_reference(secret_access_key).with_context(|| {
                        format!(
                            "aws_bedrock provider `{}` static_credentials.secret_access_key",
                            self.id
                        )
                    })?;
                if secret_access_key.trim().is_empty() {
                    bail!(
                        "aws_bedrock provider `{}` static_credentials.secret_access_key cannot be empty",
                        self.id
                    );
                }
                if let Some(session_token) = session_token {
                    let session_token =
                        resolve_secret_reference(session_token).with_context(|| {
                            format!(
                                "aws_bedrock provider `{}` static_credentials.session_token",
                                self.id
                            )
                        })?;
                    if session_token.trim().is_empty() {
                        bail!(
                            "aws_bedrock provider `{}` static_credentials.session_token cannot be empty",
                            self.id
                        );
                    }
                }
            }
        }
        validate_provider_display_config(self.id.as_str(), self.display.as_ref())?;
        Ok(())
    }
}

impl GitHubCopilotProviderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            bail!("github_copilot provider id cannot be empty");
        }
        if self.base_url.trim().is_empty() {
            bail!(
                "github_copilot provider `{}` base_url cannot be empty",
                self.id
            );
        }
        let _ = url::Url::parse(&self.base_url).with_context(|| {
            format!("github_copilot provider `{}` base_url is invalid", self.id)
        })?;
        if let Some(github_api_url) = self.github_api_url.as_deref() {
            let _ = url::Url::parse(github_api_url).with_context(|| {
                format!(
                    "github_copilot provider `{}` github_api_url is invalid",
                    self.id
                )
            })?;
        }
        if self.editor_version.trim().is_empty() {
            bail!(
                "github_copilot provider `{}` editor_version cannot be empty",
                self.id
            );
        }
        if self.integration_id.trim().is_empty() {
            bail!(
                "github_copilot provider `{}` integration_id cannot be empty",
                self.id
            );
        }
        if let Some(pricing_provider_id) = self.pricing_provider_id.as_deref() {
            if pricing_provider_id.trim().is_empty() {
                bail!(
                    "github_copilot provider `{}` pricing_provider_id cannot be empty",
                    self.id
                );
            }
            if !is_supported_pricing_provider_id(pricing_provider_id) {
                bail!(
                    "github_copilot provider `{}` specifies unsupported pricing_provider_id `{pricing_provider_id}`",
                    self.id
                );
            }
        }
        match &self.auth {
            GitHubCopilotAuthConfig::GitHubApp {
                app_id,
                private_key,
                installation_id,
                repository_id,
            } => {
                if *app_id == 0 {
                    bail!(
                        "github_copilot provider `{}` auth.app_id cannot be 0",
                        self.id
                    );
                }
                if *installation_id == 0 {
                    bail!(
                        "github_copilot provider `{}` auth.installation_id cannot be 0",
                        self.id
                    );
                }
                if *repository_id == 0 {
                    bail!(
                        "github_copilot provider `{}` auth.repository_id cannot be 0",
                        self.id
                    );
                }
                if private_key.trim().is_empty() {
                    bail!(
                        "github_copilot provider `{}` auth.private_key cannot be empty",
                        self.id
                    );
                }
                let _ = resolve_copilot_private_key(private_key).with_context(|| {
                    format!("github_copilot provider `{}` auth.private_key", self.id)
                })?;
            }
            GitHubCopilotAuthConfig::GitHubUser => {}
            GitHubCopilotAuthConfig::Bearer { token } => {
                if token.trim().is_empty() {
                    bail!(
                        "github_copilot provider `{}` bearer.token cannot be empty",
                        self.id
                    );
                }
                let _ = resolve_secret_reference(token).with_context(|| {
                    format!("github_copilot provider `{}` bearer.token", self.id)
                })?;
            }
        }
        validate_provider_display_config(self.id.as_str(), self.display.as_ref())?;
        Ok(())
    }
}

fn validate_cloud_run_base_url(provider_id: &str, base_url: &str) -> anyhow::Result<()> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        bail!("gcp_cloud_run_openai_compat provider `{provider_id}` base_url cannot be empty");
    }
    if trimmed.len() != base_url.len() {
        bail!(
            "gcp_cloud_run_openai_compat provider `{provider_id}` base_url cannot include leading or trailing whitespace"
        );
    }

    let parsed = url::Url::parse(base_url).map_err(|error| {
        anyhow::anyhow!(
            "gcp_cloud_run_openai_compat provider `{provider_id}` base_url `{base_url}` is invalid: {error}"
        )
    })?;

    if parsed.scheme() != "https" {
        bail!("gcp_cloud_run_openai_compat provider `{provider_id}` base_url must use https");
    }
    if parsed.host().is_none() {
        bail!("gcp_cloud_run_openai_compat provider `{provider_id}` base_url must include a host");
    }

    Ok(())
}
pub(super) fn resolved_cloud_run_audience(
    configured_audience: Option<&str>,
    base_url: &str,
) -> anyhow::Result<String> {
    if let Some(audience) = configured_audience {
        let trimmed = audience.trim();
        if trimmed.is_empty() {
            bail!("audience cannot be empty");
        }
        return Ok(trimmed.to_string());
    }

    let mut parsed = url::Url::parse(base_url.trim())
        .with_context(|| format!("base_url `{base_url}` is invalid"))?;
    parsed.set_path("/");
    parsed.set_query(None);
    parsed.set_fragment(None);
    Ok(parsed.to_string())
}
fn validate_bedrock_endpoint_url(provider_id: &str, endpoint_url: &str) -> anyhow::Result<()> {
    if endpoint_url.trim().is_empty() {
        bail!("aws_bedrock provider `{provider_id}` endpoint_url cannot be empty");
    }

    let parsed = url::Url::parse(endpoint_url).map_err(|error| {
        anyhow::anyhow!(
            "aws_bedrock provider `{provider_id}` endpoint_url `{endpoint_url}` is invalid: {error}"
        )
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => bail!(
            "aws_bedrock provider `{provider_id}` endpoint_url scheme `{scheme}` is not supported"
        ),
    }
    if parsed.host().is_none() {
        bail!("aws_bedrock provider `{provider_id}` endpoint_url must include a host");
    }

    Ok(())
}
fn validate_provider_display_config(
    provider_id: &str,
    display: Option<&ProviderDisplayConfig>,
) -> anyhow::Result<()> {
    let Some(display) = display else {
        return Ok(());
    };

    if let Some(label) = display.label.as_deref()
        && label.trim().is_empty()
    {
        bail!("provider `{provider_id}` display.label cannot be empty");
    }

    if let Some(icon_key) = display.icon_key.as_deref()
        && ProviderIconKey::parse(icon_key).is_none()
    {
        bail!("provider `{provider_id}` display.icon_key `{icon_key}` is not supported");
    }

    Ok(())
}
