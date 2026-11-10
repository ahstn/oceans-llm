//! models.dev `api.json` projected into the gateway's providers document: the supported
//! providers only, with typed context tiers, service tier schedules, and 4 dp money
//! strings.

use std::collections::{BTreeMap, btree_map::Entry};

use gateway_core::{GatewayError, Money4, ServiceTier, take_requested_service_tier};
use serde::Deserialize;
use serde_json::{Number, Value};
use tracing::{debug, warn};

use super::Providers;
use crate::pricing_catalog::{
    PricingCatalogContextTier, PricingCatalogCostDocument, PricingCatalogLimitDocument,
    PricingCatalogModalitiesDocument, PricingCatalogModelDocument, PricingCatalogProviderDocument,
    PricingCatalogServiceTierCost, SUPPORTED_PRICING_PROVIDER_IDS, metadata::CatalogModelMetadata,
};

/// models.dev cost condition holding tiered rates, `[{input, output, .., tier: {type, size}}]`.
const TIERS_CONDITION: &str = "tiers";
/// models.dev tier type whose `size` is a prompt-token threshold.
const CONTEXT_TIER_TYPE: &str = "context";
/// models.dev cost condition duplicating the first context tier.
const CONTEXT_OVER_200K_CONDITION: &str = "context_over_200k";
/// Bedrock's request field nesting model-specific fields, such as a mode's `service_tier`.
const BEDROCK_MODEL_REQUEST_FIELDS: &str = "additionalModelRequestFields";

/// Project the models.dev `api.json` body into the supported providers.
///
/// # Errors
///
/// Fails when the body is not a models.dev providers object or a cost does not
/// normalize to money. An experimental mode that does not parse is skipped instead.
pub(crate) fn project(raw: &[u8]) -> Result<Providers, GatewayError> {
    let providers =
        serde_json::from_slice::<BTreeMap<String, ProviderDocument>>(raw).map_err(|error| {
            GatewayError::Internal(format!("failed parsing models.dev response: {error}"))
        })?;

    let mut projected_providers = Providers::new();
    for supported_provider_id in SUPPORTED_PRICING_PROVIDER_IDS {
        let Some(provider) = providers.get(supported_provider_id) else {
            continue;
        };

        let mut projected_models = BTreeMap::new();
        for (fallback_key, model) in &provider.models {
            let model_id = if model.id.trim().is_empty() {
                fallback_key.clone()
            } else {
                model.id.clone()
            };
            let cost = project_cost_document(supported_provider_id, &model_id, model)?;
            projected_models.insert(
                model_id.clone(),
                PricingCatalogModelDocument {
                    id: model_id,
                    display_name: model.name.clone(),
                    release_date: model.release_date.clone(),
                    last_updated: model.last_updated.clone(),
                    metadata: model.metadata.clone(),
                    cost,
                    limit: PricingCatalogLimitDocument {
                        context: model.limit.context,
                        input: model.limit.input,
                        output: model.limit.output,
                    },
                    modalities: PricingCatalogModalitiesDocument {
                        input: model.modalities.input.clone(),
                        output: model.modalities.output.clone(),
                    },
                },
            );
        }

        projected_providers.insert(
            supported_provider_id.to_string(),
            PricingCatalogProviderDocument {
                display_name: provider.name.clone(),
                models: projected_models,
            },
        );
    }

    Ok(projected_providers)
}

fn project_cost(value: Option<&Number>) -> Result<Option<String>, GatewayError> {
    value.map(normalize_money).transpose()
}

/// models.dev has no image output rate, so `output_image` stays unset.
fn project_cost_document(
    provider_id: &str,
    model_id: &str,
    model: &ModelDocument,
) -> Result<PricingCatalogCostDocument, GatewayError> {
    let cost = &model.cost;
    let mut conditions = cost.conditions.clone();
    let tiers = take_context_tiers(provider_id, model_id, &mut conditions)?;
    Ok(PricingCatalogCostDocument {
        conditions,
        input: project_cost(cost.input.as_ref())?,
        output: project_cost(cost.output.as_ref())?,
        cache_read: project_cost(cost.cache_read.as_ref())?,
        cache_write: project_cost(cost.cache_write.as_ref())?,
        input_audio: project_cost(cost.input_audio.as_ref())?,
        output_audio: project_cost(cost.output_audio.as_ref())?,
        output_image: None,
        tiers,
        service_tiers: service_tier_costs(provider_id, model_id, model.experimental.as_ref()),
    })
}

/// The schedules of the experimental modes that select a non-default service tier and
/// price it. Two modes pricing one tier differently leave the tier unpriced.
fn service_tier_costs(
    provider_id: &str,
    model_id: &str,
    experimental: Option<&Value>,
) -> BTreeMap<ServiceTier, PricingCatalogServiceTierCost> {
    let Some(modes) = experimental
        .and_then(|experimental| experimental.get("modes"))
        .and_then(Value::as_object)
    else {
        return BTreeMap::new();
    };
    let mut costs = BTreeMap::new();
    let mut conflicting = Vec::new();
    for (mode_name, mode) in modes {
        let Some((tier, cost)) = mode_service_tier_cost(provider_id, model_id, mode_name, mode)
        else {
            continue;
        };
        match costs.entry(tier) {
            Entry::Vacant(slot) => {
                slot.insert(cost);
            }
            Entry::Occupied(existing) if *existing.get() != cost => conflicting.push(tier),
            Entry::Occupied(_) => {}
        }
    }
    for tier in conflicting {
        if costs.remove(&tier).is_some() {
            warn!(
                provider_id,
                model_id,
                %tier,
                "models.dev modes price one service tier differently; ignoring the tier"
            );
        }
    }
    costs
}

/// The tier a mode selects and its schedule; `None` for a mode without rates or one that
/// changes the request beyond its service tier, such as a reasoning mode.
fn mode_service_tier_cost(
    provider_id: &str,
    model_id: &str,
    mode_name: &str,
    mode: &Value,
) -> Option<(ServiceTier, PricingCatalogServiceTierCost)> {
    let mode = match ModeDocument::deserialize(mode) {
        Ok(mode) => mode,
        Err(error) => {
            debug!(provider_id, model_id, mode_name, %error, "skipping unreadable models.dev mode");
            return None;
        }
    };
    let cost = mode.cost?;
    let tier = mode_service_tier(mode.provider.body)?;
    if tier == ServiceTier::Default {
        return None;
    }
    match project_service_tier_cost(provider_id, model_id, &cost) {
        Ok(cost) => Some((tier, cost)),
        Err(error) => {
            warn!(provider_id, model_id, mode_name, %error, "skipping models.dev mode with an invalid cost");
            None
        }
    }
}

/// The tier a mode's request body selects with the fields callers send (`service_tier`,
/// `speed`), at the top level or in Bedrock's model request fields. `None` when the body
/// selects no tier or sets any other field.
fn mode_service_tier(mut body: BTreeMap<String, Value>) -> Option<ServiceTier> {
    let tier = take_requested_service_tier(&mut body).ok()?;
    let nested_tier = match body.remove(BEDROCK_MODEL_REQUEST_FIELDS) {
        Some(Value::Object(fields)) => mode_service_tier(fields.into_iter().collect()),
        Some(_) => return None,
        None => None,
    };
    if !body.is_empty() {
        return None;
    }
    match (tier, nested_tier) {
        (Some(tier), Some(nested)) => (tier == nested).then_some(tier),
        (tier, nested) => tier.or(nested),
    }
}

fn project_service_tier_cost(
    provider_id: &str,
    model_id: &str,
    cost: &CostDocument,
) -> Result<PricingCatalogServiceTierCost, GatewayError> {
    let mut conditions = cost.conditions.clone();
    Ok(PricingCatalogServiceTierCost {
        input: project_cost(cost.input.as_ref())?,
        output: project_cost(cost.output.as_ref())?,
        cache_read: project_cost(cost.cache_read.as_ref())?,
        cache_write: project_cost(cost.cache_write.as_ref())?,
        context_tiers: take_context_tiers(provider_id, model_id, &mut conditions)?,
    })
}

/// Moves the context entries of models.dev `conditions.tiers` into typed tiers, sorted by
/// threshold with identical duplicates removed. Once typed tiers exist, the
/// `context_over_200k` condition is dropped too: it duplicates the first tier, often at a
/// different threshold. Tiers of another type stay in `conditions`.
///
/// Two different tiers at one threshold leave the model with no typed tiers and its
/// conditions untouched, so it bills at the base rates.
fn take_context_tiers(
    provider_id: &str,
    model_id: &str,
    conditions: &mut BTreeMap<String, serde_json::Value>,
) -> Result<Vec<PricingCatalogContextTier>, GatewayError> {
    let Some(serde_json::Value::Array(entries)) = conditions.get(TIERS_CONDITION) else {
        return Ok(Vec::new());
    };
    let mut tiers = Vec::new();
    let mut other_entries = Vec::new();
    for entry in entries {
        match TierDocument::deserialize(entry) {
            Ok(tier) if tier.tier.kind == CONTEXT_TIER_TYPE && tier.tier.size >= 0 => {
                tiers.push(project_context_tier(&tier)?);
            }
            _ => other_entries.push(entry.clone()),
        }
    }
    if tiers.is_empty() {
        return Ok(tiers);
    }
    tiers.sort_by_key(|tier| tier.above_input_tokens);
    tiers.dedup();
    if tiers
        .windows(2)
        .any(|pair| pair[0].above_input_tokens == pair[1].above_input_tokens)
    {
        warn!(
            provider_id,
            model_id,
            "models.dev lists different context tiers at one threshold; ignoring the model's tiers"
        );
        return Ok(Vec::new());
    }

    if other_entries.is_empty() {
        conditions.remove(TIERS_CONDITION);
    } else {
        conditions.insert(
            TIERS_CONDITION.to_string(),
            serde_json::Value::Array(other_entries),
        );
    }
    conditions.remove(CONTEXT_OVER_200K_CONDITION);
    Ok(tiers)
}

fn project_context_tier(tier: &TierDocument) -> Result<PricingCatalogContextTier, GatewayError> {
    Ok(PricingCatalogContextTier {
        above_input_tokens: tier.tier.size,
        input: project_cost(tier.input.as_ref())?,
        output: project_cost(tier.output.as_ref())?,
        cache_read: project_cost(tier.cache_read.as_ref())?,
        cache_write: project_cost(tier.cache_write.as_ref())?,
    })
}

fn normalize_money(number: &Number) -> Result<String, GatewayError> {
    let raw = number.to_string();
    if let Ok(money) = Money4::from_decimal_str(&raw) {
        return Ok(money.format_4dp());
    }

    let value = number.as_f64().ok_or_else(|| {
        GatewayError::Internal(format!(
            "failed normalizing models.dev cost `{raw}`: not finite"
        ))
    })?;
    let scaled = (value * Money4::SCALE as f64).round();
    if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(GatewayError::Internal(format!(
            "failed normalizing models.dev cost `{raw}`: rounded value overflowed"
        )));
    }

    Ok(Money4::from_scaled(scaled as i64).format_4dp())
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderDocument {
    name: String,
    #[serde(default)]
    models: BTreeMap<String, ModelDocument>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelDocument {
    #[serde(flatten)]
    metadata: CatalogModelMetadata,
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    last_updated: String,
    #[serde(default)]
    cost: CostDocument,
    #[serde(default)]
    limit: LimitDocument,
    #[serde(default)]
    modalities: ModalitiesDocument,
    /// Parsed per mode in [`service_tier_costs`], so one unreadable mode skips only itself.
    #[serde(default)]
    experimental: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct CostDocument {
    #[serde(flatten)]
    conditions: BTreeMap<String, serde_json::Value>,
    input: Option<Number>,
    output: Option<Number>,
    cache_read: Option<Number>,
    cache_write: Option<Number>,
    input_audio: Option<Number>,
    output_audio: Option<Number>,
}

/// One models.dev `experimental.modes` entry: a request variant and its rates.
#[derive(Debug, Deserialize)]
struct ModeDocument {
    cost: Option<CostDocument>,
    #[serde(default)]
    provider: ModeProviderDocument,
}

/// What a mode changes in the provider request; headers do not change billing.
#[derive(Debug, Default, Deserialize)]
struct ModeProviderDocument {
    #[serde(default)]
    body: BTreeMap<String, Value>,
}

/// One entry of models.dev `cost.tiers`.
#[derive(Debug, Deserialize)]
struct TierDocument {
    input: Option<Number>,
    output: Option<Number>,
    cache_read: Option<Number>,
    cache_write: Option<Number>,
    tier: TierCondition,
}

#[derive(Debug, Deserialize)]
struct TierCondition {
    #[serde(rename = "type")]
    kind: String,
    size: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LimitDocument {
    context: Option<i64>,
    input: Option<i64>,
    output: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ModalitiesDocument {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// Projects one OpenAI model document with `fields` beside its id and name.
    fn project_openai_model(fields: Value) -> PricingCatalogCostDocument {
        let mut model = json!({"id": "gpt-6-astra", "name": "GPT-6 Astra"});
        model
            .as_object_mut()
            .expect("model object")
            .extend(fields.as_object().expect("fields object").clone());
        let body = json!({"openai": {"name": "OpenAI", "models": {"gpt-6-astra": model}}});
        let mut providers = project(body.to_string().as_bytes()).expect("projected providers");
        providers
            .remove("openai")
            .and_then(|mut provider| provider.models.remove("gpt-6-astra"))
            .expect("projected model")
            .cost
    }

    fn project_openai_cost(cost: Value) -> PricingCatalogCostDocument {
        project_openai_model(json!({"cost": cost}))
    }

    fn project_openai_modes(modes: Value) -> BTreeMap<ServiceTier, PricingCatalogServiceTierCost> {
        project_openai_model(json!({
            "cost": {"input": 1, "output": 2},
            "experimental": {"modes": modes}
        }))
        .service_tiers
    }

    fn mode(cost: Value, body: Value) -> Value {
        json!({"cost": cost, "provider": {"body": body}})
    }

    fn tier_cost(input: &str, output: &str) -> PricingCatalogServiceTierCost {
        PricingCatalogServiceTierCost {
            input: Some(input.to_string()),
            output: Some(output.to_string()),
            ..PricingCatalogServiceTierCost::default()
        }
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
    fn money_normalization_rounds_extra_precision() {
        let cost = Number::from_f64(0.00875).expect("number");

        assert_eq!(normalize_money(&cost).expect("normalized cost"), "0.0088");
    }

    #[test]
    fn context_tiers_replace_the_context_over_200k_condition() {
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
    fn context_tiers_are_sorted_and_deduplicated_and_other_types_kept() {
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
    fn conflicting_tiers_at_one_threshold_leave_base_rates_only() {
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
    fn modes_price_the_service_tiers_their_request_bodies_select() {
        let fast_cost = json!({"input": 2, "output": 4, "cache_read": 0.2});
        let tiers = project_openai_modes(json!({
            "fast": mode(fast_cost.clone(), json!({"speed": "fast"})),
            "priority": mode(fast_cost, json!({"service_tier": "priority"})),
            "flex": mode(
                json!({
                    "input": 0.5, "output": 1,
                    "tiers": [{"input": 1, "output": 2, "tier": {"type": "context", "size": 272_000}}]
                }),
                json!({"service_tier": "flex"}),
            ),
            "ultrafast": mode(
                json!({"input": 6, "output": 12}),
                json!({"additionalModelRequestFields": {"service_tier": "ultrafast"}}),
            ),
            "pro": {"provider": {"body": {"reasoning": {"mode": "pro"}}}},
            "pro_fast": mode(
                json!({"input": 30, "output": 60}),
                json!({"service_tier": "priority", "reasoning": {"mode": "pro"}}),
            ),
        }));

        assert_eq!(
            tiers,
            BTreeMap::from([
                (
                    ServiceTier::Flex,
                    PricingCatalogServiceTierCost {
                        context_tiers: vec![tier(272_000, "1.0000", "2.0000")],
                        ..tier_cost("0.5000", "1.0000")
                    },
                ),
                (
                    ServiceTier::Priority,
                    PricingCatalogServiceTierCost {
                        cache_read: Some("0.2000".to_string()),
                        ..tier_cost("2.0000", "4.0000")
                    },
                ),
                (ServiceTier::Ultrafast, tier_cost("6.0000", "12.0000")),
            ]),
            "modes changing more than the tier are not tier prices"
        );
    }

    #[test]
    fn modes_pricing_one_tier_differently_leave_that_tier_unpriced() {
        let tiers = project_openai_modes(json!({
            "fast": mode(json!({"input": 2, "output": 4}), json!({"speed": "fast"})),
            "priority": mode(json!({"input": 2.5, "output": 5}), json!({"service_tier": "priority"})),
            "flex": mode(json!({"input": 0.5, "output": 1}), json!({"service_tier": "flex"})),
        }));

        assert_eq!(
            tiers,
            BTreeMap::from([(ServiceTier::Flex, tier_cost("0.5000", "1.0000"))])
        );
    }
}
