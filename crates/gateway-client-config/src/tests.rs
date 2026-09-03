use serde_json::Value;
use toml::Value as TomlValue;

use crate::{
    ClaudeCodeConfigTemplate, ClientConfig, ClientConfigInput, ClientConfigInputSet,
    ClientConfigTemplate, ClientModelCapabilities, CodexConfigTemplate, CodexReasoningEffort,
    OpenCodeConfigTemplate, PiConfigTemplate, ThinkingPolicy, infer_thinking_policy,
    render_default_configs, render_default_configs_for_models,
};

fn input(policy: Option<ThinkingPolicy>) -> ClientConfigInput {
    ClientConfigInput {
        model_id: "claude-sonnet".to_string(),
        display_name: "Claude Sonnet".to_string(),
        upstream_model: Some("anthropic/claude-sonnet-4-6".to_string()),
        input_cost_per_million_tokens_usd_10000: Some(30_000),
        output_cost_per_million_tokens_usd_10000: Some(150_000),
        cache_read_cost_per_million_tokens_usd_10000: Some(3_000),
        cache_write_cost_per_million_tokens_usd_10000: Some(7_500),
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

fn gemini_input(model_id: &str, upstream_model: &str) -> ClientConfigInput {
    ClientConfigInput {
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        upstream_model: Some(upstream_model.to_string()),
        input_cost_per_million_tokens_usd_10000: Some(5_000),
        output_cost_per_million_tokens_usd_10000: Some(30_000),
        context_window_tokens: Some(1_000_000),
        output_window_tokens: Some(65_536),
        capabilities: ClientModelCapabilities {
            responses: false,
            tool_calling: true,
            attachments: true,
            vision: true,
        },
        thinking_policy: infer_thinking_policy([upstream_model]),
        ..ClientConfigInput::default()
    }
}

fn setup_value<'a>(config: &'a ClientConfig, label: &str) -> &'a str {
    config
        .setup
        .iter()
        .find(|item| item.label == label)
        .map(|item| item.value.as_str())
        .unwrap_or_else(|| panic!("missing setup item {label}"))
}

fn setup_href<'a>(config: &'a ClientConfig, label: &str) -> &'a str {
    config
        .setup
        .iter()
        .find(|item| item.label == label)
        .and_then(|item| item.href.as_deref())
        .unwrap_or_else(|| panic!("missing setup href {label}"))
}

#[test]
fn opencode_shape_includes_required_cost_and_limits() {
    let rendered = OpenCodeConfigTemplate.render(&input(Some(ThinkingPolicy::AnthropicSafeEffort)));
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
    let model = &value["provider"]["oceans-llm"]["models"]["claude-sonnet"];

    assert_eq!(value["$schema"], "https://opencode.ai/config.json");
    assert_eq!(
        setup_value(&rendered, "Configuration"),
        "~/.config/opencode/opencode.json"
    );
    assert!(setup_value(&rendered, "API key").contains("OCEANS_LLM_API_KEY"));
    assert_eq!(
        setup_href(&rendered, "Docs"),
        "https://opencode.ai/docs/config/"
    );
    assert_eq!(model["limit"]["context"], 200_000);
    assert_eq!(model["limit"]["output"], 64_000);
    assert_eq!(model["cost"]["input"], 3.0);
    assert_eq!(model["cost"]["output"], 15.0);
    assert_eq!(model["cost"]["cache_read"], 0.3);
}

#[test]
fn pi_shape_includes_provider_model_cost_and_windows() {
    let rendered = PiConfigTemplate.render(&input(Some(ThinkingPolicy::AnthropicSafeEffort)));
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
    let provider = &value["providers"]["oceans-llm"];
    let model = &provider["models"][0];

    assert_eq!(provider["baseUrl"], "http://127.0.0.1:3000");
    assert!(setup_value(&rendered, "Configuration").contains("~/.pi/agent/models.json"));
    assert!(setup_value(&rendered, "Configuration").contains("~/.pi/agent/settings.json"));
    assert!(setup_value(&rendered, "Configuration").contains(".pi/settings.json"));
    assert_eq!(
        setup_href(&rendered, "Docs"),
        "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md"
    );
    assert!(setup_value(&rendered, "API key").contains("OCEANS_LLM_API_KEY"));
    assert_eq!(provider["api"], "anthropic-messages");
    assert_eq!(provider["apiKey"], "$OCEANS_LLM_API_KEY");
    assert_eq!(provider["compat"]["forceAdaptiveThinking"], true);
    assert_eq!(model["id"], "claude-sonnet");
    assert_eq!(model["contextWindow"], 200_000);
    assert_eq!(model["maxTokens"], 64_000);
    assert_eq!(model["cost"]["cacheRead"], 0.3);
    assert_eq!(model["cost"]["cacheWrite"], 0.75);
}

#[test]
fn pi_cache_costs_default_to_zero_when_missing() {
    let mut input = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    input.cache_read_cost_per_million_tokens_usd_10000 = None;
    input.cache_write_cost_per_million_tokens_usd_10000 = None;

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
    assert_eq!(
        pi["providers"]["oceans-llm"]["models"][0]["cost"]["cacheRead"],
        0
    );
    assert_eq!(
        pi["providers"]["oceans-llm"]["models"][0]["cost"]["cacheWrite"],
        0
    );
}

#[test]
fn client_context_window_is_capped_with_note() {
    let mut input = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    input.context_window_tokens = Some(1_000_000);
    input.input_window_tokens = None;

    let opencode = OpenCodeConfigTemplate.render(&input);
    let opencode_value: Value = serde_json::from_str(&opencode.blocks[0].content).expect("json");
    let pi = PiConfigTemplate.render(&input);
    let pi_value: Value = serde_json::from_str(&pi.blocks[0].content).expect("json");

    assert_eq!(
        opencode_value["provider"]["oceans-llm"]["models"]["claude-sonnet"]["limit"]["context"],
        200_000
    );
    assert_eq!(
        pi_value["providers"]["oceans-llm"]["models"][0]["contextWindow"],
        200_000
    );
    assert!(
        opencode
            .notes
            .iter()
            .any(|note| note.contains("cap the input context window at 200000 tokens"))
    );
}

#[test]
fn client_context_window_note_is_emitted_once_for_multi_model_configs() {
    let mut first = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    first.context_window_tokens = Some(1_000_000);
    let mut second = first.clone();
    second.model_id = "claude-haiku".to_string();
    second.display_name = "Claude Haiku".to_string();
    let input_set = ClientConfigInputSet::new(vec![first, second]);

    let opencode = OpenCodeConfigTemplate.render_many(&input_set);
    let pi = PiConfigTemplate.render_many(&input_set);

    assert_eq!(
        opencode
            .notes
            .iter()
            .filter(|note| note.contains("cap the input context window at 200000 tokens"))
            .count(),
        1
    );
    assert_eq!(
        pi.notes
            .iter()
            .filter(|note| note.contains("cap the input context window at 200000 tokens"))
            .count(),
        1
    );
}

#[test]
fn client_context_window_note_is_emitted_when_later_model_is_capped() {
    let mut first = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    first.context_window_tokens = Some(200_000);
    let mut second = first.clone();
    second.model_id = "claude-haiku".to_string();
    second.display_name = "Claude Haiku".to_string();
    second.context_window_tokens = Some(1_000_000);
    let input_set = ClientConfigInputSet::new(vec![first, second]);

    let opencode = OpenCodeConfigTemplate.render_many(&input_set);
    let pi = PiConfigTemplate.render_many(&input_set);

    assert!(
        opencode
            .notes
            .iter()
            .any(|note| note.contains("cap the input context window at 200000 tokens"))
    );
    assert!(
        pi.notes
            .iter()
            .any(|note| note.contains("cap the input context window at 200000 tokens"))
    );
}

#[test]
fn infers_safe_effort_for_newer_claude_models() {
    for model in [
        "anthropic/claude-fable-5",
        "anthropic/claude-fable-5-1",
        "anthropic/claude-opus-4-8",
        "anthropic/claude-sonnet-4-6",
        "anthropic/claude-sonnet-5",
    ] {
        assert_eq!(
            infer_thinking_policy([model]),
            Some(ThinkingPolicy::AnthropicSafeEffort)
        );
    }
    assert_eq!(
        infer_thinking_policy(["anthropic/claude-sonnet-50"]),
        Some(ThinkingPolicy::AnthropicManualBudget)
    );
    assert_eq!(
        infer_thinking_policy(["anthropic/claude-fable-50"]),
        Some(ThinkingPolicy::AnthropicManualBudget)
    );
}

#[test]
fn safe_thinking_variants_are_emitted_for_newer_claude_models() {
    let input = input(infer_thinking_policy([
        "anthropic/claude-sonnet-4-6",
        "Claude Sonnet 4.6",
    ]));
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
    let rendered = OpenCodeConfigTemplate.render(&input(Some(ThinkingPolicy::AnthropicSafeEffort)));
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
                        "baseURL": "http://127.0.0.1:3000"
                    }
                }
            }
        })
    );
}

#[test]
fn pi_safe_effort_config_matches_expected_full_shape() {
    let rendered = PiConfigTemplate.render(&input(Some(ThinkingPolicy::AnthropicSafeEffort)));
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(
        value,
        serde_json::json!({
            "providers": {
                "oceans-llm": {
                    "api": "anthropic-messages",
                    "apiKey": "$OCEANS_LLM_API_KEY",
                    "baseUrl": "http://127.0.0.1:3000",
                    "compat": {
                        "forceAdaptiveThinking": true
                    },
                    "models": [
                        {
                            "contextWindow": 200000,
                            "cost": {
                                "cacheRead": 0.3,
                                "cacheWrite": 0.75,
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
                                "xhigh": "xhigh",
                                "max": "max"
                            }
                        }
                    ]
                }
            }
        })
    );
}
#[test]
fn pi_fable_5_1_config_matches_expected_shape() {
    let input = ClientConfigInput {
        model_id: "claude-fable-5-1".to_string(),
        display_name: "Claude Fable 5.1".to_string(),
        upstream_model: Some("claude-fable-5-1".to_string()),
        provider_id: "oceans-llm".to_string(),
        provider_name: "Oceans LLM".to_string(),
        gateway_base_url: "https://llm.example.com".to_string(),
        api_key_env_var: "OCEANS_LLM_API_KEY".to_string(),
        input_cost_per_million_tokens_usd_10000: Some(100_000),
        output_cost_per_million_tokens_usd_10000: Some(500_000),
        cache_read_cost_per_million_tokens_usd_10000: Some(2_500),
        cache_write_cost_per_million_tokens_usd_10000: Some(125_000),
        context_window_tokens: Some(1_000_000),
        input_window_tokens: None,
        output_window_tokens: Some(128_000),
        capabilities: ClientModelCapabilities {
            responses: false,
            tool_calling: true,
            attachments: false,
            vision: true,
        },
        thinking_policy: Some(ThinkingPolicy::AnthropicSafeEffort),
        codex_reasoning_effort: None,
    };

    let rendered = PiConfigTemplate.render(&input);
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(
        value,
        serde_json::json!({
            "providers": {
                "oceans-llm": {
                    "api": "anthropic-messages",
                    "apiKey": "$OCEANS_LLM_API_KEY",
                    "baseUrl": "https://llm.example.com",
                    "compat": {
                        "forceAdaptiveThinking": true
                    },
                    "models": [
                        {
                            "contextWindow": 200000,
                            "cost": {
                                "cacheRead": 0.25,
                                "cacheWrite": 12.5,
                                "input": 10.0,
                                "output": 50.0
                            },
                            "id": "claude-fable-5-1",
                            "input": ["text", "image"],
                            "maxTokens": 128000,
                            "name": "Claude Fable 5.1",
                            "reasoning": true,
                            "thinkingLevelMap": {
                                "off": null,
                                "minimal": null,
                                "low": "low",
                                "medium": "medium",
                                "high": "high",
                                "xhigh": "xhigh",
                                "max": "max"
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
    let policy = infer_thinking_policy(["anthropic/claude-sonnet-4-5@20250929"]);
    let input = input(policy);
    let rendered = OpenCodeConfigTemplate.render(&input);
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(policy, Some(ThinkingPolicy::AnthropicManualBudget));
    assert!(
        value["provider"]["oceans-llm"]["models"]["claude-sonnet"]
            .get("variants")
            .is_none()
    );
    assert!(!rendered.notes.is_empty());
}

#[test]
fn infers_thinking_policy_for_gemini_models() {
    const FLASH: Option<ThinkingPolicy> = Some(ThinkingPolicy::GeminiLevel {
        supports_minimal: true,
        supports_medium: true,
    });
    const PRO: Option<ThinkingPolicy> = Some(ThinkingPolicy::GeminiLevel {
        supports_minimal: false,
        supports_medium: false,
    });

    assert_eq!(infer_thinking_policy(["google/gemini-3.7-flash"]), FLASH);
    assert_eq!(infer_thinking_policy(["google/gemini-3.8-flash"]), FLASH);
    assert_eq!(
        infer_thinking_policy(["google/gemini-3.1-flash-lite-preview"]),
        FLASH
    );
    assert_eq!(
        infer_thinking_policy(["google/gemini-3.1-pro-preview"]),
        PRO
    );
    assert_eq!(
        infer_thinking_policy(["google/gemini-2.5-flash"]),
        Some(ThinkingPolicy::GeminiBudget)
    );
    assert_eq!(infer_thinking_policy(["google/gemini-2.0-flash"]), None);
}

#[test]
fn gemini_pro_detection_ignores_provider_labels() {
    assert_eq!(
        infer_thinking_policy([
            "google/gemini-3.8-flash",
            "vertex-prod",
            "gcp_vertex",
            "Production Vertex",
        ]),
        Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: true,
            supports_medium: true,
        })
    );
    // A provider named after Anthropic must not reclassify a Gemini route: the upstream model
    // is consulted first.
    assert_eq!(
        infer_thinking_policy(["google/gemini-3.8-flash", "vertex-anthropic-prod"]),
        Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: true,
            supports_medium: true,
        })
    );
    // Aliases containing `-pro` as part of a longer word are not Pro.
    assert_eq!(
        infer_thinking_policy(["google/gemini-3.8-flash", "gemini-3-production"]),
        Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: true,
            supports_medium: true,
        })
    );
    assert_eq!(
        infer_thinking_policy(["google/gemini-3.1-pro-preview"]),
        Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: false,
            supports_medium: false,
        })
    );
}

#[test]
fn pi_gemini_flash_config_matches_expected_full_shape() {
    let input = gemini_input("gemini-3.8-flash", "google/gemini-3.8-flash");
    let rendered = PiConfigTemplate.render(&input);
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(
        value,
        serde_json::json!({
            "providers": {
                "oceans-llm": {
                    "api": "openai-completions",
                    "apiKey": "$OCEANS_LLM_API_KEY",
                    "baseUrl": "http://127.0.0.1:3000/v1",
                    "compat": {
                        "supportsDeveloperRole": true,
                        "supportsReasoningEffort": true,
                        "supportsUsageInStreaming": true,
                        "maxTokensField": "max_completion_tokens"
                    },
                    "models": [
                        {
                            "contextWindow": 200000,
                            "cost": {
                                "cacheRead": 0,
                                "cacheWrite": 0,
                                "input": 0.5,
                                "output": 3.0
                            },
                            "id": "gemini-3.8-flash",
                            "input": ["text", "image"],
                            "maxTokens": 65536,
                            "name": "gemini-3.8-flash",
                            "reasoning": true,
                            "thinkingLevelMap": {
                                "off": null,
                                "minimal": "minimal",
                                "low": "low",
                                "medium": "medium",
                                "high": "high",
                                "xhigh": "high",
                                "max": "high"
                            }
                        }
                    ]
                }
            }
        })
    );
    assert!(rendered.notes.iter().all(|note| !note.contains("thinking")));
}

#[test]
fn pi_gemini_pro_collapses_unsupported_thinking_levels() {
    let input = gemini_input("gemini-3.1-pro", "google/gemini-3.1-pro-preview");
    let rendered = PiConfigTemplate.render(&input);
    let value: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
    let model = &value["providers"]["oceans-llm"]["models"][0];

    assert_eq!(model["reasoning"], true);
    assert_eq!(
        model["thinkingLevelMap"],
        serde_json::json!({
            "off": null,
            "minimal": "low",
            "low": "low",
            "medium": "high",
            "high": "high",
            "xhigh": "high",
            "max": "high"
        })
    );
}

#[test]
fn pi_gemini_budget_models_map_levels_to_reasoning_effort() {
    let input = gemini_input("gemini-2.5-flash", "google/gemini-2.5-flash");
    let value: Value =
        serde_json::from_str(&PiConfigTemplate.render(&input).blocks[0].content).expect("json");
    let model = &value["providers"]["oceans-llm"]["models"][0];

    assert_eq!(model["reasoning"], true);
    assert_eq!(
        model["thinkingLevelMap"],
        serde_json::json!({
            "off": null,
            "minimal": "low",
            "low": "low",
            "medium": "medium",
            "high": "high",
            "xhigh": "high",
            "max": "high"
        })
    );
}

#[test]
fn opencode_gemini_variants_follow_supported_thinking_levels() {
    let flash = gemini_input("gemini-3.8-flash", "google/gemini-3.8-flash");
    let pro = gemini_input("gemini-3.1-pro", "google/gemini-3.1-pro-preview");
    let budget = gemini_input("gemini-2.5-flash", "google/gemini-2.5-flash");

    let render = |input: &ClientConfigInput| -> Value {
        let value: Value =
            serde_json::from_str(&OpenCodeConfigTemplate.render(input).blocks[0].content)
                .expect("json");
        value["provider"]["oceans-llm"]["models"][input.model_id.as_str()].clone()
    };

    let flash_model = render(&flash);
    assert_eq!(flash_model["reasoning"], true);
    assert_eq!(
        flash_model["variants"],
        serde_json::json!({
            "minimal": {"reasoningEffort": "minimal"},
            "low": {"reasoningEffort": "low"},
            "medium": {"reasoningEffort": "medium"},
            "high": {"reasoningEffort": "high"}
        })
    );

    let pro_model = render(&pro);
    assert_eq!(pro_model["reasoning"], true);
    assert_eq!(
        pro_model["variants"],
        serde_json::json!({
            "low": {"reasoningEffort": "low"},
            "high": {"reasoningEffort": "high"}
        })
    );

    let budget_model = render(&budget);
    assert_eq!(budget_model["reasoning"], true);
    assert_eq!(
        budget_model["variants"],
        serde_json::json!({
            "low": {"reasoningEffort": "low"},
            "medium": {"reasoningEffort": "medium"},
            "high": {"reasoningEffort": "high"}
        })
    );
}

#[test]
fn non_anthropic_models_use_openai_compatible_client_surfaces() {
    let mut input = non_anthropic_input();
    input.display_name = "Claude-compatible Qwen Coder".to_string();
    let opencode: Value =
        serde_json::from_str(&OpenCodeConfigTemplate.render(&input).blocks[0].content)
            .expect("json");
    let pi: Value =
        serde_json::from_str(&PiConfigTemplate.render(&input).blocks[0].content).expect("json");

    assert_eq!(
        opencode["provider"]["oceans-llm"]["npm"],
        "@ai-sdk/openai-compatible"
    );
    assert_eq!(
        opencode["provider"]["oceans-llm"]["options"]["baseURL"],
        "http://127.0.0.1:3000/v1"
    );
    assert_eq!(pi["providers"]["oceans-llm"]["api"], "openai-completions");
    assert_eq!(
        pi["providers"]["oceans-llm"]["baseUrl"],
        "http://127.0.0.1:3000/v1"
    );
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
fn opencode_and_pi_group_mixed_api_styles_into_separate_providers() {
    let rendered = render_default_configs_for_models(ClientConfigInputSet::new(vec![
        input(Some(ThinkingPolicy::AnthropicSafeEffort)),
        non_anthropic_input(),
    ]));

    let opencode_config = rendered
        .iter()
        .find(|config| config.key == "opencode")
        .expect("opencode config");
    let opencode: Value = serde_json::from_str(&opencode_config.blocks[0].content).expect("json");
    assert_eq!(
        opencode["provider"]["oceans-llm-anthropic-messages"]["npm"],
        "@ai-sdk/anthropic"
    );
    assert_eq!(
        opencode["provider"]["oceans-llm-anthropic-messages"]["options"]["baseURL"],
        "http://127.0.0.1:3000"
    );
    assert_eq!(
        opencode["provider"]["oceans-llm-openai-compatible"]["npm"],
        "@ai-sdk/openai-compatible"
    );
    assert_eq!(
        opencode["provider"]["oceans-llm-openai-compatible"]["options"]["baseURL"],
        "http://127.0.0.1:3000/v1"
    );
    assert!(
        opencode["provider"]["oceans-llm-anthropic-messages"]["models"]
            .get("claude-sonnet")
            .is_some()
    );
    assert!(
        opencode["provider"]["oceans-llm-openai-compatible"]["models"]
            .get("qwen-coder")
            .is_some()
    );

    let pi_config = rendered
        .iter()
        .find(|config| config.key == "pi")
        .expect("pi config");
    let pi: Value = serde_json::from_str(&pi_config.blocks[0].content).expect("json");
    assert_eq!(
        pi["providers"]["oceans-llm-anthropic-messages-adaptive-thinking"]["api"],
        "anthropic-messages"
    );
    assert_eq!(
        pi["providers"]["oceans-llm-anthropic-messages-adaptive-thinking"]["baseUrl"],
        "http://127.0.0.1:3000"
    );
    assert_eq!(
        pi["providers"]["oceans-llm-openai-compatible"]["api"],
        "openai-completions"
    );
    assert_eq!(
        pi["providers"]["oceans-llm-openai-compatible"]["baseUrl"],
        "http://127.0.0.1:3000/v1"
    );
    assert_eq!(
        pi["providers"]["oceans-llm-anthropic-messages-adaptive-thinking"]["models"][0]["id"],
        "claude-sonnet"
    );
    assert_eq!(
        pi["providers"]["oceans-llm-openai-compatible"]["models"][0]["id"],
        "qwen-coder"
    );
}

#[test]
fn pi_splits_anthropic_models_by_thinking_compatibility() {
    let safe_effort = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    let mut manual_budget = input(Some(ThinkingPolicy::AnthropicManualBudget));
    manual_budget.model_id = "claude-haiku".to_string();
    manual_budget.upstream_model = Some("anthropic/claude-haiku-3-5".to_string());

    let rendered =
        PiConfigTemplate.render_many(&ClientConfigInputSet::new(vec![safe_effort, manual_budget]));
    let pi: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(
        pi["providers"]["oceans-llm-anthropic-messages-adaptive-thinking"]["compat"]["forceAdaptiveThinking"],
        true
    );
    assert!(
        pi["providers"]["oceans-llm-anthropic-messages"]
            .get("compat")
            .is_none()
    );
    assert_eq!(
        pi["providers"]["oceans-llm-anthropic-messages-adaptive-thinking"]["models"][0]["id"],
        "claude-sonnet"
    );
    assert_eq!(
        pi["providers"]["oceans-llm-anthropic-messages"]["models"][0]["id"],
        "claude-haiku"
    );
}

#[test]
fn claude_code_filters_non_anthropic_models_from_mixed_selection() {
    let rendered = ClaudeCodeConfigTemplate
        .render_many(&ClientConfigInputSet::new(vec![
            input(Some(ThinkingPolicy::AnthropicSafeEffort)),
            non_anthropic_input(),
        ]))
        .expect("claude code config");
    let gateway_settings: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(
        gateway_settings["modelOverrides"]["claude-sonnet-4-6"],
        "claude-sonnet"
    );
    assert!(
        gateway_settings["modelOverrides"]
            .get("qwen/qwen3-coder")
            .is_none()
    );
    assert_eq!(gateway_settings["env"]["ANTHROPIC_MODEL"], "claude-sonnet");
}

#[test]
fn claude_code_deduplicates_duplicate_override_keys() {
    let first = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    let mut alias = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    alias.model_id = "claude-sonnet-alias".to_string();

    let rendered = ClaudeCodeConfigTemplate
        .render_many(&ClientConfigInputSet::new(vec![first, alias]))
        .expect("claude code config");
    let gateway_settings: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(rendered.model_ids, vec!["claude-sonnet"]);
    assert_eq!(
        gateway_settings["modelOverrides"]
            .as_object()
            .expect("model overrides")
            .len(),
        1
    );
    assert_eq!(
        gateway_settings["modelOverrides"]["claude-sonnet-4-6"],
        "claude-sonnet"
    );
}

#[test]
fn claude_code_is_omitted_when_no_anthropic_models_are_selected() {
    let rendered =
        render_default_configs_for_models(ClientConfigInputSet::new(vec![non_anthropic_input()]));
    let keys = rendered
        .iter()
        .map(|config| config.key.as_str())
        .collect::<Vec<_>>();

    assert_eq!(keys, vec!["opencode", "pi"]);
    assert!(
        ClaudeCodeConfigTemplate
            .render_many(&ClientConfigInputSet::new(vec![non_anthropic_input()]))
            .is_none()
    );
}

#[test]
fn claude_code_render_does_not_panic_for_non_anthropic_input() {
    let rendered = ClaudeCodeConfigTemplate.render(&non_anthropic_input());
    let gateway_settings: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(rendered.model_ids, vec!["qwen-coder"]);
    assert_eq!(gateway_settings["env"]["ANTHROPIC_MODEL"], "qwen-coder");
    assert!(
        rendered
            .notes
            .iter()
            .any(|note| note.contains("http://127.0.0.1:3000/v1"))
    );
}

#[test]
fn claude_code_shape_includes_gateway_env_and_model_override() {
    let rendered =
        ClaudeCodeConfigTemplate.render(&input(Some(ThinkingPolicy::AnthropicSafeEffort)));
    let gateway_settings: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");
    let lower_usage_settings: Value =
        serde_json::from_str(&rendered.blocks[1].content).expect("json");

    assert_eq!(rendered.key, "claude-code");
    assert_eq!(rendered.blocks.len(), 2);
    assert!(setup_value(&rendered, "Configuration").contains("~/.claude/settings.json"));
    assert!(setup_value(&rendered, "Configuration").contains(".claude/settings.json"));
    assert_eq!(
        setup_href(&rendered, "Docs"),
        "https://code.claude.com/docs/en/settings"
    );
    assert!(setup_value(&rendered, "API key").contains("<gateway api token>"));
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
    assert_eq!(
        lower_usage_settings["env"]["CLAUDE_CODE_SIMPLE_SYSTEM_PROMPT"],
        "1"
    );
    assert_eq!(lower_usage_settings["env"]["ENABLE_TOOL_SEARCH"], "auto");
    assert!(
        rendered
            .notes
            .iter()
            .any(|note| note.contains("/v1/messages"))
    );
    assert!(
        rendered
            .notes
            .iter()
            .all(|note| !note.contains("<gateway api token>"))
    );
}

#[test]
fn claude_code_sets_default_fable_model_env_var() {
    let mut input = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    input.model_id = "claude-fable".to_string();
    input.display_name = "Claude Fable".to_string();
    input.upstream_model = Some("anthropic/claude-fable-5".to_string());

    let rendered = ClaudeCodeConfigTemplate.render(&input);
    let gateway_settings: Value = serde_json::from_str(&rendered.blocks[0].content).expect("json");

    assert_eq!(
        gateway_settings["env"]["ANTHROPIC_DEFAULT_FABLE_MODEL"],
        "claude-fable"
    );
    assert_eq!(
        gateway_settings["modelOverrides"]["claude-fable-5"],
        "claude-fable"
    );
}

#[test]
fn codex_shape_includes_custom_responses_provider_without_unknown_reasoning_default() {
    let mut input = input(Some(ThinkingPolicy::AnthropicSafeEffort));
    input.provider_id = "oceans".to_string();
    input.provider_name = "OpenAI using LLM proxy".to_string();
    input.gateway_base_url = "https://oceans.example.com/v1".to_string();
    input.api_key_env_var = "OCEANS_API_KEY".to_string();
    let rendered = CodexConfigTemplate.render(&input);

    assert_eq!(rendered.key, "codex");
    assert_eq!(rendered.label, "Codex");
    assert_eq!(rendered.blocks.len(), 1);
    assert_eq!(
        setup_value(&rendered, "Configuration"),
        "~/.codex/config.toml"
    );
    assert!(setup_value(&rendered, "API key").contains("OCEANS_API_KEY"));
    assert_eq!(
        setup_href(&rendered, "Docs"),
        "https://developers.openai.com/codex/config-reference"
    );
    assert_eq!(rendered.blocks[0].filename, "config.toml");

    let content = &rendered.blocks[0].content;
    let toml: TomlValue = content.parse().expect("codex config toml");
    assert_eq!(toml["model"].as_str(), Some("claude-sonnet"));
    assert!(toml.get("model_reasoning_effort").is_none());
    assert_eq!(toml["model_provider"].as_str(), Some("oceans"));

    let provider = &toml["model_providers"]["oceans"];
    assert_eq!(provider["name"].as_str(), Some("OpenAI using LLM proxy"));
    assert_eq!(
        provider["base_url"].as_str(),
        Some("https://oceans.example.com/v1")
    );
    assert_eq!(provider["env_key"].as_str(), Some("OCEANS_API_KEY"));
    assert_eq!(
        provider["env_key_instructions"].as_str(),
        Some("Set OCEANS_API_KEY in your environment")
    );
    assert_eq!(provider["requires_openai_auth"].as_bool(), Some(false));
    assert_eq!(provider["wire_api"].as_str(), Some("responses"));
    assert_eq!(toml["analytics"]["enabled"].as_bool(), Some(false));
    assert_eq!(toml["otel"]["log_user_prompt"].as_bool(), Some(false));

    assert_eq!(content.matches("https://oceans.example.com/v1").count(), 1);
    assert!(!content.contains("]("));
    assert!(
        rendered
            .notes
            .iter()
            .any(|note| note.contains("~/.codex/config.toml"))
    );
}

#[test]
fn codex_emits_supported_explicit_reasoning_default() {
    let mut input = input(None);
    input.codex_reasoning_effort = Some(CodexReasoningEffort::High);

    let rendered = CodexConfigTemplate.render(&input);
    let toml: TomlValue = rendered.blocks[0]
        .content
        .parse()
        .expect("codex config toml");

    assert_eq!(toml["model_reasoning_effort"].as_str(), Some("high"));
}

#[test]
fn codex_xhigh_reasoning_effort_round_trips_with_codex_spelling() {
    let encoded = serde_json::to_value(CodexReasoningEffort::XHigh).expect("serialize effort");
    assert_eq!(encoded, serde_json::json!("xhigh"));

    let decoded: CodexReasoningEffort =
        serde_json::from_value(encoded).expect("deserialize effort");
    assert_eq!(decoded, CodexReasoningEffort::XHigh);
}

#[test]
fn codex_notes_do_not_include_thinking_variant_guidance() {
    let rendered = CodexConfigTemplate.render(&input(Some(ThinkingPolicy::AnthropicManualBudget)));

    assert!(
        rendered
            .notes
            .iter()
            .all(|note| !note.contains("thinking variants"))
    );
    assert_eq!(rendered.notes.len(), 1);
}

#[test]
fn default_configs_include_codex_only_for_responses_capable_models() {
    let responses_input = input(Some(ThinkingPolicy::AnthropicSafeEffort));
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

#[test]
fn multi_model_configs_explain_codex_single_model_requirement() {
    let rendered = render_default_configs_for_models(ClientConfigInputSet::new(vec![
        input(Some(ThinkingPolicy::AnthropicSafeEffort)),
        non_anthropic_input(),
    ]));

    assert!(!rendered.iter().any(|config| config.key == "codex"));
    assert!(rendered.iter().any(|config| {
        config
            .notes
            .iter()
            .any(|note| note.contains("Codex config snippets require a single"))
    }));
}
