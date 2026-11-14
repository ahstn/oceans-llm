//! Checks of route pricing and context overrides against the catalog: a startup check that fails
//! on a context window above the catalog limit, and a background pass that warns about overrides
//! that only repeat the catalog.

use std::collections::HashMap;

use gateway_core::{
    BudgetAlertRepository, BudgetRepository, GatewayError, IdentityRepository,
    McpToolInvocationRepository, ModelRepository, ModelRoute, Money4, PricingCatalogRepository,
    ProviderConnection, ProviderRepository, RequestLogRepository, RoutePlanner,
    RoutePricingOverride, ServiceTier, StoreHealth,
};
use serde_json::Value;
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use super::GatewayService;
use crate::{EffectiveRoutePricing, effective_route_metadata::resolve_effective_route_metadata};

/// Enabled routes that carry a pricing or context override, paired with their model key.
#[derive(Debug, Default)]
pub struct OverriddenRoutes {
    routes: Vec<(String, ModelRoute)>,
    /// Catalog values already resolved for these routes, so later checks reuse them.
    lookups: CatalogLookups,
}

/// The catalog values the checks compare against, for one lookup target.
#[derive(Debug, Clone)]
struct CatalogValue {
    pricing: Option<EffectiveRoutePricing>,
    image_output_rate: Option<Money4>,
    context: Option<i64>,
}

/// What one catalog lookup depends on: provider, upstream model, `extra_body` billing modifiers,
/// and the route's forced service tier, which selects the catalog schedule.
type CatalogTarget = (String, String, String, Option<ServiceTier>);

/// The checks read only catalog values, which depend on the [`CatalogTarget`], so routes sharing
/// one share a lookup, and each provider is read once.
#[derive(Debug, Default)]
struct CatalogLookups {
    providers: HashMap<String, Option<ProviderConnection>>,
    /// `None` marks a target whose lookup failed, so it is tried and reported once.
    values: HashMap<CatalogTarget, Option<CatalogValue>>,
}

impl CatalogLookups {
    fn target(route: &ModelRoute) -> CatalogTarget {
        (
            route.provider_key.clone(),
            route.upstream_model.clone(),
            Value::Object(route.extra_body.clone()).to_string(),
            route.service_tier,
        )
    }

    /// The catalog values for `route`, resolved on first use. `Ok(None)` means an earlier lookup
    /// for the same target failed.
    async fn get<S>(
        &mut self,
        store: &S,
        route: &ModelRoute,
        now: OffsetDateTime,
    ) -> Result<Option<CatalogValue>, GatewayError>
    where
        S: ProviderRepository + PricingCatalogRepository + Send + Sync + 'static,
    {
        let target = Self::target(route);
        if let Some(value) = self.values.get(&target) {
            return Ok(value.clone());
        }
        if !self.providers.contains_key(&route.provider_key) {
            let provider = store.get_provider_by_key(&route.provider_key).await?;
            self.providers.insert(route.provider_key.clone(), provider);
        }
        let provider = self
            .providers
            .get(&route.provider_key)
            .and_then(Option::as_ref);
        let metadata = resolve_effective_route_metadata(store, provider, route, now).await?;
        let value = CatalogValue {
            pricing: metadata.catalog_pricing,
            image_output_rate: metadata.catalog_image_output_cost_per_million_tokens,
            context: metadata.catalog_limits.context,
        };
        self.values.insert(target, Some(value.clone()));
        Ok(Some(value))
    }

    fn mark_failed(&mut self, route: &ModelRoute) {
        self.values.insert(Self::target(route), None);
    }
}

/// A route override that repeats the catalog value it replaces.
#[derive(Debug)]
struct RedundantRouteOverride {
    model_key: String,
    provider_key: String,
    upstream_model: String,
    pricing: bool,
    context_window_tokens: Option<i64>,
}

#[derive(Debug)]
struct RouteContextOverrideConflict {
    route_id: Uuid,
    provider_key: String,
    upstream_model: String,
    configured_context: i64,
    catalog_context: i64,
}

fn pricing_override_matches_catalog(
    configured: &RoutePricingOverride,
    catalog: &EffectiveRoutePricing,
    catalog_image_output_rate: Option<Money4>,
) -> bool {
    catalog.input_cost_per_million_tokens == Some(configured.input_cost_per_million_tokens)
        && catalog.output_cost_per_million_tokens == Some(configured.output_cost_per_million_tokens)
        // The override replaces catalog pricing wholesale, so an omitted cache rate leaves cache
        // tokens unpriced while the catalog would charge them. Only identical rates are redundant.
        && catalog.cache_read_cost_per_million_tokens
            == configured.cache_read_cost_per_million_tokens
        && catalog.cache_write_cost_per_million_tokens
            == configured.cache_write_cost_per_million_tokens
        && catalog.cache_write_1h_cost_per_million_tokens
            == configured.cache_write_1h_cost_per_million_tokens
        // Each side bills image tokens at its own image rate, else at its output rate.
        && configured
            .image_output_cost_per_million_tokens
            .or(Some(configured.output_cost_per_million_tokens))
            == catalog_image_output_rate.or(catalog.output_cost_per_million_tokens)
        // An override replaces the catalog's whole schedule, so it drops any context tiers and
        // the catalog's service tier rates, and its own tier rates have no catalog match.
        && catalog.context_tiers.is_empty()
        && catalog.service_tiers.is_empty()
        && configured.service_tiers.is_empty()
}

impl<S, P> GatewayService<S, P>
where
    S: gateway_core::ApiKeyRepository
        + BudgetAlertRepository
        + BudgetRepository
        + ModelRepository
        + IdentityRepository
        + PricingCatalogRepository
        + RequestLogRepository
        + McpToolInvocationRepository
        + ProviderRepository
        + StoreHealth
        + Send
        + Sync
        + 'static,
    P: RoutePlanner + Send + Sync + 'static,
{
    /// Startup check that fails on a context window above the catalog limit. It reads models
    /// and routes once and resolves catalog values only for context overrides, so pricing
    /// overrides add no catalog lookups before readiness. The returned routes feed
    /// [`Self::warn_on_redundant_route_overrides`] without reading the route table again.
    pub async fn validate_route_overrides(&self) -> Result<OverriddenRoutes, GatewayError> {
        let mut routes = self.collect_overridden_routes().await?;
        let Some(conflict) = self
            .context_conflicts(&mut routes)
            .await?
            .into_iter()
            .next()
        else {
            return Ok(routes);
        };

        Err(GatewayError::InvalidRequest(format!(
            "route `{}` ({}/{}) context_window_tokens `{}` exceeds catalog context `{}`",
            conflict.route_id,
            conflict.provider_key,
            conflict.upstream_model,
            conflict.configured_context,
            conflict.catalog_context,
        )))
    }

    /// Logs route overrides that only repeat the catalog, so admins can delete them and follow
    /// catalog updates. It only warns, so the gateway runs it in the background after startup,
    /// over the routes [`Self::validate_route_overrides`] already collected, reusing the catalog
    /// values it resolved for context overrides.
    pub async fn warn_on_redundant_route_overrides(&self, mut routes: OverriddenRoutes) {
        for redundant in self.redundant_overrides(&mut routes).await {
            if redundant.pricing {
                warn!(
                    model_key = %redundant.model_key,
                    provider_key = %redundant.provider_key,
                    upstream_model = %redundant.upstream_model,
                    "route pricing_override equals the catalog price; remove it to follow catalog updates"
                );
            }
            if let Some(context_window_tokens) = redundant.context_window_tokens {
                warn!(
                    model_key = %redundant.model_key,
                    provider_key = %redundant.provider_key,
                    upstream_model = %redundant.upstream_model,
                    context_window_tokens,
                    "route context_window_tokens equals the catalog context; remove it to follow catalog updates"
                );
            }
        }
    }

    pub(super) async fn warn_on_route_context_override_conflicts(
        &self,
    ) -> Result<(), GatewayError> {
        let mut routes = self.collect_overridden_routes().await?;
        for conflict in self.context_conflicts(&mut routes).await? {
            warn!(
                route_id = %conflict.route_id,
                provider_key = %conflict.provider_key,
                upstream_model = %conflict.upstream_model,
                configured_context_window_tokens = conflict.configured_context,
                catalog_context_window_tokens = conflict.catalog_context,
                "configured route context exceeds the current catalog limit; using catalog limit"
            );
        }
        Ok(())
    }

    /// Enabled routes with a pricing or context override, with their model keys, in one pass
    /// over models and routes.
    async fn collect_overridden_routes(&self) -> Result<OverriddenRoutes, GatewayError> {
        let mut routes = Vec::new();
        for model in self.store.list_models().await? {
            for route in self.store.list_routes_for_model(model.id).await? {
                // Disabled rows include batch-referenced routes that config no longer declares.
                if !route.enabled || route.weight <= 0.0 {
                    continue;
                }
                if route.context_window_tokens.is_some() || route.pricing_override.is_some() {
                    routes.push((model.model_key.clone(), route));
                }
            }
        }
        Ok(OverriddenRoutes {
            routes,
            lookups: CatalogLookups::default(),
        })
    }

    /// Context overrides above the catalog limit. A failed lookup is an error, because a context
    /// override must be checked before it is trusted.
    async fn context_conflicts(
        &self,
        routes: &mut OverriddenRoutes,
    ) -> Result<Vec<RouteContextOverrideConflict>, GatewayError> {
        let now = OffsetDateTime::now_utc();
        let mut conflicts = Vec::new();
        for (_, route) in &routes.routes {
            let Some(configured) = route.context_window_tokens else {
                continue;
            };
            let Some(CatalogValue {
                context: Some(catalog),
                ..
            }) = routes.lookups.get(self.store.as_ref(), route, now).await?
            else {
                continue;
            };
            if configured > catalog {
                conflicts.push(RouteContextOverrideConflict {
                    route_id: route.id,
                    provider_key: route.provider_key.clone(),
                    upstream_model: route.upstream_model.clone(),
                    configured_context: configured,
                    catalog_context: catalog,
                });
            }
        }
        Ok(conflicts)
    }

    /// Pricing and context overrides that equal the catalog value they replace. This only feeds
    /// warnings, so a failed lookup is logged once per target and its routes are skipped.
    async fn redundant_overrides(
        &self,
        routes: &mut OverriddenRoutes,
    ) -> Vec<RedundantRouteOverride> {
        let now = OffsetDateTime::now_utc();
        let mut redundant = Vec::new();
        for (model_key, route) in &routes.routes {
            let catalog = match routes.lookups.get(self.store.as_ref(), route, now).await {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(error) => {
                    warn!(
                        error = %error,
                        model_key = %model_key,
                        provider_key = %route.provider_key,
                        upstream_model = %route.upstream_model,
                        "skipping redundant route override check; route metadata lookup failed"
                    );
                    routes.lookups.mark_failed(route);
                    continue;
                }
            };
            let pricing = matches!(
                (route.pricing_override.as_ref(), catalog.pricing.as_ref()),
                (Some(configured), Some(pricing))
                    if pricing_override_matches_catalog(configured, pricing, catalog.image_output_rate)
            );
            let context_window_tokens = route
                .context_window_tokens
                .filter(|configured| Some(*configured) == catalog.context);
            if pricing || context_window_tokens.is_some() {
                redundant.push(RedundantRouteOverride {
                    model_key: model_key.clone(),
                    provider_key: route.provider_key.clone(),
                    upstream_model: route.upstream_model.clone(),
                    pricing,
                    context_window_tokens,
                });
            }
        }
        redundant
    }
}

#[cfg(test)]
mod tests {
    use gateway_core::{
        ContextPricingTier, Money4, RoutePricingOverride, RouteServiceTierPricing, ServiceTier,
        ServiceTierPricing,
    };

    use super::pricing_override_matches_catalog;
    use crate::EffectiveRoutePricing;

    fn money(value: &str) -> Money4 {
        Money4::from_decimal_str(value).expect("money")
    }

    #[test]
    fn pricing_override_matches_only_identical_catalog_rates() {
        let configured = RoutePricingOverride {
            input_cost_per_million_tokens: money("0.15"),
            output_cost_per_million_tokens: money("0.60"),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            cache_write_1h_cost_per_million_tokens: None,
            image_output_cost_per_million_tokens: None,
            service_tiers: Vec::new(),
        };
        let mut catalog = EffectiveRoutePricing {
            input_cost_per_million_tokens: Some(money("0.1500")),
            output_cost_per_million_tokens: Some(money("0.6000")),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            cache_write_1h_cost_per_million_tokens: None,
            context_tiers: Vec::new(),
            service_tiers: Vec::new(),
        };
        let matches = |configured: &RoutePricingOverride,
                       catalog: &EffectiveRoutePricing,
                       image: Option<Money4>| {
            pricing_override_matches_catalog(configured, catalog, image)
        };
        assert!(matches(&configured, &catalog, None));

        // Removing an override that omits a catalog cache rate would start charging cache tokens.
        catalog.cache_read_cost_per_million_tokens = Some(money("0.01"));
        assert!(!matches(&configured, &catalog, None));
        catalog.cache_read_cost_per_million_tokens = None;
        catalog.cache_write_1h_cost_per_million_tokens = Some(money("0.03"));
        assert!(!matches(&configured, &catalog, None));
        catalog.cache_write_1h_cost_per_million_tokens = None;

        catalog.output_cost_per_million_tokens = Some(money("0.64"));
        assert!(!matches(&configured, &catalog, None));
        catalog.output_cost_per_million_tokens = Some(money("0.60"));

        let with_cache = RoutePricingOverride {
            cache_read_cost_per_million_tokens: Some(money("0.02")),
            ..configured.clone()
        };
        assert!(!matches(&with_cache, &catalog, None));

        // Each side bills image tokens at its image rate, else its output rate.
        let with_image_rate = RoutePricingOverride {
            image_output_cost_per_million_tokens: Some(money("1.00")),
            ..configured.clone()
        };
        assert!(!matches(&with_image_rate, &catalog, None));
        assert!(matches(&with_image_rate, &catalog, Some(money("1.00"))));
        assert!(
            !matches(&configured, &catalog, Some(money("1.00"))),
            "the catalog's own image rate would bill differently"
        );

        // An override replaces the catalog's service tier rates.
        catalog.service_tiers.push(ServiceTierPricing {
            service_tier: ServiceTier::Priority,
            input: Some(money("0.30")),
            output: Some(money("1.20")),
            cache_read: None,
            cache_write: None,
            cache_write_1h: None,
            context_tiers: Vec::new(),
        });
        assert!(!matches(&configured, &catalog, None));
        catalog.service_tiers.clear();
        let with_tier = RoutePricingOverride {
            service_tiers: vec![RouteServiceTierPricing {
                service_tier: ServiceTier::Priority,
                input_cost_per_million_tokens: money("0.30"),
                output_cost_per_million_tokens: money("1.20"),
                cache_read_cost_per_million_tokens: None,
                cache_write_cost_per_million_tokens: None,
                cache_write_1h_cost_per_million_tokens: None,
            }],
            ..configured.clone()
        };
        assert!(!matches(&with_tier, &catalog, None));

        // The override drops the catalog's context tiers, so long prompts would bill differently.
        catalog.context_tiers.push(ContextPricingTier {
            above_input_tokens: 200_000,
            input: Some(money("0.30")),
            output: Some(money("1.20")),
            cache_read: None,
            cache_write: None,
            cache_write_1h: None,
        });
        assert!(!matches(&configured, &catalog, None));
    }
}
