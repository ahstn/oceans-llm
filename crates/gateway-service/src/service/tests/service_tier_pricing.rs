//! Usage priced at the billed service tier: served, else requested, else default.

use gateway_core::{
    ContextPricingTier, RoutePricingOverride, RouteServiceTierPricing, ServiceTier,
    ServiceTierOutcome, ServiceTierPricing,
};
use serde_json::Value;

use super::*;

const PRIORITY: ServiceTierOutcome = ServiceTierOutcome {
    requested: Some(ServiceTier::Priority),
    served: None,
};

fn money(scaled: i64) -> Option<Money4> {
    Some(Money4::from_scaled(scaled))
}

/// A priority schedule at $2 input and $4 output per million, with no cache rates.
fn priority_schedule() -> ServiceTierPricing {
    ServiceTierPricing {
        service_tier: ServiceTier::Priority,
        input: money(20_000),
        output: money(40_000),
        cache_read: None,
        cache_write: None,
        context_tiers: Vec::new(),
    }
}

/// [`pricing_record`]'s $1 input and $2 output per million, plus the given tier schedules.
fn catalog_pricing(service_tiers: Vec<ServiceTierPricing>) -> ModelPricingRecord {
    ModelPricingRecord {
        service_tiers,
        ..pricing_record(None)
    }
}

fn route() -> ModelRoute {
    let mut route = vertex_embedding_route(Uuid::new_v4());
    route.upstream_model = "gpt-5".to_string();
    route
}

/// $5 input and $10 output per million for the route's own tier.
fn pricing_override(service_tiers: Vec<RouteServiceTierPricing>) -> RoutePricingOverride {
    RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(50_000),
        output_cost_per_million_tokens: Money4::from_scaled(100_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
        image_output_cost_per_million_tokens: None,
        service_tiers,
    }
}

fn usage(prompt_tokens: i64) -> Value {
    json!({"prompt_tokens": prompt_tokens, "completion_tokens": 500})
}

async fn recorded_usage(
    route: &ModelRoute,
    pricing: ModelPricingRecord,
    service_tiers: ServiceTierOutcome,
    provider_usage: Value,
) -> UsageLedgerRecord {
    let repo = Arc::new(UsageAccountingRepo {
        provider: Some(openai_provider(&route.provider_key)),
        pricing: Some(pricing),
        ..Default::default()
    });
    let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));
    service
        .record_chat_usage(
            &auth(),
            &model(route.model_id),
            route,
            "req_service_tier",
            Some(provider_usage),
            service_tiers,
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("usage should be recorded");
    let events = repo.events.lock().expect("events lock");
    events.first().expect("usage event").clone()
}

fn assert_priced(event: &UsageLedgerRecord, input: i64, output: i64, cost: i64) {
    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.unpriced_reason, None);
    assert_eq!(event.input_cost_per_million_tokens, money(input));
    assert_eq!(event.output_cost_per_million_tokens, money(output));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(cost));
}

fn assert_unpriced(event: &UsageLedgerRecord, reason: &str) {
    assert_eq!(event.pricing_status, UsagePricingStatus::Unpriced);
    assert_eq!(event.unpriced_reason.as_deref(), Some(reason));
    assert_eq!(event.computed_cost_usd, Money4::ZERO);
}

#[tokio::test]
async fn priority_usage_bills_the_catalog_priority_schedule() {
    let event = recorded_usage(
        &route(),
        catalog_pricing(vec![priority_schedule()]),
        PRIORITY,
        usage(1_000),
    )
    .await;

    // 1,000 input at $2 ($0.002) + 500 output at $4 ($0.002).
    assert_priced(&event, 20_000, 40_000, 40);
    assert_eq!(event.requested_service_tier, Some(ServiceTier::Priority));
}

#[tokio::test]
async fn a_tier_the_catalog_has_no_schedule_for_is_unpriced() {
    let event = recorded_usage(
        &route(),
        catalog_pricing(Vec::new()),
        PRIORITY,
        usage(1_000),
    )
    .await;

    assert_unpriced(&event, "service_tier:priority");
    assert_eq!(event.input_cost_per_million_tokens, None);
}

#[tokio::test]
async fn a_served_default_tier_bills_the_default_schedule() {
    let served_default = ServiceTierOutcome {
        served: Some(ServiceTier::Default),
        ..PRIORITY
    };
    let event = recorded_usage(
        &route(),
        catalog_pricing(vec![priority_schedule()]),
        served_default,
        usage(1_000),
    )
    .await;

    // 1,000 input at $1 ($0.001) + 500 output at $2 ($0.001).
    assert_priced(&event, 10_000, 20_000, 20);
}

#[tokio::test]
async fn a_used_bucket_without_a_tier_rate_is_unpriced_even_when_the_base_has_one() {
    let mut pricing = catalog_pricing(vec![priority_schedule()]);
    pricing.cache_read_cost_per_million_tokens = money(1_000);
    let event = recorded_usage(
        &route(),
        pricing,
        PRIORITY,
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 500,
            "prompt_tokens_details": {"cached_tokens": 400}
        }),
    )
    .await;

    assert_unpriced(&event, "service_tier:priority");
}

#[tokio::test]
async fn image_output_the_base_rates_apart_is_unpriced_at_a_tier() {
    let mut pricing = catalog_pricing(vec![priority_schedule()]);
    pricing.output_image_usd_per_million = money(400_000);
    let event = recorded_usage(
        &route(),
        pricing,
        PRIORITY,
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 500,
            "completion_tokens_details": {"image_tokens": 300}
        }),
    )
    .await;

    assert_unpriced(&event, "service_tier:priority");
}

#[tokio::test]
async fn a_base_context_tier_needs_a_twin_in_the_service_tier() {
    let base_context = ContextPricingTier {
        above_input_tokens: 1_000,
        input: money(30_000),
        output: None,
        cache_read: None,
        cache_write: None,
    };
    let mut pricing = catalog_pricing(vec![priority_schedule()]);
    pricing.context_tiers = vec![base_context.clone()];

    let below = recorded_usage(&route(), pricing.clone(), PRIORITY, usage(1_000)).await;
    assert_priced(&below, 20_000, 40_000, 40);
    let above = recorded_usage(&route(), pricing.clone(), PRIORITY, usage(1_200)).await;
    assert_unpriced(&above, "service_tier:priority");

    let mut with_twin = priority_schedule();
    with_twin.context_tiers = vec![ContextPricingTier {
        input: money(60_000),
        ..base_context
    }];
    pricing.service_tiers = vec![with_twin];
    let twinned = recorded_usage(&route(), pricing, PRIORITY, usage(1_200)).await;
    // 1,200 input at $6 ($0.0072) + 500 output at the tier's $4 ($0.002).
    assert_priced(&twinned, 60_000, 40_000, 92);
    assert_eq!(twinned.pricing_tier_above_input_tokens, Some(1_000));
}

#[tokio::test]
async fn override_rates_price_the_routes_own_tier_and_its_entries_the_others() {
    let mut forced = route();
    forced.service_tier = Some(ServiceTier::Priority);
    forced.pricing_override = Some(pricing_override(vec![RouteServiceTierPricing {
        service_tier: ServiceTier::Default,
        input_cost_per_million_tokens: Money4::from_scaled(10_000),
        output_cost_per_million_tokens: Money4::from_scaled(20_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
    }]));
    let served = |served| ServiceTierOutcome { served, ..PRIORITY };

    let own = recorded_usage(&forced, pricing_record(None), served(None), usage(1_000)).await;
    // 1,000 input at $5 ($0.005) + 500 output at $10 ($0.005).
    assert_priced(&own, 50_000, 100_000, 100);
    let default = recorded_usage(
        &forced,
        pricing_record(None),
        served(Some(ServiceTier::Default)),
        usage(1_000),
    )
    .await;
    assert_priced(&default, 10_000, 20_000, 20);
    let flex = recorded_usage(
        &forced,
        pricing_record(None),
        served(Some(ServiceTier::Flex)),
        usage(1_000),
    )
    .await;
    assert_unpriced(&flex, "service_tier:flex");
}

#[tokio::test]
async fn an_override_without_an_entry_leaves_a_requested_tier_unpriced() {
    let mut route = route();
    route.pricing_override = Some(pricing_override(Vec::new()));

    let event = recorded_usage(&route, pricing_record(None), PRIORITY, usage(1_000)).await;

    assert_unpriced(&event, "service_tier:priority");
}
