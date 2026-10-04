use std::collections::HashMap;

use gateway_core::{ModelRoute, ProviderConnection};

use crate::{ProviderIconKey, resolve_provider_display};

/// Administrative route metadata excludes request defaults and authentication material.
#[derive(Debug, Clone)]
pub struct AdminModelRouteSummary {
    pub id: String,
    pub provider_key: String,
    pub provider_label: String,
    pub provider_icon_key: ProviderIconKey,
    pub upstream_model: String,
    pub priority: i32,
    pub weight: f64,
    pub enabled: bool,
    /// The provider has a stored connection; this does not report runtime health.
    pub provider_configured: bool,
}

pub(super) fn summarize_routes(
    routes: &[ModelRoute],
    providers: &HashMap<String, ProviderConnection>,
) -> Vec<AdminModelRouteSummary> {
    let mut ordered = routes.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|route| (route.priority, route.id));
    ordered
        .into_iter()
        .map(|route| {
            let provider = providers.get(&route.provider_key);
            let display = resolve_provider_display(&route.provider_key, provider);
            AdminModelRouteSummary {
                id: route.id.to_string(),
                provider_key: route.provider_key.clone(),
                provider_label: display.label,
                provider_icon_key: display.icon_key,
                upstream_model: route.upstream_model.clone(),
                priority: route.priority,
                weight: route.weight,
                enabled: route.enabled,
                provider_configured: provider.is_some(),
            }
        })
        .collect()
}
