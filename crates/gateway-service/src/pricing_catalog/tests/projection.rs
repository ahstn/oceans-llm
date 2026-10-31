use super::{super::PricingCatalogContextTier, support::*};

fn project_openai_cost(cost: Value) -> PricingCatalogCostDocument {
    let body = json!({
        "openai": {
            "name": "OpenAI",
            "models": {
                "gpt-6-astra": {"id": "gpt-6-astra", "name": "GPT-6 Astra", "cost": cost}
            }
        }
    });
    let projected = project_models_dev_snapshot(
        body.to_string().as_bytes(),
        REMOTE_SOURCE,
        None,
        OffsetDateTime::UNIX_EPOCH,
    )
    .expect("projected snapshot");
    projected.snapshot.document.providers["openai"].models["gpt-6-astra"]
        .cost
        .clone()
}

fn tier(above_input_tokens: i64, input: &str, output: &str) -> PricingCatalogContextTier {
    PricingCatalogContextTier {
        above_input_tokens,
        input: Some(input.to_string()),
        output: Some(output.to_string()),
        cache_read: None,
        cache_write: None,
    }
}

#[test]
fn models_dev_context_tiers_replace_the_context_over_200k_condition() {
    let cost = project_openai_cost(json!({
        "input": 10, "output": 50, "cache_read": 1,
        "reasoning": 60,
        "context_over_200k": {"input": 20, "output": 75, "cache_read": 2},
        "tiers": [{
            "input": 20, "output": 75, "cache_read": 2,
            "tier": {"type": "context", "size": 272_000}
        }]
    }));

    assert_eq!(
        cost.tiers,
        vec![PricingCatalogContextTier {
            cache_read: Some("2.0000".to_string()),
            ..tier(272_000, "20.0000", "75.0000")
        }]
    );
    assert_eq!(
        cost.conditions.keys().collect::<Vec<_>>(),
        vec!["reasoning"],
        "only non-tier conditions stay informational"
    );
    assert_eq!(cost.output_image, None);
}

#[test]
fn models_dev_context_tiers_are_sorted_and_deduplicated_and_other_types_kept() {
    let cost = project_openai_cost(json!({
        "input": 1, "output": 2,
        "tiers": [
            {"input": 4, "output": 8, "tier": {"type": "context", "size": 1_000_000}},
            {"input": 2, "output": 4, "tier": {"type": "context", "size": 200_000}},
            {"input": 2, "output": 4, "tier": {"type": "context", "size": 200_000}},
            {"input": 9, "output": 9, "tier": {"type": "time_of_day", "size": 3}}
        ]
    }));

    assert_eq!(
        cost.tiers,
        vec![
            tier(200_000, "2.0000", "4.0000"),
            tier(1_000_000, "4.0000", "8.0000"),
        ]
    );
    assert_eq!(
        cost.conditions["tiers"],
        json!([{"input": 9, "output": 9, "tier": {"type": "time_of_day", "size": 3}}])
    );
}

#[test]
fn conflicting_models_dev_tiers_at_one_threshold_leave_base_rates_only() {
    let conditions = json!({
        "context_over_200k": {"input": 2, "output": 4},
        "tiers": [
            {"input": 2, "output": 4, "tier": {"type": "context", "size": 200_000}},
            {"input": 3, "output": 4, "tier": {"type": "context", "size": 200_000}}
        ]
    });
    let mut cost = json!({"input": 1, "output": 2});
    cost.as_object_mut()
        .expect("cost object")
        .extend(conditions.as_object().expect("conditions object").clone());

    let projected = project_openai_cost(cost);

    assert!(projected.tiers.is_empty());
    assert_eq!(
        serde_json::to_value(&projected.conditions).expect("conditions json"),
        conditions
    );
}

#[test]
fn pricing_record_carries_context_tiers_and_image_rate() {
    let snapshot = fallback_snapshot();
    let mut document = snapshot.document.providers["openai"].models["gpt-5"].clone();
    document.cost.output_image = Some("40.0000".to_string());
    document.cost.tiers = vec![
        PricingCatalogContextTier {
            cache_read: Some("0.5000".to_string()),
            ..tier(200_000, "2.5000", "15.0000")
        },
        tier(272_000, "5.0000", "22.5000"),
    ];

    let record = build_model_pricing_record(&snapshot.metadata, "openai", "gpt-5", &document)
        .expect("pricing record");

    assert_eq!(
        record.output_image_usd_per_million,
        Some(Money4::from_scaled(400_000))
    );
    assert_eq!(
        record
            .context_tiers
            .iter()
            .map(|tier| (tier.above_input_tokens, tier.input, tier.cache_read))
            .collect::<Vec<_>>(),
        vec![
            (
                200_000,
                Some(Money4::from_scaled(25_000)),
                Some(Money4::from_scaled(5_000))
            ),
            (272_000, Some(Money4::from_scaled(50_000)), None),
        ]
    );
}
