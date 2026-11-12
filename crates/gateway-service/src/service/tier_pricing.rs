//! The rate schedule one request bills at. The billed service tier (served, else
//! requested, else default) chooses it, and a tier without a rate for a token bucket the
//! request used leaves the request unpriced: premium usage is never billed at the
//! standard rate.

use gateway_core::{
    ContextPricingTier, GatewayError, Money4, ResolvedModelPricing, RoutePricingOverride,
    ServiceTier, ServiceTierPricing, UsageLedgerRecord, UsagePricingStatus, billed_service_tier,
};

use super::{apply_token_rates, select_context_tier};
use crate::usage::CacheUsageNormalization;

/// Which schedule a request bills at, which names the reason for a missing rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RateSchedule {
    /// The route's own schedule: a gap names the missing rate.
    Base,
    /// Another service tier's schedule: a gap leaves the tier unpriced.
    ServiceTier(ServiceTier),
}

impl RateSchedule {
    /// The unpriced reason for a used token bucket (`input`, `cache_read`, ...) that the
    /// schedule has no rate for.
    pub(super) fn missing_rate_reason(self, bucket: &str) -> String {
        match self {
            Self::Base => format!("missing_{bucket}_rate"),
            Self::ServiceTier(tier) => service_tier_unpriced_reason(tier),
        }
    }
}

/// Per-million rates one request bills at and the context tier threshold that chose them.
#[derive(Debug, Clone, Copy)]
struct BilledRates {
    schedule: RateSchedule,
    input: Option<Money4>,
    output: Option<Money4>,
    cache_read: Option<Money4>,
    cache_write: Option<Money4>,
    image_output: Option<Money4>,
    above_input_tokens: Option<i64>,
}

/// Catalog pricing for one request. The context tier its normalized prompt tokens select
/// replaces the schedule's flat rates for the whole request, an unset context tier rate
/// inheriting the flat one; the ledger records the rates billed and the tier threshold.
pub(super) fn apply_exact_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &ResolvedModelPricing,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    record.pricing_row_id = Some(pricing.model_pricing_id);
    record.pricing_provider_id = Some(pricing.pricing_provider_id.clone());
    record.pricing_model_id = Some(pricing.model_id.clone());
    record.pricing_source = Some(pricing.provenance.source.clone());
    record.pricing_source_etag = pricing.provenance.etag.clone();
    record.pricing_source_fetched_at = Some(pricing.provenance.fetched_at);
    record.pricing_last_updated = Some(pricing.last_updated.clone());

    let tier = billed_service_tier(record.requested_service_tier, record.served_service_tier);
    let rates = catalog_rates(pricing, tier, record);
    apply_billed_rates(record, tier, rates, cache_usage)
}

/// Route override pricing for one request. The override's rates price `route_tier`, the
/// tier the route serves by default; its `service_tiers` entries price the others.
pub(super) fn apply_configured_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &RoutePricingOverride,
    route_tier: ServiceTier,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    record.pricing_source = Some("configured_override".to_string());

    let tier = billed_service_tier(record.requested_service_tier, record.served_service_tier);
    let rates = override_rates(pricing, route_tier, tier, record);
    apply_billed_rates(record, tier, rates, cache_usage)
}

fn apply_billed_rates(
    record: &mut UsageLedgerRecord,
    tier: ServiceTier,
    rates: Option<BilledRates>,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    let Some(rates) = rates else {
        record.pricing_status = UsagePricingStatus::Unpriced;
        let reason = service_tier_unpriced_reason(tier);
        record.unpriced_reason = Some(match record.unpriced_reason.take() {
            Some(normalization_reason) => format!("{normalization_reason};{reason}"),
            None => reason,
        });
        return Ok(());
    };
    record.input_cost_per_million_tokens = rates.input;
    record.output_cost_per_million_tokens = rates.output;
    record.cache_read_cost_per_million_tokens = rates.cache_read;
    record.cache_write_cost_per_million_tokens = rates.cache_write;
    record.image_output_cost_per_million_tokens = rates.image_output;
    record.pricing_tier_above_input_tokens = rates.above_input_tokens;
    apply_token_rates(
        record,
        rates.input,
        rates.output,
        rates.schedule,
        cache_usage,
    )
}

/// The catalog rates `tier` bills at. Default bills the base schedule. Any other tier
/// bills its own schedule, also on a route that forces the tier: the catalog's base
/// schedule is always the default tier's.
fn catalog_rates(
    pricing: &ResolvedModelPricing,
    tier: ServiceTier,
    record: &UsageLedgerRecord,
) -> Option<BilledRates> {
    let base_context = select_context_tier(&pricing.context_tiers, record.prompt_tokens);
    if tier == ServiceTier::Default {
        return Some(BilledRates {
            schedule: RateSchedule::Base,
            input: base_context
                .and_then(|context| context.input)
                .or(pricing.input_cost_per_million_tokens),
            output: base_context
                .and_then(|context| context.output)
                .or(pricing.output_cost_per_million_tokens),
            cache_read: base_context
                .and_then(|context| context.cache_read)
                .or(pricing.cache_read_cost_per_million_tokens),
            cache_write: base_context
                .and_then(|context| context.cache_write)
                .or(pricing.cache_write_cost_per_million_tokens),
            image_output: pricing.output_image_usd_per_million,
            above_input_tokens: base_context.map(|context| context.above_input_tokens),
        });
    }
    let schedule = pricing
        .service_tiers
        .iter()
        .find(|schedule| schedule.service_tier == tier)?;
    if bills_image_output_apart(pricing.output_image_usd_per_million, record) {
        return None;
    }
    service_tier_rates(schedule, base_context, record.prompt_tokens)
}

/// A catalog service tier's rates for a request. A base context tier the request crosses
/// must have a twin at the same threshold in the service tier: without one the tier has
/// no rate for a request that size.
fn service_tier_rates(
    schedule: &ServiceTierPricing,
    base_context: Option<&ContextPricingTier>,
    prompt_tokens: Option<i64>,
) -> Option<BilledRates> {
    if let Some(base_context) = base_context
        && !schedule
            .context_tiers
            .iter()
            .any(|context| context.above_input_tokens == base_context.above_input_tokens)
    {
        return None;
    }
    let context = select_context_tier(&schedule.context_tiers, prompt_tokens);
    Some(BilledRates {
        schedule: RateSchedule::ServiceTier(schedule.service_tier),
        input: context.and_then(|context| context.input).or(schedule.input),
        output: context
            .and_then(|context| context.output)
            .or(schedule.output),
        cache_read: context
            .and_then(|context| context.cache_read)
            .or(schedule.cache_read),
        cache_write: context
            .and_then(|context| context.cache_write)
            .or(schedule.cache_write),
        image_output: None,
        above_input_tokens: context.map(|context| context.above_input_tokens),
    })
}

/// The override rates `tier` bills at: the override's own for the route's tier, else the
/// tier's entry in its `service_tiers`.
fn override_rates(
    pricing: &RoutePricingOverride,
    route_tier: ServiceTier,
    tier: ServiceTier,
    record: &UsageLedgerRecord,
) -> Option<BilledRates> {
    if tier == route_tier {
        return Some(BilledRates {
            schedule: RateSchedule::Base,
            input: Some(pricing.input_cost_per_million_tokens),
            output: Some(pricing.output_cost_per_million_tokens),
            cache_read: pricing.cache_read_cost_per_million_tokens,
            cache_write: pricing.cache_write_cost_per_million_tokens,
            image_output: pricing.image_output_cost_per_million_tokens,
            above_input_tokens: None,
        });
    }
    let rates = pricing
        .service_tiers
        .iter()
        .find(|rates| rates.service_tier == tier)?;
    if bills_image_output_apart(pricing.image_output_cost_per_million_tokens, record) {
        return None;
    }
    Some(BilledRates {
        schedule: RateSchedule::ServiceTier(tier),
        input: Some(rates.input_cost_per_million_tokens),
        output: Some(rates.output_cost_per_million_tokens),
        cache_read: rates.cache_read_cost_per_million_tokens,
        cache_write: rates.cache_write_cost_per_million_tokens,
        image_output: None,
        above_input_tokens: None,
    })
}

/// Whether the base schedule prices the request's image output tokens at an image rate.
/// Service tier schedules have no image rate, so such a request has no tier rate.
fn bills_image_output_apart(base_image_rate: Option<Money4>, record: &UsageLedgerRecord) -> bool {
    base_image_rate.is_some() && record.image_output_tokens.unwrap_or_default() > 0
}

fn service_tier_unpriced_reason(tier: ServiceTier) -> String {
    format!("service_tier:{tier}")
}
