use super::*;

#[tokio::test]
async fn normalizes_mantle_anthropic_messages_sse() {
    let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![
        Ok(Bytes::from_static(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\"}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}\n\n",
        )),
    ];
    let mut stream = normalize_anthropic_messages_stream(
        futures_util::stream::iter(chunks),
        context_with_api_style(
            "anthropic.claude-sonnet-4-5",
            AwsBedrockApiStyle::MantleAnthropicMessages,
            None,
        ),
    );
    let mut transcript = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk");
        transcript.push_str(std::str::from_utf8(&chunk).expect("utf8"));
    }

    assert!(transcript.contains(r#""id":"msg_123""#));
    assert!(transcript.contains(r#""delta":{"role":"assistant"}"#));
    assert!(transcript.contains(r#""delta":{"content":"Hello"}"#));
    assert!(transcript.contains(r#""finish_reason":"stop""#));
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[test]
fn normalizes_text_response_with_usage() {
    let response = json!({
        "responseId": "bedrock-response-id",
        "output": {
            "message": {
                "role": "assistant",
                "content": [{"text": "Hello from Bedrock."}]
            }
        },
        "stopReason": "end_turn",
        "usage": {
            "inputTokens": 12,
            "outputTokens": 5,
            "totalTokens": 17,
            "cacheReadInputTokens": 2
        }
    });

    let normalized = normalize_converse_response(
        &response,
        &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
    );

    assert_eq!(normalized["id"], "bedrock-response-id");
    assert_eq!(normalized["object"], "chat.completion");
    assert_eq!(normalized["model"], "gateway-model");
    assert_eq!(normalized["choices"][0]["message"]["role"], "assistant");
    assert_eq!(
        normalized["choices"][0]["message"]["content"],
        "Hello from Bedrock."
    );
    assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    assert_eq!(normalized["usage"]["prompt_tokens"], 12);
    assert_eq!(normalized["usage"]["completion_tokens"], 5);
    assert_eq!(normalized["usage"]["total_tokens"], 17);
    assert_eq!(
        normalized["usage"]["provider_usage"]["cacheReadInputTokens"],
        2
    );
}

#[test]
fn normalizes_converse_reasoning_metadata_without_leaking_into_content() {
    let response = json!({
        "responseId": "bedrock-response-id",
        "output": {
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "reasoningContent": {
                            "reasoningText": {
                                "text": "visible summarized reasoning",
                                "signature": "sig-reasoning"
                            }
                        }
                    },
                    {
                        "reasoningContent": {
                            "redactedContent": "cmVkYWN0ZWQ="
                        }
                    },
                    {"text": "Final answer."}
                ]
            }
        },
        "stopReason": "end_turn",
        "usage": {
            "inputTokens": 12,
            "outputTokens": 9,
            "totalTokens": 21
        }
    });

    let normalized = normalize_converse_response(
        &response,
        &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
    );
    let message = &normalized["choices"][0]["message"];

    assert_eq!(message["content"], "Final answer.");
    assert!(
        !message["content"]
            .as_str()
            .expect("content string")
            .contains("visible summarized reasoning")
    );
    assert_eq!(
        message["provider_metadata"]["aws_bedrock"]["reasoning"]["source"],
        "bedrock_converse"
    );
    assert_eq!(
        message["provider_metadata"]["aws_bedrock"]["reasoning"]["blocks"],
        json!([
            {
                "type": "reasoning_text",
                "text": "visible summarized reasoning",
                "signature": "sig-reasoning"
            },
            {
                "type": "redacted_reasoning",
                "data": "cmVkYWN0ZWQ="
            }
        ])
    );
}

#[test]
fn normalizes_tool_use_response() {
    let response = json!({
        "output": {
            "message": {
                "role": "assistant",
                "content": [{
                    "toolUse": {
                        "toolUseId": "tooluse_123",
                        "name": "get_weather",
                        "input": {"city": "London"}
                    }
                }]
            }
        },
        "stopReason": "tool_use",
        "usage": {
            "inputTokens": 30,
            "outputTokens": 8,
            "totalTokens": 38
        }
    });

    let normalized = normalize_converse_response(&response, &context("amazon.nova-pro-v1:0"));

    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        normalized["choices"][0]["message"]["tool_calls"][0],
        json!({
            "id": "tooluse_123",
            "type": "function",
            "function": {
                "name": "get_weather",
                "arguments": "{\"city\":\"London\"}"
            }
        })
    );
}

#[test]
fn normalizes_anthropic_messages_response_with_usage_and_cache_tokens() {
    let response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "model": "claude-3-5-sonnet",
        "content": [{"type": "text", "text": "Hello from Claude."}],
        "stop_reason": "stop_sequence",
        "usage": {
            "input_tokens": 12,
            "output_tokens": 5,
            "cache_read_input_tokens": 2,
            "cache_creation_input_tokens": 3
        }
    });

    let normalized = normalize_anthropic_messages_response(
        &response,
        &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
    );

    assert_eq!(normalized["id"], "msg_123");
    assert_eq!(
        normalized["choices"][0]["message"]["content"],
        "Hello from Claude."
    );
    assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    assert_eq!(normalized["usage"]["prompt_tokens"], 12);
    assert_eq!(normalized["usage"]["completion_tokens"], 5);
    assert_eq!(normalized["usage"]["total_tokens"], 17);
    assert_eq!(
        normalized["usage"]["provider_usage"]["cache_read_input_tokens"],
        2
    );
    assert_eq!(
        normalized["usage"]["provider_usage"]["cache_creation_input_tokens"],
        3
    );
}

#[test]
fn normalizes_anthropic_thinking_metadata_without_leaking_into_content() {
    let response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "thinking",
                "thinking": "summarized hidden reasoning",
                "signature": "sig-thinking"
            },
            {
                "type": "redacted_thinking",
                "data": "ZW5jcnlwdGVk"
            },
            {
                "type": "tool_use",
                "id": "toolu_123",
                "name": "get_weather",
                "input": {"city": "London"}
            },
            {
                "type": "text",
                "text": "I will check."
            }
        ],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 30,
            "output_tokens": 12
        }
    });

    let normalized =
        normalize_anthropic_messages_response(&response, &context("anthropic.claude-opus-4-7"));
    let message = &normalized["choices"][0]["message"];

    assert_eq!(message["content"], "I will check.");
    assert!(
        !message["content"]
            .as_str()
            .expect("content string")
            .contains("summarized hidden reasoning")
    );
    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        message["tool_calls"][0]["function"]["arguments"],
        "{\"city\":\"London\"}"
    );
    assert_eq!(
        message["provider_metadata"]["aws_bedrock"]["reasoning"]["source"],
        "anthropic_messages"
    );
    assert_eq!(
        message["provider_metadata"]["aws_bedrock"]["reasoning"]["blocks"],
        json!([
            {
                "type": "thinking",
                "thinking": "summarized hidden reasoning",
                "signature": "sig-thinking"
            },
            {
                "type": "redacted_thinking",
                "data": "ZW5jcnlwdGVk"
            }
        ])
    );
}

#[test]
fn normalizes_anthropic_tool_use_response() {
    let response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "tool_use",
            "id": "toolu_123",
            "name": "get_weather",
            "input": {"city": "London"}
        }],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 30,
            "output_tokens": 8
        }
    });

    let normalized = normalize_anthropic_messages_response(
        &response,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    );

    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        normalized["choices"][0]["message"]["tool_calls"][0],
        json!({
            "id": "toolu_123",
            "type": "function",
            "function": {
                "name": "get_weather",
                "arguments": "{\"city\":\"London\"}"
            }
        })
    );
}
