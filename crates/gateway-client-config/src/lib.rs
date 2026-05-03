use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const DEFAULT_GATEWAY_BASE_URL: &str = "http://127.0.0.1:3000/v1";
pub const DEFAULT_API_KEY_ENV_VAR: &str = "OCEANS_LLM_API_KEY";
pub const DEFAULT_PROVIDER_ID: &str = "oceans-llm";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicThinkingPolicy {
    SafeEffort,
    ManualBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClientModelCapabilities {
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
    pub filename: String,
    pub content: String,
    pub notes: Vec<String>,
}

pub trait ClientConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenCodeConfigTemplate;

#[derive(Debug, Default, Clone, Copy)]
pub struct PiConfigTemplate;

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
            filename: "opencode.json".to_string(),
            content: to_pretty_json(&config),
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
            filename: "models.json".to_string(),
            content: to_pretty_json(&config),
            notes: thinking_notes(input),
        }
    }
}

#[must_use]
pub fn render_default_configs(input: &ClientConfigInput) -> Vec<ClientConfig> {
    vec![
        OpenCodeConfigTemplate.render(input),
        PiConfigTemplate.render(input),
    ]
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

fn to_pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("client config JSON should serialize")
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{
        AnthropicThinkingPolicy, ClientConfigInput, ClientConfigTemplate, ClientModelCapabilities,
        OpenCodeConfigTemplate, PiConfigTemplate, infer_anthropic_thinking_policy,
    };

    fn input(policy: Option<AnthropicThinkingPolicy>) -> ClientConfigInput {
        ClientConfigInput {
            model_id: "claude-sonnet".to_string(),
            display_name: "Claude Sonnet".to_string(),
            input_cost_per_million_tokens_usd_10000: Some(30_000),
            output_cost_per_million_tokens_usd_10000: Some(150_000),
            cache_read_cost_per_million_tokens_usd_10000: Some(3_000),
            context_window_tokens: Some(200_000),
            output_window_tokens: Some(64_000),
            capabilities: ClientModelCapabilities {
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
        let value: Value = serde_json::from_str(&rendered.content).expect("json");
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
        let value: Value = serde_json::from_str(&rendered.content).expect("json");
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
            serde_json::from_str(&OpenCodeConfigTemplate.render(&input).content).expect("json");
        let pi: Value =
            serde_json::from_str(&PiConfigTemplate.render(&input).content).expect("json");

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
            serde_json::from_str(&OpenCodeConfigTemplate.render(&input).content).expect("json");
        let pi: Value =
            serde_json::from_str(&PiConfigTemplate.render(&input).content).expect("json");

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
        let value: Value = serde_json::from_str(&rendered.content).expect("json");

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
        let value: Value = serde_json::from_str(&rendered.content).expect("json");

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
        let value: Value = serde_json::from_str(&rendered.content).expect("json");

        assert_eq!(policy, Some(AnthropicThinkingPolicy::ManualBudget));
        assert!(
            value["provider"]["oceans-llm"]["models"]["claude-sonnet"]
                .get("variants")
                .is_none()
        );
        assert!(!rendered.notes.is_empty());
    }
}
