//! Adapter-side service tier helpers.
//!
//! The gateway hands each adapter the effective tier in
//! [`ProviderRequestContext::service_tier`]. Adapters check it against the tiers their route can
//! send before any upstream call, write their own wire form, and report the tier the upstream
//! served where [`gateway_core::served_service_tier`] reads it.
//!
//! [`route_service_tiers`] is the one table of the tiers a route can send. Adapters check
//! requests against it, routing keeps only the routes it lets serve a tier, and config loading
//! rejects a route that forces a tier it does not list.

use gateway_core::{
    ProviderError, ProviderRequestContext, RouteCompatibility, ServiceTier, ServiceTierApi,
};
use serde_json::Value;

use crate::anthropic::service_tier::{ClaudePlatform, claude_service_tiers};

/// Adapters and routes with no tier selection. An explicit default sends nothing.
pub(crate) const DEFAULT_ONLY: &[ServiceTier] = &[ServiceTier::Default];

/// Standard capacity and priority: Claude fast mode, and Gemini on the `us` and `eu`
/// endpoints, which sell Priority PayGo but not Flex.
pub(crate) const DEFAULT_PRIORITY: &[ServiceTier] = &[ServiceTier::Default, ServiceTier::Priority];

/// Every tier but `ultrafast`: the capacity grades Vertex Gemini and Bedrock sell, and the
/// tiers OpenAI Chat Completions takes.
pub(crate) const DEFAULT_FLEX_PRIORITY: &[ServiceTier] = &[
    ServiceTier::Default,
    ServiceTier::Flex,
    ServiceTier::Priority,
];

/// A provider connection, as far as it decides the tiers its routes can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceTierProvider<'a> {
    /// `openai_compat` or `gcp_cloud_run_openai_compat` at `base_url`.
    OpenAiCompat { base_url: &'a str },
    /// `anthropic_compat`, which serves the Claude API.
    AnthropicCompat,
    /// `gcp_vertex` at `location`.
    GcpVertex { location: &'a str },
    /// `aws_bedrock`.
    AwsBedrock,
    /// `github_copilot` and `typesafe`, which send no tier.
    Untiered,
}

/// The tiers a route to `upstream_model` on `provider` can send on `api`.
#[must_use]
pub fn route_service_tiers(
    provider: ServiceTierProvider<'_>,
    upstream_model: &str,
    compatibility: &RouteCompatibility,
    api: ServiceTierApi,
) -> &'static [ServiceTier] {
    match provider {
        ServiceTierProvider::OpenAiCompat { base_url } => {
            crate::openai_compat::openai_compat_service_tiers(base_url, compatibility, api)
        }
        ServiceTierProvider::AnthropicCompat => {
            claude_service_tiers(upstream_model, ClaudePlatform::ClaudeApi)
        }
        ServiceTierProvider::GcpVertex { location } => {
            crate::vertex::vertex_service_tiers(location, upstream_model)
        }
        ServiceTierProvider::AwsBedrock => crate::bedrock::bedrock_service_tiers(compatibility),
        ServiceTierProvider::Untiered => DEFAULT_ONLY,
    }
}

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
