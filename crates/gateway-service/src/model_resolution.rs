use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gateway_core::{
    AuthenticatedApiKey, GatewayError, GatewayModel, ModelRepository, ProviderConnection,
    RouteError,
};
use serde_json::Value;

use crate::redaction::mask_secret_leaf_values;

#[derive(Debug, Clone)]
pub struct ResolvedModelSelection {
    pub requested_model: GatewayModel,
    pub execution_model: GatewayModel,
    pub alias_chain: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProviderConnection {
    pub provider_key: String,
    pub provider_type: String,
    pub config: Value,
    pub redacted_secrets: Option<Value>,
}

impl ResolvedProviderConnection {
    #[must_use]
    pub fn from_provider_connection(provider: &ProviderConnection) -> Self {
        Self {
            provider_key: provider.provider_key.clone(),
            provider_type: provider.provider_type.clone(),
            config: provider.config.clone(),
            redacted_secrets: provider.secrets.as_ref().map(mask_secret_leaf_values),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedGatewayRequest {
    pub auth: AuthenticatedApiKey,
    pub selection: ResolvedModelSelection,
    pub routes: Vec<gateway_core::ModelRoute>,
    pub provider_connections: HashMap<String, ResolvedProviderConnection>,
}

#[derive(Clone)]
pub struct ModelResolver<R> {
    repo: Arc<R>,
}

impl<R> ModelResolver<R>
where
    R: ModelRepository,
{
    const MAX_ALIAS_DEPTH: usize = 8;

    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn canonicalize_requested_model(
        &self,
        requested_model: GatewayModel,
    ) -> Result<ResolvedModelSelection, GatewayError> {
        let requested_model_key = requested_model.model_key.clone();
        let mut current = requested_model.clone();
        let mut seen_keys = BTreeSet::from([requested_model.model_key.clone()]);
        let mut alias_chain = vec![requested_model.model_key.clone()];
        let mut alias_hops = 0usize;

        loop {
            let Some(alias_target_model_key) = current.alias_target_model_key.clone() else {
                return Ok(ResolvedModelSelection {
                    requested_model,
                    execution_model: current,
                    alias_chain,
                });
            };

            if alias_hops >= Self::MAX_ALIAS_DEPTH {
                break;
            }

            let next = self
                .repo
                .get_model_by_key(&alias_target_model_key)
                .await?
                .ok_or_else(|| RouteError::ModelNotFound(requested_model_key.clone()))?;

            if !seen_keys.insert(next.model_key.clone()) {
                return Err(RouteError::Policy(format!(
                    "model alias cycle detected for requested model `{requested_model_key}`"
                ))
                .into());
            }

            alias_chain.push(next.model_key.clone());
            current = next;
            alias_hops += 1;
        }

        Err(RouteError::Policy(format!(
            "model alias depth exceeded for requested model `{requested_model_key}`"
        ))
        .into())
    }
}
