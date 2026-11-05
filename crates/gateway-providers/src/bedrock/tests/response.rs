use super::*;

#[tokio::test]
async fn independent_anthropic_replay_errors_drain_same_event_and_later_usage() {
    for close_invalid_tool in [false, true] {
        let mut events = vec![
            json!({"type":"message_start","message":{"id":"msg-drain","content":[],"usage":{"input_tokens":3,"output_tokens":0,"cache_read_input_tokens":2}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call-a","name":"lookup","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}),
        ];
        if close_invalid_tool {
            events.push(json!({"type":"content_block_stop","index":0}));
        }
        events.extend([
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"SUPPRESSED_AFTER_REPLAY_FAILURE"}}),
            json!({"type":"message_delta","usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ]);
        let transcript = collect_anthropic_messages_stream(
            events
                .into_iter()
                .map(|event| {
                    Ok(Bytes::from(format!(
                        "event: {}\ndata: {event}\n\n",
                        event["type"].as_str().unwrap()
                    )))
                })
                .collect(),
        )
        .await;
        let chunks = transcript
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect::<Vec<_>>();
        let error_index = chunks
            .iter()
            .position(|chunk| chunk.get("error").is_some())
            .unwrap();
        let final_usage_index = chunks
            .iter()
            .rposition(|chunk| chunk.get("usage").is_some())
            .unwrap();
        assert!(final_usage_index < error_index);
        assert_eq!(
            chunks[error_index]["error"]["code"],
            "anthropic_messages_replay_error"
        );
        assert_eq!(chunks[final_usage_index]["usage"]["prompt_tokens"], 3);
        assert_eq!(chunks[final_usage_index]["usage"]["completion_tokens"], 4);
        assert_eq!(
            chunks[final_usage_index]["usage"]["provider_usage"]["cache_read_input_tokens"],
            2
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk["usage"]["completion_tokens"] == 3)
        );
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk["choices"][0]["finish_reason"].is_null())
        );
        assert!(chunks.iter().all(|chunk| {
            chunk["choices"][0]["delta"]["provider_metadata"]["aws_bedrock"]
                .get("native_content")
                .is_none()
        }));
        assert!(!transcript.contains("SUPPRESSED_AFTER_REPLAY_FAILURE"));
        assert!(!transcript.contains("[DONE]"));
    }
}

#[tokio::test]
async fn independent_anthropic_truncated_tool_json_keeps_expected_terminal_and_usage() {
    for (reason, expected) in [
        ("max_tokens", "length"),
        ("model_context_window_exceeded", "length"),
        ("refusal", "content_filter"),
    ] {
        for close_invalid_tool in [false, true] {
            let mut events = vec![
                json!({"type":"message_start","message":{"id":"msg-cut","content":[],"usage":{"input_tokens":3,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call-a","name":"lookup","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}),
            ];
            if close_invalid_tool {
                events.push(json!({"type":"content_block_stop","index":0}));
            }
            events.extend([
                json!({"type":"message_delta","delta":{"stop_reason":reason},"usage":{"output_tokens":3}}),
                json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"SUPPRESSED_AFTER_TRUNCATION"}}),
                json!({"type":"message_delta","usage":{"output_tokens":4}}),
                json!({"type":"message_stop"}),
            ]);
            let transcript = collect_anthropic_messages_stream(
                events
                    .into_iter()
                    .map(|event| {
                        Ok(Bytes::from(format!(
                            "event: {}\ndata: {event}\n\n",
                            event["type"].as_str().unwrap()
                        )))
                    })
                    .collect(),
            )
            .await;
            let chunks = transcript
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                .collect::<Vec<_>>();
            assert!(
                chunks.iter().all(|chunk| chunk.get("error").is_none()),
                "{transcript}"
            );
            assert!(
                chunks
                    .iter()
                    .any(|chunk| chunk["choices"][0]["finish_reason"] == expected)
            );
            let final_usage = chunks
                .iter()
                .rfind(|chunk| chunk.get("usage").is_some())
                .unwrap();
            assert_eq!(final_usage["usage"]["completion_tokens"], 4);
            assert_eq!(final_usage["usage"]["prompt_tokens"], 3);
            assert!(chunks.iter().all(|chunk| {
                chunk["choices"][0]["delta"]["provider_metadata"]["aws_bedrock"]
                    .get("native_content")
                    .is_none()
            }));
            assert!(!transcript.contains("SUPPRESSED_AFTER_TRUNCATION"));
            assert!(transcript.ends_with("data: [DONE]\n\n"), "{transcript}");
        }
    }
}

#[test]
fn malformed_converse_generations_fail_without_fabricating_http_status_and_keep_usage() {
    for reason in ["malformed_tool_use", "malformed_model_output"] {
        let error = normalize_converse_response(
            &json!({
                "output":{"message":{"content":[{"text":"incomplete"}]}},
                "stopReason":reason, "usage":{"inputTokens":8,"outputTokens":3}
            }),
            &context("nova"),
        )
        .expect_err("generation failure");
        let ProviderError::PartialUsage {
            source,
            provider_usage,
        } = error
        else {
            panic!("missing partial usage error");
        };
        assert!(matches!(source.as_ref(), ProviderError::Transport(_)));
        assert!(source.to_string().contains(reason));
        assert_eq!(provider_usage.unwrap()["total_tokens"], 11);
    }
}

#[tokio::test]
async fn mantle_anthropic_stream_classifies_body_timeout() {
    let error = crate::http::tests::timed_out_body_error().await;
    let upstream = futures_util::stream::once(async move { Err(error) });
    let chunks = normalize_anthropic_messages_stream(upstream, context("test"), "aws_bedrock")
        .collect::<Vec<_>>()
        .await;
    let output = chunks
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect::<String>();
    assert!(output.contains("Upstream response timed out"), "{output}");
    assert!(!output.contains("[DONE]"));
}

#[tokio::test]
async fn normalizes_mantle_anthropic_messages_sse() {
    let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![
        Ok(Bytes::from_static(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\",\"usage\":{\"input_tokens\":3,\"output_tokens\":0,\"cache_read_input_tokens\":2,\"cache_creation_input_tokens\":1,\"cache_creation\":{\"ephemeral_5m_input_tokens\":1}}}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )),
    ];
    let mut stream = normalize_anthropic_messages_stream(
        futures_util::stream::iter(chunks),
        context_with_api_style(
            "anthropic.claude-sonnet-4-5",
            AwsBedrockApiStyle::MantleAnthropicMessages,
            None,
        ),
        "aws_bedrock",
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
    assert!(transcript.contains(r#""prompt_tokens":3"#));
    assert!(transcript.contains(r#""completion_tokens":1"#));
    assert!(transcript.contains(r#""total_tokens":4"#));
    let final_usage = transcript
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|chunk| chunk["choices"][0]["finish_reason"] == "stop")
        .expect("final usage chunk");
    assert_eq!(
        final_usage["usage"]["provider_usage"],
        json!({
            "input_tokens": 3,
            "output_tokens": 1,
            "cache_read_input_tokens": 2,
            "cache_creation_input_tokens": 1,
            "cache_creation": {"ephemeral_5m_input_tokens": 1}
        })
    );
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn normalizes_mantle_anthropic_messages_sse_error_without_done() {
    let transcript = collect_anthropic_messages_stream(vec![Ok(Bytes::from_static(
        b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"rate limited\"}}\n\n",
    ))])
    .await;

    assert!(transcript.contains(r#""code":"overloaded_error""#));
    assert!(transcript.contains(r#""message":"rate limited""#));
    assert!(!transcript.contains("[DONE]"));
}

#[tokio::test]
async fn normalizes_mantle_anthropic_messages_streamed_tool_call_metadata() {
    let transcript = collect_anthropic_messages_stream(vec![
        Ok(Bytes::from_static(
            b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\"}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"get_weather\",\"input\":{}}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"London\\\"}\"}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"input_tokens\":8,\"output_tokens\":4}}\n\n",
        )),
        Ok(Bytes::from_static(
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )),
    ])
    .await;

    assert!(transcript.contains(r#""delta":{"role":"assistant"}"#));
    assert!(transcript.contains(r#""id":"toolu_123""#));
    assert!(transcript.contains(r#""type":"function""#));
    assert!(transcript.contains(r#""name":"get_weather""#));
    let tool_indexes = transcript
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .flat_map(|event| {
            event["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|call| call["index"].as_u64())
        .collect::<Vec<_>>();
    assert_eq!(tool_indexes, vec![0, 0]);
    assert!(transcript.contains(r#""arguments":"{\"city\":\"London\"}""#));
    assert!(transcript.contains(r#""finish_reason":"tool_calls""#));
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
    )
    .expect("valid generation");

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
    assert_eq!(normalized["usage"]["usage_source"], "bedrock");
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
    )
    .expect("valid generation");
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

    let normalized = normalize_converse_response(&response, &context("amazon.nova-pro-v1:0"))
        .expect("valid generation");

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

async fn collect_anthropic_messages_stream(chunks: Vec<Result<Bytes, reqwest::Error>>) -> String {
    collect_anthropic_messages_stream_for_namespace(chunks, "aws_bedrock").await
}

async fn collect_anthropic_messages_stream_for_namespace(
    chunks: Vec<Result<Bytes, reqwest::Error>>,
    provider_namespace: &'static str,
) -> String {
    let mut stream = normalize_anthropic_messages_stream(
        futures_util::stream::iter(chunks),
        context_with_api_style(
            "anthropic.claude-sonnet-4-5",
            AwsBedrockApiStyle::MantleAnthropicMessages,
            None,
        ),
        provider_namespace,
    );
    let mut transcript = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk");
        transcript.push_str(std::str::from_utf8(&chunk).expect("utf8"));
    }

    transcript
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
        "aws_bedrock",
    );

    assert_eq!(normalized["id"], "msg_123");
    assert_eq!(
        normalized["choices"][0]["message"]["content"],
        "Hello from Claude."
    );
    assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    assert_eq!(normalized["usage"]["prompt_tokens"], 12);
    assert_eq!(normalized["usage"]["completion_tokens"], 5);
    assert_eq!(normalized["usage"]["total_tokens"], 22);
    assert_eq!(normalized["usage"]["usage_source"], "bedrock");
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

    let normalized = normalize_anthropic_messages_response(
        &response,
        &context("anthropic.claude-opus-4-7"),
        "aws_bedrock",
    );
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
        "aws_bedrock",
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
