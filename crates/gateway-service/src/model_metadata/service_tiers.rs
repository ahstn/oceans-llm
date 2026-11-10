//! The service tiers discovery advertises: the tiers a request for a model can be served at,
//! by the same rules routing applies.
use std::collections::BTreeSet;

use gateway_core::{GatewayModel, ModelRoute, ProviderConnection, ProviderRegistry, ServiceTier};

use crate::pricing_catalog::{PricingCatalogSnapshot, billable_service_tiers};

/// The tiers each provider adapter can send for a route. The gateway's provider registry
/// answers in production; discovery reads it through this trait so listings stay testable
/// without real adapters.
pub trait AdapterServiceTiers: Send + Sync {
    fn adapter_service_tiers(&self, route: &ModelRoute) -> &'static [ServiceTier];
}

impl AdapterServiceTiers for ProviderRegistry {
    fn adapter_service_tiers(&self, route: &ModelRoute) -> &'static [ServiceTier] {
        // A route whose provider has no loaded client serves no request at any tier.
        match self.get(&route.provider_key) {
            Some(provider) => provider.service_tiers(route),
            None => &[],
        }
    }
}

/// The allow-list policy enforces: the requested model's own list, else its execution
/// model's. `None` allows every tier.
pub(crate) fn allowed_service_tiers<'a>(
    requested: &'a GatewayModel,
    execution: &'a GatewayModel,
) -> Option<&'a [ServiceTier]> {
    requested
        .service_tiers
        .as_deref()
        .or(execution.service_tiers.as_deref())
}

/// The union of the tiers each route serves, ascending. A route serves its forced tier, or
/// else default plus the tiers `allowed` permits; in both cases only tiers its adapter can
/// send, and non-default tiers only when routing can bill them (a catalog or override rate,
/// provider-reported cost, or no price at all).
pub(crate) fn servable_service_tiers<'a>(
    routes: impl IntoIterator<Item = (&'a ModelRoute, &'a ProviderConnection)>,
    allowed: Option<&[ServiceTier]>,
    snapshot: &PricingCatalogSnapshot,
    adapter: &dyn AdapterServiceTiers,
) -> Vec<ServiceTier> {
    let mut servable = BTreeSet::new();
    for (route, provider) in routes {
        let sendable = adapter.adapter_service_tiers(route);
        let requestable = |tier: ServiceTier| match route.service_tier {
            Some(forced) => tier == forced,
            None => {
                tier == ServiceTier::Default
                    || allowed.is_none_or(|allowed| allowed.contains(&tier))
            }
        };
        let candidates = ServiceTier::ALL
            .into_iter()
            .filter(|&tier| requestable(tier) && sendable.contains(&tier))
            .collect::<Vec<_>>();
        if candidates.iter().all(|&tier| tier == ServiceTier::Default) {
            servable.extend(candidates);
            continue;
        }
        let billable = billable_service_tiers(snapshot, provider, route);
        servable.extend(
            candidates
                .into_iter()
                .filter(|tier| *tier == ServiceTier::Default || billable.contains(tier)),
        );
    }
    servable.into_iter().collect()
}

/// The tiers worth advertising: `None` when a request can only be served at default.
pub(super) fn advertised_service_tiers(servable: &[ServiceTier]) -> Option<Vec<ServiceTier>> {
    servable
        .iter()
        .any(|&tier| tier != ServiceTier::Default)
        .then(|| servable.to_vec())
}
