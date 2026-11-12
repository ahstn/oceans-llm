//! Startup check that no stored route selects a service tier through its raw
//! `extra_body` or `extra_headers`.

use gateway_core::{GatewayError, ModelRepository, ServiceTierPassthrough};

use super::GatewayService;

impl<S, P> GatewayService<S, P>
where
    S: ModelRepository + Send + Sync + 'static,
{
    /// Rejects a selectable stored route whose `extra_body` or `extra_headers` selects a
    /// service tier. Config load rejects these settings, but a route seeded by an earlier
    /// release keeps them until the config is seeded again. Serving that route would send
    /// a tier the gateway neither routes nor prices for, and would record no tier.
    ///
    /// # Errors
    ///
    /// Returns `InvalidRequest` naming the first such route, or the store's error.
    pub async fn validate_route_tier_passthrough(&self) -> Result<(), GatewayError> {
        for model in self.store.list_models().await? {
            for route in self.store.list_routes_for_model(model.id).await? {
                if !route.enabled || route.weight <= 0.0 {
                    continue;
                }
                if let Some(passthrough) =
                    ServiceTierPassthrough::find(&route.extra_body, route.extra_headers.keys())
                {
                    return Err(GatewayError::InvalidRequest(format!(
                        "model `{}` route `{}` ({}/{}) {passthrough}; set the route's `service_tier` in the config and seed it again with `gateway seed-config`",
                        model.model_key, route.id, route.provider_key, route.upstream_model,
                    )));
                }
            }
        }
        Ok(())
    }
}
