//! Cache writes billed by TTL, and estimated costs for requests that the rates or the
//! provider's usage leave open.

use serde_json::Value;

use super::*;

/// Claude Sonnet's catalog rates: $3 input, $15 output, $0.30 cache reads and $3.75
/// 5-minute cache writes per million tokens. The catalog has no 1-hour write rate.
fn claude_pricing_record() -> ModelPricingRecord {
    let mut record = pricing_record(None);
    record.input_cost_per_million_tokens = Some(Money4::from_scaled(30_000));
    record.output_cost_per_million_tokens = Some(Money4::from_scaled(150_000));
    record.cache_read_cost_per_million_tokens = Some(Money4::from_scaled(3_000));
    record.cache_write_cost_per_million_tokens = Some(Money4::from_scaled(37_500));
    record
}

fn catalog_route() -> ModelRoute {
    let mut route = vertex_embedding_route(Uuid::new_v4());
    route.upstream_model = "gpt-5".to_string();
    route
}

async fn recorded_usage(
    route: &ModelRoute,
    provider: ProviderConnection,
    pricing: Option<ModelPricingRecord>,
    provider_usage: Option<Value>,
    usage_estimate: UsageEstimateInput,
) -> UsageLedgerRecord {
    let repo = Arc::new(UsageAccountingRepo {
        provider: Some(provider),
        pricing,
        ..Default::default()
    });
    let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));
    service
        .record_chat_usage(
            &auth(),
            &model(route.model_id),
            route,
            "req_cache_ttl_and_estimates",
            provider_usage,
            gateway_core::ServiceTierOutcome::default(),
            usage_estimate,
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("usage should be recorded");
    let events = repo.events.lock().expect("events lock");
    events.first().expect("usage event").clone()
}

/// Usage the provider reported on a route the catalog prices at `pricing`.
async fn recorded_catalog_usage(
    pricing: ModelPricingRecord,
    provider_usage: Value,
) -> UsageLedgerRecord {
    let route = catalog_route();
    let provider = openai_provider(&route.provider_key);
    recorded_usage(
        &route,
        provider,
        Some(pricing),
        Some(provider_usage),
        UsageEstimateInput::default(),
    )
    .await
}

/// A successful request the provider reported no usage for.
async fn recorded_usage_without_counts(
    pricing: Option<ModelPricingRecord>,
    usage_estimate: UsageEstimateInput,
) -> UsageLedgerRecord {
    let route = catalog_route();
    let provider = openai_provider(&route.provider_key);
    recorded_usage(&route, provider, pricing, None, usage_estimate).await
}

/// 1,000 input tokens, 2,000 tokens written to the 1-hour cache and 100 output tokens.
fn one_hour_write_usage() -> Value {
    json!({
        "input_tokens": 1_000,
        "output_tokens": 100,
        "cache_read_input_tokens": 0,
        "cache_creation_input_tokens": 2_000,
        "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 2_000}
    })
}

#[tokio::test]
async fn one_hour_cache_writes_without_a_rate_bill_at_twice_the_input_rate() {
    let event = recorded_catalog_usage(claude_pricing_record(), one_hour_write_usage()).await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.unpriced_reason, None);
    assert_eq!(event.prompt_tokens, Some(3_000));
    assert_eq!(event.cache_write_tokens, Some(2_000));
    assert_eq!(event.cache_write_1h_tokens, Some(2_000));
    assert_eq!(
        event.cache_write_1h_cost_per_million_tokens,
        Some(Money4::from_scaled(60_000))
    );
    // 1,000 input tokens at $3, 2,000 1-hour writes at $6 and 100 output tokens at $15
    // per million: $0.003 + $0.012 + $0.0015.
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(165));
}

#[tokio::test]
async fn one_hour_cache_writes_bill_at_the_catalog_rate() {
    let mut pricing = claude_pricing_record();
    pricing.cache_write_1h_cost_per_million_tokens = Some(Money4::from_scaled(50_000));
    let event = recorded_catalog_usage(pricing, one_hour_write_usage()).await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(
        event.cache_write_1h_cost_per_million_tokens,
        Some(Money4::from_scaled(50_000))
    );
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(145));
}

#[tokio::test]
async fn bedrock_cache_writes_bill_each_ttl_at_its_own_rate() {
    let event = recorded_catalog_usage(
        claude_pricing_record(),
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 100,
            "provider_usage": {
                "inputTokens": 1_000,
                "outputTokens": 100,
                "cacheReadInputTokens": 4_000,
                "cacheWriteInputTokens": 3_000,
                "cacheDetails": [
                    {"ttl": "5m", "inputTokens": 1_000},
                    {"ttl": "1h", "inputTokens": 2_000}
                ]
            }
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.unpriced_reason, None);
    assert_eq!(event.prompt_tokens, Some(8_000));
    assert_eq!(event.cache_write_tokens, Some(3_000));
    assert_eq!(event.cache_write_1h_tokens, Some(2_000));
    // In $0.0001 units: input 30, reads 12, 5-minute writes 38 (37.5 rounded half up),
    // 1-hour writes 120 and output 15.
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(215));
}

#[tokio::test]
async fn a_cache_read_without_a_rate_is_estimated_at_the_input_rate() {
    let event = recorded_catalog_usage(
        pricing_record(None),
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 500,
            "prompt_tokens_details": {"cached_tokens": 400}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::LegacyEstimated);
    assert!(event.pricing_status.counts_toward_spend());
    assert_eq!(
        event.unpriced_reason.as_deref(),
        Some("estimated_missing_cache_read_rate")
    );
    assert_eq!(
        event.cache_read_cost_per_million_tokens,
        Some(Money4::from_scaled(10_000))
    );
    // 600 uncached and 400 cached tokens at $1 and 500 output tokens at $2 per million.
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(20));
}

#[tokio::test]
async fn missing_usage_is_estimated_from_the_request_and_response_sizes() {
    let event = recorded_usage_without_counts(
        Some(pricing_record(None)),
        UsageEstimateInput {
            request_body_bytes: Some(4_001),
            response_text_bytes: Some(2_001),
        },
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::LegacyEstimated);
    assert_eq!(
        event.unpriced_reason.as_deref(),
        Some("estimated_usage_missing")
    );
    // A token per four bytes, rounded up.
    assert_eq!(event.prompt_tokens, Some(1_001));
    assert_eq!(event.completion_tokens, Some(501));
    assert_eq!(event.total_tokens, Some(1_502));
    assert_eq!(
        event.input_cost_per_million_tokens,
        Some(Money4::from_scaled(10_000))
    );
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(20));
}

#[tokio::test]
async fn usage_with_only_a_total_is_estimated_from_the_sizes() {
    let route = catalog_route();
    let provider = openai_provider(&route.provider_key);
    let event = recorded_usage(
        &route,
        provider,
        Some(pricing_record(None)),
        Some(json!({"total_tokens": 1_500})),
        UsageEstimateInput {
            request_body_bytes: Some(4_001),
            response_text_bytes: Some(2_001),
        },
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::LegacyEstimated);
    assert_eq!(event.prompt_tokens, Some(1_001));
    assert_eq!(event.completion_tokens, Some(501));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(20));
}

#[tokio::test]
async fn missing_usage_stays_unbilled_without_a_request_size_or_rates() {
    let sizes = UsageEstimateInput {
        request_body_bytes: Some(4_000),
        response_text_bytes: Some(2_000),
    };
    for (pricing, usage_estimate) in [
        (Some(pricing_record(None)), UsageEstimateInput::default()),
        (None, sizes),
    ] {
        let event = recorded_usage_without_counts(pricing, usage_estimate).await;

        assert_eq!(event.pricing_status, UsagePricingStatus::UsageMissing);
        assert_eq!(event.unpriced_reason, None);
        assert_eq!(event.prompt_tokens, None);
        assert_eq!(event.completion_tokens, None);
        assert_eq!(event.computed_cost_usd, Money4::ZERO);
    }
}

#[tokio::test]
async fn regional_vertex_anthropic_usage_is_estimated_at_the_global_rates_plus_ten_percent() {
    let mut route = vertex_embedding_route(Uuid::new_v4());
    route.upstream_model = "anthropic/claude-sonnet-4-6".to_string();
    let provider = ProviderConnection {
        provider_key: route.provider_key.clone(),
        provider_type: "gcp_vertex".to_string(),
        config: json!({"location": "us-central1"}),
        secrets: None,
    };
    let event = recorded_usage(
        &route,
        provider,
        Some(pricing_record(None)),
        Some(json!({"input_tokens": 1_000, "output_tokens": 500})),
        UsageEstimateInput::default(),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::LegacyEstimated);
    assert_eq!(
        event.unpriced_reason.as_deref(),
        Some("estimated_regional_vertex_rate:us-central1")
    );
    assert_eq!(
        event.input_cost_per_million_tokens,
        Some(Money4::from_scaled(11_000))
    );
    assert_eq!(
        event.output_cost_per_million_tokens,
        Some(Money4::from_scaled(22_000))
    );
    // 1,000 input tokens at $1.10 and 500 output tokens at $2.20 per million.
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(22));
}
