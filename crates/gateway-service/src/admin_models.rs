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
        let execution_models = models
            .iter()
            .map(|model| {
                (
                    model.model_key.clone(),
                    resolve_execution_model(&by_key, model).unwrap_or_else(|| model.clone()),
                )
            })
            .collect::<HashMap<_, _>>();
        let execution_model_ids = execution_models
            .values()
            .map(|model| model.id)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let routes_by_model = self
            .repo
            .list_routes_for_models(&execution_model_ids)
            .await?;
        let provider_keys = routes_by_model
            .values()
            .flat_map(|routes| routes.iter().map(|route| route.provider_key.clone()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let providers_by_key = self.repo.list_providers_by_keys(&provider_keys).await?;
        let mut items = Vec::with_capacity(models.len());

        for model in models {
            let execution_model = execution_models
                .get(&model.model_key)
                .cloned()
                .unwrap_or_else(|| model.clone());
            let routes = routes_by_model
                .get(&execution_model.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let primary_route = routes
                .iter()
                .find(|route| route.enabled)
                .or_else(|| routes.first());
            let primary_provider =
                primary_route.and_then(|route| providers_by_key.get(&route.provider_key));
            let status = route_health(&providers_by_key, routes);
            let provider_display = primary_route.map(|route| {
                resolve_provider_display(route.provider_key.as_str(), primary_provider)
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

fn route_health(
    providers_by_key: &HashMap<String, ProviderConnection>,
    routes: &[gateway_core::ModelRoute],
) -> AdminModelStatus {
    for route in routes {
        if route.enabled && providers_by_key.contains_key(&route.provider_key) {
            return AdminModelStatus::Healthy;
        }
    }

    AdminModelStatus::Degraded
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use gateway_core::{
        GatewayModel, ModelRepository, ModelRoute, ProviderConnection, ProviderRepository,
        StoreError,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::{AdminModelStatus, AdminModelsService};

    #[derive(Default)]
    struct CountingRepo {
        models: Vec<GatewayModel>,
        routes_by_model: HashMap<Uuid, Vec<ModelRoute>>,
        providers_by_key: HashMap<String, ProviderConnection>,
        list_routes_for_model_calls: AtomicUsize,
        list_routes_for_models_calls: AtomicUsize,
        get_provider_by_key_calls: AtomicUsize,
        list_providers_by_keys_calls: AtomicUsize,
    }

    #[async_trait]
    impl ModelRepository for CountingRepo {
        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(self.models.clone())
        }

        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self
                .models
                .iter()
                .find(|model| model.model_key == model_key)
                .cloned())
        }

        async fn list_models_for_api_key(
            &self,
            _api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_routes_for_model(
            &self,
            model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            self.list_routes_for_model_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(self
                .routes_by_model
                .get(&model_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_routes_for_models(
            &self,
            model_ids: &[Uuid],
        ) -> Result<HashMap<Uuid, Vec<ModelRoute>>, StoreError> {
            self.list_routes_for_models_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(model_ids
                .iter()
                .filter_map(|model_id| {
                    self.routes_by_model
                        .get(model_id)
                        .cloned()
                        .map(|routes| (*model_id, routes))
                })
                .collect())
        }
    }

    #[async_trait]
    impl ProviderRepository for CountingRepo {
        async fn get_provider_by_key(
            &self,
            provider_key: &str,
        ) -> Result<Option<ProviderConnection>, StoreError> {
            self.get_provider_by_key_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(self.providers_by_key.get(provider_key).cloned())
        }

        async fn list_providers_by_keys(
            &self,
            provider_keys: &[String],
        ) -> Result<HashMap<String, ProviderConnection>, StoreError> {
            self.list_providers_by_keys_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(provider_keys
                .iter()
                .filter_map(|provider_key| {
                    self.providers_by_key
                        .get(provider_key)
                        .cloned()
                        .map(|provider| (provider_key.clone(), provider))
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn list_models_batches_route_and_provider_loading() {
        let execution_model_id = Uuid::new_v4();
        let alias_model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();

        let repo = Arc::new(CountingRepo {
            models: vec![
                GatewayModel {
                    id: alias_model_id,
                    model_key: "friendly-alias".to_string(),
                    alias_target_model_key: Some("gpt-4.1".to_string()),
                    description: Some("alias".to_string()),
                    tags: vec!["alias".to_string()],
                    rank: 1,
                },
                GatewayModel {
                    id: execution_model_id,
                    model_key: "gpt-4.1".to_string(),
                    alias_target_model_key: None,
                    description: Some("base".to_string()),
                    tags: vec!["base".to_string()],
                    rank: 2,
                },
            ],
            routes_by_model: HashMap::from([(
                execution_model_id,
                vec![ModelRoute {
                    id: route_id,
                    model_id: execution_model_id,
                    provider_key: "openai".to_string(),
                    upstream_model: "gpt-4.1".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: Default::default(),
                }],
            )]),
            providers_by_key: HashMap::from([(
                "openai".to_string(),
                ProviderConnection {
                    provider_key: "openai".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({"display": {"label": "OpenAI"}}),
                    secrets: None,
                },
            )]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo.clone());
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items.len(), 2);
        assert_eq!(repo.list_routes_for_models_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.list_routes_for_model_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repo.list_providers_by_keys_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.get_provider_by_key_calls.load(Ordering::SeqCst), 0);

        let alias = items
            .iter()
            .find(|item| item.id == "friendly-alias")
            .expect("alias item");
        assert_eq!(alias.resolved_model_key, "gpt-4.1");
        assert_eq!(alias.status, AdminModelStatus::Healthy);
        assert_eq!(alias.provider_key.as_deref(), Some("openai"));
        assert_eq!(alias.upstream_model.as_deref(), Some("gpt-4.1"));
    }

    #[tokio::test]
    async fn list_models_keeps_degraded_status_when_provider_is_missing() {
        let model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let repo = Arc::new(CountingRepo {
            models: vec![GatewayModel {
                id: model_id,
                model_key: "missing-provider-model".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: Vec::new(),
                rank: 1,
            }],
            routes_by_model: HashMap::from([(
                model_id,
                vec![ModelRoute {
                    id: route_id,
                    model_id,
                    provider_key: "missing".to_string(),
                    upstream_model: "upstream".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: Default::default(),
                }],
            )]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo);
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items[0].status, AdminModelStatus::Degraded);
        assert_eq!(items[0].provider_key.as_deref(), Some("missing"));
    }
}
