use gateway_core::{GatewayError, ModelRepository, PricingCatalogRepository, ProviderRepository};

use super::{AdminModelsService, CatalogRoutes, select_display_route};
use crate::resolve_provider_display;

/// One model as a selector lists it: what it is called, what it stores, and what it resolves to.
/// Carries no route, pricing, or allow-list detail, so every authenticated role may read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminModelOption {
    /// The model key that requests name and request logs store.
    pub id: String,
    /// The model's UUID, which model-scoped budgets store.
    pub model_id: String,
    /// The model key an alias finally resolves to; the model's own key otherwise.
    pub resolved_model_key: String,
    pub alias_of: Option<String>,
    /// Label of the provider behind the display route, as `AdminModelSummary` reports it.
    pub provider_label: Option<String>,
}

impl<R> AdminModelsService<R>
where
    R: ModelRepository + ProviderRepository + PricingCatalogRepository + Send + Sync + 'static,
{
    /// Every model, aliases included and in the same order as `list_models`, without the
    /// per-route pricing reads, benchmark matching, and client config rendering a summary needs.
    pub async fn list_model_options(&self) -> Result<Vec<AdminModelOption>, GatewayError> {
        let CatalogRoutes {
            models,
            execution_models,
            routes_by_model,
            providers_by_key,
            ..
        } = self.load_catalog_routes().await?;

        Ok(models
            .into_iter()
            .map(|model| {
                let execution_model = execution_models.get(&model.model_key).unwrap_or(&model);
                let routes = routes_by_model
                    .get(&execution_model.id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let provider_label = select_display_route(&providers_by_key, routes).map(|route| {
                    resolve_provider_display(
                        &route.provider_key,
                        providers_by_key.get(&route.provider_key),
                    )
                    .label
                });
                AdminModelOption {
                    id: model.model_key.clone(),
                    model_id: model.id.to_string(),
                    resolved_model_key: execution_model.model_key.clone(),
                    alias_of: model.alias_target_model_key.clone(),
                    provider_label,
                }
            })
            .collect())
    }
}
