mod claude_code;
mod codex;
mod notes;
mod opencode;
mod pi;

pub use claude_code::ClaudeCodeConfigTemplate;
pub use codex::CodexConfigTemplate;
pub use opencode::OpenCodeConfigTemplate;
pub use pi::PiConfigTemplate;

use crate::types::{ClientConfig, ClientConfigInput, ClientConfigInputSet, ClientConfigTemplate};

#[must_use]
pub fn render_default_configs(input: &ClientConfigInput) -> Vec<ClientConfig> {
    render_default_configs_for_models(ClientConfigInputSet::new(vec![input.clone()]))
}

#[must_use]
pub fn render_default_configs_for_models(input_set: ClientConfigInputSet) -> Vec<ClientConfig> {
    if input_set.is_empty() {
        return Vec::new();
    }

    let mut configs = vec![
        OpenCodeConfigTemplate.render_many(&input_set),
        PiConfigTemplate.render_many(&input_set),
    ];

    if let Some(config) = ClaudeCodeConfigTemplate.render_many(&input_set) {
        configs.push(config);
    }

    if let Some(input) = codex_input(&input_set) {
        configs.push(CodexConfigTemplate.render(input));
    }

    configs
}

fn codex_input(input_set: &ClientConfigInputSet) -> Option<&ClientConfigInput> {
    input_set
        .first()
        .filter(|input| input_set.models.len() == 1 && input.capabilities.responses)
}
