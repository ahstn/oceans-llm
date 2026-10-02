use serde_json::{Value, json};

pub(crate) use crate::types::ApiFormat;
use crate::types::{ClientConfigInput, ThinkingPolicy};

/// Gateway inference APIs that can serve this model. `/v1/messages` is translated onto the
/// chat pipeline, so any chat-capable model accepts Anthropic Messages as well.
#[must_use]
pub fn accepted_api_formats(input: &ClientConfigInput) -> Vec<ApiFormat> {
    let mut formats = Vec::new();
    if input.capabilities.chat_completions {
        formats.push(ApiFormat::OpenAiChatCompletions);
    }
    if input.capabilities.responses {
        formats.push(ApiFormat::OpenAiResponses);
    }
    if input.capabilities.chat_completions {
        formats.push(ApiFormat::AnthropicMessages);
    }
    formats
}

pub(crate) fn uses_anthropic_messages_api(input: &ClientConfigInput) -> bool {
    if !input.capabilities.chat_completions {
        return false;
    }
    // A format derived from the route's provider wins; name matching is only the fallback.
    if let Some(native) = input.native_api_format {
        return native == ApiFormat::AnthropicMessages;
    }
    let joined = [
        input.model_id.as_str(),
        input.upstream_model.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    joined.contains("anthropic") || joined.contains("claude")
}

/// The API format a harness should prefer for this model.
#[must_use]
pub fn client_api_style(input: &ClientConfigInput) -> ApiFormat {
    if input.capabilities.responses && !input.capabilities.chat_completions {
        ApiFormat::OpenAiResponses
    } else if uses_anthropic_messages_api(input) {
        ApiFormat::AnthropicMessages
    } else {
        ApiFormat::OpenAiChatCompletions
    }
}

pub(crate) const fn opencode_provider_package_for_style(style: ApiFormat) -> &'static str {
    match style {
        ApiFormat::OpenAiChatCompletions => "@ai-sdk/openai-compatible",
        ApiFormat::OpenAiResponses => "@ai-sdk/openai",
        ApiFormat::AnthropicMessages => "@ai-sdk/anthropic",
    }
}

pub(crate) const fn pi_provider_api_for_style(style: ApiFormat) -> &'static str {
    match style {
        ApiFormat::OpenAiChatCompletions => "openai-completions",
        ApiFormat::OpenAiResponses => "openai-responses",
        ApiFormat::AnthropicMessages => "anthropic-messages",
    }
}

pub(crate) fn pi_api_key_env_reference(input: &ClientConfigInput) -> String {
    format!("${}", input.api_key_env_var)
}

pub(crate) fn pi_provider_compat(input: &ClientConfigInput) -> Option<Value> {
    if client_api_style(input) == ApiFormat::OpenAiResponses {
        return None;
    }
    if client_api_style(input) == ApiFormat::AnthropicMessages {
        return (input.thinking_policy == Some(ThinkingPolicy::AnthropicSafeEffort))
            .then(|| json!({"forceAdaptiveThinking": true}));
    }

    Some(json!({
        "supportsDeveloperRole": true,
        "supportsReasoningEffort": true,
        "supportsUsageInStreaming": true,
        "maxTokensField": "max_completion_tokens",
    }))
}
