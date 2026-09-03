use std::collections::BTreeMap;

use gateway_providers::{BearerAuthHeader, BedrockEndpointKind};
use serde::{Deserialize, Serialize};

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
    #[serde(default = "default_vertex_api_host")]
    pub api_host: String,
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

fn default_vertex_api_host() -> String {
    "aiplatform.googleapis.com".to_string()
}
