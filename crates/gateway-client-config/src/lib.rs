use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const DEFAULT_GATEWAY_BASE_URL: &str = "http://127.0.0.1:3000/v1";
pub const DEFAULT_API_KEY_ENV_VAR: &str = "OCEANS_LLM_API_KEY";
pub const DEFAULT_PROVIDER_ID: &str = "oceans-llm";
const CODEX_WIRE_API_RESPONSES: &str = "responses";
const CLAUDE_CODE_SETTINGS_SCHEMA: &str = "https://json.schemastore.org/claude-code-settings.json";
const CLAUDE_CODE_AUTH_TOKEN_PLACEHOLDER: &str = "<gateway api token>";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicThinkingPolicy {
    SafeEffort,
    ManualBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClientModelCapabilities {
    #[serde(default)]
    pub responses: bool,
    pub tool_calling: bool,
    pub attachments: bool,
    pub vision: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfigInput {
    pub model_id: String,
    pub display_name: String,
    pub upstream_model: Option<String>,
    pub provider_id: String,
    pub provider_name: String,
    pub gateway_base_url: String,
    pub api_key_env_var: String,
    pub input_cost_per_million_tokens_usd_10000: Option<i64>,
    pub output_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_read_cost_per_million_tokens_usd_10000: Option<i64>,
    pub context_window_tokens: Option<i64>,
    pub input_window_tokens: Option<i64>,
    pub output_window_tokens: Option<i64>,
    pub capabilities: ClientModelCapabilities,
    pub thinking_policy: Option<AnthropicThinkingPolicy>,
}

impl ClientConfigInput {
    #[must_use]
    pub fn context_window(&self) -> i64 {
        self.input_window_tokens
            .or(self.context_window_tokens)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn output_window(&self) -> i64 {
        self.output_window_tokens.unwrap_or_default()
    }

    #[must_use]
    pub fn input_modalities(&self) -> Vec<&'static str> {
        let mut modalities = vec!["text"];
        if self.capabilities.vision || self.capabilities.attachments {
            modalities.push("image");
        }
        modalities
    }
}

impl Default for ClientConfigInput {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            display_name: String::new(),
            upstream_model: None,
            provider_id: DEFAULT_PROVIDER_ID.to_string(),
            provider_name: DEFAULT_PROVIDER_ID.to_string(),
            gateway_base_url: DEFAULT_GATEWAY_BASE_URL.to_string(),
            api_key_env_var: DEFAULT_API_KEY_ENV_VAR.to_string(),
            input_cost_per_million_tokens_usd_10000: None,
            output_cost_per_million_tokens_usd_10000: None,
            cache_read_cost_per_million_tokens_usd_10000: None,
            context_window_tokens: None,
            input_window_tokens: None,
            output_window_tokens: None,
            capabilities: ClientModelCapabilities::default(),
            thinking_policy: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfig {
    pub key: String,
    pub label: String,
    pub blocks: Vec<ClientConfigCodeBlock>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfigCodeBlock {
    pub label: String,
    pub filename: String,
    pub content: String,
}

pub trait ClientConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenCodeConfigTemplate;

#[derive(Debug, Default, Clone, Copy)]
pub struct PiConfigTemplate;

#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeCodeConfigTemplate;

#[derive(Debug, Default, Clone, Copy)]
pub struct CodexConfigTemplate;

impl ClientConfigTemplate for OpenCodeConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let mut model = Map::from_iter([
            ("name".to_string(), json!(input.display_name)),
            (
                "reasoning".to_string(),
                json!(input.thinking_policy.is_some()),
            ),
            (
                "tool_call".to_string(),
                json!(input.capabilities.tool_calling),
            ),
            (
                "limit".to_string(),
                json!({
                    "context": input.context_window(),
                    "output": input.output_window(),
                }),
            ),
            ("cost".to_string(), opencode_cost(input)),
        ]);

        if input.capabilities.attachments || input.capabilities.vision {
            model.insert("attachment".to_string(), json!(true));
        }
        if input.thinking_policy == Some(AnthropicThinkingPolicy::SafeEffort) {
            model.insert(
                "variants".to_string(),
                json!({
                    "high": {
                        "reasoningEffort": "high",
                    },
                    "max": {
                        "reasoningEffort": "xhigh",
                    },
                }),
            );
        }

        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            "provider": {
                input.provider_id.as_str(): {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": input.provider_name,
                    "options": {
                        "baseURL": input.gateway_base_url,
                        "apiKey": format!("{{env:{}}}", input.api_key_env_var),
                    },
                    "models": {
                        input.model_id.as_str(): Value::Object(model),
                    },
                },
            },
            "model": format!("{}/{}", input.provider_id, input.model_id),
        });

        ClientConfig {
            key: "opencode".to_string(),
            label: "OpenCode".to_string(),
            blocks: vec![ClientConfigCodeBlock {
                label: "opencode.json".to_string(),
                filename: "opencode.json".to_string(),
                content: to_pretty_json(&config),
            }],
            notes: thinking_notes(input),
        }
    }
}

impl ClientConfigTemplate for PiConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let mut model = Map::from_iter([
            ("id".to_string(), json!(input.model_id)),
            ("name".to_string(), json!(input.display_name)),
            (
                "reasoning".to_string(),
                json!(input.thinking_policy.is_some()),
            ),
            ("input".to_string(), json!(input.input_modalities())),
            ("contextWindow".to_string(), json!(input.context_window())),
            ("maxTokens".to_string(), json!(input.output_window())),
            ("cost".to_string(), pi_cost(input)),
        ]);

        if input.thinking_policy == Some(AnthropicThinkingPolicy::SafeEffort) {
            model.insert(
                "thinkingLevelMap".to_string(),
                json!({
                    "off": null,
                    "minimal": null,
                    "low": "low",
                    "medium": "medium",
                    "high": "high",
                    "xhigh": "xhigh",
                }),
            );
        }

        let config = json!({
            "providers": {
                input.provider_id.as_str(): {
                    "baseUrl": input.gateway_base_url,
                    "api": "openai-completions",
                    "apiKey": input.api_key_env_var,
                    "compat": {
                        "supportsDeveloperRole": true,
                        "supportsReasoningEffort": true,
                        "supportsUsageInStreaming": true,
                        "maxTokensField": "max_completion_tokens",
                    },
                    "models": [Value::Object(model)],
                },
            },
        });

        ClientConfig {
            key: "pi".to_string(),
            label: "Pi".to_string(),
            blocks: vec![ClientConfigCodeBlock {
                label: "models.json".to_string(),
                filename: "models.json".to_string(),
                content: to_pretty_json(&config),
            }],
            notes: thinking_notes(input),
        }
    }
}

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

impl ClientConfigTemplate for CodexConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let mut model_providers = BTreeMap::new();
        model_providers.insert(
            input.provider_id.clone(),
            CodexModelProviderConfig {
                name: input.provider_name.clone(),
                base_url: input.gateway_base_url.clone(),
                env_key: input.api_key_env_var.clone(),
                wire_api: CODEX_WIRE_API_RESPONSES,
            },
        );

        let config = CodexConfigToml {
            model: input.model_id.clone(),
            model_provider: input.provider_id.clone(),
            model_providers,
        };

        ClientConfig {
            key: "codex".to_string(),
            label: "Codex".to_string(),
            blocks: vec![ClientConfigCodeBlock {
                label: "config.toml".to_string(),
                filename: "config.toml".to_string(),
                content: to_pretty_toml(&config),
            }],
            notes: codex_notes(input),
        }
    }
}

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

#[must_use]
pub fn infer_anthropic_thinking_policy(
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<AnthropicThinkingPolicy> {
    let joined = values
        .into_iter()
        .map(|value| value.as_ref().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    if joined.contains("claude-mythos-preview")
        || joined.contains("claude-opus-4-7")
        || joined.contains("claude-opus-4-8")
        || joined.contains("claude-opus-4-9")
        || joined.contains("claude-opus-5")
        || joined.contains("claude-opus-6")
        || joined.contains("claude-sonnet-4-6")
        || joined.contains("claude-opus-4-6")
    {
        return Some(AnthropicThinkingPolicy::SafeEffort);
    }

    if joined.contains("anthropic") || joined.contains("claude") {
        return Some(AnthropicThinkingPolicy::ManualBudget);
    }

    None
}

fn opencode_cost(input: &ClientConfigInput) -> Value {
    let mut cost = Map::from_iter([
        (
            "input".to_string(),
            required_money4_to_number(input.input_cost_per_million_tokens_usd_10000),
        ),
        (
            "output".to_string(),
            required_money4_to_number(input.output_cost_per_million_tokens_usd_10000),
        ),
    ]);
    if let Some(cache_read) = money4_to_number(input.cache_read_cost_per_million_tokens_usd_10000) {
        cost.insert("cache_read".to_string(), cache_read);
    }
    Value::Object(cost)
}

fn pi_cost(input: &ClientConfigInput) -> Value {
    let mut cost = Map::from_iter([
        (
            "input".to_string(),
            required_money4_to_number(input.input_cost_per_million_tokens_usd_10000),
        ),
        (
            "output".to_string(),
            required_money4_to_number(input.output_cost_per_million_tokens_usd_10000),
        ),
    ]);
    if let Some(cache_read) = money4_to_number(input.cache_read_cost_per_million_tokens_usd_10000) {
        cost.insert("cacheRead".to_string(), cache_read);
    }
    Value::Object(cost)
}

fn money4_to_number(value: Option<i64>) -> Option<Value> {
    Some(json!((value? as f64) / 10_000.0))
}

fn required_money4_to_number(value: Option<i64>) -> Value {
    money4_to_number(value).unwrap_or_else(|| json!(0))
}

fn thinking_notes(input: &ClientConfigInput) -> Vec<String> {
    match input.thinking_policy {
        Some(AnthropicThinkingPolicy::ManualBudget) => {
            vec![
                "This Anthropic model is marked as reasoning-capable, but no thinking variants are generated because it requires caller-supplied manual budget tokens.".to_string(),
            ]
        }
        _ => Vec::new(),
    }
}

fn claude_code_notes(input: &ClientConfigInput) -> Vec<String> {
    let mut notes = thinking_notes(input);
    notes.push(format!(
        "Replace {CLAUDE_CODE_AUTH_TOKEN_PLACEHOLDER} with a gateway API key before using Claude Code settings."
    ));
    notes.push(format!(
        "ANTHROPIC_BASE_URL is set to the Claude-compatible gateway base URL; Claude Code appends Anthropic endpoints such as /v1/messages and /v1/models. Keep the OpenAI-compatible base URL ({}) for OpenCode and Pi.",
        input.gateway_base_url
    ));
    notes
}

fn codex_notes(input: &ClientConfigInput) -> Vec<String> {
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

#[derive(Debug, Serialize)]
struct CodexConfigToml {
    model: String,
    model_provider: String,
    model_providers: BTreeMap<String, CodexModelProviderConfig>,
}

#[derive(Debug, Serialize)]
struct CodexModelProviderConfig {
    name: String,
    base_url: String,
    env_key: String,
    wire_api: &'static str,
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

fn to_pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("client config JSON should serialize")
}

fn to_pretty_toml<T: Serialize>(value: &T) -> String {
    toml::to_string_pretty(value).expect("client config TOML should serialize")
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{
        AnthropicThinkingPolicy, ClaudeCodeConfigTemplate, ClientConfigInput, ClientConfigTemplate,
        ClientModelCapabilities, CodexConfigTemplate, OpenCodeConfigTemplate, PiConfigTemplate,
        infer_anthropic_thinking_policy,
    };

    fn input(policy: Option<AnthropicThinkingPolicy>) -> ClientConfigInput {
        ClientConfigInput {
            model_id: "claude-sonnet".to_string(),
            display_name: "Claude Sonnet".to_string(),
            upstream_model: Some("anthropic/claude-sonnet-4-6".to_string()),
            input_cost_per_million_tokens_usd_10000: Some(30_000),
            output_cost_per_million_tokens_usd_10000: Some(150_000),
            cache_read_cost_per_million_tokens_usd_10000: Some(3_000),
            context_window_tokens: Some(200_000),
            output_window_tokens: Some(64_000),
            capabilities: ClientModelCapabilities {
                responses: true,
                tool_calling: true,
                attachments: true,
                vision: true,
            },
            thinking_policy: policy,
            ..ClientConfigInput::default()
        }
    }

    #[test]
    fn opencode_shape_includes_required_cost_and_limits() {
        let rendered =
            OpenCodeConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
        let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
        let model = &value["provider"]["oceans-llm"]["models"]["claude-sonnet"];

        assert_eq!(value["$schema"], "https://opencode.ai/config.json");
        assert_eq!(model["limit"]["context"], 200_000);
        assert_eq!(model["limit"]["output"], 64_000);
        assert_eq!(model["cost"]["input"], 3.0);
        assert_eq!(model["cost"]["output"], 15.0);
        assert_eq!(model["cost"]["cache_read"], 0.3);
    }

    #[test]
    fn pi_shape_includes_provider_model_cost_and_windows() {
        let rendered = PiConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
        let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
        let provider = &value["providers"]["oceans-llm"];
        let model = &provider["models"][0];

        assert_eq!(provider["baseUrl"], "http://127.0.0.1:3000/v1");
        assert_eq!(provider["api"], "openai-completions");
        assert_eq!(model["id"], "claude-sonnet");
        assert_eq!(model["contextWindow"], 200_000);
        assert_eq!(model["maxTokens"], 64_000);
        assert_eq!(model["cost"]["cacheRead"], 0.3);
    }

    #[test]
    fn cache_read_is_omitted_when_missing() {
        let mut input = input(Some(AnthropicThinkingPolicy::SafeEffort));
        input.cache_read_cost_per_million_tokens_usd_10000 = None;

        let opencode: Value =
            serde_json::from_str(&OpenCodeConfigTemplate.render(&input).blocks[0].content)
                .expect("json");
        let pi: Value =
            serde_json::from_str(&PiConfigTemplate.render(&input).blocks[0].content).expect("json");

        assert!(
            opencode["provider"]["oceans-llm"]["models"]["claude-sonnet"]["cost"]
                .get("cache_read")
                .is_none()
        );
        assert!(
            pi["providers"]["oceans-llm"]["models"][0]["cost"]
                .get("cacheRead")
                .is_none()
        );
    }

    #[test]
    fn safe_thinking_variants_are_emitted_for_newer_claude_models() {
        let policy =
            infer_anthropic_thinking_policy(["anthropic/claude-sonnet-4-6", "Claude Sonnet 4.6"]);
        let input = input(policy);
        let opencode: Value =
            serde_json::from_str(&OpenCodeConfigTemplate.render(&input).blocks[0].content)
                .expect("json");
        let pi: Value =
            serde_json::from_str(&PiConfigTemplate.render(&input).blocks[0].content).expect("json");

        assert_eq!(
            opencode["provider"]["oceans-llm"]["models"]["claude-sonnet"]["variants"]["high"]["reasoningEffort"],
            "high"
        );
        assert_eq!(
            pi["providers"]["oceans-llm"]["models"][0]["thinkingLevelMap"]["xhigh"],
            "xhigh"
        );
    }

    #[test]
    fn opencode_safe_effort_config_matches_expected_full_shape() {
        let rendered =
            OpenCodeConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
        let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

        assert_eq!(
            value,
            serde_json::json!({
                "$schema": "https://opencode.ai/config.json",
                "model": "oceans-llm/claude-sonnet",
                "provider": {
                    "oceans-llm": {
                        "models": {
                            "claude-sonnet": {
                                "attachment": true,
                                "cost": {
                                    "cache_read": 0.3,
                                    "input": 3.0,
                                    "output": 15.0
                                },
                                "limit": {
                                    "context": 200000,
                                    "output": 64000
                                },
                                "name": "Claude Sonnet",
                                "reasoning": true,
                                "tool_call": true,
                                "variants": {
                                    "high": {
                                        "reasoningEffort": "high"
                                    },
                                    "max": {
                                        "reasoningEffort": "xhigh"
                                    }
                                }
                            }
                        },
                        "name": "oceans-llm",
                        "npm": "@ai-sdk/openai-compatible",
                        "options": {
                            "apiKey": "{env:OCEANS_LLM_API_KEY}",
                            "baseURL": "http://127.0.0.1:3000/v1"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn pi_safe_effort_config_matches_expected_full_shape() {
        let rendered = PiConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
        let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

        assert_eq!(
            value,
            serde_json::json!({
                "providers": {
                    "oceans-llm": {
                        "api": "openai-completions",
                        "apiKey": "OCEANS_LLM_API_KEY",
                        "baseUrl": "http://127.0.0.1:3000/v1",
                        "compat": {
                            "maxTokensField": "max_completion_tokens",
                            "supportsDeveloperRole": true,
                            "supportsReasoningEffort": true,
                            "supportsUsageInStreaming": true
                        },
                        "models": [
                            {
                                "contextWindow": 200000,
                                "cost": {
                                    "cacheRead": 0.3,
                                    "input": 3.0,
                                    "output": 15.0
                                },
                                "id": "claude-sonnet",
                                "input": ["text", "image"],
                                "maxTokens": 64000,
                                "name": "Claude Sonnet",
                                "reasoning": true,
                                "thinkingLevelMap": {
                                    "high": "high",
                                    "low": "low",
                                    "medium": "medium",
                                    "minimal": null,
                                    "off": null,
                                    "xhigh": "xhigh"
                                }
                            }
                        ]
                    }
                }
            })
        );
    }

    #[test]
    fn manual_budget_models_do_not_emit_variants() {
        let policy = infer_anthropic_thinking_policy(["anthropic/claude-sonnet-4-5@20250929"]);
        let input = input(policy);
        let rendered = OpenCodeConfigTemplate.render(&input);
        let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

        assert_eq!(policy, Some(AnthropicThinkingPolicy::ManualBudget));
        assert!(
            value["provider"]["oceans-llm"]["models"]["claude-sonnet"]
                .get("variants")
                .is_none()
        );
        assert!(!rendered.notes.is_empty());
    }

    #[test]
    fn claude_code_shape_includes_gateway_env_and_model_override() {
        let rendered =
            ClaudeCodeConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
        let gateway_settings: Value =
            serde_json::from_str(&rendered.blocks[0].content).expect("json");
        let lower_usage_settings: Value =
            serde_json::from_str(&rendered.blocks[1].content).expect("json");

        assert_eq!(rendered.key, "claude-code");
        assert_eq!(rendered.blocks.len(), 2);
        assert_eq!(
            gateway_settings["$schema"],
            "https://json.schemastore.org/claude-code-settings.json"
        );
        assert_eq!(
            gateway_settings["env"]["ANTHROPIC_AUTH_TOKEN"],
            "<gateway api token>"
        );
        assert_eq!(
            gateway_settings["env"]["ANTHROPIC_BASE_URL"],
            "http://127.0.0.1:3000"
        );
        assert_eq!(gateway_settings["env"]["ANTHROPIC_MODEL"], "claude-sonnet");
        assert_eq!(
            gateway_settings["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "claude-sonnet"
        );
        assert_eq!(
            gateway_settings["modelOverrides"]["claude-sonnet-4-6"],
            "claude-sonnet"
        );
        assert_eq!(
            lower_usage_settings["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            "200000"
        );
        assert_eq!(lower_usage_settings["env"]["ENABLE_TOOL_SEARCH"], "auto");
        assert!(
            rendered
                .notes
                .iter()
                .any(|note| note.contains("/v1/messages"))
        );
    }

    #[test]
    fn codex_shape_includes_custom_responses_provider() {
        let rendered =
            CodexConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));

        assert_eq!(rendered.key, "codex");
        assert_eq!(rendered.label, "Codex");
        assert_eq!(rendered.blocks.len(), 1);
        assert_eq!(rendered.blocks[0].filename, "config.toml");
        assert!(
            rendered.blocks[0]
                .content
                .contains("model = \"claude-sonnet\"")
        );
        assert!(
            rendered.blocks[0]
                .content
                .contains("model_provider = \"oceans-llm\"")
        );
        assert!(
            rendered.blocks[0]
                .content
                .contains("[model_providers.oceans-llm]")
        );
        assert!(
            rendered.blocks[0]
                .content
                .contains("base_url = \"http://127.0.0.1:3000/v1\"")
        );
        assert!(
            rendered.blocks[0]
                .content
                .contains("env_key = \"OCEANS_LLM_API_KEY\"")
        );
        assert!(
            rendered.blocks[0]
                .content
                .contains("wire_api = \"responses\"")
        );
        assert!(
            rendered
                .notes
                .iter()
                .any(|note| note.contains("~/.codex/config.toml"))
        );
    }

    #[test]
    fn codex_notes_do_not_include_thinking_variant_guidance() {
        let rendered =
            CodexConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::ManualBudget)));

        assert!(
            rendered
                .notes
                .iter()
                .all(|note| !note.contains("thinking variants"))
        );
        assert_eq!(rendered.notes.len(), 2);
    }

    #[test]
    fn default_configs_include_codex_only_for_responses_capable_models() {
        let responses_input = input(Some(AnthropicThinkingPolicy::SafeEffort));
        let response_keys = super::render_default_configs(&responses_input)
            .into_iter()
            .map(|config| config.key)
            .collect::<Vec<_>>();

        assert_eq!(
            response_keys,
            vec!["opencode", "pi", "claude-code", "codex"]
        );

        let mut chat_only_input = responses_input;
        chat_only_input.capabilities.responses = false;
        let chat_only_keys = super::render_default_configs(&chat_only_input)
            .into_iter()
            .map(|config| config.key)
            .collect::<Vec<_>>();

        assert_eq!(chat_only_keys, vec!["opencode", "pi", "claude-code"]);
    }
}
