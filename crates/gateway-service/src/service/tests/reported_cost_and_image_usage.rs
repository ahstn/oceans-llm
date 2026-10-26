//! Usage pricing from provider-reported costs and image output rates.

use serde_json::Value;

use super::*;

/// OpenRouter's list prices for `google/gemini-3.1-flash-lite-image`: $0.25
/// input, $1.50 output, and, when given, $30 image output per million tokens.
fn image_model_route(image_output_rate: Option<Money4>) -> ModelRoute {
    let mut route = vertex_embedding_route(Uuid::new_v4());
    route.pricing_override = Some(RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(2_500),
        output_cost_per_million_tokens: Money4::from_scaled(15_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
        image_output_cost_per_million_tokens: image_output_rate,
    });
    route
}

async fn recorded_usage_event(route: &ModelRoute, provider_usage: Value) -> UsageLedgerRecord {
    let repo = Arc::new(UsageAccountingRepo::default());
    let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));
    service
        .record_chat_usage(
            &auth(),
            &model(route.model_id),
            route,
            "req_image_usage",
            Some(provider_usage),
            OffsetDateTime::now_utc(),
        )
        .await
        .expect("usage should be recorded");
    let events = repo.events.lock().expect("events lock");
    events.first().expect("usage event").clone()
}

#[tokio::test]
async fn image_output_usage_is_charged_at_the_configured_image_rate() {
    let route = image_model_route(Some(Money4::from_scaled(300_000)));
    let event = recorded_usage_event(
        &route,
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 1_290,
            "total_tokens": 2_290,
            "prompt_tokens_details": {"cached_tokens": 0},
            "completion_tokens_details": {"reasoning_tokens": 0, "image_tokens": 1_120}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.pricing_source.as_deref(), Some("configured_override"));
    assert_eq!(event.completion_tokens, Some(1_290));
    assert_eq!(event.image_output_tokens, Some(1_120));
    assert_eq!(
        event.image_output_cost_per_million_tokens,
        Some(Money4::from_scaled(300_000))
    );
    // $0.0003 input + $0.0003 for 170 text tokens + $0.0336 for 1,120 image tokens.
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(342));
}

#[tokio::test]
async fn image_output_usage_without_an_image_rate_keeps_output_pricing() {
    let event = recorded_usage_event(
        &image_model_route(None),
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 1_290,
            "total_tokens": 2_290,
            "completion_tokens_details": {"image_tokens": 1_120}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.image_output_tokens, Some(1_120));
    assert_eq!(event.image_output_cost_per_million_tokens, None);
    // $0.0003 input + $0.0019 for all 1,290 completion tokens at the output rate.
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(22));
}

#[tokio::test]
async fn provider_reported_usage_cost_wins_over_configured_pricing() {
    let event = recorded_usage_event(
        &image_model_route(Some(Money4::from_scaled(300_000))),
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 1_290,
            "total_tokens": 2_290,
            "cost": 0.0123,
            "completion_tokens_details": {"image_tokens": 1_120}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.pricing_source.as_deref(), Some("provider_reported"));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(123));
    assert_eq!(event.image_output_tokens, Some(1_120));
    assert_eq!(event.input_cost_per_million_tokens, None);
    assert_eq!(event.output_cost_per_million_tokens, None);
    assert_eq!(event.image_output_cost_per_million_tokens, None);
}

#[tokio::test]
async fn byok_usage_cost_is_charged_without_route_pricing() {
    // OpenRouter's live BYOK image response, on a route with no pricing override.
    let event = recorded_usage_event(
        &vertex_embedding_route(Uuid::new_v4()),
        json!({
            "prompt_tokens": 14,
            "completion_tokens": 229,
            "total_tokens": 243,
            "cost": 0,
            "is_byok": true,
            "prompt_tokens_details": {"cached_tokens": 0},
            "cost_details": {
                "upstream_inference_cost": 0.00694,
                "upstream_inference_prompt_cost": 0.00007,
                "upstream_inference_completions_cost": 0.00687
            },
            "completion_tokens_details": {"reasoning_tokens": 0, "image_tokens": 229}
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.pricing_source.as_deref(), Some("provider_reported"));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(69));
    assert_eq!(event.image_output_tokens, Some(229));
}

#[tokio::test]
async fn reported_usage_cost_is_charged_without_token_counts() {
    let event = recorded_usage_event(
        &vertex_embedding_route(Uuid::new_v4()),
        json!({"cost": "0.5"}),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.pricing_source.as_deref(), Some("provider_reported"));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(5_000));
    assert_eq!(event.total_tokens, None);
}

#[tokio::test]
async fn malformed_reported_usage_cost_falls_back_to_route_pricing() {
    let event = recorded_usage_event(
        &image_model_route(None),
        json!({
            "prompt_tokens": 1_000,
            "completion_tokens": 1_290,
            "total_tokens": 2_290,
            "cost": "n/a"
        }),
    )
    .await;

    assert_eq!(event.pricing_status, UsagePricingStatus::Priced);
    assert_eq!(event.pricing_source.as_deref(), Some("configured_override"));
    assert_eq!(event.computed_cost_usd, Money4::from_scaled(22));
}

#[test]
fn image_output_usage_is_read_only_within_completion_tokens() {
    let image_tokens = |usage: &Value| {
        usage_summary_from_value(Some(usage))
            .expect("usage should parse")
            .image_output_tokens
    };

    assert_eq!(
        image_tokens(&json!({"output_tokens": 5, "output_tokens_details": {"image_tokens": 0}})),
        Some(0)
    );
    assert_eq!(
        image_tokens(
            &json!({"completion_tokens": 5, "completion_tokens_details": {"image_tokens": 5}})
        ),
        Some(5)
    );
    for usage in [
        json!({"completion_tokens": 5, "completion_tokens_details": {"image_tokens": 6}}),
        json!({"completion_tokens": 5, "completion_tokens_details": {"image_tokens": -1}}),
        json!({"completion_tokens": 5, "completion_tokens_details": {"image_tokens": "5"}}),
        json!({"completion_tokens": 5, "completion_tokens_details": {"image_tokens": null}}),
        json!({"prompt_tokens": 5, "completion_tokens_details": {"image_tokens": 5}}),
    ] {
        assert_eq!(image_tokens(&usage), None, "{usage}");
    }
}

#[test]
fn aggregate_batch_usage_keeps_image_tokens_with_their_completion_tokens() {
    let result = |custom_id: &str, provider_usage: Option<Value>| ProviderBatchResult {
        custom_id: custom_id.to_string(),
        response_body: None,
        error: None,
        provider_request_id: None,
        provider_usage,
        completed_at: None,
        cost_usd: None,
    };
    let image_usage = |image_tokens: i64| {
        Some(json!({
            "prompt_tokens": 10,
            "completion_tokens": 300,
            "completion_tokens_details": {"image_tokens": image_tokens}
        }))
    };

    let items = [
        result("first", image_usage(256)),
        result("second", image_usage(200)),
    ];
    let (usage, complete) = aggregate_batch_usage(&items, None).expect("aggregate usage");
    assert!(complete);
    assert_eq!(usage.completion_tokens, Some(600));
    assert_eq!(usage.image_output_tokens, Some(456));

    // Provider totals replace incomplete item usage; their completion tokens
    // carry no image count, so none is borrowed from the items.
    let items = [result("first", image_usage(256)), result("missing", None)];
    let provider_usage =
        json!({"prompt_tokens": 30, "completion_tokens": 900, "total_tokens": 930});
    let (usage, _) =
        aggregate_batch_usage(&items, Some(&provider_usage)).expect("aggregate provider usage");
    assert_eq!(usage.completion_tokens, Some(900));
    assert_eq!(usage.image_output_tokens, None);
}
