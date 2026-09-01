use super::*;

#[test]
fn maps_openai_request_to_anthropic_payload_with_default_version() {
    let request = CoreChatRequest {
        model: "fast".to_string(),
        messages: vec![
            CoreChatMessage {
                role: "system".to_string(),
                content: Value::String("be concise".to_string()),
                name: None,
                extra: std::collections::BTreeMap::new(),
            },
            CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("ping".to_string()),
                name: None,
                extra: std::collections::BTreeMap::new(),
            },
        ],
        stream: false,
        extra: std::collections::BTreeMap::new(),
    };
    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");
    assert_eq!(mapped["anthropic_version"], "vertex-2023-10-16");
    assert_eq!(mapped["messages"][0]["role"], "user");
    assert_eq!(mapped["system"], "be concise");
}

#[test]
fn maps_openai_tools_tool_calls_and_tool_results_to_anthropic_payload() {
    let mut assistant_extra = BTreeMap::new();
    assistant_extra.insert(
        "tool_calls".to_string(),
        json!([{
            "id": "call_123",
            "type": "function",
            "function": {
                "name": "lookup",
                "arguments": "{\"city\":\"London\"}"
            }
        }]),
    );
    let mut tool_extra = BTreeMap::new();
    tool_extra.insert("tool_call_id".to_string(), json!("call_123"));
    let mut request = chat_request(vec![
        CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("weather?".to_string()),
            name: None,
            extra: BTreeMap::new(),
        },
        CoreChatMessage {
            role: "assistant".to_string(),
            content: Value::Null,
            name: None,
            extra: assistant_extra,
        },
        CoreChatMessage {
            role: "tool".to_string(),
            content: Value::String("sunny".to_string()),
            name: Some("lookup".to_string()),
            extra: tool_extra,
        },
    ]);
    request.extra.insert(
        "tools".to_string(),
        json!([{
            "type": "function",
            "function": {
                "name": "lookup",
                "description": "Look up weather",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }
        }]),
    );
    request.extra.insert(
        "tool_choice".to_string(),
        json!({"type":"function","function":{"name":"lookup"}}),
    );

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert_eq!(
        mapped["tools"][0],
        json!({
            "name": "lookup",
            "description": "Look up weather",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        })
    );
    assert_eq!(
        mapped["tool_choice"],
        json!({"type":"tool","name":"lookup"})
    );
    assert_eq!(
        mapped["messages"][1]["content"][0],
        json!({
            "type": "tool_use",
            "id": "call_123",
            "name": "lookup",
            "input": {"city": "London"}
        })
    );
    assert_eq!(
        mapped["messages"][2]["content"][0],
        json!({
            "type": "tool_result",
            "tool_use_id": "call_123",
            "content": "sunny"
        })
    );
}

#[test]
fn maps_openai_none_tool_choice_for_anthropic_payload() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("do not use tools".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "tools".to_string(),
        json!([{
            "type": "function",
            "function": {
                "name": "lookup",
                "parameters": {"type": "object", "properties": {}}
            }
        }]),
    );
    request
        .extra
        .insert("tool_choice".to_string(), json!("none"));

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert_eq!(mapped["tool_choice"], json!({"type": "none"}));
}

#[test]
fn route_override_cannot_replace_anthropic_messages_or_stream_mode() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("mapped message".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("anthropic_version".to_string(), json!("caller-version"));
    request
        .extra
        .insert("model".to_string(), json!("caller-model"));
    let mut context = context("anthropic/claude-sonnet-4-6");
    context.extra_body.insert(
        "messages".to_string(),
        json!([{"role": "assistant", "content": "route override"}]),
    );
    context
        .extra_body
        .insert("stream".to_string(), json!(false));
    context
        .extra_body
        .insert("anthropic_version".to_string(), json!("route-version"));
    context
        .extra_body
        .insert("model".to_string(), json!("route-model"));

    let mapped = map_anthropic_request(&request, &context, true).expect("mapped");

    assert_eq!(mapped["messages"][0]["role"], "user");
    assert_eq!(mapped["messages"][0]["content"], "mapped message");
    assert_eq!(mapped["stream"], true);
    assert_eq!(mapped["anthropic_version"], "vertex-2023-10-16");
    assert!(mapped.get("model").is_none());
}

#[test]
fn rejects_malformed_openai_tool_call_arguments_for_anthropic_payload() {
    let mut assistant_extra = BTreeMap::new();
    assistant_extra.insert(
        "tool_calls".to_string(),
        json!([{
            "id": "call_123",
            "type": "function",
            "function": {
                "name": "lookup",
                "arguments": "{\"city\":"
            }
        }]),
    );
    let request = chat_request(vec![
        CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("weather?".to_string()),
            name: None,
            extra: BTreeMap::new(),
        },
        CoreChatMessage {
            role: "assistant".to_string(),
            content: Value::Null,
            name: None,
            extra: assistant_extra,
        },
    ]);

    let error = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect_err("malformed arguments should fail");

    assert!(error.to_string().contains("valid JSON"));
}

#[test]
fn preserves_anthropic_text_block_metadata_for_prompt_caching() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "text",
            "text": "cached prompt",
            "cache_control": {"type": "ephemeral"}
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert_eq!(
        mapped["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
}

#[test]
fn preserves_native_anthropic_tools_for_messages_requests() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {"type":"text","text":"use the tool"},
            {"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "tools".to_string(),
        json!([{
            "name": "lookup",
            "description": "Look up weather",
            "input_schema": {"type": "object", "properties": {}}
        }]),
    );

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert_eq!(mapped["tools"][0]["name"], "lookup");
    assert_eq!(mapped["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(mapped["messages"][0]["content"][1]["type"], "tool_result");
}

#[test]
fn preserves_anthropic_thinking_blocks_for_continuation() {
    let thinking = json!({
        "type": "thinking",
        "thinking": "hidden reasoning",
        "signature": "sig_123"
    });
    let redacted = json!({
        "type": "redacted_thinking",
        "data": "encrypted"
    });
    let request = chat_request(vec![
        CoreChatMessage {
            role: "user".to_string(),
            content: json!("start"),
            name: None,
            extra: BTreeMap::new(),
        },
        CoreChatMessage {
            role: "assistant".to_string(),
            content: json!([
                thinking.clone(),
                {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {"key": "value"}},
                redacted.clone(),
                {"type": "text", "text": "visible"}
            ]),
            name: None,
            extra: BTreeMap::new(),
        },
    ]);

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert_eq!(
        mapped["messages"][1]["content"],
        json!([
            thinking,
            {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {"key": "value"}},
            redacted,
            {"type": "text", "text": "visible"}
        ])
    );
}

#[test]
fn omits_unqualified_context_management_from_vertex_request() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!("start"),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "context_management".to_string(),
        json!({
            "edits": [{
                "type": "clear_thinking_20251015",
                "keep": "all"
            }]
        }),
    );
    let mut context = context("anthropic/claude-sonnet-4-6");
    context.request_headers.insert(
        "anthropic-beta".to_string(),
        "context-management-2025-06-27".to_string(),
    );

    let mapped = map_anthropic_request(&request, &context, false).expect("mapped");

    assert!(mapped.get("context_management").is_none());
}

#[test]
fn allows_route_owned_context_management_with_matching_beta_header() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!("start"),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "context_management".to_string(),
        json!({"edits": [{"type": "caller_edit"}]}),
    );
    let mut context = context("anthropic/claude-sonnet-4-6");
    context.extra_body.insert(
        "context_management".to_string(),
        json!({
            "edits": [{
                "type": "clear_thinking_20251015",
                "keep": "all"
            }]
        }),
    );
    context.extra_headers.insert(
        "anthropic-beta".to_string(),
        json!("context-management-2025-06-27"),
    );

    let mapped = map_anthropic_request(&request, &context, false).expect("mapped");

    assert_eq!(
        mapped["context_management"],
        context.extra_body["context_management"]
    );
}

#[test]
fn does_not_duplicate_native_and_openai_tool_uses() {
    let mut assistant_extra = BTreeMap::new();
    assistant_extra.insert(
        "tool_calls".to_string(),
        json!([
            {
                "id": "toolu_1",
                "type": "function",
                "function": {
                    "name": "lookup",
                    "arguments": "{\"key\":\"value\"}"
                }
            },
            {
                "id": "toolu_2",
                "type": "function",
                "function": {
                    "name": "notify",
                    "arguments": "{}"
                }
            }
        ]),
    );
    let request = chat_request(vec![
        CoreChatMessage {
            role: "user".to_string(),
            content: json!("start"),
            name: None,
            extra: BTreeMap::new(),
        },
        CoreChatMessage {
            role: "assistant".to_string(),
            content: json!([
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "lookup",
                    "input": {"key": "value"}
                },
                {
                    "type": "server_tool_use",
                    "id": "srvtoolu_1",
                    "name": "tool_search",
                    "input": {"query": "weather"}
                }
            ]),
            name: None,
            extra: assistant_extra,
        },
    ]);

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert_eq!(
        mapped["messages"][1]["content"],
        json!([
            {
                "type": "tool_use",
                "id": "toolu_1",
                "name": "lookup",
                "input": {"key": "value"}
            },
            {
                "type": "server_tool_use",
                "id": "srvtoolu_1",
                "name": "tool_search",
                "input": {"query": "weather"}
            },
            {
                "type": "tool_use",
                "id": "toolu_2",
                "name": "notify",
                "input": {}
            }
        ])
    );
}
