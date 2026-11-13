//! The rates one request bills at. The billed service tier (served, else requested, else
//! default) chooses the schedule. When that schedule lacks an input or output rate the
//! request needs, the base schedule prices it instead: the catalog's default tier, or the
//! route override's own rates. A cache bucket without a rate bills at a multiple of the
//! input rate. Both are estimates: the request counts toward spend as `legacy_estimated`,
//! with an `estimated_*` reason, instead of costing nothing.

use gateway_core::{
    ContextPricingTier, GatewayError, Money4, ResolvedModelPricing, RoutePricingOverride,
    ServiceTier, ServiceTierPricing, UsageLedgerRecord, UsagePricingStatus, billed_service_tier,
};

use super::{cache_write_buckets, output_token_buckets, select_context_tier, token_cost};
use crate::usage::CacheUsageNormalization;

/// Which schedule a request bills at, which names the reason for a missing rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateSchedule {
    /// The route's own schedule: a gap names the missing rate.
    Base,
    /// Another service tier's schedule: a gap names the tier.
    ServiceTier(ServiceTier),
}

impl RateSchedule {
    /// The unpriced reason for a used token bucket (`input`, `cache_read`, ...) that the
    /// schedule has no rate for.
    fn missing_rate_reason(self, bucket: &str) -> String {
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
    cache_write_1h: Option<Money4>,
    image_output: Option<Money4>,
    above_input_tokens: Option<i64>,
}

impl BilledRates {
    /// The token bucket the request used that these rates cannot price. A cache bucket
    /// without a rate is priced from the input rate when there is one.
    fn unpriced_bucket(&self, record: &UsageLedgerRecord) -> Option<&'static str> {
        let input_tokens = record
            .uncached_input_tokens
            .unwrap_or_else(|| record.prompt_tokens.unwrap_or_default());
        if input_tokens > 0 && self.input.is_none() {
            return Some("input");
        }
        let (text_output_tokens, _) = output_token_buckets(record, self.image_output);
        if text_output_tokens.unwrap_or_default() > 0 && self.output.is_none() {
            return Some("output");
        }
        if self.input.is_some() {
            return None;
        }
        let reads = record.cache_read_tokens.unwrap_or_default();
        let (writes_5m, writes_1h) = cache_write_buckets(record);
        [
            ("cache_read", reads, self.cache_read),
            ("cache_write", writes_5m, self.cache_write),
            ("cache_write_1h", writes_1h, self.cache_write_1h),
        ]
        .into_iter()
        .find_map(|(bucket, tokens, rate)| (tokens > 0 && rate.is_none()).then_some(bucket))
    }

    /// These rates times `numerator / denominator`.
    fn scaled(self, numerator: i64, denominator: i64) -> Self {
        let scale =
            |rate: Option<Money4>| rate.map(|rate| scaled_rate(rate, numerator, denominator));
        Self {
            input: scale(self.input),
            output: scale(self.output),
            cache_read: scale(self.cache_read),
            cache_write: scale(self.cache_write),
            cache_write_1h: scale(self.cache_write_1h),
            image_output: scale(self.image_output),
            ..self
        }
    }

    fn record_on(&self, record: &mut UsageLedgerRecord) {
        record.input_cost_per_million_tokens = self.input;
        record.output_cost_per_million_tokens = self.output;
        record.cache_read_cost_per_million_tokens = self.cache_read;
        record.cache_write_cost_per_million_tokens = self.cache_write;
        record.cache_write_1h_cost_per_million_tokens = self.cache_write_1h;
        record.image_output_cost_per_million_tokens = self.image_output;
        record.pricing_tier_above_input_tokens = self.above_input_tokens;
    }
}

/// How a request bills.
enum RateChoice {
    /// At the billed tier's rates.
    Billed(BilledRates),
    /// At the base schedule's rates, standing in for the billed tier's.
    TierEstimate(BilledRates),
    /// Not at all. Holds the billed tier's rates and the bucket they lack, when the tier
    /// has rates.
    Unpriced(Option<(BilledRates, &'static str)>),
}

/// Catalog pricing for one request. The context tier its normalized prompt tokens select
/// replaces the schedule's flat rates for the whole request, an unset context tier rate
/// inheriting the flat one; the ledger records the rates billed and the tier threshold.
pub(super) fn apply_exact_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &ResolvedModelPricing,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    record_catalog_provenance(record, pricing);
    let tier = billed_service_tier(record.requested_service_tier, record.served_service_tier);
    let choice = choose_catalog_rates(pricing, tier, record);
    apply_rate_choice(record, tier, choice, cache_usage)
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
    let fallback = (tier != route_tier)
        .then(|| override_rates(pricing, route_tier, route_tier, record))
        .flatten();
    let rates = override_rates(pricing, route_tier, tier, record);
    let choice = choose_rates(record, rates, fallback);
    apply_rate_choice(record, tier, choice, cache_usage)
}

/// Prices a request on a Vertex Anthropic route in a location the catalog does not price
/// at `global`, the model's global rates, plus Google's 10% regional endpoint premium, as
/// an estimate. Returns `false`, leaving `record` untouched, when `global` cannot price
/// the request.
pub(super) fn apply_regional_vertex_estimate(
    record: &mut UsageLedgerRecord,
    global: &ResolvedModelPricing,
    location: &str,
    cache_usage: &CacheUsageNormalization,
) -> Result<bool, GatewayError> {
    let tier = billed_service_tier(record.requested_service_tier, record.served_service_tier);
    let choice = match choose_catalog_rates(global, tier, record) {
        RateChoice::Billed(rates) => RateChoice::Billed(rates.scaled(11, 10)),
        RateChoice::TierEstimate(rates) => RateChoice::TierEstimate(rates.scaled(11, 10)),
        RateChoice::Unpriced(_) => return Ok(false),
    };
    record_catalog_provenance(record, global);
    apply_rate_choice(record, tier, choice, cache_usage)?;
    mark_estimated(
        record,
        &format!("estimated_regional_vertex_rate:{location}"),
    );
    Ok(true)
}

/// Marks a priced row as an estimate, `reason` first among its reasons.
pub(super) fn mark_estimated(record: &mut UsageLedgerRecord, reason: &str) {
    record.pricing_status = UsagePricingStatus::LegacyEstimated;
    record.unpriced_reason = Some(match record.unpriced_reason.take() {
        Some(other) => format!("{reason};{other}"),
        None => reason.to_string(),
    });
}

/// The rate a 1-hour cache write bills at without its own: twice the input rate,
/// Anthropic's price for every Claude model.
pub(crate) fn fallback_cache_write_1h_rate(input: Money4) -> Money4 {
    scaled_rate(input, 2, 1)
}

fn record_catalog_provenance(record: &mut UsageLedgerRecord, pricing: &ResolvedModelPricing) {
    record.pricing_row_id = Some(pricing.model_pricing_id);
    record.pricing_provider_id = Some(pricing.pricing_provider_id.clone());
    record.pricing_model_id = Some(pricing.model_id.clone());
    record.pricing_source = Some(pricing.provenance.source.clone());
    record.pricing_source_etag = pricing.provenance.etag.clone();
    record.pricing_source_fetched_at = Some(pricing.provenance.fetched_at);
    record.pricing_last_updated = Some(pricing.last_updated.clone());
}

/// The catalog rates `tier` bills at, the default tier's standing in when they cannot
/// price the request.
fn choose_catalog_rates(
    pricing: &ResolvedModelPricing,
    tier: ServiceTier,
    record: &UsageLedgerRecord,
) -> RateChoice {
    let fallback = (tier != ServiceTier::Default)
        .then(|| catalog_rates(pricing, ServiceTier::Default, record))
        .flatten();
    choose_rates(record, catalog_rates(pricing, tier, record), fallback)
}

/// The billed tier's `rates` when they price the request, else the base schedule's
/// `fallback` when those do.
fn choose_rates(
    record: &UsageLedgerRecord,
    rates: Option<BilledRates>,
    fallback: Option<BilledRates>,
) -> RateChoice {
    let gap = match rates {
        Some(rates) => match rates.unpriced_bucket(record) {
            None => return RateChoice::Billed(rates),
            Some(bucket) => Some((rates, bucket)),
        },
        None => None,
    };
    match fallback.filter(|fallback| fallback.unpriced_bucket(record).is_none()) {
        Some(fallback) => RateChoice::TierEstimate(fallback),
        None => RateChoice::Unpriced(gap),
    }
}

fn apply_rate_choice(
    record: &mut UsageLedgerRecord,
    tier: ServiceTier,
    choice: RateChoice,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    match choice {
        RateChoice::Billed(rates) => price_at(record, rates, cache_usage)?,
        RateChoice::TierEstimate(rates) => {
            price_at(record, rates, cache_usage)?;
            mark_estimated(record, &format!("estimated_service_tier:{tier}"));
        }
        RateChoice::Unpriced(gap) => leave_unpriced(record, tier, gap, cache_usage),
    }
    Ok(())
}

/// Records `rates` on the row and prices the request at them.
fn price_at(
    record: &mut UsageLedgerRecord,
    rates: BilledRates,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    rates.record_on(record);
    let cache_estimate = fill_cache_rate_gaps(record);
    record.computed_cost_usd = token_cost(record)?;
    record.pricing_status = match cache_usage {
        // Inconsistent cache counters bill every prompt token at the input rate.
        CacheUsageNormalization::Invalid(_) => UsagePricingStatus::LegacyEstimated,
        CacheUsageNormalization::Unavailable | CacheUsageNormalization::Valid(_) => {
            UsagePricingStatus::Priced
        }
    };
    if let Some(reason) = cache_estimate {
        mark_estimated(record, &reason);
    }
    Ok(())
}

/// Fills the cache rates the schedule lacks, for buckets the request used, from the input
/// rate: reads at the input rate, 5-minute writes at 1.25 times it and 1-hour writes at
/// twice it, as Anthropic prices every Claude model. Returns the `estimated_*` reasons.
/// The 1-hour rate is not an estimate, since only Claude usage reports 1-hour writes.
fn fill_cache_rate_gaps(record: &mut UsageLedgerRecord) -> Option<String> {
    let input = record.input_cost_per_million_tokens?;
    let (cache_write_5m_tokens, cache_write_1h_tokens) = cache_write_buckets(record);
    if cache_write_1h_tokens > 0 && record.cache_write_1h_cost_per_million_tokens.is_none() {
        record.cache_write_1h_cost_per_million_tokens = Some(fallback_cache_write_1h_rate(input));
    }
    let mut estimates = Vec::new();
    if record.cache_read_tokens.unwrap_or_default() > 0
        && record.cache_read_cost_per_million_tokens.is_none()
    {
        record.cache_read_cost_per_million_tokens = Some(input);
        estimates.push("estimated_missing_cache_read_rate");
    }
    if cache_write_5m_tokens > 0 && record.cache_write_cost_per_million_tokens.is_none() {
        record.cache_write_cost_per_million_tokens = Some(scaled_rate(input, 5, 4));
        estimates.push("estimated_missing_cache_write_rate");
    }
    (!estimates.is_empty()).then(|| estimates.join(";"))
}

/// Leaves the request unpriced: the billed tier's rates lack `gap`'s bucket, or the tier
/// has no rates at all.
fn leave_unpriced(
    record: &mut UsageLedgerRecord,
    tier: ServiceTier,
    gap: Option<(BilledRates, &'static str)>,
    cache_usage: &CacheUsageNormalization,
) {
    record.pricing_status = UsagePricingStatus::Unpriced;
    let Some((rates, bucket)) = gap else {
        let reason = service_tier_unpriced_reason(tier);
        record.unpriced_reason = Some(match record.unpriced_reason.take() {
            Some(normalization_reason) => format!("{normalization_reason};{reason}"),
            None => reason,
        });
        return;
    };
    rates.record_on(record);
    // Inconsistent cache counters keep their own diagnostic.
    if !matches!(cache_usage, CacheUsageNormalization::Invalid(_)) {
        record.unpriced_reason = Some(rates.schedule.missing_rate_reason(bucket));
    }
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
            cache_write_1h: base_context
                .and_then(|context| context.cache_write_1h)
                .or(pricing.cache_write_1h_cost_per_million_tokens),
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
        cache_write_1h: context
            .and_then(|context| context.cache_write_1h)
            .or(schedule.cache_write_1h),
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
            cache_write_1h: pricing.cache_write_1h_cost_per_million_tokens,
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
        cache_write_1h: rates.cache_write_1h_cost_per_million_tokens,
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

/// `rate` times `numerator / denominator`, rounded half up to Money4.
fn scaled_rate(rate: Money4, numerator: i64, denominator: i64) -> Money4 {
    let scaled = i128::from(rate.as_scaled_i64()) * i128::from(numerator);
    let rounded = (scaled + i128::from(denominator / 2)) / i128::from(denominator);
    Money4::from_scaled(i64::try_from(rounded).unwrap_or(i64::MAX))
}
