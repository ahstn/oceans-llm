use serde_json::{Map, Value, json};

use crate::{
    format::to_pretty_json,
    templates::notes::claude_code_notes,
    types::{ClientConfig, ClientConfigCodeBlock, ClientConfigInput, ClientConfigTemplate},
};

pub(crate) const CLAUDE_CODE_AUTH_TOKEN_PLACEHOLDER: &str = "<gateway api token>";

const CLAUDE_CODE_SETTINGS_SCHEMA: &str = "https://json.schemastore.org/claude-code-settings.json";
const CLAUDE_CODE_LOWER_TOKEN_USAGE_ENV: [(&str, &str); 10] = [
    ("CLAUDE_CODE_ENABLE_TELEMETRY", "0"),
    ("CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS", "1"),
    ("CLAUDE_CODE_DISABLE_1M_CONTEXT", "1"),
    ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "200000"),
    ("ENABLE_TOOL_SEARCH", "auto"),
    ("CLAUDE_CODE_NO_FLICKER", "1"),
    ("CLAUDE_CODE_DISABLE_TERMINAL_TITLE", "1"),
    ("CLAUDE_CODE_ATTRIBUTION_HEADER", "0"),
    ("DISABLE_ERROR_REPORTING", "1"),
    ("DISABLE_TELEMETRY", "1"),
];

#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeCodeConfigTemplate;

impl ClientConfigTemplate for ClaudeCodeConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let config = claude_code_gateway_model_config(input);

        ClientConfig {
            key: "claude-code".to_string(),
            label: "Claude Code".to_string(),
            blocks: vec![
                ClientConfigCodeBlock {
                    label: "Gateway model settings".to_string(),
                    filename: "settings.json".to_string(),
                    content: to_pretty_json(&config),
                },
                ClientConfigCodeBlock {
                    label: "Lower token usage settings".to_string(),
                    filename: "settings.json".to_string(),
                    content: to_pretty_json(&claude_code_minimal_experience_config()),
                },
            ],
            notes: claude_code_notes(input),
        }
    }
}

fn claude_code_gateway_model_config(input: &ClientConfigInput) -> Value {
    let model_override_key = claude_code_model_override_key(input);
    json!({
        "$schema": CLAUDE_CODE_SETTINGS_SCHEMA,
        "env": Value::Object(claude_code_gateway_env(input)),
        "modelOverrides": {
            model_override_key.as_str(): input.model_id.as_str(),
        },
    })
}

fn claude_code_gateway_env(input: &ClientConfigInput) -> Map<String, Value> {
    let mut env = Map::from_iter([
        (
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            json!(CLAUDE_CODE_AUTH_TOKEN_PLACEHOLDER),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            json!(claude_code_gateway_base_url(input)),
        ),
        (
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".to_string(),
            json!("1"),
        ),
        ("ANTHROPIC_MODEL".to_string(), json!(input.model_id)),
        (
            "ANTHROPIC_SMALL_FAST_MODEL".to_string(),
            json!(input.model_id),
        ),
    ]);

    if let Some(env_var) = claude_code_default_model_env_var(input) {
        env.insert(env_var.to_string(), json!(input.model_id));
    }

    env
}

fn claude_code_gateway_base_url(input: &ClientConfigInput) -> String {
    input
        .gateway_base_url
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or_else(|| input.gateway_base_url.trim_end_matches('/'))
        .to_string()
}

fn claude_code_model_override_key(input: &ClientConfigInput) -> String {
    input
        .upstream_model
        .as_deref()
        .and_then(canonical_claude_code_model_id)
        .or_else(|| canonical_claude_code_model_id(&input.model_id))
        .unwrap_or_else(|| input.model_id.clone())
}

fn canonical_claude_code_model_id(value: &str) -> Option<String> {
    let model = value
        .rsplit('/')
        .next()
        .unwrap_or(value)
        .split('@')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
        .replace('.', "-");

    if model.starts_with("claude-") {
        Some(model)
    } else {
        None
    }
}

fn claude_code_default_model_env_var(input: &ClientConfigInput) -> Option<&'static str> {
    let joined = [
        input.model_id.as_str(),
        input.upstream_model.as_deref().unwrap_or_default(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    if joined.contains("opus") {
        Some("ANTHROPIC_DEFAULT_OPUS_MODEL")
    } else if joined.contains("sonnet") {
        Some("ANTHROPIC_DEFAULT_SONNET_MODEL")
    } else if joined.contains("haiku") {
        Some("ANTHROPIC_DEFAULT_HAIKU_MODEL")
    } else {
        None
    }
}

fn claude_code_minimal_experience_config() -> Value {
    json!({
        "$schema": CLAUDE_CODE_SETTINGS_SCHEMA,
        "env": env_from_pairs(&CLAUDE_CODE_LOWER_TOKEN_USAGE_ENV),
    })
}

fn env_from_pairs(pairs: &[(&str, &str)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), json!(value)))
            .collect(),
    )
}
