use std::{collections::HashMap, sync::Arc};

use gateway_core::{
    GatewayError, GatewayModel, ModelRepository, ProviderConnection, ProviderRepository,
};

use crate::{ModelIconKey, ProviderIconKey, resolve_model_icon_key, resolve_provider_display};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminModelStatus {
    Healthy,
    Degraded,
}

impl AdminModelStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdminModelSummary {
    pub id: String,
    pub resolved_model_key: String,
    pub alias_of: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub status: AdminModelStatus,
    pub provider_key: Option<String>,
    pub provider_label: Option<String>,
    pub provider_icon_key: Option<ProviderIconKey>,
    pub upstream_model: Option<String>,
    pub model_icon_key: Option<ModelIconKey>,
}

#[derive(Debug, Clone)]
pub struct AdminModelsService<R> {
    repo: Arc<R>,
}

impl<R> AdminModelsService<R>
where
    R: ModelRepository + ProviderRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_models(&self) -> Result<Vec<AdminModelSummary>, GatewayError> {
        let models = self.repo.list_models().await?;
        let by_key = models
            .iter()
            .cloned()
            .map(|model| (model.model_key.clone(), model))
            .collect::<HashMap<_, _>>();
        let mut provider_cache = HashMap::<String, Option<ProviderConnection>>::new();
        let mut items = Vec::with_capacity(models.len());

        for model in models {
            let execution_model =
                resolve_execution_model(&by_key, &model).unwrap_or_else(|| model.clone());
            let routes = self.repo.list_routes_for_model(execution_model.id).await?;

            let mut primary_provider: Option<ProviderConnection> = None;
            let primary_route = if let Some(route) = routes.iter().find(|route| route.enabled) {
                primary_provider =
                    load_provider(&self.repo, &mut provider_cache, &route.provider_key).await?;
                Some(route)
            } else {
                if let Some(route) = routes.first() {
                    primary_provider =
                        load_provider(&self.repo, &mut provider_cache, &route.provider_key).await?;
                }
                routes.first()
            };

            let status = route_health(&self.repo, &mut provider_cache, &routes).await?;
            let provider_display = primary_route.map(|route| {
                resolve_provider_display(route.provider_key.as_str(), primary_provider.as_ref())
            });
            let model_icon_key = resolve_model_icon_key(
                primary_route
                    .map(|route| route.upstream_model.as_str())
                    .into_iter()
                    .chain([execution_model.model_key.as_str(), model.model_key.as_str()]),
            );

            items.push(AdminModelSummary {
                id: model.model_key.clone(),
                resolved_model_key: execution_model.model_key.clone(),
                alias_of: model.alias_target_model_key.clone(),
                description: model.description.clone(),
                tags: model.tags.clone(),
                status,
                provider_key: primary_route.map(|route| route.provider_key.clone()),
                provider_label: provider_display
                    .as_ref()
                    .map(|display| display.label.clone()),
                provider_icon_key: provider_display.map(|display| display.icon_key),
                upstream_model: primary_route.map(|route| route.upstream_model.clone()),
                model_icon_key,
            });
        }

        Ok(items)
    }
}

async fn route_health<R>(
    repo: &Arc<R>,
    provider_cache: &mut HashMap<String, Option<ProviderConnection>>,
    routes: &[gateway_core::ModelRoute],
) -> Result<AdminModelStatus, GatewayError>
where
    R: ProviderRepository + Send + Sync + 'static,
{
    for route in routes {
        if !route.enabled {
            continue;
        }
        if load_provider(repo, provider_cache, &route.provider_key)
            .await?
            .is_some()
        {
            return Ok(AdminModelStatus::Healthy);
        }
    }

    Ok(AdminModelStatus::Degraded)
}

async fn load_provider<R>(
    repo: &Arc<R>,
    provider_cache: &mut HashMap<String, Option<ProviderConnection>>,
    provider_key: &str,
) -> Result<Option<ProviderConnection>, GatewayError>
where
    R: ProviderRepository + Send + Sync + 'static,
{
    if let Some(provider) = provider_cache.get(provider_key) {
        return Ok(provider.clone());
    }

    let provider = repo.get_provider_by_key(provider_key).await?;
    provider_cache.insert(provider_key.to_string(), provider.clone());
    Ok(provider)
}

fn resolve_execution_model(
    by_key: &HashMap<String, GatewayModel>,
    model: &GatewayModel,
) -> Option<GatewayModel> {
    let mut current = model.clone();
    let mut seen = std::collections::BTreeSet::from([model.model_key.clone()]);

    loop {
        let Some(alias_of) = current.alias_target_model_key.clone() else {
            return Some(current);
        };

        let next = by_key.get(&alias_of)?.clone();
        if !seen.insert(next.model_key.clone()) {
            return None;
        }
        current = next;
    }
}
