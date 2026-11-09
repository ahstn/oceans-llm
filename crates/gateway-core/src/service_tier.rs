//! Provider service tiers: the capacity grade a request asks for, and the one that
//! actually served it.
//!
//! Callers ask for a tier through OpenAI's `service_tier`, Anthropic's `speed`, or the
//! camelCase `serviceTier` Gemini and Bedrock use. The gateway removes those fields from
//! the request, decides the effective tier, and hands it to the provider adapter in
//! [`crate::ProviderRequestContext::service_tier`], so each adapter writes its own wire
//! form. Responses report the served tier in the canonical places
//! [`served_service_tier`] reads, which is what usage accounting bills.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::GatewayError;

/// Request fields that select a tier. [`take_requested_service_tier`] removes all of
/// them so no spelling reaches a provider unpriced.
pub const SERVICE_TIER_REQUEST_FIELDS: [&str; 3] = ["service_tier", "serviceTier", "speed"];

/// A provider capacity grade. `Priority` is the tier OpenAI calls Fast, Anthropic serves
/// as `speed: "fast"`, and OpenRouter accepts as `priority` or `fast`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    Default,
    Flex,
    Priority,
    Ultrafast,
}

impl ServiceTier {
    pub const ALL: [Self; 4] = [Self::Default, Self::Flex, Self::Priority, Self::Ultrafast];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Flex => "flex",
            Self::Priority => "priority",
            Self::Ultrafast => "ultrafast",
        }
    }

    /// A configuration spelling: the canonical names plus `fast` and `standard`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" | "standard" => Some(Self::Default),
            "flex" => Some(Self::Flex),
            "priority" | "fast" => Some(Self::Priority),
            "ultrafast" => Some(Self::Ultrafast),
            _ => None,
        }
    }

    /// A tier reported by a provider response. Commitment and batch lanes (`scale`,
    /// `reserved`, `batch`) bill outside per-token tier rates, so they read as `None`
    /// alongside unknown values; Google's and Anthropic's base labels read as default.
    #[must_use]
    pub fn from_served(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "default" | "standard" | "on_demand" => Some(Self::Default),
            "flex" | "on_demand_flex" => Some(Self::Flex),
            "priority" | "fast" | "on_demand_priority" => Some(Self::Priority),
            "ultrafast" => Some(Self::Ultrafast),
            _ => None,
        }
    }

    /// Whether the tier bills above the standard rate.
    #[must_use]
    pub const fn is_premium(self) -> bool {
        matches!(self, Self::Priority | Self::Ultrafast)
    }
}

impl fmt::Display for ServiceTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Removes every tier-selecting field from a request and returns the tier they ask for.
///
/// - `service_tier`: `auto` and `null` ask for nothing; Anthropic's `standard_only` asks
///   for default; otherwise any [`ServiceTier::parse`] spelling.
/// - `serviceTier`: the same values, as a string or Bedrock's `{"type": ...}` object.
/// - `speed`: Anthropic's `fast` (priority) or `standard` (default).
///
/// # Errors
///
/// Returns `InvalidRequest` for an unknown value, a value of the wrong type, or fields
/// that ask for different tiers.
pub fn take_requested_service_tier(
    fields: &mut BTreeMap<String, Value>,
) -> Result<Option<ServiceTier>, GatewayError> {
    let mut requested: Option<(ServiceTier, &'static str)> = None;
    for field in SERVICE_TIER_REQUEST_FIELDS {
        let Some(value) = fields.remove(field) else {
            continue;
        };
        let Some(tier) = requested_field_tier(field, &value)? else {
            continue;
        };
        match requested {
            Some((existing, existing_field)) if existing != tier => {
                return Err(GatewayError::InvalidRequest(format!(
                    "`{existing_field}` asks for service tier `{existing}` but `{field}` asks for `{tier}`"
                )));
            }
            Some(_) => {}
            None => requested = Some((tier, field)),
        }
    }
    Ok(requested.map(|(tier, _)| tier))
}

fn requested_field_tier(field: &str, value: &Value) -> Result<Option<ServiceTier>, GatewayError> {
    let text = match (field, value) {
        (_, Value::Null) => return Ok(None),
        (_, Value::String(text)) => text.as_str(),
        ("serviceTier", Value::Object(object)) => match object.get("type") {
            Some(Value::String(text)) => text.as_str(),
            _ => {
                return Err(GatewayError::InvalidRequest(
                    "`serviceTier.type` must be a string".to_string(),
                ));
            }
        },
        _ => {
            return Err(GatewayError::InvalidRequest(format!(
                "`{field}` must be a string"
            )));
        }
    };
    let tier = match (field, text) {
        ("speed", "fast") => Some(ServiceTier::Priority),
        ("speed", "standard") => Some(ServiceTier::Default),
        ("speed", _) => None,
        (_, "auto") => return Ok(None),
        (_, "standard_only") => Some(ServiceTier::Default),
        (_, text) => ServiceTier::parse(text),
    };
    tier.map(Some).ok_or_else(|| {
        GatewayError::InvalidRequest(format!(
            "unsupported `{field}` value `{text}`; expected one of default, flex, priority (fast), ultrafast"
        ))
    })
}

/// The tier a provider response says served the request, from the canonical places
/// adapters report it: Anthropic `usage.speed` (authoritative for fast mode), then a
/// top-level `service_tier`, then `usage.service_tier`. Responses stream events nest the
/// response under `response`, and Anthropic's `message_start` nests it under `message`.
#[must_use]
pub fn served_service_tier(body: &Value) -> Option<ServiceTier> {
    let body = body
        .get("response")
        .filter(|nested| nested.is_object())
        .or_else(|| body.get("message").filter(|nested| nested.is_object()))
        .unwrap_or(body);
    let usage = body.get("usage");
    if let Some(speed) = usage
        .and_then(|usage| usage.get("speed"))
        .and_then(Value::as_str)
    {
        match speed {
            "fast" => return Some(ServiceTier::Priority),
            "standard" => return Some(ServiceTier::Default),
            _ => {}
        }
    }
    body.get("service_tier")
        .and_then(Value::as_str)
        .and_then(ServiceTier::from_served)
        .or_else(|| {
            usage
                .and_then(|usage| usage.get("service_tier"))
                .and_then(Value::as_str)
                .and_then(ServiceTier::from_served)
        })
}

/// The tier usage is billed at: what the provider reported serving, else what was
/// requested, else default. Billing the requested tier when a provider reports nothing
/// errs toward overcharging rather than silently billing a premium tier at standard.
#[must_use]
pub fn billed_service_tier(
    requested: Option<ServiceTier>,
    served: Option<ServiceTier>,
) -> ServiceTier {
    served.or(requested).unwrap_or(ServiceTier::Default)
}

/// Rejects a requested tier that the model's `service_tiers` allow-list leaves out.
/// `None` allows every tier; `default` is always allowed.
///
/// # Errors
///
/// Returns `ServiceTierNotAllowed` (403) for a tier outside the list.
pub fn enforce_service_tier_policy(
    model_key: &str,
    allowed: Option<&[ServiceTier]>,
    requested: Option<ServiceTier>,
) -> Result<(), GatewayError> {
    let (Some(allowed), Some(tier)) = (allowed, requested) else {
        return Ok(());
    };
    if tier == ServiceTier::Default || allowed.contains(&tier) {
        return Ok(());
    }
    Err(GatewayError::ServiceTierNotAllowed {
        model_key: model_key.to_string(),
        service_tier: tier,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fields(value: Value) -> BTreeMap<String, Value> {
        serde_json::from_value(value).expect("object")
    }

    #[test]
    fn request_spellings_normalize_and_are_removed() {
        let cases = [
            (json!({"service_tier": "fast"}), Some(ServiceTier::Priority)),
            (
                json!({"service_tier": "priority"}),
                Some(ServiceTier::Priority),
            ),
            (
                json!({"service_tier": "standard"}),
                Some(ServiceTier::Default),
            ),
            (
                json!({"service_tier": "standard_only"}),
                Some(ServiceTier::Default),
            ),
            (json!({"service_tier": "auto"}), None),
            (json!({"service_tier": null}), None),
            (json!({"speed": "fast"}), Some(ServiceTier::Priority)),
            (json!({"speed": "standard"}), Some(ServiceTier::Default)),
            (json!({"serviceTier": "flex"}), Some(ServiceTier::Flex)),
            (
                json!({"serviceTier": {"type": "priority"}}),
                Some(ServiceTier::Priority),
            ),
            (
                json!({"service_tier": "ultrafast", "speed": null}),
                Some(ServiceTier::Ultrafast),
            ),
            (
                json!({"service_tier": "auto", "speed": "fast"}),
                Some(ServiceTier::Priority),
            ),
            (
                json!({"service_tier": "priority", "speed": "fast"}),
                Some(ServiceTier::Priority),
            ),
            (json!({"temperature": 0}), None),
        ];
        for (body, want) in cases {
            let mut request = fields(body.clone());
            assert_eq!(
                take_requested_service_tier(&mut request).expect("valid"),
                want,
                "{body}"
            );
            for field in SERVICE_TIER_REQUEST_FIELDS {
                assert!(!request.contains_key(field), "{field} left in {body}");
            }
        }
    }

    #[test]
    fn invalid_and_conflicting_requests_are_rejected() {
        for body in [
            json!({"service_tier": "turbo"}),
            json!({"service_tier": "scale"}),
            json!({"service_tier": 2}),
            json!({"speed": "slow"}),
            json!({"serviceTier": {"type": 1}}),
            json!({"service_tier": "flex", "speed": "fast"}),
            json!({"service_tier": "default", "serviceTier": "priority"}),
        ] {
            let error = take_requested_service_tier(&mut fields(body.clone()))
                .expect_err(&body.to_string());
            assert_eq!(error.http_status_code(), 400, "{body}");
        }
    }

    #[test]
    fn served_tier_reads_canonical_locations() {
        let cases = [
            (
                json!({"service_tier": "priority"}),
                Some(ServiceTier::Priority),
            ),
            (
                json!({"service_tier": "default"}),
                Some(ServiceTier::Default),
            ),
            (json!({"service_tier": "scale"}), None),
            (
                json!({"response": {"service_tier": "flex"}}),
                Some(ServiceTier::Flex),
            ),
            (
                json!({"usage": {"speed": "fast", "service_tier": "standard"}}),
                Some(ServiceTier::Priority),
            ),
            (
                json!({"usage": {"speed": "standard", "service_tier": "priority"}}),
                Some(ServiceTier::Default),
            ),
            (
                json!({"usage": {"service_tier": "priority"}}),
                Some(ServiceTier::Priority),
            ),
            (
                json!({"type": "message_start", "message": {"usage": {"speed": "fast"}}}),
                Some(ServiceTier::Priority),
            ),
            (json!({"usage": {"service_tier": "batch"}}), None),
            (json!({"choices": []}), None),
        ];
        for (body, want) in cases {
            assert_eq!(served_service_tier(&body), want, "{body}");
        }
    }

    #[test]
    fn billing_prefers_served_then_requested_then_default() {
        use ServiceTier::*;
        assert_eq!(billed_service_tier(Some(Priority), Some(Default)), Default);
        assert_eq!(billed_service_tier(Some(Priority), None), Priority);
        assert_eq!(billed_service_tier(None, Some(Flex)), Flex);
        assert_eq!(billed_service_tier(None, None), Default);
    }

    #[test]
    fn policy_allows_default_and_listed_tiers_only() {
        use ServiceTier::*;
        let allowed = [Flex];
        assert!(enforce_service_tier_policy("m", Some(&allowed), Some(Default)).is_ok());
        assert!(enforce_service_tier_policy("m", Some(&allowed), Some(Flex)).is_ok());
        assert!(enforce_service_tier_policy("m", Some(&allowed), None).is_ok());
        assert!(enforce_service_tier_policy("m", None, Some(Ultrafast)).is_ok());
        let error = enforce_service_tier_policy("m", Some(&allowed), Some(Priority))
            .expect_err("priority is not allowed");
        assert_eq!(error.http_status_code(), 403);
        assert_eq!(error.error_code(), "service_tier_not_allowed");
    }
}
