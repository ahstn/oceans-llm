use anyhow::Context;
use gateway_providers::{
    BedrockAuthConfig, BedrockProviderConfig, CloudRunOpenAiCompatAuth, CopilotAuthConfig,
    CopilotProviderConfig, OpenAiCompatConfig, VertexAuthConfig, VertexBatchConfig,
    VertexProviderConfig,
};

use super::{
    GatewayConfig,
    providers::{
        self, AwsBedrockAuthConfig, GcpCloudRunOpenAiCompatAuthConfig, GcpVertexAuthConfig,
        GitHubCopilotAuthConfig, OpenAiBatchDialectConfig, ProviderConfig,
    },
    references::{
        ResolvedCopilotPrivateKey, resolve_copilot_private_key, resolve_path_reference,
        resolve_secret_reference,
    },
};

const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 120_000;

impl GatewayConfig {
    pub fn openai_compatible_provider_configs(&self) -> anyhow::Result<Vec<OpenAiCompatConfig>> {
        let mut configs = Vec::new();

        for provider in &self.providers {
            match provider {
                ProviderConfig::OpenAiCompat(provider) => {
                    let mut config =
                        OpenAiCompatConfig::new(provider.id.clone(), provider.base_url.clone());
                    config.default_headers = provider.default_headers.clone();
                    config.request_timeout_ms = provider
                        .timeouts
                        .as_ref()
                        .map(|timeouts| timeouts.total_ms)
                        .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);
                    if let Some(batch) = &provider.batch {
                        config.batch = gateway_providers::OpenAiBatchConfig {
                            dialect: match batch.dialect {
                                OpenAiBatchDialectConfig::OpenAi => {
                                    gateway_providers::OpenAiBatchDialect::OpenAi
                                }
                                OpenAiBatchDialectConfig::OpenRouter => {
                                    gateway_providers::OpenAiBatchDialect::OpenRouter
                                }
                            },
                            base_url: batch.base_url.clone(),
                        };
                    }

                    if let Some(auth) = &provider.auth
                        && let Some(token) = &auth.token
                    {
                        config.bearer_token = Some(resolve_secret_reference(token)?);
                    }

                    configs.push(config);
                }
                ProviderConfig::GcpCloudRunOpenAiCompat(provider) => {
                    let audience = providers::resolved_cloud_run_audience(
                        provider.audience.as_deref(),
                        &provider.base_url,
                    )
                    .with_context(|| {
                        format!(
                            "gcp_cloud_run_openai_compat provider `{}` audience",
                            provider.id
                        )
                    })?;
                    let auth = match &provider.auth {
                        GcpCloudRunOpenAiCompatAuthConfig::Adc => {
                            CloudRunOpenAiCompatAuth::Adc { audience }
                        }
                        GcpCloudRunOpenAiCompatAuthConfig::ServiceAccount { credentials_path } => {
                            CloudRunOpenAiCompatAuth::ServiceAccount {
                                credentials_path: resolve_path_reference(credentials_path)?.into(),
                                audience,
                            }
                        }
                        GcpCloudRunOpenAiCompatAuthConfig::Bearer { token } => {
                            CloudRunOpenAiCompatAuth::Bearer {
                                token: resolve_secret_reference(token)?,
                            }
                        }
                    };

                    let mut config = OpenAiCompatConfig::new_cloud_run(
                        provider.id.clone(),
                        provider.base_url.clone(),
                        provider.auth_header.into_provider_header(),
                        auth,
                    )?;
                    config.default_headers = provider.default_headers.clone();
                    config.request_timeout_ms = provider
                        .timeouts
                        .as_ref()
                        .map(|timeouts| timeouts.total_ms)
                        .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);

                    configs.push(config);
                }
                ProviderConfig::AnthropicCompat(_)
                | ProviderConfig::GcpVertex(_)
                | ProviderConfig::AwsBedrock(_)
                | ProviderConfig::GitHubCopilot(_) => {}
            }
        }

        Ok(configs)
    }
    pub fn anthropic_compatible_provider_configs(
        &self,
    ) -> anyhow::Result<Vec<gateway_providers::AnthropicCompatConfig>> {
        let mut configs = Vec::new();

        for provider in &self.providers {
            let ProviderConfig::AnthropicCompat(provider) = provider else {
                continue;
            };

            let mut config = gateway_providers::AnthropicCompatConfig::new(
                provider.id.clone(),
                provider.base_url.clone(),
            );
            config.default_headers = provider.default_headers.clone();
            config.request_timeout_ms = provider
                .timeouts
                .as_ref()
                .map(|timeouts| timeouts.total_ms)
                .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);

            if let Some(auth) = &provider.auth
                && let Some(token) = &auth.token
            {
                config.auth = Some(gateway_providers::AnthropicCompatAuth {
                    kind: auth.kind.into_provider_kind(),
                    token: resolve_secret_reference(token)?,
                });
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
                api_host: provider.resolved_api_host(),
                auth,
                default_headers: provider.default_headers.clone(),
                request_timeout_ms: provider
                    .timeouts
                    .as_ref()
                    .map(|timeouts| timeouts.total_ms)
                    .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS),
                batch: provider.batch.as_ref().map(|batch| VertexBatchConfig {
                    bigquery_project_id: batch
                        .bigquery_project_id
                        .clone()
                        .unwrap_or_else(|| provider.project_id.clone()),
                    dataset: batch.dataset.clone(),
                }),
            });
        }

        Ok(configs)
    }

    pub fn bedrock_provider_configs(&self) -> anyhow::Result<Vec<BedrockProviderConfig>> {
        let mut configs = Vec::new();

        for provider in &self.providers {
            let ProviderConfig::AwsBedrock(provider) = provider else {
                continue;
            };

            let endpoint_url = BedrockProviderConfig::resolved_endpoint_url(
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

            let auth = match &provider.auth {
                AwsBedrockAuthConfig::DefaultChain => BedrockAuthConfig::DefaultChain,
                AwsBedrockAuthConfig::Bearer { token } => BedrockAuthConfig::Bearer {
                    token: resolve_secret_reference(token)?,
                },
                AwsBedrockAuthConfig::StaticCredentials {
                    access_key_id,
                    secret_access_key,
                    session_token,
                } => BedrockAuthConfig::StaticCredentials {
                    access_key_id: resolve_secret_reference(access_key_id)?,
                    secret_access_key: resolve_secret_reference(secret_access_key)?,
                    session_token: session_token
                        .as_deref()
                        .map(resolve_secret_reference)
                        .transpose()?,
                },
            };

            configs.push(BedrockProviderConfig {
                provider_key: provider.id.clone(),
                region: provider.region.trim().to_string(),
                endpoint_kind: provider.endpoint_kind,
                endpoint_url,
                auth,
                default_headers: provider.default_headers.clone(),
                request_timeout_ms: provider
                    .timeouts
                    .as_ref()
                    .map(|timeouts| timeouts.total_ms)
                    .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS),
            });
        }

        Ok(configs)
    }
    pub fn copilot_provider_configs(&self) -> anyhow::Result<Vec<CopilotProviderConfig>> {
        let mut configs = Vec::new();

        for provider in &self.providers {
            let ProviderConfig::GitHubCopilot(provider) = provider else {
                continue;
            };

            let auth = match &provider.auth {
                GitHubCopilotAuthConfig::GitHubApp {
                    app_id,
                    private_key,
                    installation_id,
                    repository_id,
                } => match resolve_copilot_private_key(private_key)? {
                    ResolvedCopilotPrivateKey::Pem(private_key_pem) => {
                        CopilotAuthConfig::GitHubApp {
                            app_id: *app_id,
                            private_key_pem,
                            installation_id: *installation_id,
                            repository_id: *repository_id,
                        }
                    }
                    ResolvedCopilotPrivateKey::Path(private_key_path) => {
                        CopilotAuthConfig::GitHubAppKeyFile {
                            app_id: *app_id,
                            private_key_path: private_key_path.into(),
                            installation_id: *installation_id,
                            repository_id: *repository_id,
                        }
                    }
                },
                GitHubCopilotAuthConfig::GitHubUser => CopilotAuthConfig::GitHubUser,
                GitHubCopilotAuthConfig::Bearer { token } => CopilotAuthConfig::Bearer {
                    token: resolve_secret_reference(token)?,
                },
            };

            let mut config = CopilotProviderConfig::new(provider.id.clone(), auth);
            config.base_url = provider.base_url.trim_end_matches('/').to_string();
            config.github_api_url = provider
                .github_api_url
                .as_deref()
                .map(|url| url.trim_end_matches('/').to_string());
            config.editor_version = provider.editor_version.clone();
            config.integration_id = provider.integration_id.clone();
            config.default_headers = provider.default_headers.clone();
            config.request_timeout_ms = provider
                .timeouts
                .as_ref()
                .map(|timeouts| timeouts.total_ms)
                .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);

            configs.push(config);
        }

        Ok(configs)
    }
}
