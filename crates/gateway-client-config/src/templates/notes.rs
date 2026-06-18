use crate::{
    api_style::uses_anthropic_messages_api,
    types::{AnthropicThinkingPolicy, ClientConfigInput},
};

pub(crate) fn thinking_notes(input: &ClientConfigInput) -> Vec<String> {
    match input.thinking_policy {
        Some(AnthropicThinkingPolicy::ManualBudget) => {
            vec![
                "This Anthropic model is marked as reasoning-capable, but no thinking variants are generated because it requires caller-supplied manual budget tokens.".to_string(),
            ]
        }
        _ => Vec::new(),
    }
}

pub(crate) fn claude_code_notes(input: &ClientConfigInput) -> Vec<String> {
    let mut notes = thinking_notes(input);
    notes.push(format!(
        "Replace {} with a gateway API key before using Claude Code settings.",
        super::claude_code::CLAUDE_CODE_AUTH_TOKEN_PLACEHOLDER
    ));
    if uses_anthropic_messages_api(input) {
        notes.push(format!(
            "ANTHROPIC_BASE_URL is set to the Claude-compatible gateway base URL; Claude Code appends Anthropic endpoints such as /v1/messages and /v1/models. OpenCode and Pi also use Anthropic Messages for this model via {}.",
            input.gateway_base_url
        ));
    } else {
        notes.push(format!(
            "ANTHROPIC_BASE_URL is set to the Claude-compatible gateway base URL; Claude Code appends Anthropic endpoints such as /v1/messages and /v1/models. Keep the OpenAI-compatible base URL ({}) for OpenCode and Pi.",
            input.gateway_base_url
        ));
    }
    notes
}

pub(crate) fn codex_notes(input: &ClientConfigInput) -> Vec<String> {
    let mut notes = Vec::new();
    notes.push(
        "Add this provider configuration to user-level ~/.codex/config.toml; Codex ignores provider and auth keys in project-local .codex/config.toml files."
            .to_string(),
    );
    notes.push(format!(
        "Set {} to a gateway API key before using this Codex config.",
        input.api_key_env_var
    ));
    notes
}
