use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gateway_core::{
    AuthenticatedApiKey, GatewayError, GatewayModel, ModelRepository, ProviderConnection,
    ReasoningEffort, RouteError,
};
use serde_json::Value;

use crate::redaction::mask_secret_leaf_values;

const MAX_MODEL_ALIAS_DEPTH: usize = 8;

pub(crate) async fn load_missing_alias_targets<R: ModelRepository>(
    repo: &R,
    visible: &[GatewayModel],
) -> Result<Vec<GatewayModel>, GatewayError> {
    let mut known = visible
        .iter()
        .map(|model| model.model_key.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = visible
        .iter()
        .filter_map(|model| model.alias_target_model_key.clone())
        .collect::<BTreeSet<_>>();
    let mut targets = Vec::new();
    for _ in 0..MAX_MODEL_ALIAS_DEPTH {
        let missing = pending.difference(&known).cloned().collect::<Vec<_>>();
        if missing.is_empty() {
            break;
        }
        // Remember absent keys as well, so broken aliases are not fetched again.
        known.extend(missing.iter().cloned());
        let fetched = repo.list_models_by_keys(&missing).await?;
        pending = fetched
            .iter()
            .filter_map(|model| model.alias_target_model_key.clone())
            .collect();
        targets.extend(fetched);
    }
    Ok(targets)
}

pub(crate) fn execution_model_from_snapshot<'a>(
    models: &HashMap<&str, &'a GatewayModel>,
    model: &'a GatewayModel,
) -> Option<&'a GatewayModel> {
    let mut current = model;
    let mut seen = BTreeSet::new();
    for _ in 0..=MAX_MODEL_ALIAS_DEPTH {
        if !seen.insert(current.model_key.as_str()) {
            return None;
        }
        let Some(target) = current.alias_target_model_key.as_deref() else {
            return Some(current);
        };
        current = models.get(target)?;
    }
    None
}

fn strictest_reasoning_effort(
    current: Option<ReasoningEffort>,
    candidate: Option<ReasoningEffort>,
) -> Option<ReasoningEffort> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (Some(effort), None) | (None, Some(effort)) => Some(effort),
        (None, None) => None,
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedModelSelection {
    pub requested_model: GatewayModel,
    pub execution_model: GatewayModel,
    pub alias_chain: Vec<String>,
    pub max_reasoning_effort: Option<ReasoningEffort>,
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
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    #[tracing::instrument(
        name = "gateway.model.alias_resolution",
        skip_all,
        fields(gen_ai.request.model = %requested_model.model_key)
    )]
    pub async fn canonicalize_requested_model(
        &self,
        requested_model: GatewayModel,
    ) -> Result<ResolvedModelSelection, GatewayError> {
        let requested_model_key = requested_model.model_key.clone();
        let mut current = requested_model.clone();
        let mut seen_keys = BTreeSet::from([requested_model.model_key.clone()]);
        let mut alias_chain = vec![requested_model.model_key.clone()];
        let mut alias_hops = 0usize;
        let mut max_reasoning_effort: Option<ReasoningEffort> = None;

        loop {
            max_reasoning_effort =
                strictest_reasoning_effort(max_reasoning_effort, current.max_reasoning_effort);

            let Some(alias_target_model_key) = current.alias_target_model_key.clone() else {
                return Ok(ResolvedModelSelection {
                    requested_model,
                    execution_model: current,
                    alias_chain,
                    max_reasoning_effort,
                });
            };

            if alias_hops >= MAX_MODEL_ALIAS_DEPTH {
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

#[cfg(test)]
mod tests {
    use gateway_core::ReasoningEffort;

    use super::strictest_reasoning_effort;

    #[test]
    fn strictest_reasoning_effort_uses_lowest_configured_ceiling() {
        assert_eq!(strictest_reasoning_effort(None, None), None);
        assert_eq!(
            strictest_reasoning_effort(None, Some(ReasoningEffort::High)),
            Some(ReasoningEffort::High)
        );
        assert_eq!(
            strictest_reasoning_effort(Some(ReasoningEffort::Medium), Some(ReasoningEffort::XHigh)),
            Some(ReasoningEffort::Medium)
        );
        assert_eq!(
            strictest_reasoning_effort(Some(ReasoningEffort::Max), Some(ReasoningEffort::Minimal)),
            Some(ReasoningEffort::Minimal)
        );
    }
}
