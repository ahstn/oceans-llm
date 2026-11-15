//! Bedrock service tiers per api style: the tiers a route can request, their wire form, and
//! the tier Bedrock reports serving.

use gateway_core::{
    AwsBedrockApiStyle, ProviderError, ProviderRequestContext, RouteCompatibility, ServiceTier,
};
use serde_json::{Value, json};

use crate::service_tier::{DEFAULT_FLEX_PRIORITY, DEFAULT_ONLY, checked_service_tier};

/// InvokeModel request and response header naming the Bedrock service tier.
pub(super) const SERVICE_TIER_HEADER: &str = "x-amzn-bedrock-service-tier";

/// Claude on Mantle ignores the service tier header, so it serves the default tier only.
const fn api_style_service_tiers(api_style: AwsBedrockApiStyle) -> &'static [ServiceTier] {
    match api_style {
        AwsBedrockApiStyle::RuntimeConverse
        | AwsBedrockApiStyle::RuntimeAnthropicInvoke
        | AwsBedrockApiStyle::RuntimeOpenaiChat
        | AwsBedrockApiStyle::MantleOpenaiChat
        | AwsBedrockApiStyle::MantleOpenaiResponses => DEFAULT_FLEX_PRIORITY,
        AwsBedrockApiStyle::MantleAnthropicMessages => DEFAULT_ONLY,
    }
}

/// The tiers a Bedrock route can send: those of its api style, or the default tier alone for
/// a route that names none.
pub(crate) fn bedrock_service_tiers(compatibility: &RouteCompatibility) -> &'static [ServiceTier] {
    compatibility
        .aws_bedrock
        .as_ref()
        .map_or(DEFAULT_ONLY, |compatibility| {
            api_style_service_tiers(compatibility.api_style)
        })
}

/// Writes the effective tier into the body: Converse's `serviceTier` object, or the
/// OpenAI-shaped `service_tier`. InvokeModel takes the tier as a header instead, so this
/// returns the header value for the transport to set.
pub(super) fn apply_service_tier(
    body: &mut Value,
    context: &ProviderRequestContext,
    api_style: AwsBedrockApiStyle,
) -> Result<Option<&'static str>, ProviderError> {
    let Some(tier) = checked_service_tier(context, api_style_service_tiers(api_style))? else {
        return Ok(None);
    };
    let (field, value) = match api_style {
        AwsBedrockApiStyle::RuntimeAnthropicInvoke => return Ok(Some(tier.as_str())),
        AwsBedrockApiStyle::MantleAnthropicMessages => return Ok(None),
        AwsBedrockApiStyle::RuntimeConverse => ("serviceTier", json!({"type": tier.as_str()})),
        AwsBedrockApiStyle::RuntimeOpenaiChat
        | AwsBedrockApiStyle::MantleOpenaiChat
        | AwsBedrockApiStyle::MantleOpenaiResponses => ("service_tier", json!(tier.as_str())),
    };
    if let Some(object) = body.as_object_mut() {
        object.insert(field.to_string(), value);
    }
    Ok(None)
}

/// The tier in a Converse response's or ConverseStream `metadata` event's
/// `serviceTier.type`. `reserved` capacity reads as none.
pub(super) fn converse_served_tier(value: &Value) -> Option<ServiceTier> {
    value
        .get("serviceTier")?
        .get("type")?
        .as_str()
        .and_then(ServiceTier::from_served)
}

/// The tier an InvokeModel response header reports.
pub(super) fn invoke_served_tier(headers: &reqwest::header::HeaderMap) -> Option<ServiceTier> {
    headers
        .get(SERVICE_TIER_HEADER)?
        .to_str()
        .ok()
        .and_then(ServiceTier::from_served)
}

/// Records an InvokeModel header tier in a native Messages body's `usage.service_tier`,
/// where `gateway_core::served_service_tier` reads it. Bedrock reports the tier only in the
/// header.
pub(super) fn report_native_served_tier(body: &mut Value, tier: Option<ServiceTier>) {
    if let (Some(tier), Some(usage)) = (tier, body.get_mut("usage").and_then(Value::as_object_mut))
    {
        usage.insert("service_tier".to_string(), Value::from(tier.as_str()));
    }
}
