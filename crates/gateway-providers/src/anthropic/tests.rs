use std::collections::BTreeMap;

use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    CoreChatMessage, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderClient,
    ProviderError, ProviderRequestContext,
};
use serde_json::{Value, json};

use super::error::AnthropicAdapterError;
use super::request::{AnthropicRequestOptions, map_anthropic_request};
use super::response::{map_anthropic_finish_reason, normalize_anthropic_response};
use super::streaming::normalize_anthropic_stream;
use super::thinking::{
    ClaudeThinkingPolicy, apply_anthropic_thinking_compatibility, claude_thinking_policy,
};
use crate::anthropic_compat::{
    AnthropicCompatAuth, AnthropicCompatAuthKind, AnthropicCompatConfig, AnthropicCompatProvider,
};

fn test_context(upstream_model: &str) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "req_test_123".to_string(),
        model_key: "claude-fable-5-1".to_string(),
        provider_key: "opencode-zen".to_string(),
        upstream_model: upstream_model.to_string(),
        owner_user_id: None,
        extra_headers: serde_json::Map::new(),
        extra_body: serde_json::Map::new(),
        request_headers: BTreeMap::new(),
        compatibility: Default::default(),
    }
}

#[test]
fn maps_anthropic_stop_reasons_to_openai_finish_reasons() {
    for (stop_reason, finish_reason) in [
        ("end_turn", "stop"),
        ("stop_sequence", "stop"),
        ("max_tokens", "length"),
        ("tool_use", "tool_calls"),
        ("refusal", "content_filter"),
        ("pause_turn", "stop"),
    ] {
        assert_eq!(
            map_anthropic_finish_reason(stop_reason),
            finish_reason,
            "{stop_reason}"
        );
    }
}

#[test]
fn endpoint_url_appends_v1_messages_correctly() {
    for (base, expected) in [
        (
            "https://opencode.ai/zen",
            "https://opencode.ai/zen/v1/messages",
        ),
        (
            "https://opencode.ai/zen/",
            "https://opencode.ai/zen/v1/messages",
        ),
        (
            "https://opencode.ai/zen/v1",
            "https://opencode.ai/zen/v1/messages",
        ),
        (
            "https://opencode.ai/zen/v1/",
            "https://opencode.ai/zen/v1/messages",
        ),
    ] {
        let config = AnthropicCompatConfig::new("opencode-zen".to_string(), base.to_string());
        let provider = AnthropicCompatProvider::new(config).expect("provider");
        assert_eq!(
            provider.messages_endpoint_url().expect("endpoint"),
            expected
        );
    }
}

#[test]
fn provider_advertises_no_responses_or_embeddings() {
    let config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let caps = provider.capabilities();

    assert!(caps.chat_completions);
    assert!(!caps.responses);
    assert!(!caps.embeddings);
    assert!(caps.stream);
    assert!(caps.tools);
    assert!(caps.vision);
    assert!(!caps.json_schema);
    assert!(!caps.developer_role);
}

#[tokio::test]
async fn rejected_operations_return_not_implemented() {
    let config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-fable-5-1");

    let embed_req = CoreEmbeddingsRequest {
        model: "claude-fable-5-1".to_string(),
        input: Value::String("test".to_string()),
        extra: BTreeMap::new(),
    };
    assert!(matches!(
        provider.embeddings(&embed_req, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    let resp_req = CoreResponsesRequest {
        model: "claude-fable-5-1".to_string(),
        input: json!([]),
        stream: false,
        instructions: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: BTreeMap::new(),
    };
    assert!(matches!(
        provider.responses(&resp_req, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    assert!(matches!(
        provider.responses_stream(&resp_req, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
}

#[test]
fn builds_request_with_x_api_key_and_anthropic_version() {
    let mut config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    config.auth = Some(AnthropicCompatAuth {
        kind: AnthropicCompatAuthKind::XApiKey,
        token: "test-secret-key".to_string(),
    });

    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-fable-5-1");
    let body = json!({
        "model": "claude-fable-5-1",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let request = provider.build_request(body, &context).expect("request");
    assert_eq!(
        request
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok()),
        Some("test-secret-key")
    );
    assert_eq!(
        request
            .headers()
            .get("anthropic-version")
            .and_then(|v| v.to_str().ok()),
        Some("2023-06-01")
    );
    assert_eq!(
        request
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("req_test_123")
    );
}

#[test]
fn builds_request_with_bearer_auth_and_custom_version() {
    let mut config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    config.auth = Some(AnthropicCompatAuth {
        kind: AnthropicCompatAuthKind::Bearer,
        token: "bearer-test-key".to_string(),
    });
    config
        .default_headers
        .insert("anthropic-version".to_string(), "2024-01-01".to_string());

    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-fable-5-1");
    let body = json!({
        "model": "claude-fable-5-1",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let request = provider.build_request(body, &context).expect("request");
    assert_eq!(
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok()),
        Some("Bearer bearer-test-key")
    );
    assert_eq!(
        request
            .headers()
            .get("anthropic-version")
            .and_then(|v| v.to_str().ok()),
        Some("2024-01-01")
    );
}

#[test]
fn adaptive_thinking_policy_for_fable_5_1() {
    assert_eq!(
        claude_thinking_policy("claude-fable-5-1"),
        ClaudeThinkingPolicy::AdaptiveOnly
    );
}

#[test]
fn fable_5_1_defaults_to_adaptive_thinking_with_high_effort() {
    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("solve this puzzle".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::new(),
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let body = map_anthropic_request(&request, &context, false, &options).expect("mapped");

    assert_eq!(body["model"], "claude-fable-5-1");
    assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(body["output_config"], json!({ "effort": "high" }));
}

#[test]
fn fable_5_1_maps_custom_reasoning_effort_levels() {
    for effort in ["low", "medium", "high", "xhigh", "max"] {
        let mut extra = BTreeMap::new();
        extra.insert("reasoning_effort".to_string(), json!(effort));

        let request = CoreChatRequest {
            model: "claude-fable-5-1".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("solve this puzzle".to_string()),
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra,
        };

        let context = test_context("claude-fable-5-1");
        let options = AnthropicRequestOptions::default();
        let body = map_anthropic_request(&request, &context, false, &options).expect("mapped");

        assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(body["output_config"], json!({ "effort": effort }));
    }
}

#[test]
fn fable_5_1_rejects_manual_thinking_budget() {
    let mut extra = BTreeMap::new();
    extra.insert(
        "thinking".to_string(),
        json!({ "type": "enabled", "budget_tokens": 10000 }),
    );

    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("solve this".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra,
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let err =
        map_anthropic_request(&request, &context, false, &options).expect_err("should reject");
    assert!(err.to_string().contains("adaptive"));
}

#[test]
fn fable_5_1_rejects_disabled_thinking() {
    let mut extra = BTreeMap::new();
    extra.insert("thinking".to_string(), json!({ "type": "disabled" }));

    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("solve this".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra,
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let err =
        map_anthropic_request(&request, &context, false, &options).expect_err("should reject");
    assert!(
        err.to_string()
            .contains("adaptive thinking is always enabled")
    );
}

#[test]
fn fable_5_1_rejects_forced_tool_choice() {
    let forbidden_choices = [
        json!("required"),
        json!({"type": "any"}),
        json!({"type": "tool", "name": "get_weather"}),
        json!({"type": "function", "function": {"name": "get_weather"}}),
    ];

    for choice in forbidden_choices {
        let mut extra = BTreeMap::new();
        extra.insert("tool_choice".to_string(), choice.clone());
        extra.insert(
            "tools".to_string(),
            json!([{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {"type": "object"}
                }
            }]),
        );

        let request = CoreChatRequest {
            model: "claude-fable-5-1".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("weather in sf".to_string()),
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra,
        };

        let context = test_context("claude-fable-5-1");
        let options = AnthropicRequestOptions::default();
        let err = map_anthropic_request(&request, &context, false, &options)
            .expect_err(&format!("should reject forced choice: {choice}"));
        assert!(
            err.to_string()
                .contains("forced `tool_choice` is not supported")
        );
    }
}

#[test]
fn fable_5_1_allows_auto_and_none_tool_choice() {
    for choice in [json!("auto"), json!("none"), json!({"type": "auto"})] {
        let mut extra = BTreeMap::new();
        extra.insert("tool_choice".to_string(), choice.clone());
        extra.insert(
            "tools".to_string(),
            json!([{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {"type": "object"}
                }
            }]),
        );

        let request = CoreChatRequest {
            model: "claude-fable-5-1".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("weather in sf".to_string()),
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra,
        };

        let context = test_context("claude-fable-5-1");
        let options = AnthropicRequestOptions::default();
        let body = map_anthropic_request(&request, &context, false, &options).expect("allowed");
        assert!(body.get("tools").is_some());
    }
}

#[test]
fn preserves_thinking_blocks_in_assistant_content_across_turns() {
    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![
            CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                name: None,
                extra: BTreeMap::new(),
            },
            CoreChatMessage {
                role: "assistant".to_string(),
                content: json!([
                    {
                        "type": "thinking",
                        "thinking": "Step 1: greeting analyzed.",
                        "signature": "sig_turn_1"
                    },
                    {
                        "type": "text",
                        "text": "Hello there!"
                    }
                ]),
                name: None,
                extra: BTreeMap::new(),
            },
            CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("what was your first thought?".to_string()),
                name: None,
                extra: BTreeMap::new(),
            },
        ],
        stream: false,
        extra: BTreeMap::new(),
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let body = map_anthropic_request(&request, &context, false, &options).expect("mapped");

    let messages = body["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[1]["content"],
        json!([
            {
                "type": "thinking",
                "thinking": "Step 1: greeting analyzed.",
                "signature": "sig_turn_1"
            },
            {
                "type": "text",
                "text": "Hello there!"
            }
        ])
    );
}

#[test]
fn preserves_thinking_blocks_from_provider_metadata_across_turns() {
    let mut assistant_extra = BTreeMap::new();
    assistant_extra.insert(
        "provider_metadata".to_string(),
        json!({
            "anthropic_compat": {
                "reasoning": {
                    "source": "anthropic_messages",
                    "blocks": [
                        {
                            "type": "thinking",
                            "thinking": "Deep thoughts from turn 1.",
                            "signature": "sig_meta_1"
                        }
                    ]
                }
            }
        }),
    );

    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![
            CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                name: None,
                extra: BTreeMap::new(),
            },
            CoreChatMessage {
                role: "assistant".to_string(),
                content: Value::String("Hello!".to_string()),
                name: None,
                extra: assistant_extra,
            },
            CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("follow up".to_string()),
                name: None,
                extra: BTreeMap::new(),
            },
        ],
        stream: false,
        extra: BTreeMap::new(),
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let body = map_anthropic_request(&request, &context, false, &options).expect("mapped");

    let messages = body["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[1]["content"],
        json!([
            {
                "type": "thinking",
                "thinking": "Deep thoughts from turn 1.",
                "signature": "sig_meta_1"
            },
            {
                "type": "text",
                "text": "Hello!"
            }
        ])
    );
}

#[test]
fn response_normalization_includes_usage_cache_and_thinking() {
    let context = test_context("claude-fable-5-1");
    let raw_response = json!({
        "id": "msg_01XyZ",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "thinking",
                "thinking": "Internal reasoning trace.",
                "signature": "sig_test_123"
            },
            {
                "type": "text",
                "text": "The answer is 42."
            }
        ],
        "model": "claude-fable-5-1",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 20,
            "cache_creation_input_tokens": 10
        }
    });

    let normalized = normalize_anthropic_response(
        &raw_response,
        &context,
        "anthropic_compat",
        "anthropic_compat",
    );

    assert_eq!(
        normalized["choices"][0]["message"]["content"],
        "The answer is 42."
    );
    assert_eq!(normalized["choices"][0]["finish_reason"], "stop");

    let reasoning =
        &normalized["choices"][0]["message"]["provider_metadata"]["anthropic_compat"]["reasoning"];
    assert_eq!(reasoning["source"], "anthropic_messages");
    assert_eq!(
        reasoning["blocks"],
        json!([
            {
                "type": "thinking",
                "thinking": "Internal reasoning trace.",
                "signature": "sig_test_123"
            }
        ])
    );

    let usage = &normalized["usage"];
    assert_eq!(usage["prompt_tokens"], 100);
    assert_eq!(usage["completion_tokens"], 50);
    assert_eq!(usage["total_tokens"], 180);
    assert_eq!(usage["provider_usage"]["cache_read_input_tokens"], 20);
    assert_eq!(usage["provider_usage"]["cache_creation_input_tokens"], 10);
    assert_eq!(usage["usage_source"], "anthropic_compat");
}

#[tokio::test]
async fn stream_normalization_converts_sse_events_and_preserves_thinking_and_usage() {
    let sse_chunks = vec![
        Ok(Bytes::from(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream_1\",\"model\":\"claude-fable-5-1\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0,\"cache_read_input_tokens\":5}}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Reasoning delta 1.\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig_stream_xyz\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"world!\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":15}}\n\n",
        )),
        Ok(Bytes::from(
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )),
    ];

    let stream = futures_util::stream::iter(sse_chunks);
    let mut provider_stream = normalize_anthropic_stream(
        stream,
        "chatcmpl-stream-test".to_string(),
        1700000000,
        "claude-fable-5-1".to_string(),
        "anthropic_compat",
        "anthropic_compat",
    );

    let mut chunks = Vec::new();
    while let Some(item) = provider_stream.next().await {
        let bytes = item.expect("valid chunk");
        let text = String::from_utf8(bytes.to_vec()).expect("utf-8");
        chunks.push(text);
    }

    let all_text = chunks.join("");
    assert!(all_text.contains("\"role\":\"assistant\""));
    assert!(all_text.contains("Reasoning delta 1."));
    assert!(all_text.contains("sig_stream_xyz"));
    assert!(all_text.contains("\"anthropic_compat\""));
    assert!(all_text.contains("Hello "));
    assert!(all_text.contains("world!"));
    assert!(all_text.contains("\"finish_reason\":\"stop\""));
    assert!(all_text.contains("data: [DONE]\n\n"));
}

#[tokio::test]
async fn stream_normalization_yields_error_and_halts_on_corrupt_sse_json() {
    let sse_chunks = vec![
        Ok(Bytes::from(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-fable-5-1\",\"role\":\"assistant\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: message_delta\ndata: {corrupt json here\n\n",
        )),
    ];

    let stream = futures_util::stream::iter(sse_chunks);
    let mut provider_stream = normalize_anthropic_stream(
        stream,
        "chatcmpl-corrupt-test".to_string(),
        1700000000,
        "claude-fable-5-1".to_string(),
        "anthropic_compat",
        "anthropic_compat",
    );

    let mut chunks = Vec::new();
    while let Some(item) = provider_stream.next().await {
        let bytes = item.expect("valid chunk");
        let text = String::from_utf8(bytes.to_vec()).expect("utf-8");
        chunks.push(text);
    }

    let all_text = chunks.join("");
    assert!(all_text.contains("anthropic_sse_json_error"));
    assert!(!all_text.contains("data: [DONE]"));
}

#[tokio::test]
async fn stream_normalization_yields_error_on_truncated_stream_without_message_stop() {
    let sse_chunks = vec![
        Ok(Bytes::from(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-fable-5-1\",\"role\":\"assistant\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        )),
        Ok(Bytes::from(
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Incomplete\"}}\n\n",
        )),
    ];

    let stream = futures_util::stream::iter(sse_chunks);
    let mut provider_stream = normalize_anthropic_stream(
        stream,
        "chatcmpl-truncated-test".to_string(),
        1700000000,
        "claude-fable-5-1".to_string(),
        "anthropic_compat",
        "anthropic_compat",
    );

    let mut chunks = Vec::new();
    while let Some(item) = provider_stream.next().await {
        let bytes = item.expect("valid chunk");
        let text = String::from_utf8(bytes.to_vec()).expect("utf-8");
        chunks.push(text);
    }

    let all_text = chunks.join("");
    assert!(all_text.contains("anthropic_stream_truncated"));
    assert!(!all_text.contains("data: [DONE]"));
}

#[test]
fn map_anthropic_request_translates_openai_image_url_data_url() {
    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "Describe this image:"},
                {
                    "type": "image_url",
                    "image_url": {
                        "url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
                    }
                }
            ]),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let body = map_anthropic_request(&request, &context, false, &options).expect("map request");

    let messages = body["messages"].as_array().expect("messages array");
    let content = messages[0]["content"].as_array().expect("content array");
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["type"], "base64");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(
        content[1]["source"]["data"],
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
    );
}

#[test]
fn map_anthropic_request_rejects_remote_image_urls() {
    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {
                    "type": "image_url",
                    "image_url": {
                        "url": "https://example.com/test.png"
                    }
                }
            ]),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };
    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let error =
        map_anthropic_request(&request, &context, false, &options).expect_err("should reject");
    assert!(
        error
            .to_string()
            .contains("remote image URLs are not supported")
    );
}

#[test]
fn map_anthropic_request_normalizes_max_completion_tokens() {
    let mut request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("Hello"),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };
    request
        .extra
        .insert("max_completion_tokens".to_string(), json!(2048));

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let body = map_anthropic_request(&request, &context, false, &options).expect("map request");

    assert_eq!(body["max_tokens"], 2048);
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn map_anthropic_request_rejects_conflicting_max_completion_tokens() {
    let mut request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("Hello"),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };
    request
        .extra
        .insert("max_completion_tokens".to_string(), json!(2048));
    request.extra.insert("max_tokens".to_string(), json!(4096));

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let error =
        map_anthropic_request(&request, &context, false, &options).expect_err("should conflict");
    assert!(error.to_string().contains("conflicts with `max_tokens`"));
}

#[test]
fn fable_adaptive_thinking_rejects_budget_tokens_in_adaptive_object() {
    let mut body = serde_json::Map::from_iter([(
        "thinking".to_string(),
        json!({"type": "adaptive", "budget_tokens": 10000}),
    )]);

    let error = apply_anthropic_thinking_compatibility(&mut body, "claude-fable-5-1")
        .expect_err("should reject manual budget for adaptive only");
    assert!(
        matches!(
            error,
            AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported { .. }
        ),
        "expected AdaptiveOnlyBudgetNotSupported, got {error:?}"
    );
}

#[test]
fn fable_adaptive_thinking_rejects_unsupported_effort_levels() {
    for invalid_effort in ["minimal", "off", "none", "extreme"] {
        let mut body = serde_json::Map::from_iter([(
            "output_config".to_string(),
            json!({"effort": invalid_effort}),
        )]);
        let error = apply_anthropic_thinking_compatibility(&mut body, "claude-fable-5-1")
            .expect_err("should reject invalid effort");
        assert!(
            matches!(
                error,
                AnthropicAdapterError::UnsupportedAdaptiveEffort { .. }
            ),
            "expected UnsupportedAdaptiveEffort for {invalid_effort}, got {error:?}"
        );
    }

    for valid_effort in ["low", "medium", "high", "xhigh", "max"] {
        let mut body = serde_json::Map::from_iter([(
            "output_config".to_string(),
            json!({"effort": valid_effort}),
        )]);
        apply_anthropic_thinking_compatibility(&mut body, "claude-fable-5-1")
            .expect("valid effort should succeed");
        assert_eq!(body["output_config"]["effort"], valid_effort);
    }
}

#[test]
fn anthropic_compat_default_headers_preserve_context_management_beta() {
    let mut request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("Hello"),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };
    request
        .extra
        .insert("context_management".to_string(), json!({"enabled": true}));

    let context = test_context("claude-fable-5-1");
    let default_headers = BTreeMap::from([(
        "anthropic-beta".to_string(),
        "context-management-2025-06-27".to_string(),
    )]);
    let options = AnthropicRequestOptions {
        include_model: true,
        anthropic_version_body: None,
        default_max_tokens: Some(4096),
        default_headers: Some(&default_headers),
    };

    let body = map_anthropic_request(&request, &context, false, &options).expect("map request");
    assert_eq!(body["context_management"], json!({"enabled": true}));
}

#[test]
fn tool_arguments_decoding_to_non_object_rejected() {
    let request = CoreChatRequest {
        model: "claude-fable-5-1".to_string(),
        messages: vec![CoreChatMessage {
            role: "assistant".to_string(),
            content: json!("Calling tool"),
            name: None,
            extra: BTreeMap::from([(
                "tool_calls".to_string(),
                json!([
                    {
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "calc",
                            "arguments": "42"
                        }
                    }
                ]),
            )]),
        }],
        stream: false,
        extra: Default::default(),
    };

    let context = test_context("claude-fable-5-1");
    let options = AnthropicRequestOptions::default();
    let error =
        map_anthropic_request(&request, &context, false, &options).expect_err("should reject");
    assert!(
        error
            .to_string()
            .contains("arguments must decode to a JSON object")
    );
}

#[tokio::test]
#[ignore = "live integration test against OpenCode Zen"]
async fn live_opencode_zen_non_streaming() {
    let api_key = std::env::var("OPENCODE_ZEN_API_KEY")
        .or_else(|_| std::env::var("OPENCODE_API_KEY"))
        .expect("OPENCODE_ZEN_API_KEY or OPENCODE_API_KEY must be set");

    let mut config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    config.auth = Some(AnthropicCompatAuth {
        kind: AnthropicCompatAuthKind::XApiKey,
        token: api_key,
    });

    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-sonnet-5");

    let request = CoreChatRequest {
        model: "claude-sonnet-5".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("Say hello in exactly 3 words."),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };

    let response = provider
        .chat_completions(&request, &context)
        .await
        .expect("non-streaming chat response");

    let text = response["choices"][0]["message"]["content"]
        .as_str()
        .expect("content text");
    assert!(!text.is_empty(), "expected non-empty response text");
    assert!(response["usage"]["total_tokens"].as_i64().unwrap_or(0) > 0);
}

#[tokio::test]
#[ignore = "live integration test against OpenCode Zen"]
async fn live_opencode_zen_streaming() {
    let api_key = std::env::var("OPENCODE_ZEN_API_KEY")
        .or_else(|_| std::env::var("OPENCODE_API_KEY"))
        .expect("OPENCODE_ZEN_API_KEY or OPENCODE_API_KEY must be set");

    let mut config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    config.auth = Some(AnthropicCompatAuth {
        kind: AnthropicCompatAuthKind::XApiKey,
        token: api_key,
    });

    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-sonnet-5");

    let request = CoreChatRequest {
        model: "claude-sonnet-5".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("Count from 1 to 3."),
            name: None,
            extra: Default::default(),
        }],
        stream: true,
        extra: Default::default(),
    };

    let mut stream = provider
        .chat_completions_stream(&request, &context)
        .await
        .expect("streaming response");

    let mut full_text = String::new();
    let mut saw_done = false;
    let mut saw_usage = false;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.expect("valid stream chunk");
        let text = String::from_utf8(bytes.to_vec()).expect("utf-8");
        full_text.push_str(&text);
        if text.contains("data: [DONE]") {
            saw_done = true;
        }
        if text.contains("\"usage\"") {
            saw_usage = true;
        }
    }

    assert!(saw_done, "stream must terminate with [DONE]");
    assert!(saw_usage, "stream must emit usage");
    assert!(!full_text.contains("anthropic_stream_truncated"));
    assert!(!full_text.contains("anthropic_sse_json_error"));
}

#[tokio::test]
#[ignore = "live integration test against OpenCode Zen"]
async fn live_opencode_zen_vision() {
    let api_key = std::env::var("OPENCODE_ZEN_API_KEY")
        .or_else(|_| std::env::var("OPENCODE_API_KEY"))
        .expect("OPENCODE_ZEN_API_KEY or OPENCODE_API_KEY must be set");

    let mut config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    config.auth = Some(AnthropicCompatAuth {
        kind: AnthropicCompatAuthKind::XApiKey,
        token: api_key,
    });

    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-sonnet-5");

    let red_dot = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let request = CoreChatRequest {
        model: "claude-sonnet-5".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "What color is this 1x1 pixel image? Reply with one word."},
                {"type": "image_url", "image_url": {"url": red_dot}}
            ]),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };

    let response = provider
        .chat_completions(&request, &context)
        .await
        .expect("vision chat response");

    let text = response["choices"][0]["message"]["content"]
        .as_str()
        .expect("content text");
    assert!(
        text.to_ascii_lowercase().contains("red") || text.to_ascii_lowercase().contains("pink"),
        "expected color description, got: {text}"
    );
}

#[tokio::test]
#[ignore = "live integration test against OpenCode Zen"]
async fn live_opencode_zen_tools() {
    let api_key = std::env::var("OPENCODE_ZEN_API_KEY")
        .or_else(|_| std::env::var("OPENCODE_API_KEY"))
        .expect("OPENCODE_ZEN_API_KEY or OPENCODE_API_KEY must be set");

    let mut config = AnthropicCompatConfig::new(
        "opencode-zen".to_string(),
        "https://opencode.ai/zen".to_string(),
    );
    config.auth = Some(AnthropicCompatAuth {
        kind: AnthropicCompatAuthKind::XApiKey,
        token: api_key,
    });

    let provider = AnthropicCompatProvider::new(config).expect("provider");
    let context = test_context("claude-sonnet-5");

    let mut request = CoreChatRequest {
        model: "claude-sonnet-5".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("What is the weather in Tokyo right now? Call get_weather tool."),
            name: None,
            extra: Default::default(),
        }],
        stream: false,
        extra: Default::default(),
    };
    request.extra.insert(
        "tools".to_string(),
        json!([
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get current weather for a city",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "City name"
                            }
                        },
                        "required": ["location"]
                    }
                }
            }
        ]),
    );

    let response = provider
        .chat_completions(&request, &context)
        .await
        .expect("tool call response");

    let tool_calls = response["choices"][0]["message"]["tool_calls"]
        .as_array()
        .expect("tool calls array");
    assert!(!tool_calls.is_empty(), "expected tool calls");
    assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
    let args_str = tool_calls[0]["function"]["arguments"]
        .as_str()
        .expect("args str");
    assert!(args_str.to_ascii_lowercase().contains("tokyo"));
}
