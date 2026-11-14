//! Stored-response fields on Responses requests. OpenRouter's Responses API keeps no state:
//! it rejects `store: true` and a non-null `previous_response_id` with a 400.

use gateway_core::{ProviderError, ProviderRequestContext};
use serde_json::Value;

use super::{OpenAiCompatProvider, routes_to_openrouter};

impl OpenAiCompatProvider {
    /// Removes `store` from a Responses body when the upstream keeps no state (OpenRouter) or
    /// the route's profile sets `supports_store: false`. OpenRouter cannot continue a stored
    /// response, so a `previous_response_id` fails here instead of upstream.
    pub(super) fn apply_responses_store(
        &self,
        body: &mut Value,
        context: &ProviderRequestContext,
    ) -> Result<(), ProviderError> {
        let Some(object) = body.as_object_mut() else {
            return Ok(());
        };
        let stateless = routes_to_openrouter(&self.config.base_url, &context.compatibility);
        if stateless
            && object
                .get("previous_response_id")
                .is_some_and(|id| !id.is_null())
        {
            return Err(ProviderError::UnsupportedParameter {
                param: "previous_response_id",
                message: format!(
                    "provider `{}` routes to OpenRouter, whose Responses API keeps no state and does not accept `previous_response_id`; send the full conversation in `input` instead",
                    context.provider_key
                ),
            });
        }
        let supports_store = context
            .compatibility
            .openai_compat
            .as_ref()
            .is_none_or(|profile| profile.supports_store);
        if stateless || !supports_store {
            object.remove("store");
        }
        Ok(())
    }
}
