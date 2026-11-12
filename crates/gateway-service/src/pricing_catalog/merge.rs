//! Pure merge of the projected catalog sources into one providers document.
//!
//! Per provider and model, each field takes the value of the first source that has one,
//! in precedence order: OpenRouter (only for the `openrouter` provider), then models.dev,
//! then LiteLLM. Pricing is taken as a whole schedule from the first source with both an
//! input and an output rate, so rates from two sources never mix. Each service tier's
//! schedule extends it, taken whole from the first source that prices that tier.
//! Disagreements are recorded as conflicts, never resolved by comparing values.

use std::collections::{BTreeMap, BTreeSet};

use gateway_core::ServiceTier;
use serde::Serialize;
use serde_json::Value;

use super::{
    PricingCatalogContextTier, PricingCatalogCostDocument, PricingCatalogLimitDocument,
    PricingCatalogModelDocument, PricingCatalogProviderDocument, PricingCatalogServiceTierCost,
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
    /// The source of each service tier's schedule.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub service_tiers: BTreeMap<ServiceTier, CatalogSourceId>,
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
    // Tier schedules extend a base schedule; a model without one has none either.
    let (service_tiers, service_tier_sources) = match &pricing {
        Some((_, schedule)) => merge_service_tiers(candidates, schedule.cost, &mut conflicts),
        None => Default::default(),
    };
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
            .map(|(_, schedule)| with_service_tiers(schedule.cost, service_tiers))
            .unwrap_or_default(),
        limit,
        modalities: modalities
            .map(|(_, modalities)| modalities.clone())
            .unwrap_or_default(),
    };
    let report = MergedModelReport {
        provenance: MergedModelProvenance {
            pricing: pricing.map(|(source, _)| source),
            service_tiers: service_tier_sources,
            limits,
            metadata: metadata_provenance,
            modalities: modalities.map(|(source, _)| source),
        },
        conflicts,
    };
    (document, report)
}

/// Each service tier's schedule and the source it came from. The first source whose
/// schedule has a context tier at every context threshold of the merged `base` wins, so
/// long requests stay priced. Without one, the first source with the tier wins and
/// billing fails closed above the thresholds it lacks.
fn merge_service_tiers(
    candidates: &[Candidate<'_>],
    base: &PricingCatalogCostDocument,
    conflicts: &mut Vec<FieldConflict>,
) -> (
    BTreeMap<ServiceTier, PricingCatalogServiceTierCost>,
    BTreeMap<ServiceTier, CatalogSourceId>,
) {
    let mut schedules = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for tier in ServiceTier::ALL {
        let tier_schedules = || {
            candidates.iter().filter_map(move |&(source, model)| {
                model
                    .cost
                    .service_tiers
                    .get(&tier)
                    .map(|schedule| (source, schedule))
            })
        };
        let Some((source, schedule)) = tier_schedules()
            .find(|(_, schedule)| covers_context_tiers(schedule, base))
            .or_else(|| tier_schedules().next())
        else {
            continue;
        };
        let mut others = tier_schedules()
            .filter(|(other, _)| *other != source)
            .peekable();
        if others.peek().is_some() {
            record_conflict(
                service_tier_field(tier),
                (source, schedule),
                others,
                conflicts,
            );
        }
        schedules.insert(tier, schedule.clone());
        sources.insert(tier, source);
    }
    (schedules, sources)
}

/// Whether `schedule` has a context tier at every context threshold of `base`.
fn covers_context_tiers(
    schedule: &PricingCatalogServiceTierCost,
    base: &PricingCatalogCostDocument,
) -> bool {
    base.tiers.iter().all(|base_tier| {
        schedule
            .context_tiers
            .iter()
            .any(|tier| tier.above_input_tokens == base_tier.above_input_tokens)
    })
}

/// The conflict field name of a tier's schedule.
const fn service_tier_field(tier: ServiceTier) -> &'static str {
    match tier {
        ServiceTier::Default => "pricing.service_tiers.default",
        ServiceTier::Flex => "pricing.service_tiers.flex",
        ServiceTier::Priority => "pricing.service_tiers.priority",
        ServiceTier::Ultrafast => "pricing.service_tiers.ultrafast",
    }
}

/// The base schedule of `cost` with the merged tier schedules in place of its own.
fn with_service_tiers(
    cost: &PricingCatalogCostDocument,
    service_tiers: BTreeMap<ServiceTier, PricingCatalogServiceTierCost>,
) -> PricingCatalogCostDocument {
    PricingCatalogCostDocument {
        conditions: cost.conditions.clone(),
        input: cost.input.clone(),
        output: cost.output.clone(),
        cache_read: cost.cache_read.clone(),
        cache_write: cost.cache_write.clone(),
        input_audio: cost.input_audio.clone(),
        output_audio: cost.output_audio.clone(),
        output_image: cost.output_image.clone(),
        tiers: cost.tiers.clone(),
        service_tiers,
    }
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
    let mut rest = present.peekable();
    if rest.peek().is_some() {
        record_conflict(field, (winner_source, &winner_value), rest, conflicts);
    }
    Some((winner_source, winner_value))
}

/// Records a conflict on `field` when any of `others` differs from the winning value.
/// Values compare by their JSON form.
fn record_conflict<W: Serialize, V: Serialize>(
    field: &'static str,
    (winner_source, winner_value): (CatalogSourceId, &W),
    others: impl Iterator<Item = (CatalogSourceId, V)>,
    conflicts: &mut Vec<FieldConflict>,
) {
    let winner_json = to_json(winner_value);
    let others: Vec<SourceValue> = others
        .map(|(source, value)| SourceValue {
            source,
            value: to_json(&value),
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

fn to_json(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("catalog field values have string keys and serialize")
}

#[cfg(test)]
mod tests;
