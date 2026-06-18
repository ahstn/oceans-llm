use serde_json::Value;

use crate::{
    AnthropicThinkingPolicy, ClaudeCodeConfigTemplate, ClientConfigInput, ClientConfigTemplate,
    ClientModelCapabilities, CodexConfigTemplate, OpenCodeConfigTemplate, PiConfigTemplate,
    infer_anthropic_thinking_policy, render_default_configs,
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

fn non_anthropic_input() -> ClientConfigInput {
    ClientConfigInput {
        model_id: "qwen-coder".to_string(),
        display_name: "Qwen Coder".to_string(),
        upstream_model: Some("qwen/qwen3-coder".to_string()),
        capabilities: ClientModelCapabilities {
            responses: false,
            tool_calling: true,
            attachments: false,
            vision: false,
        },
        ..ClientConfigInput::default()
    }
}

#[test]
fn opencode_shape_includes_required_cost_and_limits() {
    let rendered = OpenCodeConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
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
    assert_eq!(provider["api"], "anthropic-messages");
    assert_eq!(provider["apiKey"], "$OCEANS_LLM_API_KEY");
    assert_eq!(provider["compat"]["forceAdaptiveThinking"], true);
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
    let rendered = OpenCodeConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
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
                    "npm": "@ai-sdk/anthropic",
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
                    "api": "anthropic-messages",
                    "apiKey": "$OCEANS_LLM_API_KEY",
                    "baseUrl": "http://127.0.0.1:3000/v1",
                    "compat": {
                        "forceAdaptiveThinking": true
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
fn non_anthropic_models_use_openai_compatible_client_surfaces() {
    let input = non_anthropic_input();
    let opencode: Value =
        serde_json::from_str(&OpenCodeConfigTemplate.render(&input).blocks[0].content)
            .expect("json");
    let pi: Value =
        serde_json::from_str(&PiConfigTemplate.render(&input).blocks[0].content).expect("json");

    assert_eq!(
        opencode["provider"]["oceans-llm"]["npm"],
        "@ai-sdk/openai-compatible"
    );
    assert_eq!(pi["providers"]["oceans-llm"]["api"], "openai-completions");
    assert_eq!(
        pi["providers"]["oceans-llm"]["apiKey"],
        "$OCEANS_LLM_API_KEY"
    );
    assert_eq!(
        pi["providers"]["oceans-llm"]["compat"]["maxTokensField"],
        "max_completion_tokens"
    );
}

#[test]
fn claude_code_shape_includes_gateway_env_and_model_override() {
    let rendered =
        ClaudeCodeConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));
    let gateway_settings: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
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
    let rendered = CodexConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::SafeEffort)));

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
    let rendered = CodexConfigTemplate.render(&input(Some(AnthropicThinkingPolicy::ManualBudget)));

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
    let response_keys = render_default_configs(&responses_input)
        .into_iter()
        .map(|config| config.key)
        .collect::<Vec<_>>();

    assert_eq!(
        response_keys,
        vec!["opencode", "pi", "claude-code", "codex"]
    );

    let mut chat_only_input = responses_input;
    chat_only_input.capabilities.responses = false;
    let chat_only_keys = render_default_configs(&chat_only_input)
        .into_iter()
        .map(|config| config.key)
        .collect::<Vec<_>>();

    assert_eq!(chat_only_keys, vec!["opencode", "pi", "claude-code"]);
}
