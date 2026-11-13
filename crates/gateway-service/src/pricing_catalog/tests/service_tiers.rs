//! The service tiers a route can bill, read from the merged catalog snapshot.

use gateway_core::{RoutePricingOverride, RouteServiceTierPricing};

use super::support::*;
use crate::pricing_catalog::billable_service_tiers;

const NON_DEFAULT_TIERS: [ServiceTier; 3] = [
    ServiceTier::Flex,
    ServiceTier::Priority,
    ServiceTier::Ultrafast,
];

fn model_mut<'snapshot>(
    snapshot: &'snapshot mut PricingCatalogSnapshotFile,
    provider_id: &str,
    model_id: &str,
) -> &'snapshot mut PricingCatalogModelDocument {
    snapshot
        .providers
        .get_mut(provider_id)
        .and_then(|provider| provider.models.get_mut(model_id))
        .expect("fixture model")
}

/// The fallback catalog with a priority schedule on OpenAI's GPT-5 and Vertex's Claude
/// Sonnet 4.6.
fn catalog_with_priority_schedules() -> PricingCatalogSnapshot {
    let mut snapshot = fallback_snapshot();
    for (provider_id, model_id) in [
        ("openai", "gpt-5"),
        ("google-vertex-anthropic", "claude-sonnet-4-6@default"),
    ] {
        model_mut(&mut snapshot, provider_id, model_id)
            .cost
            .service_tiers = BTreeMap::from([(
            ServiceTier::Priority,
            PricingCatalogServiceTierCost {
                input: Some("2.5000".to_string()),
                output: Some("20.0000".to_string()),
                ..PricingCatalogServiceTierCost::default()
            },
        )]);
    }
    merged(snapshot)
}

fn tier_rates(service_tier: ServiceTier) -> RouteServiceTierPricing {
    RouteServiceTierPricing {
        service_tier,
        input_cost_per_million_tokens: Money4::from_scaled(10_000),
        output_cost_per_million_tokens: Money4::from_scaled(20_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
        cache_write_1h_cost_per_million_tokens: None,
    }
}

fn pricing_override(service_tiers: Vec<RouteServiceTierPricing>) -> RoutePricingOverride {
    RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(20_000),
        output_cost_per_million_tokens: Money4::from_scaled(40_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
        cache_write_1h_cost_per_million_tokens: None,
        image_output_cost_per_million_tokens: None,
        service_tiers,
    }
}

#[test]
fn catalog_priced_routes_bill_only_tiers_with_a_schedule() {
    let catalog = catalog_with_priority_schedules();
    let openai = openai_provider("openai");
    let mut forced_flex = route("openai-prod", "gpt-5");
    forced_flex.service_tier = Some(ServiceTier::Flex);

    assert_eq!(
        billable_service_tiers(&catalog, &openai, &route("openai-prod", "gpt-5")),
        [ServiceTier::Priority]
    );
    assert_eq!(
        billable_service_tiers(&catalog, &openai, &forced_flex),
        [ServiceTier::Priority],
        "a forced tier needs a catalog schedule too"
    );
    assert_eq!(
        billable_service_tiers(
            &catalog,
            &vertex_provider("global"),
            &route("vertex-prod", "anthropic/claude-sonnet-4-6"),
        ),
        [ServiceTier::Priority],
        "the `@default` fallback id is read as pricing resolution reads it"
    );
    assert!(
        billable_service_tiers(
            &catalog,
            &bedrock_provider(),
            &route("bedrock-prod", "us.anthropic.claude-sonnet-4-6"),
        )
        .is_empty(),
        "a model priced at default alone bills no other tier"
    );
}

#[test]
fn routes_without_a_default_price_bill_every_tier() {
    let mut snapshot = fallback_snapshot();
    model_mut(&mut snapshot, "openai", "gpt-5").cost = PricingCatalogCostDocument::default();
    let catalog = merged(snapshot);
    let openai = openai_provider("openai");

    assert_eq!(
        billable_service_tiers(&catalog, &openai, &route("openai-prod", "gpt-5")),
        NON_DEFAULT_TIERS,
        "the model has no base rate"
    );
    assert_eq!(
        billable_service_tiers(&catalog, &openai, &route("openai-prod", "gpt-unknown")),
        NON_DEFAULT_TIERS,
        "the catalog has no such model"
    );
    assert_eq!(
        billable_service_tiers(
            &catalog,
            &vertex_provider("us-central1"),
            &route("vertex-prod", "anthropic/claude-sonnet-4-6"),
        ),
        NON_DEFAULT_TIERS,
        "the catalog does not price this location"
    );
}

#[test]
fn reported_cost_routes_bill_every_tier() {
    assert_eq!(
        billable_service_tiers(
            &catalog_with_priority_schedules(),
            &openai_provider("openrouter"),
            &route("openrouter", "deepseek/deepseek-v4-flash"),
        ),
        NON_DEFAULT_TIERS
    );
}

#[test]
fn override_routes_bill_their_own_tier_and_the_tiers_they_list() {
    let catalog = catalog_with_priority_schedules();
    let openai = openai_provider("openai");
    let mut forced_priority = route("openai-prod", "gpt-5");
    forced_priority.service_tier = Some(ServiceTier::Priority);
    forced_priority.pricing_override = Some(pricing_override(vec![
        tier_rates(ServiceTier::Default),
        tier_rates(ServiceTier::Flex),
    ]));
    let mut default_route = route("openai-prod", "gpt-5");
    default_route.pricing_override = Some(pricing_override(Vec::new()));

    assert_eq!(
        billable_service_tiers(&catalog, &openai, &forced_priority),
        [ServiceTier::Flex, ServiceTier::Priority]
    );
    assert!(
        billable_service_tiers(&catalog, &openai, &default_route).is_empty(),
        "an override replaces the catalog's tier schedules"
    );
}
