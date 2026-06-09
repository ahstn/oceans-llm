use super::*;

#[test]
fn maps_function_tools_and_tool_choice() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Check weather")],
        stream: false,
        extra: BTreeMap::from([
            (
                "tools".to_string(),
                json!([{
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                            "required": ["city"]
                        }
                    }
                }]),
            ),
            (
                "tool_choice".to_string(),
                json!({"type":"function","function":{"name":"get_weather"}}),
            ),
        ]),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");

    assert_eq!(
        body["toolConfig"],
        json!({
            "tools": [{
                "toolSpec": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "inputSchema": {
                        "json": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                            "required": ["city"]
                        }
                    }
                }
            }],
            "toolChoice": {"tool": {"name": "get_weather"}}
        })
    );
}

#[test]
fn omits_converse_tool_config_when_tool_choice_is_none() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Do not call tools")],
        stream: false,
        extra: BTreeMap::from([
            (
                "tools".to_string(),
                json!([{
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "parameters": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}}
                        }
                    }
                }]),
            ),
            ("tool_choice".to_string(), json!("none")),
        ]),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");

    assert!(body.get("toolConfig").is_none());
}

#[test]
fn maps_anthropic_function_tools_tool_choice_and_tool_results() {
    let mut assistant = message("assistant", "Calling tool");
    assistant.extra.insert(
        "tool_calls".to_string(),
        json!([{
            "id": "toolu_123",
            "type": "function",
            "function": {
                "name": "get_weather",
                "arguments": "{\"city\":\"London\"}"
            }
        }]),
    );
    let mut tool = message("tool", "12 C");
    tool.extra
        .insert("tool_call_id".to_string(), json!("toolu_123"));
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Check weather"), assistant, tool],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(256)),
            (
                "tools".to_string(),
                json!([{
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                            "required": ["city"]
                        }
                    }
                }]),
            ),
            (
                "tool_choice".to_string(),
                json!({"type":"function","function":{"name":"get_weather"}}),
            ),
        ]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect("mapped");

    assert_eq!(
        body["tools"],
        json!([{
            "name": "get_weather",
            "description": "Get weather",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }])
    );
    assert_eq!(
        body["tool_choice"],
        json!({"type": "tool", "name": "get_weather"})
    );
    assert_eq!(
        body["messages"][1]["content"][1],
        json!({
            "type": "tool_use",
            "id": "toolu_123",
            "name": "get_weather",
            "input": {"city": "London"}
        })
    );
    assert_eq!(
        body["messages"][2]["content"][0],
        json!({
            "type": "tool_result",
            "tool_use_id": "toolu_123",
            "content": "12 C"
        })
    );
}
