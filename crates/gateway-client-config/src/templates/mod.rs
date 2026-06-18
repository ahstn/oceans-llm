mod claude_code;
mod codex;
mod notes;
mod opencode;
mod pi;

pub use claude_code::ClaudeCodeConfigTemplate;
pub use codex::CodexConfigTemplate;
pub use opencode::OpenCodeConfigTemplate;
pub use pi::PiConfigTemplate;

use crate::types::{ClientConfig, ClientConfigInput, ClientConfigTemplate};

#[must_use]
pub fn render_default_configs(input: &ClientConfigInput) -> Vec<ClientConfig> {
    let mut configs = vec![
        OpenCodeConfigTemplate.render(input),
        PiConfigTemplate.render(input),
        ClaudeCodeConfigTemplate.render(input),
    ];

    if input.capabilities.responses {
        configs.push(CodexConfigTemplate.render(input));
    }

    configs
}
