use std::collections::BTreeMap;

use anyhow::{Context, bail};
use gateway_providers::{BearerAuthHeader, BedrockEndpointKind, BedrockProviderConfig};
use gateway_service::{ProviderIconKey, is_supported_pricing_provider_id};
use serde::{Deserialize, Serialize};

use super::{resolve_copilot_private_key, resolve_secret_reference};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    #[serde(rename = "openai_compat")]
    OpenAiCompat(OpenAiCompatProviderConfig),
    #[serde(rename = "gcp_cloud_run_openai_compat")]
    GcpCloudRunOpenAiCompat(GcpCloudRunOpenAiCompatProviderConfig),
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

pub(super) fn validate_providers(providers: &[ProviderConfig]) -> anyhow::Result<()> {
    for provider in providers {
        match provider {
            ProviderConfig::OpenAiCompat(provider) => validate_openai_compat_provider(provider)?,
            ProviderConfig::GcpCloudRunOpenAiCompat(provider) => {
                validate_cloud_run_provider(provider)?;
            }
            ProviderConfig::GcpVertex(provider) => validate_vertex_provider(provider)?,
            ProviderConfig::AwsBedrock(provider) => validate_bedrock_provider(provider)?,
            ProviderConfig::GitHubCopilot(provider) => validate_copilot_provider(provider)?,
        }
    }

    Ok(())
}

fn validate_openai_compat_provider(provider: &OpenAiCompatProviderConfig) -> anyhow::Result<()> {
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
    validate_provider_display_config(provider.id.as_str(), provider.display.as_ref())?;

    Ok(())
}

fn validate_cloud_run_provider(
    provider: &GcpCloudRunOpenAiCompatProviderConfig,
) -> anyhow::Result<()> {
    if provider.id.trim().is_empty() {
        bail!("gcp_cloud_run_openai_compat provider id cannot be empty");
    }
    validate_cloud_run_base_url(&provider.id, &provider.base_url)?;
    if provider.pricing_provider_id.trim().is_empty() {
        bail!(
            "gcp_cloud_run_openai_compat provider `{}` pricing_provider_id cannot be empty",
            provider.id
        );
    }
    if !is_supported_pricing_provider_id(&provider.pricing_provider_id) {
        bail!(
            "gcp_cloud_run_openai_compat provider `{}` pricing_provider_id `{}` is not supported",
            provider.id,
            provider.pricing_provider_id
        );
    }
    if let Some(audience) = provider.audience.as_deref()
        && audience.trim().is_empty()
    {
        bail!(
            "gcp_cloud_run_openai_compat provider `{}` audience cannot be empty",
            provider.id
        );
    }
    match &provider.auth {
        GcpCloudRunOpenAiCompatAuthConfig::Adc => {}
        GcpCloudRunOpenAiCompatAuthConfig::ServiceAccount { credentials_path } => {
            if credentials_path.trim().is_empty() {
                bail!(
                    "gcp_cloud_run_openai_compat provider `{}` service_account.credentials_path cannot be empty",
                    provider.id
                );
            }
        }
        GcpCloudRunOpenAiCompatAuthConfig::Bearer { token } => {
            if token.trim().is_empty() {
                bail!(
                    "gcp_cloud_run_openai_compat provider `{}` bearer.token cannot be empty",
                    provider.id
                );
            }
        }
    }
    validate_provider_display_config(provider.id.as_str(), provider.display.as_ref())?;

    Ok(())
}

fn validate_vertex_provider(provider: &GcpVertexProviderConfig) -> anyhow::Result<()> {
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

    validate_provider_display_config(provider.id.as_str(), provider.display.as_ref())?;

    Ok(())
}

fn validate_bedrock_provider(provider: &AwsBedrockProviderConfig) -> anyhow::Result<()> {
    if provider.id.trim().is_empty() {
        bail!("aws_bedrock provider id cannot be empty");
    }
    if provider.region.trim().is_empty() {
        bail!(
            "aws_bedrock provider `{}` region cannot be empty",
            provider.id
        );
    }
    if let Some(endpoint_url) = provider.endpoint_url.as_deref() {
        validate_bedrock_endpoint_url(&provider.id, endpoint_url)?;
    }
    BedrockProviderConfig::resolved_endpoint_url(
        provider.endpoint_kind,
        provider.region.trim(),
        provider.endpoint_url.as_deref(),
    )
    .with_context(|| {
        format!(
            "aws_bedrock provider `{}` endpoint_url is invalid",
            provider.id
        )
    })?;
    match &provider.auth {
        AwsBedrockAuthConfig::DefaultChain => {}
        AwsBedrockAuthConfig::Bearer { token } => {
            if token.trim().is_empty() {
                bail!(
                    "aws_bedrock provider `{}` bearer.token cannot be empty",
                    provider.id
                );
            }
            resolve_secret_reference(token)
                .with_context(|| format!("aws_bedrock provider `{}` bearer.token", provider.id))?;
        }
        AwsBedrockAuthConfig::StaticCredentials {
            access_key_id,
            secret_access_key,
            session_token,
        } => {
            let access_key_id = resolve_secret_reference(access_key_id).with_context(|| {
                format!(
                    "aws_bedrock provider `{}` static_credentials.access_key_id",
                    provider.id
                )
            })?;
            if access_key_id.trim().is_empty() {
                bail!(
                    "aws_bedrock provider `{}` static_credentials.access_key_id cannot be empty",
                    provider.id
                );
            }
            let secret_access_key =
                resolve_secret_reference(secret_access_key).with_context(|| {
                    format!(
                        "aws_bedrock provider `{}` static_credentials.secret_access_key",
                        provider.id
                    )
                })?;
            if secret_access_key.trim().is_empty() {
                bail!(
                    "aws_bedrock provider `{}` static_credentials.secret_access_key cannot be empty",
                    provider.id
                );
            }
            if let Some(session_token) = session_token {
                let session_token = resolve_secret_reference(session_token).with_context(|| {
                    format!(
                        "aws_bedrock provider `{}` static_credentials.session_token",
                        provider.id
                    )
                })?;
                if session_token.trim().is_empty() {
                    bail!(
                        "aws_bedrock provider `{}` static_credentials.session_token cannot be empty",
                        provider.id
                    );
                }
            }
        }
    }
    validate_provider_display_config(provider.id.as_str(), provider.display.as_ref())?;

    Ok(())
}

fn validate_copilot_provider(provider: &GitHubCopilotProviderConfig) -> anyhow::Result<()> {
    if provider.id.trim().is_empty() {
        bail!("github_copilot provider id cannot be empty");
    }
    if provider.base_url.trim().is_empty() {
        bail!(
            "github_copilot provider `{}` base_url cannot be empty",
            provider.id
        );
    }
    url::Url::parse(&provider.base_url).with_context(|| {
        format!(
            "github_copilot provider `{}` base_url is invalid",
            provider.id
        )
    })?;
    if let Some(github_api_url) = provider.github_api_url.as_deref() {
        url::Url::parse(github_api_url).with_context(|| {
            format!(
                "github_copilot provider `{}` github_api_url is invalid",
                provider.id
            )
        })?;
    }
    if provider.editor_version.trim().is_empty() {
        bail!(
            "github_copilot provider `{}` editor_version cannot be empty",
            provider.id
        );
    }
    if provider.integration_id.trim().is_empty() {
        bail!(
            "github_copilot provider `{}` integration_id cannot be empty",
            provider.id
        );
    }
    if let Some(pricing_provider_id) = provider.pricing_provider_id.as_deref() {
        if pricing_provider_id.trim().is_empty() {
            bail!(
                "github_copilot provider `{}` pricing_provider_id cannot be empty",
                provider.id
            );
        }
        if !is_supported_pricing_provider_id(pricing_provider_id) {
            bail!(
                "github_copilot provider `{}` specifies unsupported pricing_provider_id `{pricing_provider_id}`",
                provider.id
            );
        }
    }
    match &provider.auth {
        GitHubCopilotAuthConfig::GitHubApp {
            app_id,
            private_key,
            installation_id,
            repository_id,
        } => {
            if *app_id == 0 {
                bail!(
                    "github_copilot provider `{}` auth.app_id cannot be 0",
                    provider.id
                );
            }
            if *installation_id == 0 {
                bail!(
                    "github_copilot provider `{}` auth.installation_id cannot be 0",
                    provider.id
                );
            }
            if *repository_id == 0 {
                bail!(
                    "github_copilot provider `{}` auth.repository_id cannot be 0",
                    provider.id
                );
            }
            if private_key.trim().is_empty() {
                bail!(
                    "github_copilot provider `{}` auth.private_key cannot be empty",
                    provider.id
                );
            }
            resolve_copilot_private_key(private_key).with_context(|| {
                format!("github_copilot provider `{}` auth.private_key", provider.id)
            })?;
        }
        GitHubCopilotAuthConfig::Bearer { token } => {
            if token.trim().is_empty() {
                bail!(
                    "github_copilot provider `{}` bearer.token cannot be empty",
                    provider.id
                );
            }
            resolve_secret_reference(token).with_context(|| {
                format!("github_copilot provider `{}` bearer.token", provider.id)
            })?;
        }
    }
    validate_provider_display_config(provider.id.as_str(), provider.display.as_ref())?;

    Ok(())
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
