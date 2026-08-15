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
fn omits_openai_none_tool_choice_for_anthropic_payload() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("do not use tools".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("tool_choice".to_string(), json!("none"));

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect("mapped");

    assert!(mapped.get("tool_choice").is_none());
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
