use serde_json::{Value, json};

use crate::types::{AnthropicThinkingPolicy, ClientConfigInput};

pub(crate) fn uses_anthropic_messages_api(input: &ClientConfigInput) -> bool {
    let joined = [
        input.model_id.as_str(),
        input.upstream_model.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    joined.contains("anthropic") || joined.contains("claude")
}

// When multi-model config rendering lands, group models by this client API style first.
// OpenCode and Pi put the API adapter at provider scope, so a mixed Anthropic
// Messages + OpenAI-compatible selection needs one generated provider per style.
pub(crate) fn opencode_provider_package(input: &ClientConfigInput) -> &'static str {
    if uses_anthropic_messages_api(input) {
        "@ai-sdk/anthropic"
    } else {
        "@ai-sdk/openai-compatible"
    }
}

pub(crate) fn pi_provider_api(input: &ClientConfigInput) -> &'static str {
    if uses_anthropic_messages_api(input) {
        "anthropic-messages"
    } else {
        "openai-completions"
    }
}

pub(crate) fn pi_api_key_env_reference(input: &ClientConfigInput) -> String {
    format!("${}", input.api_key_env_var)
}

pub(crate) fn pi_provider_compat(input: &ClientConfigInput) -> Option<Value> {
    if uses_anthropic_messages_api(input) {
        return (input.thinking_policy == Some(AnthropicThinkingPolicy::SafeEffort))
            .then(|| json!({"forceAdaptiveThinking": true}));
    }

    Some(json!({
        "supportsDeveloperRole": true,
        "supportsReasoningEffort": true,
        "supportsUsageInStreaming": true,
        "maxTokensField": "max_completion_tokens",
    }))
}
