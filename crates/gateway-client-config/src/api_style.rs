use serde_json::{Value, json};

use crate::types::{AnthropicThinkingPolicy, ClientConfigInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ClientApiStyle {
    OpenAiCompatible,
    AnthropicMessages,
}

pub(crate) fn uses_anthropic_messages_api(input: &ClientConfigInput) -> bool {
    let joined = [
        input.model_id.as_str(),
        input.upstream_model.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    joined.contains("anthropic") || joined.contains("claude")
}

pub(crate) fn client_api_style(input: &ClientConfigInput) -> ClientApiStyle {
    if uses_anthropic_messages_api(input) {
        ClientApiStyle::AnthropicMessages
    } else {
        ClientApiStyle::OpenAiCompatible
    }
}

pub(crate) const fn opencode_provider_package_for_style(style: ClientApiStyle) -> &'static str {
    match style {
        ClientApiStyle::OpenAiCompatible => "@ai-sdk/openai-compatible",
        ClientApiStyle::AnthropicMessages => "@ai-sdk/anthropic",
    }
}

pub(crate) const fn pi_provider_api_for_style(style: ClientApiStyle) -> &'static str {
    match style {
        ClientApiStyle::OpenAiCompatible => "openai-completions",
        ClientApiStyle::AnthropicMessages => "anthropic-messages",
    }
}

pub(crate) fn pi_api_key_env_reference(input: &ClientConfigInput) -> String {
    format!("${}", input.api_key_env_var)
}

pub(crate) fn pi_provider_compat(input: &ClientConfigInput) -> Option<Value> {
    if client_api_style(input) == ClientApiStyle::AnthropicMessages {
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
