//! Adapter-side service tier helpers.
//!
//! The gateway hands each adapter the effective tier in
//! [`ProviderRequestContext::service_tier`]. Adapters check it against the tiers their route can
//! send before any upstream call, write their own wire form, and report the tier the upstream
//! served where [`gateway_core::served_service_tier`] reads it.

use gateway_core::{ProviderError, ProviderRequestContext, ServiceTier};
use serde_json::Value;

/// Adapters and routes with no tier selection. An explicit default sends nothing.
pub(crate) const DEFAULT_ONLY: &[ServiceTier] = &[ServiceTier::Default];

/// Claude standard capacity and fast mode.
pub(crate) const DEFAULT_PRIORITY: &[ServiceTier] = &[ServiceTier::Default, ServiceTier::Priority];

/// The capacity grades Vertex Gemini and Bedrock sell.
pub(crate) const DEFAULT_FLEX_PRIORITY: &[ServiceTier] = &[
    ServiceTier::Default,
    ServiceTier::Flex,
    ServiceTier::Priority,
];

/// Returns the effective tier, or rejects one `supported` does not list.
///
/// Routing only sends a tier to a route whose adapter lists it, so this guards adapters
/// against a context built elsewhere; it never reaches the upstream.
pub(crate) fn checked_service_tier(
    context: &ProviderRequestContext,
    supported: &[ServiceTier],
) -> Result<Option<ServiceTier>, ProviderError> {
    match context.service_tier {
        Some(tier) if !supported.contains(&tier) => Err(ProviderError::InvalidRequest(format!(
            "provider `{}` route does not support service tier `{tier}`",
            context.provider_key
        ))),
        tier => Ok(tier),
    }
}

/// Writes a served tier as the top-level `service_tier` of an OpenAI-shaped response or
/// stream chunk.
pub(crate) fn report_served_tier(target: &mut Value, tier: Option<ServiceTier>) {
    if let (Some(tier), Some(object)) = (tier, target.as_object_mut()) {
        object.insert("service_tier".to_string(), Value::from(tier.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(tier: Option<ServiceTier>) -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-tier".to_string(),
            model_key: "model".to_string(),
            provider_key: "provider-a".to_string(),
            upstream_model: "upstream".to_string(),
            owner_user_id: None,
            expected_provider_credential_id: None,
            chat_protocol: Default::default(),
            extra_headers: Default::default(),
            extra_body: Default::default(),
            request_headers: Default::default(),
            compatibility: Default::default(),
            service_tier: tier,
        }
    }

    #[test]
    fn unsupported_tier_is_rejected_with_the_provider_and_tier() {
        let error = checked_service_tier(&context(Some(ServiceTier::Flex)), DEFAULT_PRIORITY)
            .expect_err("flex is not listed");
        let ProviderError::InvalidRequest(message) = error else {
            panic!("expected InvalidRequest, got {error:?}");
        };
        assert_eq!(
            message,
            "provider `provider-a` route does not support service tier `flex`"
        );
        assert_eq!(
            checked_service_tier(&context(Some(ServiceTier::Default)), DEFAULT_ONLY).unwrap(),
            Some(ServiceTier::Default)
        );
        assert_eq!(
            checked_service_tier(&context(None), DEFAULT_ONLY).unwrap(),
            None
        );
    }
}
