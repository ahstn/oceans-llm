//! The cost a provider bills for a request, as reported in its usage object
//! (OpenRouter's `usage.cost`).

use std::borrow::Cow;

use gateway_core::{Money4, UsageLedgerRecord, UsagePricingStatus};
use serde_json::Value;

/// Ledger `pricing_source` for costs taken from the provider's usage report.
const PROVIDER_REPORTED_PRICING_SOURCE: &str = "provider_reported";

/// Fractional digits in a Money4 amount.
const MONEY4_PLACES: u32 = 4;
const _: () = assert!(Money4::SCALE == 10_i64.pow(MONEY4_PLACES));

/// Most fractional digits a reported amount may carry; `10^38` fits in `i128`.
const MAX_FRACTION_DIGITS: u32 = 38;

/// The provider-billed cost of a request, rounded half up to Money4 once.
///
/// `cost` is what OpenRouter charges. On bring-your-own-key requests
/// (`is_byok: true`) that is only OpenRouter's fee, and the upstream provider
/// bills `cost_details.upstream_inference_cost` to the caller's own key, so the
/// request cost is their sum. Returns `Ok(None)` when no cost is reported, and
/// `Err` when a reported cost is malformed or a BYOK report lacks its upstream
/// part; callers then price the request from token rates.
pub(super) fn provider_reported_cost(usage: &Value) -> Result<Option<Money4>, String> {
    let Some(cost) = usage.get("cost").filter(|cost| !cost.is_null()) else {
        return Ok(None);
    };
    let mut total = ReportedAmount::parse(cost, "cost")?;
    if usage.get("is_byok").and_then(Value::as_bool) == Some(true) {
        let upstream = usage
            .pointer("/cost_details/upstream_inference_cost")
            .filter(|cost| !cost.is_null())
            .ok_or("BYOK usage omits `cost_details.upstream_inference_cost`")?;
        let upstream = ReportedAmount::parse(upstream, "cost_details.upstream_inference_cost")?;
        total = total
            .checked_add(upstream)
            .ok_or("reported cost overflowed")?;
    }
    total
        .round_half_up_to_money4()
        .map(Some)
        .ok_or_else(|| "reported cost overflowed".to_string())
}

/// Charges a provider-reported cost. It is the provider's own bill, so it wins
/// over the route override and the catalog, and the event keeps no token rates.
pub(super) fn apply_provider_reported_cost(record: &mut UsageLedgerRecord, cost_usd: Money4) {
    record.pricing_status = UsagePricingStatus::Priced;
    record.pricing_source = Some(PROVIDER_REPORTED_PRICING_SOURCE.to_string());
    record.computed_cost_usd = cost_usd;
}

/// An exact non-negative decimal amount, `units / 10^scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReportedAmount {
    units: i128,
    scale: u32,
}

impl ReportedAmount {
    fn parse(value: &Value, field: &str) -> Result<Self, String> {
        let text = match value {
            // serde_json prints a float as its shortest round-trip decimal. That
            // is the amount the provider wrote when it has at most 15 significant
            // digits (OpenRouter costs have far fewer); summing the decimals
            // avoids the error `f64` addition would add on top.
            Value::Number(number) => Cow::Owned(number.to_string()),
            Value::String(text) => Cow::Borrowed(text.trim()),
            _ => return Err(format!("`{field}` must be a number or a decimal string")),
        };
        let amount = Self::parse_decimal(&text)
            .ok_or_else(|| format!("`{field}` is not a decimal amount: `{text}`"))?;
        if amount.units < 0 {
            return Err(format!("`{field}` cannot be negative: `{text}`"));
        }
        Ok(amount)
    }

    /// Parses `[-]digits[.digits][(e|E)[+|-]digits]` exactly.
    fn parse_decimal(text: &str) -> Option<Self> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(unsigned) => (true, unsigned),
            None => (false, text),
        };
        let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
            None => (unsigned, 0),
        };
        let (integer, fraction) = match mantissa.split_once('.') {
            Some((_, "")) => return None,
            Some((integer, fraction)) => (integer, fraction),
            None => (mantissa, ""),
        };
        let is_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if integer.is_empty() || !is_digits(integer) || !is_digits(fraction) {
            return None;
        }
        let fraction = fraction.trim_end_matches('0');
        let mut units = 0_i128;
        for digit in integer.bytes().chain(fraction.bytes()) {
            units = units
                .checked_mul(10)?
                .checked_add(i128::from(digit - b'0'))?;
        }
        let scale = i64::try_from(fraction.len()).ok()? - i64::from(exponent);
        let (units, scale) = match u32::try_from(scale) {
            Ok(scale) => (units, scale),
            Err(_) => {
                let shift = u32::try_from(-scale).ok()?;
                (units.checked_mul(10_i128.checked_pow(shift)?)?, 0)
            }
        };
        if scale > MAX_FRACTION_DIGITS {
            return None;
        }
        Some(Self {
            units: if negative { -units } else { units },
            scale,
        })
    }

    fn checked_add(self, other: Self) -> Option<Self> {
        let scale = self.scale.max(other.scale);
        let rescale = |amount: Self| {
            amount
                .units
                .checked_mul(10_i128.checked_pow(scale - amount.scale)?)
        };
        Some(Self {
            units: rescale(self)?.checked_add(rescale(other)?)?,
            scale,
        })
    }

    /// Rounds half up to Money4. Only non-negative amounts get here.
    fn round_half_up_to_money4(self) -> Option<Money4> {
        let scaled = match self.scale.checked_sub(MONEY4_PLACES) {
            None | Some(0) => self
                .units
                .checked_mul(10_i128.checked_pow(MONEY4_PLACES - self.scale)?)?,
            Some(excess_places) => {
                let divisor = 10_i128.checked_pow(excess_places)?;
                let remainder = self.units % divisor;
                self.units / divisor + i128::from(remainder >= divisor - remainder)
            }
        };
        i64::try_from(scaled).ok().map(Money4::from_scaled)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn reported(usage: Value) -> Result<Option<Money4>, String> {
        provider_reported_cost(&usage)
    }

    #[test]
    fn usage_without_reported_cost_has_none() {
        assert_eq!(reported(json!({"prompt_tokens": 14})), Ok(None));
        assert_eq!(reported(json!({"cost": null})), Ok(None));
        assert_eq!(reported(json!({})), Ok(None));
    }

    #[test]
    fn byok_usage_cost_adds_the_upstream_inference_cost() {
        // OpenRouter's live BYOK image response: no OpenRouter fee and a
        // $0.00694 upstream bill.
        let usage = json!({
            "prompt_tokens": 14,
            "completion_tokens": 229,
            "total_tokens": 243,
            "cost": 0,
            "is_byok": true,
            "cost_details": {
                "upstream_inference_cost": 0.00694,
                "upstream_inference_prompt_cost": 0.00007,
                "upstream_inference_completions_cost": 0.00687
            }
        });
        assert_eq!(reported(usage), Ok(Some(Money4::from_scaled(69))));

        // The fee and the upstream bill are summed before the single rounding.
        let usage = json!({
            "cost": 0.00003,
            "is_byok": true,
            "cost_details": {"upstream_inference_cost": "0.00002"}
        });
        assert_eq!(reported(usage), Ok(Some(Money4::from_scaled(1))));
    }

    #[test]
    fn non_byok_usage_cost_ignores_the_upstream_inference_cost() {
        let usage = json!({
            "cost": 0.0123,
            "is_byok": false,
            "cost_details": {"upstream_inference_cost": 0.0123}
        });
        assert_eq!(reported(usage), Ok(Some(Money4::from_scaled(123))));

        let usage = json!({"cost": "0.0123", "cost_details": {"upstream_inference_cost": 1}});
        assert_eq!(reported(usage), Ok(Some(Money4::from_scaled(123))));
    }

    #[test]
    fn reported_usage_cost_rounds_the_exact_decimal_half_up() {
        for (cost, expected_scaled) in [
            // `f64` arithmetic gets 0.00015 * 10_000 = 1.4999999999999998.
            (json!(0.00015), 2),
            (json!("0.00005"), 1),
            (json!("0.000049999"), 0),
            (json!("0.12345"), 1_235),
            (json!("1.5e-4"), 2),
            (json!("2.5E+0"), 25_000),
            (json!(1e-7), 0),
            (json!(12), 120_000),
            (json!(" 0.5000000 "), 5_000),
        ] {
            assert_eq!(
                reported(json!({"cost": cost.clone()})),
                Ok(Some(Money4::from_scaled(expected_scaled))),
                "cost {cost}"
            );
        }
    }

    #[test]
    fn malformed_reported_usage_cost_is_rejected() {
        for usage in [
            json!({"cost": -0.01}),
            json!({"cost": "-1"}),
            json!({"cost": "abc"}),
            json!({"cost": true}),
            json!({"cost": "1."}),
            json!({"cost": ".5"}),
            json!({"cost": "1e"}),
            json!({"cost": "1e999"}),
            json!({"cost": "1e-39"}),
            json!({"cost": 0, "is_byok": true}),
            json!({"cost": 0, "is_byok": true, "cost_details": {"upstream_inference_cost": null}}),
            json!({"cost": 0, "is_byok": true, "cost_details": {"upstream_inference_cost": "-1"}}),
        ] {
            assert!(
                reported(usage.clone()).is_err(),
                "{usage} should be rejected"
            );
        }
    }
}
