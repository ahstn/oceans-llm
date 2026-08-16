use anyhow::Context;
use gateway_core::{
    ApiKeySecretStorageKind, ManagedApiKeySource, SeedApiKeySecretMaterial,
    SeedHumanBudgetDefaults, SeedManagedServiceAccountApiKey, SeedModel, SeedModelRoute,
    SeedOauthProvider, SeedOidcProvider, SeedProvider, SeedServiceAccount, SeedTeam, SeedUser,
    SeedUserMembership, SeedUserModelBudgetDefault, hash_gateway_key_secret, parse_gateway_api_key,
};
use gateway_providers::BedrockProviderConfig;
use gateway_service::encrypt_gateway_api_key_secret;
use serde_json::{Map, Value, json};

use super::{
    GatewayConfig,
    auth::{normalize_config_oauth_provider_key, normalize_config_oidc_provider_key},
    budgets::BudgetConfig,
    identity::{normalize_config_managed_api_key, normalize_config_service_account_key},
    models::{config_model_uuid, normalize_config_model_key, normalize_model_allowlist},
    normalization::{
        normalize_config_email, normalize_config_team_key, normalize_optional_config_entity_tags,
    },
    providers::{
        self, AwsBedrockAuthConfig, GcpCloudRunOpenAiCompatAuthConfig, GcpVertexAuthConfig,
        GitHubCopilotAuthConfig, ProviderConfig,
    },
    references::{resolve_secret_reference, validate_env_reference_if_needed},
};

impl GatewayConfig {
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
                        "display": provider.display,
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
                ProviderConfig::GcpCloudRunOpenAiCompat(provider) => {
                    match &provider.auth {
                        GcpCloudRunOpenAiCompatAuthConfig::Adc => {}
                        GcpCloudRunOpenAiCompatAuthConfig::ServiceAccount { credentials_path } => {
                            validate_env_reference_if_needed(credentials_path)?;
                        }
                        GcpCloudRunOpenAiCompatAuthConfig::Bearer { token } => {
                            validate_env_reference_if_needed(token)?;
                        }
                    }

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
                    let config = json!({
                        "base_url": provider.base_url,
                        "audience": audience,
                        "pricing_provider_id": provider.pricing_provider_id,
                        "auth_header": provider.auth_header,
                        "default_headers": provider.default_headers,
                        "timeouts": provider.timeouts,
                        "display": provider.display,
                    });

                    let secrets = Some(match &provider.auth {
                        GcpCloudRunOpenAiCompatAuthConfig::Adc => json!({"mode": "adc"}),
                        GcpCloudRunOpenAiCompatAuthConfig::ServiceAccount { credentials_path } => {
                            json!({"mode": "service_account", "credentials_path": credentials_path})
                        }
                        GcpCloudRunOpenAiCompatAuthConfig::Bearer { token } => {
                            json!({"mode": "bearer", "token": token})
                        }
                    });

                    providers.push(SeedProvider {
                        provider_key: provider.id.clone(),
                        provider_type: "gcp_cloud_run_openai_compat".to_string(),
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
                        "display": provider.display,
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
                ProviderConfig::AwsBedrock(provider) => {
                    match &provider.auth {
                        AwsBedrockAuthConfig::DefaultChain => {}
                        AwsBedrockAuthConfig::Bearer { token } => {
                            validate_env_reference_if_needed(token)?;
                        }
                        AwsBedrockAuthConfig::StaticCredentials {
                            access_key_id,
                            secret_access_key,
                            session_token,
                        } => {
                            validate_env_reference_if_needed(access_key_id)?;
                            validate_env_reference_if_needed(secret_access_key)?;
                            if let Some(session_token) = session_token {
                                validate_env_reference_if_needed(session_token)?;
                            }
                        }
                    }

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

                    let config = json!({
                        "region": provider.region.trim(),
                        "endpoint_kind": provider.endpoint_kind,
                        "endpoint_url": endpoint_url,
                        "default_headers": provider.default_headers,
                        "timeouts": provider.timeouts,
                        "display": provider.display,
                    });

                    let secrets = Some(match &provider.auth {
                        AwsBedrockAuthConfig::DefaultChain => json!({"mode": "default_chain"}),
                        AwsBedrockAuthConfig::Bearer { token } => {
                            json!({"mode": "bearer", "token": token})
                        }
                        AwsBedrockAuthConfig::StaticCredentials {
                            access_key_id,
                            secret_access_key,
                            session_token,
                        } => json!({
                            "mode": "static_credentials",
                            "access_key_id": access_key_id,
                            "secret_access_key": secret_access_key,
                            "session_token": session_token,
                        }),
                    });

                    providers.push(SeedProvider {
                        provider_key: provider.id.clone(),
                        provider_type: "aws_bedrock".to_string(),
                        config,
                        secrets,
                    });
                }
                ProviderConfig::GitHubCopilot(provider) => {
                    match &provider.auth {
                        GitHubCopilotAuthConfig::GitHubApp { private_key, .. } => {
                            validate_env_reference_if_needed(private_key)?;
                        }
                        GitHubCopilotAuthConfig::Bearer { token } => {
                            validate_env_reference_if_needed(token)?;
                        }
                    }

                    let config = json!({
                        "base_url": provider.base_url.trim_end_matches('/'),
                        "github_api_url": provider.github_api_url.as_deref().map(|url| url.trim_end_matches('/')),
                        "pricing_provider_id": provider.pricing_provider_id,
                        "editor_version": provider.editor_version,
                        "integration_id": provider.integration_id,
                        "default_headers": provider.default_headers,
                        "timeouts": provider.timeouts,
                        "display": provider.display,
                    });

                    let secrets = Some(match &provider.auth {
                        GitHubCopilotAuthConfig::GitHubApp {
                            app_id,
                            private_key,
                            installation_id,
                            repository_id,
                        } => json!({
                            "mode": "github_app",
                            "app_id": app_id,
                            "private_key": private_key,
                            "installation_id": installation_id,
                            "repository_id": repository_id,
                        }),
                        GitHubCopilotAuthConfig::Bearer { token } => json!({
                            "mode": "bearer",
                            "token": token,
                        }),
                    });

                    providers.push(SeedProvider {
                        provider_key: provider.id.clone(),
                        provider_type: "github_copilot".to_string(),
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
            .map(|model| {
                Ok(SeedModel {
                    model_key: model.id.clone(),
                    alias_target_model_key: model.alias_of.clone(),
                    description: model.description.clone(),
                    tags: model.tags.clone(),
                    rank: model.rank,
                    routes: model
                        .routes
                        .iter()
                        .map(|route| {
                            Ok(SeedModelRoute {
                                provider_key: route.provider.clone(),
                                upstream_model: route.upstream_model.clone(),
                                priority: route.priority,
                                weight: route.weight,
                                enabled: route.enabled,
                                context_window_tokens: route.context_window_tokens,
                                pricing_override: route
                                    .pricing_override
                                    .as_ref()
                                    .map(|pricing| {
                                        pricing.resolve(&format!(
                                            "model `{}` route `{}` pricing_override",
                                            model.id, route.upstream_model
                                        ))
                                    })
                                    .transpose()?,
                                extra_headers: route
                                    .extra_headers
                                    .iter()
                                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                                    .collect::<Map<String, Value>>(),
                                extra_body: route.extra_body.clone(),
                                capabilities: route.capabilities.clone().into_capabilities(),
                                compatibility: route.compatibility.clone().into_compatibility(),
                            })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?,
                    allowlist: model
                        .allowlist
                        .as_ref()
                        .map(|allowlist| normalize_model_allowlist(&model.id, allowlist))
                        .transpose()?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(models)
    }

    pub fn seed_service_accounts(&self) -> anyhow::Result<Vec<SeedServiceAccount>> {
        self.service_accounts
            .iter()
            .map(|service_account| {
                let service_account_key = normalize_config_service_account_key(&service_account.id)?;
                let service_account_name = service_account
                    .name
                    .as_deref()
                    .unwrap_or(&service_account.id)
                    .trim()
                    .to_string();

                let mut managed_api_keys = Vec::with_capacity(service_account.keys.len());
                for key in &service_account.keys {
                    let config_key = normalize_config_managed_api_key(&key.id)?;
                    let key_name = key.name.as_deref().unwrap_or(&key.id).trim().to_string();
                    let (source, public_id, secret_hash, secret_material) =
                        match key.value.as_deref() {
                            Some(value_ref) => {
                                let raw_value =
                                    resolve_secret_reference(value_ref).with_context(|| {
                                        format!(
                                            "service account `{service_account_key}` key `{config_key}` value"
                                        )
                                    })?;
                                let parsed = parse_gateway_api_key(&raw_value).with_context(|| {
                                    format!(
                                        "invalid gateway key configured for service account `{service_account_key}` key `{config_key}`"
                                    )
                                })?;
                                let secret_hash =
                                    hash_gateway_key_secret(&parsed.secret).with_context(|| {
                                        format!(
                                            "failed hashing gateway key for service account `{service_account_key}` key `{config_key}`"
                                        )
                                    })?;
                                let encrypted =
                                    encrypt_gateway_api_key_secret(&raw_value).map_err(|error| {
                                        anyhow::anyhow!(
                                            "failed encrypting gateway key for service account `{service_account_key}` key `{config_key}`: {error}"
                                        )
                                    })?;
                                (
                                    ManagedApiKeySource::ConfiguredValue,
                                    Some(parsed.public_id),
                                    Some(secret_hash),
                                    Some(SeedApiKeySecretMaterial {
                                        storage_kind: ApiKeySecretStorageKind::EncryptedBlob,
                                        secret_ciphertext: encrypted.ciphertext,
                                        secret_nonce: encrypted.nonce,
                                        secret_key_id: encrypted.key_id.to_string(),
                                    }),
                                )
                            }
                            None => (ManagedApiKeySource::Generated, None, None, None),
                        };

                    managed_api_keys.push(SeedManagedServiceAccountApiKey {
                        config_key,
                        name: key_name,
                        auto_create: key.auto_create,
                        source,
                        public_id,
                        secret_hash,
                        secret_material,
                        allowed_models: key.allowed_models.clone(),
                    });
                }
                let tags = normalize_optional_config_entity_tags(
                    service_account.tags.as_deref(),
                    &format!("service account `{service_account_key}` tags"),
                )?;

                Ok(SeedServiceAccount {
                    service_account_key,
                    service_account_name,
                    team_key: normalize_config_team_key(&service_account.team)?,
                    tags,
                    budget: service_account.budget.seed_budget()?,
                    managed_api_keys,
                })
            })
            .collect()
    }

    pub fn seed_oidc_providers(&self) -> anyhow::Result<Vec<SeedOidcProvider>> {
        self.auth
            .oidc
            .providers
            .iter()
            .map(|provider| provider.seed_provider())
            .collect()
    }

    pub fn seed_oauth_providers(&self) -> anyhow::Result<Vec<SeedOauthProvider>> {
        self.auth
            .oauth
            .providers
            .iter()
            .map(|provider| provider.seed_provider())
            .collect()
    }

    pub fn seed_teams(&self) -> anyhow::Result<Vec<SeedTeam>> {
        self.teams
            .iter()
            .map(|team| {
                Ok(SeedTeam {
                    team_key: normalize_config_team_key(&team.id)?,
                    team_name: team.name.trim().to_string(),
                    tags: normalize_optional_config_entity_tags(
                        team.tags.as_deref(),
                        &format!("team `{}` tags", team.id),
                    )?,
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
                    tags: normalize_optional_config_entity_tags(
                        user.tags.as_deref(),
                        &format!("user `{}` tags", user.email),
                    )?,
                    oidc_provider_key: user
                        .oidc_provider_key
                        .as_deref()
                        .map(normalize_config_oidc_provider_key)
                        .transpose()?,
                    oauth_provider_key: user
                        .oauth_provider_key
                        .as_deref()
                        .map(normalize_config_oauth_provider_key)
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

    pub fn seed_human_budget_defaults(&self) -> anyhow::Result<SeedHumanBudgetDefaults> {
        let default_user_budget = self
            .budgets
            .users
            .default
            .as_ref()
            .map(BudgetConfig::seed_budget)
            .transpose()?;
        let model_defaults = self
            .budgets
            .users
            .model_defaults
            .iter()
            .map(|model_default| {
                let model_key = normalize_config_model_key(&model_default.model)?;
                Ok(SeedUserModelBudgetDefault {
                    model_id: config_model_uuid(&model_key),
                    model_key,
                    budget: model_default.budget.seed_budget()?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(SeedHumanBudgetDefaults {
            default_user_budget,
            model_defaults,
        })
    }
}
