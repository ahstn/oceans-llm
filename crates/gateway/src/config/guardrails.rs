use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, bail};
use gateway_guardrails::{
    BearerTokenProvider, BedrockApplyGuardrail, BedrockApplyGuardrailConfig, BedrockAuth,
    BedrockManagedAuthConfig, BuiltInEvaluator, EvaluationError, GuardrailEngine, ManagedCheckKind,
    ManagedEvaluator, ModelArmor, ModelArmorAuthConfig, ModelArmorConfig,
};

use super::{GatewayConfig, references::resolve_secret_reference};

struct SecretReferenceBearerTokenProvider {
    reference: String,
}

#[async_trait::async_trait]
impl BearerTokenProvider for SecretReferenceBearerTokenProvider {
    async fn bearer_token(&self) -> Result<String, EvaluationError> {
        resolve_model_armor_token_reference_async(&self.reference)
            .await
            .map_err(|error| EvaluationError::Unavailable(error.to_string()))
    }
}

fn resolve_model_armor_token_reference(value: &str) -> anyhow::Result<String> {
    let token = resolve_secret_reference(value)?.trim().to_string();
    if token.is_empty() {
        bail!("Model Armor bearer token cannot be empty");
    }
    Ok(token)
}

async fn resolve_model_armor_token_reference_async(value: &str) -> anyhow::Result<String> {
    let token = if let Some(path) = value.strip_prefix("file.") {
        tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read secret file `{path}`"))?
    } else {
        resolve_secret_reference(value)?
    }
    .trim()
    .to_string();
    if token.is_empty() {
        bail!("Model Armor bearer token cannot be empty");
    }
    Ok(token)
}

impl GatewayConfig {
    pub fn guardrail_model_route_keys(&self) -> std::collections::BTreeSet<String> {
        self.models
            .iter()
            .flat_map(|model| {
                model.routes.iter().map(move |route| {
                    format!("{}/{}/{}", model.id, route.provider, route.upstream_model)
                })
            })
            .collect()
    }

    pub fn validate_guardrail_mcp_server_keys(
        &self,
        known_mcp_servers: &std::collections::BTreeSet<String>,
    ) -> anyhow::Result<()> {
        self.guardrails
            .validate(&self.guardrail_model_route_keys(), known_mcp_servers)
            .map_err(Into::into)
    }

    pub fn guardrail_engine(&self) -> anyhow::Result<GuardrailEngine> {
        let mut managed = BTreeMap::<String, Arc<dyn ManagedEvaluator>>::new();
        for (name, check) in &self.guardrails.managed_checks {
            let evaluator: Arc<dyn ManagedEvaluator> = match check.kind {
                ManagedCheckKind::AmazonBedrock => {
                    let config = check.bedrock.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "managed guardrail check `{name}` is missing bedrock config"
                        )
                    })?;
                    let auth = match &config.auth {
                        BedrockManagedAuthConfig::DefaultChain => BedrockAuth::DefaultChain,
                        BedrockManagedAuthConfig::StaticCredentials {
                            access_key_id,
                            secret_access_key,
                            session_token,
                        } => BedrockAuth::StaticCredentials {
                            access_key_id: resolve_secret_reference(access_key_id)?,
                            secret_access_key: resolve_secret_reference(secret_access_key)?,
                            session_token: session_token
                                .as_deref()
                                .map(resolve_secret_reference)
                                .transpose()?,
                        },
                    };
                    Arc::new(BedrockApplyGuardrail::new(BedrockApplyGuardrailConfig {
                        evaluator_id: name.clone(),
                        region: config.region.clone(),
                        guardrail_identifier: config.guardrail_identifier.clone(),
                        guardrail_version: config.guardrail_version.clone(),
                        endpoint_url: config.endpoint_url.clone(),
                        auth,
                        max_retries: config.max_retries,
                    })?)
                }
                ManagedCheckKind::GoogleModelArmor => {
                    let config = check.model_armor.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "managed guardrail check `{name}` is missing model_armor config"
                        )
                    })?;
                    let token_provider: Arc<dyn BearerTokenProvider> = match &config.auth {
                        ModelArmorAuthConfig::BearerToken { token } => {
                            resolve_model_armor_token_reference(token)?;
                            Arc::new(SecretReferenceBearerTokenProvider {
                                reference: token.clone(),
                            })
                        }
                    };
                    Arc::new(ModelArmor::new(
                        ModelArmorConfig {
                            evaluator_id: name.clone(),
                            project: config.project.clone(),
                            location: config.location.clone(),
                            prompt_template: config.prompt_template.clone(),
                            response_template: config.response_template.clone(),
                            endpoint_url: config.endpoint_url.clone(),
                        },
                        token_provider,
                    )?)
                }
            };
            managed.insert(name.clone(), evaluator);
        }
        Ok(GuardrailEngine::new(
            vec![Arc::new(BuiltInEvaluator)],
            managed,
        ))
    }
}

#[cfg(test)]
#[path = "tests/guardrails.rs"]
mod tests;
