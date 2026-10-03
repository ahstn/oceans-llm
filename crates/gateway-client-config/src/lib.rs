mod api_style;
mod cost;
mod format;
mod hints;
mod mcp;
mod templates;
mod thinking;
mod types;

pub use api_style::{accepted_api_formats, client_api_style};
pub use hints::{
    ClaudeCodeHints, ClientHints, CodexHints, HarnessHints, OpenCodeHints, PiHints, client_hints,
};
pub use mcp::render_mcp_client_configs;
pub use templates::{
    ClaudeCodeConfigTemplate, CodexConfigTemplate, OpenCodeConfigTemplate, PiConfigTemplate,
    render_default_configs, render_default_configs_for_models,
};
pub use thinking::infer_thinking_policy;
pub use types::{
    ApiFormat, ClientConfig, ClientConfigCodeBlock, ClientConfigInput, ClientConfigInputSet,
    ClientConfigSetupItem, ClientConfigTemplate, ClientModelCapabilities, CodexReasoningEffort,
    DEFAULT_API_KEY_ENV_VAR, DEFAULT_GATEWAY_BASE_URL, DEFAULT_PROVIDER_ID, ReasoningLevel,
    ThinkingPolicy, normalize_gateway_base_url,
};

#[cfg(test)]
mod tests;
