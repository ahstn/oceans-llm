use crate::{
    api_style::uses_anthropic_messages_api,
    types::{AnthropicThinkingPolicy, ClientConfigInput},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ClientConfigNoteKind {
    ThinkingPolicy,
    ClaudeCodeAuth,
    ClaudeCodeBaseUrl,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientConfigNote {
    kind: ClientConfigNoteKind,
    message: String,
}

impl ClientConfigNote {
    fn new(kind: ClientConfigNoteKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn kind(&self) -> ClientConfigNoteKind {
        self.kind
    }

    pub(crate) fn into_message(self) -> String {
        self.message
    }
}

pub(crate) fn thinking_notes(input: &ClientConfigInput) -> Vec<String> {
    thinking_note_items(input)
        .into_iter()
        .map(ClientConfigNote::into_message)
        .collect()
}

pub(crate) fn thinking_note_items(input: &ClientConfigInput) -> Vec<ClientConfigNote> {
    match input.thinking_policy {
        Some(AnthropicThinkingPolicy::ManualBudget) => vec![ClientConfigNote::new(
            ClientConfigNoteKind::ThinkingPolicy,
            "This Anthropic model is marked as reasoning-capable, but no thinking variants are generated because it requires caller-supplied manual budget tokens.",
        )],
        _ => Vec::new(),
    }
}

pub(crate) fn claude_code_note_items(input: &ClientConfigInput) -> Vec<ClientConfigNote> {
    let mut notes = thinking_note_items(input);
    notes.push(ClientConfigNote::new(
        ClientConfigNoteKind::ClaudeCodeAuth,
        format!(
            "Replace {} with a gateway API key before using Claude Code settings.",
            super::claude_code::CLAUDE_CODE_AUTH_TOKEN_PLACEHOLDER
        ),
    ));
    if uses_anthropic_messages_api(input) {
        notes.push(ClientConfigNote::new(
            ClientConfigNoteKind::ClaudeCodeBaseUrl,
            format!(
            "ANTHROPIC_BASE_URL is set to the Claude-compatible gateway base URL; Claude Code appends Anthropic endpoints such as /v1/messages and /v1/models. OpenCode and Pi also use Anthropic Messages for this model via {}.",
            input.gateway_base_url
            ),
        ));
    } else {
        notes.push(ClientConfigNote::new(
            ClientConfigNoteKind::ClaudeCodeBaseUrl,
            format!(
            "ANTHROPIC_BASE_URL is set to the Claude-compatible gateway base URL; Claude Code appends Anthropic endpoints such as /v1/messages and /v1/models. Keep the OpenAI-compatible base URL ({}) for OpenCode and Pi.",
            input.gateway_base_url
            ),
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
