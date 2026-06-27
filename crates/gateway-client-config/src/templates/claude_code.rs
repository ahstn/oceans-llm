use serde_json::{Map, Value, json};

use crate::{
    api_style::uses_anthropic_messages_api,
    format::to_pretty_json,
    templates::notes::{claude_code_notes, thinking_notes},
    types::{
        ClientConfig, ClientConfigCodeBlock, ClientConfigInput, ClientConfigInputSet,
        ClientConfigTemplate,
    },
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
        self.render_many(&ClientConfigInputSet::new(vec![input.clone()]))
            .expect("single Claude Code rendering requires an Anthropic Messages model")
    }
}

impl ClaudeCodeConfigTemplate {
    #[must_use]
    pub fn render_many(&self, input_set: &ClientConfigInputSet) -> Option<ClientConfig> {
        let inputs = input_set
            .models
            .iter()
            .filter(|input| uses_anthropic_messages_api(input))
            .collect::<Vec<_>>();
        let first = inputs.first()?;
        let config = claude_code_gateway_model_config(&inputs);

        Some(ClientConfig {
            key: "claude-code".to_string(),
            label: "Claude Code".to_string(),
            model_ids: inputs.iter().map(|input| input.model_id.clone()).collect(),
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
            notes: claude_code_notes_for_models(&inputs, first),
        })
    }
}

fn claude_code_gateway_model_config(inputs: &[&ClientConfigInput]) -> Value {
    json!({
        "$schema": CLAUDE_CODE_SETTINGS_SCHEMA,
        "env": Value::Object(claude_code_gateway_env(inputs)),
        "modelOverrides": Value::Object(claude_code_model_overrides(inputs)),
    })
}

fn claude_code_gateway_env(inputs: &[&ClientConfigInput]) -> Map<String, Value> {
    let input = inputs[0];
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

    for input in inputs {
        if let Some(env_var) = claude_code_default_model_env_var(input) {
            env.entry(env_var.to_string())
                .or_insert_with(|| json!(input.model_id));
        }
    }

    env
}

fn claude_code_model_overrides(inputs: &[&ClientConfigInput]) -> Map<String, Value> {
    inputs
        .iter()
        .map(|input| {
            (
                claude_code_model_override_key(input),
                json!(input.model_id.as_str()),
            )
        })
        .collect()
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

fn claude_code_notes_for_models(
    inputs: &[&ClientConfigInput],
    first: &ClientConfigInput,
) -> Vec<String> {
    let mut notes = inputs
        .iter()
        .flat_map(|input| thinking_notes(input))
        .collect::<Vec<_>>();
    notes.extend(
        claude_code_notes(first)
            .into_iter()
            .filter(|note| !note.contains("thinking variants")),
    );
    notes
}

fn env_from_pairs(pairs: &[(&str, &str)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), json!(value)))
            .collect(),
    )
}
