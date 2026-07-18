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
fn omits_strict_only_for_affected_claude_opus_models() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Return structured data")],
        stream: false,
        extra: BTreeMap::from([(
            "tools".to_string(),
            json!([{
                "type": "function",
                "function": {
                    "name": "emit_result",
                    "strict": true,
                    "parameters": {"type": "object", "properties": {}}
                }
            }]),
        )]),
    };

    for model in [
        "global.anthropic.claude-opus-4-7-v1:0",
        "anthropic.claude-opus-4-8-v1:0",
    ] {
        let body = map_chat_request_to_converse(&request, &context(model)).expect("mapped");
        assert!(
            body["toolConfig"]["tools"][0]["toolSpec"]
                .get("strict")
                .is_none(),
            "strict should be omitted for {model}"
        );
    }

    for model in [
        "us.anthropic.claude-sonnet-4-6-v1:0",
        "us.anthropic.claude-opus-4-70-v1:0",
    ] {
        let supported = map_chat_request_to_converse(&request, &context(model)).expect("mapped");
        assert_eq!(
            supported["toolConfig"]["tools"][0]["toolSpec"]["strict"],
            json!(true),
            "strict should be retained for {model}"
        );
    }
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
fn maps_rich_converse_tool_result_content() {
    let mut tool = CoreChatMessage {
        role: "tool".to_string(),
        content: json!([
            {"type": "text", "text": "summary"},
            {
                "type": "file",
                "data": "aW1hZ2U=",
                "media_type": "image/png",
                "filename": "chart.png"
            },
            {
                "type": "file",
                "data": "ZmlsZQ==",
                "media_type": "application/pdf",
                "filename": "Quarterly_Report!.pdf"
            },
            {"type": "file", "data": "ZmlsZQ==", "media_type": "text/csv", "filename": "data.csv"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "application/msword", "filename": "memo.doc"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document", "filename": "memo.docx"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "application/vnd.ms-excel", "filename": "sheet.xls"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "filename": "sheet.xlsx"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "text/html", "filename": "page.html"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "text/markdown", "filename": "notes.md"},
            {"type": "file", "data": "ZmlsZQ==", "media_type": "text/plain", "filename": "notes.txt"},
            {"type": "json", "json": {"ok": true}}
        ]),
        name: None,
        extra: BTreeMap::new(),
    };
    tool.extra
        .insert("tool_call_id".to_string(), json!("toolu_rich"));
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![tool],
        stream: false,
        extra: BTreeMap::new(),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    let content = body["messages"][0]["content"][0]["toolResult"]["content"]
        .as_array()
        .expect("tool result content");

    assert_eq!(content[0], json!({"text": "summary"}));
    assert_eq!(
        content[1],
        json!({"image": {"format": "png", "source": {"bytes": "aW1hZ2U="}}})
    );
    let formats = content[2..11]
        .iter()
        .map(|block| block["document"]["format"].as_str().expect("format"))
        .collect::<Vec<_>>();
    assert_eq!(
        formats,
        [
            "pdf", "csv", "doc", "docx", "xls", "xlsx", "html", "md", "txt"
        ]
    );
    assert_eq!(content[2]["document"]["name"], json!("Quarterly Report"));
    assert_eq!(content[11], json!({"json": {"ok": true}}));
}

#[test]
fn normalizes_replayed_bedrock_tool_ids_consistently() {
    let invalid_id = "copilot.tool:call";
    let long_id = "x".repeat(65);
    let native_id = "toolu_native-123";
    let mut assistant = message("assistant", "");
    assistant.extra.insert(
        "tool_calls".to_string(),
        json!([
            {
                "id": invalid_id,
                "type": "function",
                "function": {"name": "lookup", "arguments": "{}"}
            },
            {
                "id": long_id,
                "type": "function",
                "function": {"name": "lookup", "arguments": "{}"}
            },
            {
                "id": native_id,
                "type": "function",
                "function": {"name": "lookup", "arguments": "{}"}
            }
        ]),
    );
    let mut invalid_result = message("tool", "invalid result");
    invalid_result
        .extra
        .insert("tool_call_id".to_string(), json!(invalid_id));
    let mut long_result = message("tool", "long result");
    long_result
        .extra
        .insert("tool_call_id".to_string(), json!(long_id));
    let mut native_result = message("tool", "native result");
    native_result
        .extra
        .insert("tool_call_id".to_string(), json!(native_id));
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![
            message("user", "Use tools"),
            assistant,
            invalid_result,
            long_result,
            native_result,
        ],
        stream: false,
        extra: BTreeMap::new(),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    for (call_index, result_index) in [(1, 2), (2, 3)] {
        let normalized_call_id = body["messages"][1]["content"][call_index]["toolUse"]["toolUseId"]
            .as_str()
            .expect("normalized tool id");
        let normalized_result_id =
            body["messages"][result_index]["content"][0]["toolResult"]["toolUseId"]
                .as_str()
                .expect("normalized result id");
        assert_eq!(normalized_call_id, normalized_result_id);
        assert!(normalized_call_id.starts_with("tool_"));
        assert!(normalized_call_id.len() <= 64);
    }
    assert_eq!(
        body["messages"][1]["content"][3]["toolUse"]["toolUseId"],
        json!(native_id)
    );
    assert_eq!(
        body["messages"][4]["content"][0]["toolResult"]["toolUseId"],
        json!(native_id)
    );
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
