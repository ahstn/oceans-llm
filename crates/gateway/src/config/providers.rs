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
    GcpVertex(GcpVertexProviderConfig),
    AwsBedrock(AwsBedrockProviderConfig),
}

impl ProviderConfig {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::OpenAiCompat(provider) => &provider.id,
            Self::GcpCloudRunOpenAiCompat(provider) => &provider.id,
            Self::GcpVertex(provider) => &provider.id,
            Self::AwsBedrock(provider) => &provider.id,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAiCompatAuthConfig {
    pub kind: String,
    #[serde(default)]
    pub token: Option<String>,
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
