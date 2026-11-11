//! LiteLLM per-token rate keys projected into a cost document: the base schedule, its
//! context tiers, and one schedule per service tier.
//!
//! A rate key is `<rate>[_above_<N>k_tokens][_flex|_priority|_ultrafast]`. Keys with any
//! other suffix (`_batches`, `_above_1hr`, ...) price other dimensions and are ignored.

use std::collections::BTreeMap;

use gateway_core::ServiceTier;
use serde_json::{Map, Value};

use super::super::f64_per_token_to_per_million;
use crate::pricing_catalog::{
    PricingCatalogContextTier, PricingCatalogCostDocument, PricingCatalogServiceTierCost,
};

/// LiteLLM per-token rate keys in [`Rates`] order.
const RATE_KEYS: [&str; 4] = [
    "input_cost_per_token",
    "output_cost_per_token",
    "cache_read_input_token_cost",
    "cache_creation_input_token_cost",
];
const OUTPUT_IMAGE_RATE_KEY: &str = "output_cost_per_image_token";
/// Rate key suffixes naming a non-default service tier.
const SERVICE_TIER_SUFFIXES: [(&str, ServiceTier); 3] = [
    ("_flex", ServiceTier::Flex),
    ("_priority", ServiceTier::Priority),
    ("_ultrafast", ServiceTier::Ultrafast),
];

/// Per-token input, output, cache read and cache write rates.
type Rates = [Option<f64>; 4];

/// One service tier's per-token rates and their context tiers, keyed by threshold.
#[derive(Debug, Clone, Default)]
struct TierRates {
    flat: Rates,
    context: BTreeMap<i64, Rates>,
}

impl TierRates {
    fn scaled(&self, multiplier: f64) -> Self {
        let scale = |rates: &Rates| rates.map(|rate| rate.map(|rate| rate * multiplier));
        Self {
            flat: scale(&self.flat),
            context: self
                .context
                .iter()
                .map(|(threshold, rates)| (*threshold, scale(rates)))
                .collect(),
        }
    }
}

/// The cost document of a LiteLLM entry's `fields`. A `fast_multiplier` prices the
/// priority tier as the base schedule times the multiplier when the entry lists no
/// priority rates of its own.
pub(super) fn cost_document(
    fields: &Map<String, Value>,
    fast_multiplier: Option<f64>,
) -> PricingCatalogCostDocument {
    let mut schedules = tier_rates(fields);
    let base = schedules.remove(&ServiceTier::Default).unwrap_or_default();
    if let Some(multiplier) = fast_multiplier
        && base.flat.iter().any(Option::is_some)
    {
        schedules
            .entry(ServiceTier::Priority)
            .or_insert_with(|| base.scaled(multiplier));
    }
    let [input, output, cache_read, cache_write] = per_million(&base.flat);
    PricingCatalogCostDocument {
        input,
        output,
        cache_read,
        cache_write,
        output_image: fields
            .get(OUTPUT_IMAGE_RATE_KEY)
            .and_then(Value::as_f64)
            .and_then(f64_per_token_to_per_million),
        tiers: context_tiers(&base.context),
        service_tiers: schedules
            .iter()
            .map(|(tier, rates)| (*tier, service_tier_cost(rates)))
            .collect(),
        ..PricingCatalogCostDocument::default()
    }
}

/// Every rate key's value, grouped by service tier.
fn tier_rates(fields: &Map<String, Value>) -> BTreeMap<ServiceTier, TierRates> {
    let mut schedules = BTreeMap::<ServiceTier, TierRates>::new();
    for (key, value) in fields {
        let Some((bucket, above_input_tokens, tier)) = parse_rate_key(key) else {
            continue;
        };
        let Some(rate) = value.as_f64().filter(|rate| rate.is_finite()) else {
            continue;
        };
        let schedule = schedules.entry(tier).or_default();
        let rates = match above_input_tokens {
            Some(threshold) => schedule.context.entry(threshold).or_default(),
            None => &mut schedule.flat,
        };
        rates[bucket] = Some(rate);
    }
    schedules
}

/// The [`Rates`] index, context threshold and service tier a rate key names.
fn parse_rate_key(key: &str) -> Option<(usize, Option<i64>, ServiceTier)> {
    let (bucket, rest) = RATE_KEYS
        .iter()
        .enumerate()
        .find_map(|(bucket, rate_key)| Some((bucket, key.strip_prefix(rate_key)?)))?;
    let (rest, tier) = SERVICE_TIER_SUFFIXES
        .iter()
        .find_map(|(suffix, tier)| Some((rest.strip_suffix(suffix)?, *tier)))
        .unwrap_or((rest, ServiceTier::Default));
    if rest.is_empty() {
        return Some((bucket, None, tier));
    }
    let thousands = rest.strip_prefix("_above_")?.strip_suffix("k_tokens")?;
    if thousands.is_empty() || !thousands.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let above_input_tokens = thousands.parse::<i64>().ok()?.checked_mul(1000)?;
    Some((bucket, Some(above_input_tokens), tier))
}

fn per_million(rates: &Rates) -> [Option<String>; 4] {
    rates
        .each_ref()
        .map(|rate| rate.and_then(f64_per_token_to_per_million))
}

/// Context tiers ascending by threshold.
fn context_tiers(context: &BTreeMap<i64, Rates>) -> Vec<PricingCatalogContextTier> {
    context
        .iter()
        .map(|(above_input_tokens, rates)| {
            let [input, output, cache_read, cache_write] = per_million(rates);
            PricingCatalogContextTier {
                above_input_tokens: *above_input_tokens,
                input,
                output,
                cache_read,
                cache_write,
            }
        })
        .collect()
}

fn service_tier_cost(rates: &TierRates) -> PricingCatalogServiceTierCost {
    let [input, output, cache_read, cache_write] = per_million(&rates.flat);
    PricingCatalogServiceTierCost {
        input,
        output,
        cache_read,
        cache_write,
        context_tiers: context_tiers(&rates.context),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn cost_of(fields: &Value, fast_multiplier: Option<f64>) -> PricingCatalogCostDocument {
        cost_document(fields.as_object().expect("fields object"), fast_multiplier)
    }

    fn context_tier(
        above_input_tokens: i64,
        input: &str,
        output: &str,
    ) -> PricingCatalogContextTier {
        PricingCatalogContextTier {
            above_input_tokens,
            input: Some(input.to_string()),
            output: Some(output.to_string()),
            cache_read: None,
            cache_write: None,
        }
    }

    #[test]
    fn tier_suffixed_keys_build_one_schedule_per_service_tier() {
        let cost = cost_of(
            &json!({
                "input_cost_per_token": 1e-07,
                "output_cost_per_token": 5e-07,
                "input_cost_per_token_above_272k_tokens": 2e-07,
                "output_cost_per_token_above_272k_tokens": 7.5e-07,
                "input_cost_per_token_priority": 2e-07,
                "output_cost_per_token_priority": 1e-06,
                "cache_read_input_token_cost_priority": 2e-08,
                "input_cost_per_token_above_272k_tokens_priority": 4e-07,
                "output_cost_per_token_above_272k_tokens_priority": 1.5e-06,
                "input_cost_per_token_flex": 5e-08,
                "output_cost_per_token_flex": 2.5e-07,
                "output_cost_per_token_batches": 2.5e-07,
                "cache_creation_input_token_cost_above_1hr_priority": 9e-06,
                "cache_creation_input_token_cost_above_12hr_above_272k_tokens_ultrafast": 9e-06,
            }),
            None,
        );

        assert_eq!(cost.input.as_deref(), Some("0.1000"));
        assert_eq!(cost.tiers, [context_tier(272_000, "0.2000", "0.7500")]);
        assert_eq!(
            cost.service_tiers.keys().copied().collect::<Vec<_>>(),
            [ServiceTier::Flex, ServiceTier::Priority],
            "keys of other dimensions add no tier"
        );
        assert_eq!(
            cost.service_tiers[&ServiceTier::Priority],
            PricingCatalogServiceTierCost {
                input: Some("0.2000".to_string()),
                output: Some("1.0000".to_string()),
                cache_read: Some("0.0200".to_string()),
                cache_write: None,
                context_tiers: vec![context_tier(272_000, "0.4000", "1.5000")],
            }
        );
        assert_eq!(
            cost.service_tiers[&ServiceTier::Flex],
            PricingCatalogServiceTierCost {
                input: Some("0.0500".to_string()),
                output: Some("0.2500".to_string()),
                ..PricingCatalogServiceTierCost::default()
            }
        );
    }

    #[test]
    fn fast_multiplier_prices_priority_from_the_whole_base_schedule() {
        let cost = cost_of(
            &json!({
                "input_cost_per_token": 4e-06,
                "output_cost_per_token": 2e-05,
                "cache_read_input_token_cost": 2e-07,
                "cache_creation_input_token_cost": 5e-06,
                "input_cost_per_token_above_200k_tokens": 8e-06,
                "output_cost_per_token_above_200k_tokens": 3e-05,
            }),
            Some(2.0),
        );

        assert_eq!(
            cost.service_tiers[&ServiceTier::Priority],
            PricingCatalogServiceTierCost {
                input: Some("8.0000".to_string()),
                output: Some("40.0000".to_string()),
                cache_read: Some("0.4000".to_string()),
                cache_write: Some("10.0000".to_string()),
                context_tiers: vec![context_tier(200_000, "16.0000", "60.0000")],
            }
        );
        assert_eq!(
            cost.input.as_deref(),
            Some("4.0000"),
            "the base is unscaled"
        );
    }

    #[test]
    fn listed_priority_rates_win_over_the_fast_multiplier() {
        let cost = cost_of(
            &json!({
                "input_cost_per_token": 4e-06,
                "output_cost_per_token": 2e-05,
                "input_cost_per_token_priority": 6e-06,
            }),
            Some(2.0),
        );

        assert_eq!(
            cost.service_tiers[&ServiceTier::Priority],
            PricingCatalogServiceTierCost {
                input: Some("6.0000".to_string()),
                ..PricingCatalogServiceTierCost::default()
            }
        );
    }

    #[test]
    fn fast_multiplier_without_base_rates_adds_no_tier() {
        let cost = cost_of(&json!({"output_cost_per_image_token": 4e-05}), Some(2.0));

        assert!(cost.service_tiers.is_empty());
        assert_eq!(cost.output_image.as_deref(), Some("40.0000"));
    }
}
