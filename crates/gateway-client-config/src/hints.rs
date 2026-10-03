//! Machine-readable harness hints. These reuse the template helpers so `/v1/models` and the
//! rendered snippets cannot disagree. Endpoints and credentials are deliberately absent:
//! clients already know the base URL they called and hold their own key.
use serde::Serialize;
use serde_json::Value;

use crate::{
    api_style::{
        accepted_api_formats, client_api_style, opencode_provider_package_for_style,
        pi_provider_api_for_style, pi_provider_compat, uses_anthropic_messages_api,
    },
    templates::{
        claude_code::claude_code_default_model_env_var, codex::CODEX_WIRE_API_RESPONSES,
        opencode::opencode_variants, pi::pi_thinking_level_map,
    },
    types::{ApiFormat, ClientConfigInput},
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientHints {
    /// Every gateway API that can serve this model.
    pub api_formats: Vec<ApiFormat>,
    /// The format harnesses should pick when they support several.
    pub preferred_api_format: ApiFormat,
    pub harnesses: HarnessHints,
}

/// Each harness block uses that harness's own config keys so values can be merged verbatim.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HarnessHints {
    pub opencode: OpenCodeHints,
    pub pi: PiHints,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_code: Option<ClaudeCodeHints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexHints>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenCodeHints {
    pub npm: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PiHints {
    pub api: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
    #[serde(rename = "thinkingLevelMap", skip_serializing_if = "Option::is_none")]
    pub thinking_level_map: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaudeCodeHints {
    /// The `ANTHROPIC_DEFAULT_*_MODEL` slot this model fits, when its family is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_env_var: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodexHints {
    pub wire_api: &'static str,
}

/// Returns `None` for models no chat-shaped harness can use (embeddings, decisions).
#[must_use]
pub fn client_hints(input: &ClientConfigInput) -> Option<ClientHints> {
    if !input.capabilities.chat_completions && !input.capabilities.responses {
        return None;
    }
    let style = client_api_style(input);
    Some(ClientHints {
        api_formats: accepted_api_formats(input),
        preferred_api_format: style,
        harnesses: HarnessHints {
            opencode: OpenCodeHints {
                npm: opencode_provider_package_for_style(style),
                variants: opencode_variants(input),
            },
            pi: PiHints {
                api: pi_provider_api_for_style(style),
                compat: pi_provider_compat(input),
                thinking_level_map: pi_thinking_level_map(input),
            },
            claude_code: uses_anthropic_messages_api(input).then(|| ClaudeCodeHints {
                model_env_var: claude_code_default_model_env_var(input),
            }),
            codex: input.capabilities.responses.then_some(CodexHints {
                wire_api: CODEX_WIRE_API_RESPONSES,
            }),
        },
    })
}
