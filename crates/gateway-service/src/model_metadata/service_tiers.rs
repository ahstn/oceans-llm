//! The service tiers discovery advertises: the tiers a request for a model can be served at,
//! by the same rules routing applies, and the subset a caller may ask for.
use std::collections::BTreeSet;

use gateway_core::{
    GatewayModel, ModelRoute, ProviderCapabilities, ProviderConnection, ProviderRegistry,
    ServiceTier, ServiceTierApi,
};

use crate::pricing_catalog::{PricingCatalogSnapshot, billable_service_tiers};

/// The tiers each provider adapter can send for a route on an API. The gateway's provider
/// registry answers in production; discovery reads it through this trait so listings stay
/// testable without real adapters.
pub trait AdapterServiceTiers: Send + Sync {
    fn adapter_service_tiers(
        &self,
        route: &ModelRoute,
        api: ServiceTierApi,
    ) -> &'static [ServiceTier];
}

impl AdapterServiceTiers for ProviderRegistry {
    fn adapter_service_tiers(
        &self,
        route: &ModelRoute,
        api: ServiceTierApi,
    ) -> &'static [ServiceTier] {
        // A route whose provider has no loaded client serves no request at any tier.
        match self.get(&route.provider_key) {
            Some(provider) => provider.service_tiers(route, api),
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

/// Whether the allow-list lets a caller ask for `tier`, as `enforce_service_tier_policy`
/// decides: `default` always, another tier when `allowed` is `None` or lists it.
fn may_request(allowed: Option<&[ServiceTier]>, tier: ServiceTier) -> bool {
    tier == ServiceTier::Default || allowed.is_none_or(|allowed| allowed.contains(&tier))
}

/// The union of the tiers each route serves, ascending. A route serves its forced tier, or
/// else default plus the tiers `allowed` permits; in both cases only tiers its adapter can
/// send on at least one API the route serves, and non-default tiers only when routing can
/// bill them (a catalog or override rate, provider-reported cost, or no price at all).
pub(crate) fn servable_service_tiers<'a>(
    routes: impl IntoIterator<Item = (&'a ModelRoute, &'a ProviderConnection, ProviderCapabilities)>,
    allowed: Option<&[ServiceTier]>,
    snapshot: &PricingCatalogSnapshot,
    adapter: &dyn AdapterServiceTiers,
) -> Vec<ServiceTier> {
    let mut servable = BTreeSet::new();
    for (route, provider, capabilities) in routes {
        let sendable = |tier: ServiceTier| {
            ServiceTierApi::served_by(capabilities)
                .any(|api| adapter.adapter_service_tiers(route, api).contains(&tier))
        };
        let requestable = |tier: ServiceTier| match route.service_tier {
            Some(forced) => tier == forced,
            None => may_request(allowed, tier),
        };
        let candidates = ServiceTier::ALL
            .into_iter()
            .filter(|&tier| requestable(tier) && sendable(tier))
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

/// The `servable` tiers a caller may ask for, ascending. A tier outside the allow-list is
/// servable only through a route that forces it, and an explicit request for it fails with
/// 403, so harness variants must not offer it.
pub(crate) fn selectable_service_tiers(
    servable: &[ServiceTier],
    allowed: Option<&[ServiceTier]>,
) -> Vec<ServiceTier> {
    servable
        .iter()
        .copied()
        .filter(|&tier| may_request(allowed, tier))
        .collect()
}

/// The tiers worth advertising: `None` when a request can only be served at default.
pub(super) fn advertised_service_tiers(servable: &[ServiceTier]) -> Option<Vec<ServiceTier>> {
    servable
        .iter()
        .any(|&tier| tier != ServiceTier::Default)
        .then(|| servable.to_vec())
}
