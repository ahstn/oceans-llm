//! Pure merge of the projected catalog sources into one providers document.
//!
//! Per provider and model, each field takes the value of the first source that has one,
//! in precedence order: OpenRouter (only for the `openrouter` provider), then models.dev,
//! then LiteLLM. Pricing is taken as a whole schedule from the first source with both an
//! input and an output rate, so rates from two sources never mix. Disagreements are
//! recorded as conflicts, never resolved by comparing values.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use super::{
    PricingCatalogContextTier, PricingCatalogCostDocument, PricingCatalogLimitDocument,
    PricingCatalogModelDocument, PricingCatalogProviderDocument,
    metadata::CatalogModelMetadata,
    sources::{CatalogSourceId, Providers},
    target::OPENROUTER_PRICING_PROVIDER_ID,
};

/// The merged providers document plus, per `(provider_id, model_id)`, where each field
/// came from and where sources disagreed.
#[derive(Debug, Clone, Default)]
pub(crate) struct MergedCatalog {
    pub(crate) providers: Providers,
    pub(crate) reports: BTreeMap<(String, String), MergedModelReport>,
}

/// Where one merged model's fields came from and where sources disagreed. Served by
/// `/v1/model-metadata`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MergedModelReport {
    pub provenance: MergedModelProvenance,
    pub conflicts: Vec<FieldConflict>,
}

/// The source that supplied each merged field; `None` when no source had a value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MergedModelProvenance {
    pub pricing: Option<CatalogSourceId>,
    pub limits: LimitProvenance,
    pub metadata: MetadataProvenance,
    pub modalities: Option<CatalogSourceId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LimitProvenance {
    pub context: Option<CatalogSourceId>,
    pub input: Option<CatalogSourceId>,
    pub output: Option<CatalogSourceId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MetadataProvenance {
    pub reasoning: Option<CatalogSourceId>,
    pub tool_call: Option<CatalogSourceId>,
    pub structured_output: Option<CatalogSourceId>,
    pub attachment: Option<CatalogSourceId>,
    pub temperature: Option<CatalogSourceId>,
    pub knowledge: Option<CatalogSourceId>,
    pub reasoning_options: Option<CatalogSourceId>,
    pub api_endpoints: Option<CatalogSourceId>,
}

/// Two or more sources had different values for `field`; `winner` is the merged value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldConflict {
    pub field: &'static str,
    pub winner: SourceValue,
    pub others: Vec<SourceValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceValue {
    pub source: CatalogSourceId,
    pub value: Value,
}

/// One source's document for the model being merged, in precedence order.
type Candidate<'doc> = (CatalogSourceId, &'doc PricingCatalogModelDocument);

/// The rates billing reads from a cost document; `conditions` is informational.
#[derive(Serialize)]
struct PricingSchedule<'doc> {
    #[serde(skip)]
    cost: &'doc PricingCatalogCostDocument,
    input: Option<&'doc str>,
    output: Option<&'doc str>,
    cache_read: Option<&'doc str>,
    cache_write: Option<&'doc str>,
    input_audio: Option<&'doc str>,
    output_audio: Option<&'doc str>,
    output_image: Option<&'doc str>,
    tiers: &'doc [PricingCatalogContextTier],
}

impl<'doc> PricingSchedule<'doc> {
    /// The schedule of `cost` when it has both an input and an output rate.
    fn complete(cost: &'doc PricingCatalogCostDocument) -> Option<Self> {
        (cost.input.is_some() && cost.output.is_some()).then(|| Self::of(cost))
    }

    /// The schedule of `cost` when it has an input or an output rate, such as an
    /// embedding model's input-only schedule.
    fn partial(cost: &'doc PricingCatalogCostDocument) -> Option<Self> {
        (cost.input.is_some() || cost.output.is_some()).then(|| Self::of(cost))
    }

    fn of(cost: &'doc PricingCatalogCostDocument) -> Self {
        Self {
            cost,
            input: cost.input.as_deref(),
            output: cost.output.as_deref(),
            cache_read: cost.cache_read.as_deref(),
            cache_write: cost.cache_write.as_deref(),
            input_audio: cost.input_audio.as_deref(),
            output_audio: cost.output_audio.as_deref(),
            output_image: cost.output_image.as_deref(),
            tiers: &cost.tiers,
        }
    }
}

/// Merge the three projected sources. A model present in any participating source
/// appears in the output.
pub(crate) fn merge_catalogs(
    models_dev: &Providers,
    openrouter: &Providers,
    litellm: &Providers,
) -> MergedCatalog {
    let provider_ids: BTreeSet<&str> = models_dev
        .keys()
        .chain(litellm.keys())
        .map(String::as_str)
        .chain(
            openrouter
                .contains_key(OPENROUTER_PRICING_PROVIDER_ID)
                .then_some(OPENROUTER_PRICING_PROVIDER_ID),
        )
        .collect();

    let mut merged = MergedCatalog::default();
    for provider_id in provider_ids {
        let sources = provider_sources(provider_id, models_dev, openrouter, litellm);
        let display_name = sources
            .iter()
            .map(|(_, provider)| provider.display_name.as_str())
            .find(|name| !name.is_empty())
            .unwrap_or(provider_id)
            .to_string();
        let model_ids: BTreeSet<&str> = sources
            .iter()
            .flat_map(|(_, provider)| provider.models.keys().map(String::as_str))
            .collect();

        let mut models = BTreeMap::new();
        for model_id in model_ids {
            let candidates: Vec<Candidate<'_>> = sources
                .iter()
                .filter_map(|&(source, provider)| {
                    provider.models.get(model_id).map(|model| (source, model))
                })
                .collect();
            let (document, report) = merge_model(model_id, &candidates);
            models.insert(model_id.to_string(), document);
            merged
                .reports
                .insert((provider_id.to_string(), model_id.to_string()), report);
        }
        merged.providers.insert(
            provider_id.to_string(),
            PricingCatalogProviderDocument {
                display_name,
                models,
            },
        );
    }
    merged
}

/// The sources' documents for `provider_id`, in precedence order.
fn provider_sources<'doc>(
    provider_id: &str,
    models_dev: &'doc Providers,
    openrouter: &'doc Providers,
    litellm: &'doc Providers,
) -> Vec<(CatalogSourceId, &'doc PricingCatalogProviderDocument)> {
    let openrouter = if provider_id == OPENROUTER_PRICING_PROVIDER_ID {
        openrouter.get(provider_id)
    } else {
        None
    };
    [
        (CatalogSourceId::OpenRouter, openrouter),
        (CatalogSourceId::ModelsDev, models_dev.get(provider_id)),
        (CatalogSourceId::LiteLlm, litellm.get(provider_id)),
    ]
    .into_iter()
    .filter_map(|(source, provider)| provider.map(|provider| (source, provider)))
    .collect()
}

fn merge_model(
    model_id: &str,
    candidates: &[Candidate<'_>],
) -> (PricingCatalogModelDocument, MergedModelReport) {
    let mut conflicts = Vec::new();
    // A partial schedule is used only when no source has a complete one.
    let pricing = first_with_value(
        "pricing",
        candidates,
        |model| PricingSchedule::complete(&model.cost),
        &mut conflicts,
    )
    .or_else(|| {
        first_with_value(
            "pricing",
            candidates,
            |model| PricingSchedule::partial(&model.cost),
            &mut conflicts,
        )
    });
    let (limit, limits) = merge_limits(candidates, &mut conflicts);
    let (metadata, metadata_provenance) = merge_metadata(candidates, &mut conflicts);
    let modalities = first_with_value(
        "modalities",
        candidates,
        |model| {
            let known = !model.modalities.input.is_empty() || !model.modalities.output.is_empty();
            known.then_some(&model.modalities)
        },
        &mut conflicts,
    );
    let first_text = |text_of: fn(&PricingCatalogModelDocument) -> &str| {
        candidates
            .iter()
            .map(|(_, model)| text_of(model))
            .find(|text| !text.is_empty())
            .map(str::to_string)
    };

    let document = PricingCatalogModelDocument {
        id: model_id.to_string(),
        display_name: first_text(|model| model.display_name.as_str())
            .unwrap_or_else(|| model_id.to_string()),
        release_date: first_text(|model| model.release_date.as_str()).unwrap_or_default(),
        last_updated: first_text(|model| model.last_updated.as_str()).unwrap_or_default(),
        metadata,
        cost: pricing
            .as_ref()
            .map(|(_, schedule)| schedule.cost.clone())
            .unwrap_or_default(),
        limit,
        modalities: modalities
            .map(|(_, modalities)| modalities.clone())
            .unwrap_or_default(),
    };
    let report = MergedModelReport {
        provenance: MergedModelProvenance {
            pricing: pricing.map(|(source, _)| source),
            limits,
            metadata: metadata_provenance,
            modalities: modalities.map(|(source, _)| source),
        },
        conflicts,
    };
    (document, report)
}

fn merge_limits(
    candidates: &[Candidate<'_>],
    conflicts: &mut Vec<FieldConflict>,
) -> (PricingCatalogLimitDocument, LimitProvenance) {
    let (context, context_source) = first_with_value(
        "limit.context",
        candidates,
        |model| model.limit.context,
        conflicts,
    )
    .map(swap)
    .unzip();
    let (input, input_source) = first_with_value(
        "limit.input",
        candidates,
        |model| model.limit.input,
        conflicts,
    )
    .map(swap)
    .unzip();
    let (output, output_source) = first_with_value(
        "limit.output",
        candidates,
        |model| model.limit.output,
        conflicts,
    )
    .map(swap)
    .unzip();
    (
        PricingCatalogLimitDocument {
            context,
            input,
            output,
        },
        LimitProvenance {
            context: context_source,
            input: input_source,
            output: output_source,
        },
    )
}

fn merge_metadata(
    candidates: &[Candidate<'_>],
    conflicts: &mut Vec<FieldConflict>,
) -> (CatalogModelMetadata, MetadataProvenance) {
    let mut flag = |field: &'static str, flag_of: fn(&CatalogModelMetadata) -> Option<bool>| {
        first_with_value(
            field,
            candidates,
            |model| flag_of(&model.metadata),
            conflicts,
        )
        .map(swap)
        .unzip()
    };
    let (reasoning, reasoning_source) = flag("metadata.reasoning", |m| m.reasoning);
    let (tool_call, tool_call_source) = flag("metadata.tool_call", |m| m.tool_call);
    let (structured_output, structured_output_source) =
        flag("metadata.structured_output", |m| m.structured_output);
    let (attachment, attachment_source) = flag("metadata.attachment", |m| m.attachment);
    let (temperature, temperature_source) = flag("metadata.temperature", |m| m.temperature);

    let (knowledge, knowledge_source) = first_with_value(
        "metadata.knowledge",
        candidates,
        |model| model.metadata.knowledge.as_ref(),
        conflicts,
    )
    .map(|(source, knowledge)| (knowledge.clone(), source))
    .unzip();
    let (reasoning_options, reasoning_options_source) = first_with_value(
        "metadata.reasoning_options",
        candidates,
        |model| {
            let options = &model.metadata.reasoning_options;
            (!options.is_empty()).then_some(options)
        },
        conflicts,
    )
    .map(|(source, options)| (options.clone(), source))
    .unzip();
    let (api_endpoints, api_endpoints_source) = first_with_value(
        "metadata.api_endpoints",
        candidates,
        |model| model.metadata.api_endpoints,
        conflicts,
    )
    .map(swap)
    .unzip();

    (
        CatalogModelMetadata {
            reasoning,
            tool_call,
            structured_output,
            attachment,
            temperature,
            knowledge,
            reasoning_options: reasoning_options.unwrap_or_default(),
            api_endpoints,
        },
        MetadataProvenance {
            reasoning: reasoning_source,
            tool_call: tool_call_source,
            structured_output: structured_output_source,
            attachment: attachment_source,
            temperature: temperature_source,
            knowledge: knowledge_source,
            reasoning_options: reasoning_options_source,
            api_endpoints: api_endpoints_source,
        },
    )
}

fn swap<V>((source, value): (CatalogSourceId, V)) -> (V, CatalogSourceId) {
    (value, source)
}

/// The first candidate's value for `field`, recording a conflict when a later source
/// has a different value. Values compare by their JSON form.
fn first_with_value<'doc, V: Serialize>(
    field: &'static str,
    candidates: &[Candidate<'doc>],
    value_of: impl Fn(&'doc PricingCatalogModelDocument) -> Option<V>,
    conflicts: &mut Vec<FieldConflict>,
) -> Option<(CatalogSourceId, V)> {
    let mut present = candidates
        .iter()
        .filter_map(|&(source, model)| value_of(model).map(|value| (source, value)));
    let (winner_source, winner_value) = present.next()?;
    let rest: Vec<(CatalogSourceId, V)> = present.collect();
    if !rest.is_empty() {
        let winner_json = to_json(&winner_value);
        let others: Vec<SourceValue> = rest
            .iter()
            .map(|(source, value)| SourceValue {
                source: *source,
                value: to_json(value),
            })
            .filter(|other| other.value != winner_json)
            .collect();
        if !others.is_empty() {
            conflicts.push(FieldConflict {
                field,
                winner: SourceValue {
                    source: winner_source,
                    value: winner_json,
                },
                others,
            });
        }
    }
    Some((winner_source, winner_value))
}

fn to_json(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("catalog field values have string keys and serialize")
}

#[cfg(test)]
mod tests {
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
}
