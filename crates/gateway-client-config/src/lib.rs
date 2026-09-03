mod api_style;
mod cost;
mod format;
mod templates;
mod thinking;
mod types;

pub use templates::{
    ClaudeCodeConfigTemplate, CodexConfigTemplate, OpenCodeConfigTemplate, PiConfigTemplate,
    render_default_configs, render_default_configs_for_models,
};
pub use thinking::infer_thinking_policy;
pub use types::{
    ClientConfig, ClientConfigCodeBlock, ClientConfigInput, ClientConfigInputSet,
    ClientConfigSetupItem, ClientConfigTemplate, ClientModelCapabilities, CodexReasoningEffort,
    DEFAULT_API_KEY_ENV_VAR, DEFAULT_GATEWAY_BASE_URL, DEFAULT_PROVIDER_ID, ThinkingPolicy,
};

#[cfg(test)]
mod tests;
