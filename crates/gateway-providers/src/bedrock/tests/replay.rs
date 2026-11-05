use super::*;

fn assistant_projection(response: &Value) -> CoreChatMessage {
    let projection = &response["choices"][0]["message"];
    let mut assistant = message("assistant", "");
    assistant.content = projection["content"].clone();
    for key in ["tool_calls", "provider_metadata"] {
        if let Some(value) = projection.get(key) {
            assistant.extra.insert(key.to_string(), value.clone());
        }
    }
    assistant
}

fn request_with_assistant(assistant: CoreChatMessage) -> CoreChatRequest {
    CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "question"), assistant],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    }
}

fn assistant_from_stream(transcript: &str) -> CoreChatMessage {
    let mut text = String::new();
    let mut tools = BTreeMap::<u64, Value>::new();
    let mut metadata = Value::Null;
    for chunk in transcript
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
    {
        assert!(chunk.get("error").is_none(), "{chunk}");
        let delta = &chunk["choices"][0]["delta"];
        if let Some(part) = delta["content"].as_str() {
            text.push_str(part);
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for call in calls {
                let tool = tools
                    .entry(call["index"].as_u64().unwrap())
                    .or_insert_with(|| {
                        json!({
                            "id":call["id"],"type":"function",
                            "function":{"name":call["function"]["name"],"arguments":""}
                        })
                    });
                let mut arguments = tool["function"]["arguments"].as_str().unwrap().to_string();
                arguments.push_str(call["function"]["arguments"].as_str().unwrap_or_default());
                tool["function"]["arguments"] = json!(arguments);
            }
        }
        if delta["provider_metadata"]["aws_bedrock"]
            .get("native_content")
            .is_some()
        {
            metadata = delta["provider_metadata"].clone();
        }
    }
    let mut assistant = message("assistant", &text);
    if !tools.is_empty() {
        assistant.extra.insert(
            "tool_calls".into(),
            json!(tools.into_values().collect::<Vec<_>>()),
        );
    }
    assistant.extra.insert("provider_metadata".into(), metadata);
    assistant
}

fn signed_converse_response() -> Value {
    json!({
        "output": {"message": {"role": "assistant", "content": [
            {"reasoningContent": {"reasoningText": {"text": "reason", "signature": "signed"}}},
            {"text": "before"},
            {"toolUse": {"toolUseId": "call-a", "name": "lookup", "input": {"city": "London"}}},
            {"reasoningContent": {"redactedContent": "c2VjcmV0"}},
            {"futureServerBlock": {"opaque": ["preserve", 1]}},
            {"text": "after"}
        ]}},
        "stopReason": "tool_use"
    })
}

#[test]
fn converse_chat_roundtrip_preserves_signed_redacted_and_future_block_order() {
    let native = signed_converse_response();
    let projected = normalize_converse_response(&native, &context("nova")).unwrap();
    let body = map_chat_request_to_converse(
        &request_with_assistant(assistant_projection(&projected)),
        &context("nova"),
    )
    .unwrap();
    assert_eq!(
        body["messages"][1]["content"],
        native["output"]["message"]["content"]
    );
}

#[test]
fn converse_replay_rejects_edited_text_tools_arguments_and_reasoning() {
    let projected =
        normalize_converse_response(&signed_converse_response(), &context("nova")).unwrap();
    for edit in [
        "text",
        "id",
        "name",
        "arguments",
        "reasoning",
        "reasoning_content",
    ] {
        let mut assistant = assistant_projection(&projected);
        match edit {
            "text" => assistant.content = json!("edited"),
            "id" => assistant.extra.get_mut("tool_calls").unwrap()[0]["id"] = json!("other"),
            "name" => {
                assistant.extra.get_mut("tool_calls").unwrap()[0]["function"]["name"] =
                    json!("other")
            }
            "arguments" => {
                assistant.extra.get_mut("tool_calls").unwrap()[0]["function"]["arguments"] =
                    json!(r#"{"city":"Paris"}"#)
            }
            "reasoning" => {
                assistant.extra.get_mut("provider_metadata").unwrap()["aws_bedrock"]["reasoning"]
                    ["blocks"][0]["text"] = json!("edited")
            }
            _ => {
                assistant
                    .extra
                    .insert("reasoning_content".into(), json!("edited"));
            }
        }
        let error =
            map_chat_request_to_converse(&request_with_assistant(assistant), &context("nova"))
                .expect_err(edit);
        assert!(
            matches!(error, ProviderError::InvalidRequest(_)),
            "{edit}: {error}"
        );
        assert!(
            error.to_string().contains("projection mismatch"),
            "{edit}: {error}"
        );
    }
}

#[test]
fn converse_replay_compares_tool_arguments_as_json_not_serialized_whitespace() {
    let projected =
        normalize_converse_response(&signed_converse_response(), &context("nova")).unwrap();
    let mut assistant = assistant_projection(&projected);
    assistant.extra.get_mut("tool_calls").unwrap()[0]["function"]["arguments"] =
        json!(" { \"city\" : \"London\" } ");
    assert!(
        map_chat_request_to_converse(&request_with_assistant(assistant), &context("nova")).is_ok()
    );
}

#[test]
fn converse_groups_parallel_results_without_merging_unrelated_user_turns() {
    let mut native = signed_converse_response();
    native["output"]["message"]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"toolUse":{"toolUseId":"call-b","name":"lookup","input":{"city":"Paris"}}}));
    let projected = normalize_converse_response(&native, &context("nova")).unwrap();
    let mut request = request_with_assistant(assistant_projection(&projected));
    for id in ["call-a", "call-b"] {
        let mut result = message("tool", "result");
        result.extra.insert("tool_call_id".into(), json!(id));
        request.messages.push(result);
    }
    request.messages.push(message("user", "another question"));
    let body = map_chat_request_to_converse(&request, &context("nova")).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"].as_array().unwrap().len(), 2);
    assert_eq!(
        messages[2]["content"][0]["toolResult"]["toolUseId"],
        "call-a"
    );
    assert_eq!(
        messages[2]["content"][1]["toolResult"]["toolUseId"],
        "call-b"
    );
    assert_eq!(
        messages[3]["content"],
        json!([{"text": "another question"}])
    );
}

#[test]
fn converse_requested_additional_response_fields_remain_available() {
    let mut request = CoreChatRequest {
        model: "nova".into(),
        messages: vec![message("user", "hello")],
        stream: false,
        extra: BTreeMap::from([(
            "additionalModelResponseFieldPaths".into(),
            json!(["/stop_sequence"]),
        )]),
    };
    let body = map_chat_request_to_converse(&request, &context("nova")).unwrap();
    assert_eq!(
        body["additionalModelResponseFieldPaths"],
        json!(["/stop_sequence"])
    );
    let native = json!({
        "output": {"message": {"content": [{"text": "done"}]}},
        "stopReason": "stop_sequence",
        "additionalModelResponseFields": {"stop_sequence": "END", "future": [1,2]}
    });
    let projected = normalize_converse_response(&native, &context("nova")).unwrap();
    assert_eq!(
        projected["choices"][0]["message"]["provider_metadata"]["aws_bedrock"]["additionalModelResponseFields"],
        native["additionalModelResponseFields"],
    );
    request.messages.push(assistant_projection(&projected));
    let replayed = map_chat_request_to_converse(&request, &context("nova")).unwrap();
    assert_eq!(
        replayed["messages"][1]["content"],
        native["output"]["message"]["content"]
    );
}

#[test]
fn anthropic_chat_roundtrip_replays_native_order_on_both_bedrock_targets() {
    let blocks = json!([
        {"type":"thinking","thinking":"reason","signature":"signed"},
        {"type":"text","text":"before"},
        {"type":"tool_use","id":"call-a","name":"lookup","input":{"city":"London"}},
        {"type":"redacted_thinking","data":"encrypted"},
        {"type":"server_tool_use","id":"server-a","name":"web_search","input":{"query":"news"}},
        {"type":"text","text":"after"}
    ]);
    let projected = normalize_anthropic_messages_response(
        &json!({"id":"msg-a","content":blocks,"stop_reason":"tool_use"}),
        &context("anthropic.claude-sonnet-4-6"),
        "aws_bedrock",
    );
    for target in [
        AnthropicMessagesTarget::RuntimeInvoke,
        AnthropicMessagesTarget::MantleMessages,
    ] {
        let assistant = assistant_projection(&projected);
        let body = map_chat_request_to_anthropic_messages_target(
            &request_with_assistant(assistant.clone()),
            &context("anthropic.claude-sonnet-4-6"),
            target,
        )
        .unwrap();
        assert_eq!(body["messages"][1]["content"], blocks);
        let mut edited = assistant;
        edited.extra.get_mut("tool_calls").unwrap()[0]["function"]["arguments"] =
            json!(r#"{"city":"Paris"}"#);
        assert!(
            map_chat_request_to_anthropic_messages_target(
                &request_with_assistant(edited),
                &context("anthropic.claude-sonnet-4-6"),
                target,
            )
            .is_err()
        );
    }
}

#[test]
fn converse_maps_public_projection_when_history_contains_foreign_anthropic_replay() {
    let projected = normalize_anthropic_messages_response(
        &json!({
            "id":"msg-a","stop_reason":"tool_use","content":[
                {"type":"thinking","thinking":"reason","signature":"signed"},
                {"type":"text","text":"visible"},
                {"type":"tool_use","id":"call-a","name":"lookup","input":{"city":"London"}}
            ]
        }),
        &context("anthropic.claude-sonnet-4-6"),
        "aws_bedrock",
    );
    let body = map_chat_request_to_converse(
        &request_with_assistant(assistant_projection(&projected)),
        &context("nova"),
    )
    .unwrap();
    assert_eq!(
        body["messages"][1]["content"],
        json!([
            {"text":"visible"},
            {"toolUse":{"toolUseId":"call-a","name":"lookup","input":{"city":"London"}}}
        ])
    );
}

#[test]
fn bedrock_native_messages_do_not_interpret_chat_replay_extensions() {
    let content = json!([
        {"type":"thinking","thinking":"reason","signature":"signed"},
        {"type":"text","text":"visible"},
        {"type":"redacted_thinking","data":"encrypted"},
        {"type":"future_block","opaque":{"keep":true}}
    ]);
    let native = serde_json::from_value(json!({
        "model":"claude", "max_tokens":64,
        "messages":[{"role":"user","content":"hello"}, {
            "role":"assistant", "content":content,
            "provider_metadata":{"aws_bedrock":{"native_content":{"source":"bedrock_converse","blocks":[{"text":"different"}]}}}
        }]
    })).unwrap();
    let request = gateway_core::anthropic_messages_request_to_core(&native);
    let context = ProviderRequestContext {
        chat_protocol: gateway_core::ChatRequestProtocol::AnthropicMessages,
        ..context("anthropic.claude-sonnet-4-6")
    };
    for target in [
        AnthropicMessagesTarget::RuntimeInvoke,
        AnthropicMessagesTarget::MantleMessages,
    ] {
        let body =
            map_chat_request_to_anthropic_messages_target(&request, &context, target).unwrap();
        assert_eq!(body["messages"][1]["content"], content);
    }
}

#[tokio::test]
async fn converse_stream_consumer_replays_signed_redacted_order_and_detects_edits() {
    let events = [
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{"reasoningContent":{"text":"reason"}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{"reasoningContent":{"signature":"signed"}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":1,"delta":{"text":"before"}}),
        ),
        (
            "contentBlockStart",
            json!({"contentBlockIndex":2,"start":{"toolUse":{"toolUseId":"call-a","name":"lookup"}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":2,"delta":{"toolUse":{"input":"{\"city\":\"London\"}"}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":3,"delta":{"reasoningContent":{"redactedContent":"c2VjcmV0"}}}),
        ),
        (
            "contentBlockStart",
            json!({"contentBlockIndex":4,"start":{"futureServerBlock":{"opaque":["preserve",1]}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":5,"delta":{"text":"after"}}),
        ),
        ("messageStop", json!({"stopReason":"tool_use"})),
    ];
    let frames = events
        .iter()
        .map(|(kind, payload)| {
            eventstream_frame(
                &[(":message-type", "event"), (":event-type", kind)],
                payload.to_string().as_bytes(),
            )
        })
        .collect();
    let transcript = collect_bedrock_stream(frames).await;
    let mut assistant = assistant_from_stream(&transcript);
    assert_eq!(assistant.content, "beforeafter");
    let request = request_with_assistant(assistant.clone());
    let body = map_chat_request_to_converse(&request, &context("nova")).unwrap();
    assert_eq!(
        body["messages"][1]["content"],
        signed_converse_response()["output"]["message"]["content"]
    );
    assistant.content = json!("edited");
    assert!(
        map_chat_request_to_converse(&request_with_assistant(assistant), &context("nova")).is_err()
    );
}

#[tokio::test]
async fn independent_anthropic_stream_consumer_replays_order_signatures_and_empty_input() {
    let blocks = json!([
        {"type":"thinking","thinking":"reason","signature":"signed"},
        {"type":"text","text":"before"},
        {"type":"tool_use","id":"call-a","name":"lookup","input":{}},
        {"type":"redacted_thinking","data":"encrypted"},
        {"type":"server_tool_use","id":"server-a","name":"web_search","input":{"query":"news"}},
        {"type":"text","text":"after"}
    ]);
    let mut events = vec![json!({"type":"message_start","message":{"id":"msg-a"}})];
    for (index, block) in blocks.as_array().unwrap().iter().enumerate() {
        let mut start = block.clone();
        if index == 0 {
            start["signature"] = json!("");
        }
        events.push(json!({"type":"content_block_start","index":index,"content_block":start}));
        if index == 0 {
            events.push(json!({"type":"content_block_delta","index":index,
                "delta":{"type":"signature_delta","signature":"signed"}}));
        }
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    events.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8}}));
    events.push(json!({"type":"message_stop"}));
    let upstream = events.into_iter().map(|event| {
        Ok(Bytes::from(format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().unwrap(),
        )))
    });
    let chunks = normalize_anthropic_messages_stream(
        futures_util::stream::iter(upstream),
        context("anthropic.claude-sonnet-4-6"),
        "aws_bedrock",
    )
    .collect::<Vec<_>>()
    .await;
    let transcript = chunks
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect::<String>();
    let assistant = assistant_from_stream(&transcript);
    assert_eq!(assistant.content, "beforeafter");
    assert_eq!(
        assistant.extra["tool_calls"][0]["function"]["arguments"],
        "{}"
    );
    for target in [
        AnthropicMessagesTarget::RuntimeInvoke,
        AnthropicMessagesTarget::MantleMessages,
    ] {
        let body = map_chat_request_to_anthropic_messages_target(
            &request_with_assistant(assistant.clone()),
            &context("anthropic.claude-sonnet-4-6"),
            target,
        )
        .unwrap();
        assert_eq!(body["messages"][1]["content"], blocks);
    }
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}
