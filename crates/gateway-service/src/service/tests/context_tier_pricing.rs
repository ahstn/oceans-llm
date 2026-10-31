//! Catalog usage pricing with context tiers and image output rates.

use gateway_core::ContextPricingTier;
use serde_json::Value;

use super::*;
use crate::service::select_context_tier;

fn tier(above_input_tokens: i64) -> ContextPricingTier {
    ContextPricingTier {
        above_input_tokens,
        input: None,
        output: None,
        cache_read: None,
        cache_write: None,
    }
}

/// $1 input, $2 output, $0.10 cache read per million, and above 1,000 prompt tokens $3
/// input and $6 output with the cache read rate left to the base.
fn tiered_pricing_record() -> ModelPricingRecord {
    let mut record = pricing_record(None);
    record.cache_read_cost_per_million_tokens = Some(Money4::from_scaled(1_000));
    record.context_tiers = vec![ContextPricingTier {
        input: Some(Money4::from_scaled(30_000)),
        output: Some(Money4::from_scaled(60_000)),
        ..tier(1_000)
    }];
    record
}

fn catalog_route() -> ModelRoute {
    let mut route = vertex_embedding_route(Uuid::new_v4());
    route.upstream_model = "gpt-5".to_string();
    route
}

async fn recorded_catalog_usage(
    route: &ModelRoute,
    pricing: ModelPricingRecord,
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
            "req_context_tier",
            Some(provider_usage),
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("usage should be recorded");
    let events = repo.events.lock().expect("events lock");
    events.first().expect("usage event").clone()
}

#[test]
fn context_tier_applies_only_strictly_above_its_threshold() {
    let tiers = [tier(200_000), tier(272_000)];
    let selected = |prompt_tokens| {
        select_context_tier(&tiers, prompt_tokens).map(|tier| tier.above_input_tokens)
    };

    assert_eq!(selected(Some(200_000)), None);
    assert_eq!(selected(Some(200_001)), Some(200_000));
    assert_eq!(selected(Some(271_999)), Some(200_000));
    assert_eq!(selected(Some(272_000)), Some(200_000));
    assert_eq!(selected(Some(272_001)), Some(272_000));
    assert_eq!(selected(None), None);
    assert_eq!(
        select_context_tier(&[tier(272_000), tier(200_000)], Some(300_000))
            .map(|tier| tier.above_input_tokens),
        Some(272_000),
        "the highest matching threshold wins"
    );
}

#[tokio::test]
async fn inclusive_cache_counters_crossing_the_threshold_bill_the_whole_request_at_the_tier() {
    let event = recorded_catalog_usage(
        &catalog_route(),
        tiered_pricing_record(),
        json!({
            "input_tokens": 1_200,
            "output_tokens": 100,
            "input_tokens_details": {"cached_tokens": 900}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.pricing_tier_above_input_tokens, Some(1_000));
    assert_eq!(
        event.input_cost_per_million_tokens,
        Some(Money4::from_scaled(30_000))
    );
    assert_eq!(
        event.output_cost_per_million_tokens,
        Some(Money4::from_scaled(60_000))
    );
    assert_eq!(
        event.cache_read_cost_per_million_tokens,
        Some(Money4::from_scaled(1_000)),
        "an unset tier rate inherits the base rate"
    );
    // 300 uncached at $3 ($0.0009) + 900 cached at $0.10 ($0.0001) + 100 output at $6
    // ($0.0006).
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(16));
}

#[tokio::test]
async fn exclusive_cache_counters_select_the_tier_from_the_normalized_prompt() {
    // Anthropic-shaped usage: 300 uncached input tokens plus 900 cache reads.
    let event = recorded_catalog_usage(
        &catalog_route(),
        tiered_pricing_record(),
        json!({
            "prompt_tokens": 300,
            "completion_tokens": 100,
            "provider_usage": {"input_tokens": 300, "cache_read_input_tokens": 900}
        }),
    )
    .await;

    assert_eq!(event.prompt_tokens, Some(1_200));
    assert_eq!(event.pricing_tier_above_input_tokens, Some(1_000));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(16));
}

#[tokio::test]
async fn prompt_at_the_threshold_bills_base_rates() {
    let event = recorded_catalog_usage(
        &catalog_route(),
        tiered_pricing_record(),
        json!({
            "input_tokens": 1_000,
            "output_tokens": 100,
            "input_tokens_details": {"cached_tokens": 900}
        }),
    )
    .await;

    assert_eq!(event.pricing_tier_above_input_tokens, None);
    assert_eq!(
        event.input_cost_per_million_tokens,
        Some(Money4::from_scaled(10_000))
    );
    // 100 uncached at $1 ($0.0001) + 900 cached at $0.10 ($0.0001) + 100 output at $2
    // ($0.0002).
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(4));
}

#[tokio::test]
async fn route_pricing_override_ignores_catalog_tiers() {
    let mut route = catalog_route();
    route.pricing_override = Some(RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(10_000),
        output_cost_per_million_tokens: Money4::from_scaled(20_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
        image_output_cost_per_million_tokens: None,
    });
    let event = recorded_catalog_usage(
        &route,
        tiered_pricing_record(),
        json!({"prompt_tokens": 2_000, "completion_tokens": 100}),
    )
    .await;

    assert_eq!(event.pricing_source.as_deref(), Some("configured_override"));
    assert_eq!(event.pricing_tier_above_input_tokens, None);
    // 2,000 input at $1 ($0.002) + 100 output at $2 ($0.0002).
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(22));
}

#[tokio::test]
async fn provider_reported_cost_wins_over_catalog_tiers() {
    let event = recorded_catalog_usage(
        &catalog_route(),
        tiered_pricing_record(),
        json!({"prompt_tokens": 2_000, "completion_tokens": 100, "cost": 0.0123}),
    )
    .await;

    assert_eq!(event.pricing_source.as_deref(), Some("provider_reported"));
    assert_eq!(event.pricing_tier_above_input_tokens, None);
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(123));
}

#[tokio::test]
async fn image_output_tokens_are_charged_at_the_catalog_image_rate() {
    let mut pricing = pricing_record(None);
    pricing.output_image_usd_per_million = Some(Money4::from_scaled(300_000));
    let event = recorded_catalog_usage(
        &catalog_route(),
        pricing,
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 1_200,
            "completion_tokens_details": {"image_tokens": 1_000}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(
        event.image_output_cost_per_million_tokens,
        Some(Money4::from_scaled(300_000))
    );
    // 1,000 input at $1 ($0.001) + 200 text at $2 ($0.0004) + 1,000 image at $30 ($0.03).
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(314));
}
