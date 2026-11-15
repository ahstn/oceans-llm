//! The tiers an OpenAI-compatible route can send, and their wire form: body `service_tier` on
//! Chat Completions, Responses and OpenRouter Messages alike.

use gateway_core::{
    ProviderError, ProviderRequestContext, RouteCompatibility, ServiceTier, ServiceTierApi,
};
use serde_json::Value;

use super::{OpenAiCompatProvider, routes_to_openrouter};
use crate::service_tier::{DEFAULT_FLEX_PRIORITY, DEFAULT_ONLY, checked_service_tier};

/// OpenRouter takes every tier on every API. OpenAI sells `ultrafast` on Responses only,
/// because its Chat Completions `service_tier` has no such value, and other OpenAI-compatible
/// servers follow OpenAI's contract. Decisions routes send no tier.
pub(crate) fn openai_compat_service_tiers(
    base_url: &str,
    compatibility: &RouteCompatibility,
    api: ServiceTierApi,
) -> &'static [ServiceTier] {
    if compatibility.selects_decisions_api() {
        DEFAULT_ONLY
    } else if api == ServiceTierApi::Responses || routes_to_openrouter(base_url, compatibility) {
        &ServiceTier::ALL
    } else {
        DEFAULT_FLEX_PRIORITY
    }
}

impl OpenAiCompatProvider {
    /// Writes the effective tier as body `service_tier` once `api` can carry it on this route.
    /// An explicit default sends `default`.
    pub(super) fn apply_service_tier(
        &self,
        body: &mut Value,
        context: &ProviderRequestContext,
        api: ServiceTierApi,
    ) -> Result<(), ProviderError> {
        let supported =
            openai_compat_service_tiers(&self.config.base_url, &context.compatibility, api);
        if let (Some(tier), Some(object)) = (
            checked_service_tier(context, supported)?,
            body.as_object_mut(),
        ) {
            object.insert("service_tier".to_string(), Value::from(tier.as_str()));
        }
        Ok(())
    }
}
