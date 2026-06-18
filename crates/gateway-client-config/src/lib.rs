mod api_style;
mod cost;
mod format;
mod templates;
mod thinking;
mod types;

pub use templates::{
    ClaudeCodeConfigTemplate, CodexConfigTemplate, OpenCodeConfigTemplate, PiConfigTemplate,
    render_default_configs,
};
pub use thinking::infer_anthropic_thinking_policy;
pub use types::{
    AnthropicThinkingPolicy, ClientConfig, ClientConfigCodeBlock, ClientConfigInput,
    ClientConfigTemplate, ClientModelCapabilities, DEFAULT_API_KEY_ENV_VAR,
    DEFAULT_GATEWAY_BASE_URL, DEFAULT_PROVIDER_ID,
};

#[cfg(test)]
mod tests;
