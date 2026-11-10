//! Service tiers on inference requests. The caller's tier must be on the model's
//! allow-list. Each route then serves its forced tier, else the caller's, and stays
//! eligible only when its adapter can send that tier and its pricing can bill it.
use gateway_core::{
    GatewayError, ModelRoute, ProviderRegistry, ServiceTier, ServiceTierOutcome,
    enforce_service_tier_policy,
};
use gateway_service::{RequestLogContext, ResolvedGatewayRequest};

/// Applies the requested model's tier allow-list to the caller's tier. An alias with
/// its own `service_tiers` uses that list; otherwise the execution model's applies.
/// A route's forced tier is the operator's choice, so it is not checked here.
pub(super) fn enforce_requested_service_tier(
    resolved: &ResolvedGatewayRequest,
    requested: Option<ServiceTier>,
) -> Result<(), GatewayError> {
    let selection = &resolved.selection;
    let allowed = selection
        .requested_model
        .service_tiers
        .as_deref()
        .or(selection.execution_model.service_tiers.as_deref());
    enforce_service_tier_policy(&selection.requested_model.model_key, allowed, requested)
}

/// The tier a request on `route` asks the provider for: the route's forced tier, else
/// the caller's.
pub(super) fn effective_service_tier(
    route: &ModelRoute,
    requested: Option<ServiceTier>,
) -> Option<ServiceTier> {
    route.service_tier.or(requested)
}

/// The requested and served tiers to account for. The requested tier is the one the
/// request log context recorded for the chosen route.
pub(super) fn service_tier_outcome(
    request_log_context: &RequestLogContext,
    served: Option<ServiceTier>,
) -> ServiceTierOutcome {
    ServiceTierOutcome {
        requested: request_log_context.service_tier,
        served,
    }
}

/// Keeps the routes that can serve the tier a request would use on them. Fails with a
/// 400 that names the tier when no route in a non-empty `routes` qualifies.
pub(super) fn routes_serving_service_tier(
    providers: &ProviderRegistry,
    resolved: &ResolvedGatewayRequest,
    routes: Vec<ModelRoute>,
    requested: Option<ServiceTier>,
) -> Result<Vec<ModelRoute>, GatewayError> {
    if routes.is_empty() {
        return Ok(routes);
    }
    let Some(named_tier) = requested.or_else(|| routes.iter().find_map(|route| route.service_tier))
    else {
        return Ok(routes);
    };
    let serving = routes
        .into_iter()
        .filter(|route| route_serves_service_tier(providers, resolved, route, requested))
        .collect::<Vec<_>>();
    if serving.is_empty() {
        return Err(GatewayError::InvalidRequest(format!(
            "no configured route for model `{}` supports service tier `{}`",
            resolved.selection.requested_model.model_key,
            named_tier.as_str()
        )));
    }
    Ok(serving)
}

fn route_serves_service_tier(
    providers: &ProviderRegistry,
    resolved: &ResolvedGatewayRequest,
    route: &ModelRoute,
    requested: Option<ServiceTier>,
) -> bool {
    let Some(tier) = effective_service_tier(route, requested) else {
        return true;
    };
    providers
        .get(&route.provider_key)
        .is_some_and(|provider| provider.service_tiers(route).contains(&tier))
        && resolved.route_bills_service_tier(route, tier)
}
