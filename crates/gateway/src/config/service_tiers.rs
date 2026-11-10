//! Service tier configuration: a route's forced `service_tier`, a model's
//! `service_tiers` allow-list, and the per-tier rates of a route pricing override.
//! Tier names accept the `fast` and `standard` aliases.

use std::collections::BTreeMap;

use anyhow::bail;
use gateway_core::{RouteServiceTierPricing, SERVICE_TIER_REQUEST_FIELDS, ServiceTier};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};

use super::routes::{
    deserialize_optional_yaml_string, deserialize_yaml_string, parse_non_negative_rate,
    parse_optional_non_negative_rate,
};

/// Provider headers that select a tier, lowercase. Routes set `service_tier` instead.
const SERVICE_TIER_HEADERS: [&str; 2] = [
    "x-vertex-ai-llm-shared-request-type",
    "x-amzn-bedrock-service-tier",
];

fn parse_service_tier<E: de::Error>(value: &str) -> Result<ServiceTier, E> {
    ServiceTier::parse(value).ok_or_else(|| {
        E::custom(format!(
            "unknown service tier `{value}`; expected default, flex, priority (or fast), or ultrafast"
        ))
    })
}

fn deserialize_service_tier<'de, D>(deserializer: D) -> Result<ServiceTier, D::Error>
where
    D: Deserializer<'de>,
{
    parse_service_tier(&String::deserialize(deserializer)?)
}

pub(super) fn deserialize_optional_service_tier<'de, D>(
    deserializer: D,
) -> Result<Option<ServiceTier>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .as_deref()
        .map(parse_service_tier::<D::Error>)
        .transpose()
}

pub(super) fn deserialize_optional_service_tiers<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<ServiceTier>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<String>>::deserialize(deserializer)?
        .map(|values| {
            values
                .iter()
                .map(|value| parse_service_tier::<D::Error>(value))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
}

/// Checks a model's `service_tiers` allow-list. The default tier is always allowed, so
/// the list names only the other tiers, each once.
pub(super) fn validate_model_service_tiers(
    model_id: &str,
    service_tiers: Option<&[ServiceTier]>,
) -> anyhow::Result<()> {
    let Some(service_tiers) = service_tiers else {
        return Ok(());
    };
    for (index, tier) in service_tiers.iter().enumerate() {
        if *tier == ServiceTier::Default {
            bail!(
                "model `{model_id}` service_tiers cannot list `default`; the default tier is always allowed"
            );
        }
        if service_tiers[..index].contains(tier) {
            bail!(
                "model `{model_id}` service_tiers lists `{}` more than once",
                tier.as_str()
            );
        }
    }
    Ok(())
}

/// Rejects tier selection through a route's `extra_body` or `extra_headers`. The typed
/// `service_tier` is what lets the gateway route, price and record the tier.
pub(super) fn validate_route_tier_passthrough(
    route_label: &str,
    extra_body: &Map<String, Value>,
    extra_headers: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    if let Some(field) = SERVICE_TIER_REQUEST_FIELDS
        .iter()
        .find(|field| extra_body.contains_key(**field))
    {
        bail!(
            "{route_label} extra_body cannot set `{field}`; set the route's `service_tier` instead"
        );
    }
    if let Some(header) = extra_headers.keys().find(|name| {
        SERVICE_TIER_HEADERS
            .iter()
            .any(|tier_header| name.eq_ignore_ascii_case(tier_header))
    }) {
        bail!(
            "{route_label} extra_headers cannot set `{header}`; set the route's `service_tier` instead"
        );
    }
    Ok(())
}

/// One tier's flat rates in a route `pricing_override.service_tiers` list.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteServiceTierPricingConfig {
    #[serde(deserialize_with = "deserialize_service_tier")]
    pub service_tier: ServiceTier,
    #[serde(deserialize_with = "deserialize_yaml_string")]
    pub input_usd_per_million_tokens: String,
    #[serde(deserialize_with = "deserialize_yaml_string")]
    pub output_usd_per_million_tokens: String,
    #[serde(default, deserialize_with = "deserialize_optional_yaml_string")]
    pub cache_read_usd_per_million_tokens: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_yaml_string")]
    pub cache_write_usd_per_million_tokens: Option<String>,
}

impl RouteServiceTierPricingConfig {
    fn resolve(&self, label: &str) -> anyhow::Result<RouteServiceTierPricing> {
        Ok(RouteServiceTierPricing {
            service_tier: self.service_tier,
            input_cost_per_million_tokens: parse_non_negative_rate(
                &self.input_usd_per_million_tokens,
                &format!("{label}.input_usd_per_million_tokens"),
            )?,
            output_cost_per_million_tokens: parse_non_negative_rate(
                &self.output_usd_per_million_tokens,
                &format!("{label}.output_usd_per_million_tokens"),
            )?,
            cache_read_cost_per_million_tokens: parse_optional_non_negative_rate(
                self.cache_read_usd_per_million_tokens.as_deref(),
                label,
                "cache_read_usd_per_million_tokens",
            )?,
            cache_write_cost_per_million_tokens: parse_optional_non_negative_rate(
                self.cache_write_usd_per_million_tokens.as_deref(),
                label,
                "cache_write_usd_per_million_tokens",
            )?,
        })
    }
}

/// Resolves a pricing override's tier rates, sorted by tier. The override's base rates
/// price the tier the route serves (its `service_tier`, else default), so the list
/// cannot repeat that tier; a forced-tier route may list `default` to price requests
/// the provider downgrades. Each tier appears once.
pub(super) fn resolve_service_tier_pricing(
    entries: &[RouteServiceTierPricingConfig],
    route_service_tier: Option<ServiceTier>,
    label: &str,
) -> anyhow::Result<Vec<RouteServiceTierPricing>> {
    let own_tier = route_service_tier.unwrap_or(ServiceTier::Default);
    let mut resolved: Vec<RouteServiceTierPricing> = Vec::with_capacity(entries.len());
    for entry in entries {
        let tier = entry.service_tier.as_str();
        if entry.service_tier == own_tier {
            bail!(
                "{label}.service_tiers cannot list `{tier}`, the tier the route serves; the override's base rates price it"
            );
        }
        if resolved
            .iter()
            .any(|pricing| pricing.service_tier == entry.service_tier)
        {
            bail!("{label}.service_tiers lists `{tier}` more than once");
        }
        resolved.push(entry.resolve(&format!("{label}.service_tiers.{tier}"))?);
    }
    resolved.sort_by_key(|pricing| pricing.service_tier);
    Ok(resolved)
}
