use serde_json::json;

use super::*;
use crate::pricing_catalog::PricingCatalogModalitiesDocument;

fn model(id: &str) -> PricingCatalogModelDocument {
    PricingCatalogModelDocument {
        id: id.to_string(),
        display_name: id.to_string(),
        release_date: String::new(),
        last_updated: String::new(),
        metadata: CatalogModelMetadata::default(),
        cost: PricingCatalogCostDocument::default(),
        limit: PricingCatalogLimitDocument::default(),
        modalities: PricingCatalogModalitiesDocument::default(),
    }
}

fn priced(id: &str, input: &str, output: &str) -> PricingCatalogModelDocument {
    let mut document = model(id);
    document.cost.input = Some(input.to_string());
    document.cost.output = Some(output.to_string());
    document
}

fn providers(provider_id: &str, models: Vec<PricingCatalogModelDocument>) -> Providers {
    Providers::from([(
        provider_id.to_string(),
        PricingCatalogProviderDocument {
            display_name: provider_id.to_string(),
            models: models
                .into_iter()
                .map(|model| (model.id.clone(), model))
                .collect(),
        },
    )])
}

fn report<'merged>(
    merged: &'merged MergedCatalog,
    provider_id: &str,
    model_id: &str,
) -> &'merged MergedModelReport {
    &merged.reports[&(provider_id.to_string(), model_id.to_string())]
}

#[test]
fn openrouter_wins_for_openrouter_ids_then_models_dev_then_litellm() {
    let with_context = |context| {
        let mut document = model("vendor/model");
        document.limit.context = Some(context);
        document
    };
    let mut litellm_only = with_context(3);
    litellm_only.limit.output = Some(30);
    let merged = merge_catalogs(
        &providers("openrouter", vec![with_context(2)]),
        &providers("openrouter", vec![with_context(1)]),
        &providers("openrouter", vec![litellm_only]),
    );

    let document = &merged.providers["openrouter"].models["vendor/model"];
    assert_eq!(document.limit.context, Some(1));
    assert_eq!(document.limit.output, Some(30));
    let report = report(&merged, "openrouter", "vendor/model");
    assert_eq!(
        report.provenance.limits.context,
        Some(CatalogSourceId::OpenRouter)
    );
    assert_eq!(
        report.provenance.limits.output,
        Some(CatalogSourceId::LiteLlm)
    );
    assert_eq!(
        report.conflicts,
        [FieldConflict {
            field: "limit.context",
            winner: SourceValue {
                source: CatalogSourceId::OpenRouter,
                value: json!(1),
            },
            others: vec![
                SourceValue {
                    source: CatalogSourceId::ModelsDev,
                    value: json!(2),
                },
                SourceValue {
                    source: CatalogSourceId::LiteLlm,
                    value: json!(3),
                },
            ],
        }]
    );
}

#[test]
fn models_dev_wins_elsewhere_and_openrouter_never_participates() {
    let mut models_dev = model("gpt-5");
    models_dev.metadata.reasoning = Some(true);
    let mut openrouter = model("gpt-5");
    openrouter.metadata.reasoning = Some(false);
    openrouter.metadata.tool_call = Some(true);
    let mut litellm = model("gpt-5");
    litellm.metadata.reasoning = Some(false);
    litellm.metadata.structured_output = Some(true);
    let merged = merge_catalogs(
        &providers("openai", vec![models_dev]),
        &providers("openai", vec![openrouter, model("openrouter-only")]),
        &providers("openai", vec![litellm, model("litellm-only")]),
    );

    let models = &merged.providers["openai"].models;
    assert_eq!(
        models.keys().map(String::as_str).collect::<Vec<_>>(),
        ["gpt-5", "litellm-only"]
    );
    let metadata = &models["gpt-5"].metadata;
    assert_eq!(metadata.reasoning, Some(true));
    assert_eq!(metadata.tool_call, None);
    assert_eq!(metadata.structured_output, Some(true));
    let provenance = &report(&merged, "openai", "gpt-5").provenance.metadata;
    assert_eq!(provenance.reasoning, Some(CatalogSourceId::ModelsDev));
    assert_eq!(provenance.structured_output, Some(CatalogSourceId::LiteLlm));
    assert_eq!(
        report(&merged, "openai", "litellm-only").provenance,
        MergedModelProvenance::default()
    );
}

#[test]
fn pricing_is_a_whole_schedule_from_one_source() {
    let mut models_dev = model("gpt-5");
    models_dev.cost.input = Some("1.0000".to_string());
    models_dev.cost.cache_read = Some("0.1000".to_string());
    let mut litellm = priced("gpt-5", "2.0000", "8.0000");
    litellm.cost.cache_write = Some("2.5000".to_string());
    litellm.cost.tiers = vec![PricingCatalogContextTier {
        above_input_tokens: 200_000,
        input: Some("4.0000".to_string()),
        output: None,
        cache_read: None,
        cache_write: None,
    }];
    let merged = merge_catalogs(
        &providers("openai", vec![models_dev]),
        &Providers::new(),
        &providers("openai", vec![litellm]),
    );

    let cost = &merged.providers["openai"].models["gpt-5"].cost;
    assert_eq!(cost.input.as_deref(), Some("2.0000"));
    assert_eq!(cost.output.as_deref(), Some("8.0000"));
    assert_eq!(cost.cache_read, None, "never mixed in from models.dev");
    assert_eq!(cost.cache_write.as_deref(), Some("2.5000"));
    assert_eq!(cost.tiers.len(), 1);
    let report = report(&merged, "openai", "gpt-5");
    assert_eq!(report.provenance.pricing, Some(CatalogSourceId::LiteLlm));
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
}

#[test]
fn differing_complete_schedules_record_a_pricing_conflict() {
    let merged = merge_catalogs(
        &providers("openai", vec![priced("gpt-5", "1.2500", "10.0000")]),
        &Providers::new(),
        &providers(
            "openai",
            vec![
                priced("gpt-5", "1.0000", "10.0000"),
                priced("same", "1.0000", "2.0000"),
            ],
        ),
    );

    let report = report(&merged, "openai", "gpt-5");
    assert_eq!(report.provenance.pricing, Some(CatalogSourceId::ModelsDev));
    let [conflict] = report.conflicts.as_slice() else {
        panic!("expected one conflict: {:?}", report.conflicts);
    };
    assert_eq!(conflict.field, "pricing");
    assert_eq!(conflict.winner.value["input"], json!("1.2500"));
    assert_eq!(conflict.others[0].source, CatalogSourceId::LiteLlm);
    assert_eq!(conflict.others[0].value["input"], json!("1.0000"));
}

#[test]
fn false_and_zero_win_over_later_sources() {
    let mut models_dev = priced("gpt-5", "0.0000", "0.0000");
    models_dev.metadata.tool_call = Some(false);
    models_dev.limit.output = Some(0);
    let mut litellm = priced("gpt-5", "1.0000", "2.0000");
    litellm.metadata.tool_call = Some(true);
    litellm.limit.output = Some(128_000);
    let merged = merge_catalogs(
        &providers("openai", vec![models_dev]),
        &Providers::new(),
        &providers("openai", vec![litellm]),
    );

    let document = &merged.providers["openai"].models["gpt-5"];
    assert_eq!(document.metadata.tool_call, Some(false));
    assert_eq!(document.limit.output, Some(0));
    assert_eq!(document.cost.input.as_deref(), Some("0.0000"));
    let provenance = &report(&merged, "openai", "gpt-5").provenance;
    assert_eq!(provenance.pricing, Some(CatalogSourceId::ModelsDev));
    assert_eq!(
        provenance.metadata.tool_call,
        Some(CatalogSourceId::ModelsDev)
    );
}

#[test]
fn an_input_only_schedule_is_used_only_without_a_complete_one() {
    let mut input_only = model("embedding");
    input_only.cost.input = Some("0.1500".to_string());

    let alone = merge_catalogs(
        &providers("google-vertex", vec![input_only.clone()]),
        &Providers::new(),
        &Providers::new(),
    );
    let cost = &alone.providers["google-vertex"].models["embedding"].cost;
    assert_eq!(cost.input.as_deref(), Some("0.1500"));
    assert_eq!(
        report(&alone, "google-vertex", "embedding")
            .provenance
            .pricing,
        Some(CatalogSourceId::ModelsDev)
    );

    let with_complete = merge_catalogs(
        &providers("google-vertex", vec![input_only]),
        &Providers::new(),
        &providers(
            "google-vertex",
            vec![priced("embedding", "0.2000", "0.0000")],
        ),
    );
    let cost = &with_complete.providers["google-vertex"].models["embedding"].cost;
    assert_eq!(cost.input.as_deref(), Some("0.2000"));
    assert_eq!(
        report(&with_complete, "google-vertex", "embedding")
            .provenance
            .pricing,
        Some(CatalogSourceId::LiteLlm)
    );
}

#[test]
fn each_service_tier_comes_whole_from_the_first_source_pricing_it() {
    let schedule = |input: &str| PricingCatalogServiceTierCost {
        input: Some(input.to_string()),
        output: Some("20.0000".to_string()),
        ..PricingCatalogServiceTierCost::default()
    };
    let mut models_dev = priced("gpt-5", "1.0000", "8.0000");
    models_dev.cost.service_tiers = BTreeMap::from([(ServiceTier::Priority, schedule("2.0000"))]);
    let mut litellm = priced("gpt-5", "1.0000", "8.0000");
    litellm.cost.service_tiers = BTreeMap::from([
        (ServiceTier::Flex, schedule("0.5000")),
        (ServiceTier::Priority, schedule("2.5000")),
    ]);
    let mut tier_without_base = model("tier-only");
    tier_without_base.cost.service_tiers =
        BTreeMap::from([(ServiceTier::Priority, schedule("2.0000"))]);
    let merged = merge_catalogs(
        &providers("openai", vec![models_dev]),
        &Providers::new(),
        &providers("openai", vec![litellm, tier_without_base]),
    );

    let models = &merged.providers["openai"].models;
    assert_eq!(
        models["gpt-5"].cost.service_tiers,
        BTreeMap::from([
            (ServiceTier::Flex, schedule("0.5000")),
            (ServiceTier::Priority, schedule("2.0000")),
        ])
    );
    let report = report(&merged, "openai", "gpt-5");
    assert_eq!(
        report.provenance.service_tiers,
        BTreeMap::from([
            (ServiceTier::Flex, CatalogSourceId::LiteLlm),
            (ServiceTier::Priority, CatalogSourceId::ModelsDev),
        ])
    );
    let [conflict] = report.conflicts.as_slice() else {
        panic!("expected one conflict: {:?}", report.conflicts);
    };
    assert_eq!(conflict.field, "pricing.service_tiers.priority");
    assert!(
        models["tier-only"].cost.service_tiers.is_empty(),
        "tier schedules extend a base schedule"
    );
}

#[test]
fn a_tier_schedule_covering_the_base_context_tiers_wins_over_an_earlier_one() {
    let context = |input: &str| PricingCatalogContextTier {
        above_input_tokens: 272_000,
        input: Some(input.to_string()),
        output: None,
        cache_read: None,
        cache_write: None,
    };
    let flat = |input: &str| PricingCatalogServiceTierCost {
        input: Some(input.to_string()),
        output: Some("20.0000".to_string()),
        ..PricingCatalogServiceTierCost::default()
    };
    let covering = PricingCatalogServiceTierCost {
        context_tiers: vec![context("4.0000")],
        ..flat("2.0000")
    };
    let mut models_dev = priced("gpt-6-luna", "1.0000", "8.0000");
    models_dev.cost.tiers = vec![context("2.0000")];
    models_dev.cost.service_tiers = BTreeMap::from([
        (ServiceTier::Flex, flat("0.5000")),
        (ServiceTier::Priority, flat("2.0000")),
    ]);
    let mut litellm = priced("gpt-6-luna", "1.0000", "8.0000");
    litellm.cost.service_tiers = BTreeMap::from([
        (ServiceTier::Flex, flat("0.6000")),
        (ServiceTier::Priority, covering.clone()),
    ]);
    let merged = merge_catalogs(
        &providers("openai", vec![models_dev]),
        &Providers::new(),
        &providers("openai", vec![litellm]),
    );

    assert_eq!(
        merged.providers["openai"].models["gpt-6-luna"]
            .cost
            .service_tiers,
        BTreeMap::from([
            (ServiceTier::Flex, flat("0.5000")),
            (ServiceTier::Priority, covering),
        ]),
        "with no covering flex schedule the first source's still wins"
    );
    let report = report(&merged, "openai", "gpt-6-luna");
    assert_eq!(
        report.provenance.service_tiers,
        BTreeMap::from([
            (ServiceTier::Flex, CatalogSourceId::ModelsDev),
            (ServiceTier::Priority, CatalogSourceId::LiteLlm),
        ])
    );
    let priority_conflict = report
        .conflicts
        .iter()
        .find(|conflict| conflict.field == "pricing.service_tiers.priority")
        .expect("priority conflict");
    assert_eq!(priority_conflict.winner.source, CatalogSourceId::LiteLlm);
    assert_eq!(
        priority_conflict.others,
        [SourceValue {
            source: CatalogSourceId::ModelsDev,
            value: to_json(&flat("2.0000")),
        }]
    );
}
