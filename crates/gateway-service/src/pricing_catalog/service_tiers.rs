//! The service tiers a route has a rate for. A request for a non-default tier routes only
//! where usage accounting can price that tier, so premium traffic is never billed at
//! default rates.

use gateway_core::{ModelRoute, ProviderConnection, ServiceTier};

use super::{
    PricingCatalogModelDocument, PricingCatalogSnapshot,
    target::{
        PricingTarget, catalog_model_id_fallback, pricing_target_for_route,
        route_reports_request_cost,
    },
};

/// The non-default tiers `route` can bill, ascending. Default always bills, so it is never
/// listed. The caller still applies adapter support and the model's tier allow-list.
///
/// - A provider that reports each request's cost bills every tier.
/// - A pricing override bills the route's own forced tier and every tier it lists.
/// - A route the catalog does not price at default bills every tier, since a tier adds no
///   gap the default lacks.
/// - Otherwise a tier bills when the catalog model has a schedule for it.
pub(crate) fn billable_service_tiers(
    snapshot: &PricingCatalogSnapshot,
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> Vec<ServiceTier> {
    let premium = ServiceTier::ALL
        .into_iter()
        .filter(|tier| *tier != ServiceTier::Default);
    if route_reports_request_cost(provider, route) {
        return premium.collect();
    }
    if let Some(pricing) = &route.pricing_override {
        return premium
            .filter(|tier| {
                route.service_tier == Some(*tier)
                    || pricing
                        .service_tiers
                        .iter()
                        .any(|rates| rates.service_tier == *tier)
            })
            .collect();
    }
    match catalog_model(snapshot, provider, route) {
        Some(model) if model.cost.input.is_some() || model.cost.output.is_some() => premium
            .filter(|tier| model.cost.service_tiers.contains_key(tier))
            .collect(),
        _ => premium.collect(),
    }
}

/// The merged catalog model billing prices `route` from, trying the exact id before its
/// [`catalog_model_id_fallback`] as pricing resolution does.
fn catalog_model<'a>(
    snapshot: &'a PricingCatalogSnapshot,
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> Option<&'a PricingCatalogModelDocument> {
    let PricingTarget::Exact {
        pricing_provider_id,
        model_id,
    } = pricing_target_for_route(provider, route)
    else {
        return None;
    };
    snapshot.model(&pricing_provider_id, &model_id).or_else(|| {
        catalog_model_id_fallback(&pricing_provider_id, &model_id)
            .and_then(|fallback| snapshot.model(&pricing_provider_id, &fallback))
    })
}
