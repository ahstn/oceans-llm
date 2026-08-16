use super::*;

#[test]
fn maps_openai_tools_tool_calls_and_tool_results_to_google_payload() {
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
            content: Value::String("{\"temp\": 20}".to_string()),
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

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");

    assert_eq!(
        mapped["tools"][0]["functionDeclarations"][0]["name"],
        "lookup"
    );
    assert_eq!(
        mapped["tools"][0]["functionDeclarations"][0]["description"],
        "Look up weather"
    );
    assert_eq!(mapped["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
    assert_eq!(
        mapped["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
        "lookup"
    );
    assert_eq!(
        mapped["contents"][1]["parts"][0]["functionCall"]["name"],
        "lookup"
    );
    assert_eq!(
        mapped["contents"][1]["parts"][0]["functionCall"]["id"],
        "call_123"
    );
    assert_eq!(
        mapped["contents"][1]["parts"][0]["functionCall"]["args"]["city"],
        "London"
    );
    assert_eq!(mapped["contents"][2]["role"], "user");
    assert_eq!(
        mapped["contents"][2]["parts"][0]["functionResponse"]["name"],
        "lookup"
    );
    assert_eq!(
        mapped["contents"][2]["parts"][0]["functionResponse"]["id"],
        "call_123"
    );
    assert_eq!(
        mapped["contents"][2]["parts"][0]["functionResponse"]["response"]["temp"],
        20
    );
}

#[test]
fn maps_anthropic_tools_calls_and_results_to_google_payload() {
    let mut request = chat_request(vec![
        CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("weather?".to_string()),
            name: None,
            extra: BTreeMap::new(),
        },
        CoreChatMessage {
            role: "assistant".to_string(),
            content: json!([{
                "type": "tool_use",
                "id": "toolu_123",
                "name": "lookup",
                "input": {"city": "London"}
            }]),
            name: None,
            extra: BTreeMap::new(),
        },
        CoreChatMessage {
            role: "user".to_string(),
            content: json!([{
                "type": "tool_result",
                "tool_use_id": "toolu_123",
                "content": "{\"temp\":20}"
            }]),
            name: None,
            extra: BTreeMap::new(),
        },
    ]);
    request.extra.insert(
        "tools".to_string(),
        json!([{
            "name": "lookup",
            "description": "Look up weather",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }]),
    );
    request.extra.insert(
        "tool_choice".to_string(),
        json!({"type":"tool","name":"lookup"}),
    );

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");

    assert_eq!(
        mapped["tools"][0]["functionDeclarations"][0]["parameters"]["required"][0],
        "city"
    );
    assert_eq!(
        mapped["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
        "lookup"
    );
    assert_eq!(
        mapped["contents"][1]["parts"][0]["functionCall"]["id"],
        "toolu_123"
    );
    assert_eq!(
        mapped["contents"][2]["parts"][0]["functionResponse"]["id"],
        "toolu_123"
    );
    assert_eq!(
        mapped["contents"][2]["parts"][0]["functionResponse"]["response"]["temp"],
        20
    );
}

#[test]
fn maps_openai_tool_results_without_name_and_coalesces_parallel_turns() {
    let mut assistant_extra = BTreeMap::new();
    assistant_extra.insert(
        "tool_calls".to_string(),
        json!([
            {
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "lookup_weather",
                    "arguments": "{\"city\":\"London\"}"
                }
            },
            {
                "id": "call_2",
                "type": "function",
                "function": {
                    "name": "lookup_time",
                    "arguments": "{\"city\":\"London\"}"
                }
            }
        ]),
    );
    let mut tool1_extra = BTreeMap::new();
    tool1_extra.insert("tool_call_id".to_string(), json!("call_1"));
    let mut tool2_extra = BTreeMap::new();
    tool2_extra.insert("tool_call_id".to_string(), json!("call_2"));

    let request = chat_request(vec![
        CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("check both".to_string()),
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
            content: Value::String("{\"temp\": 20}".to_string()),
            name: None,
            extra: tool1_extra,
        },
        CoreChatMessage {
            role: "tool".to_string(),
            content: Value::String("{\"time\": \"12:00\"}".to_string()),
            name: None,
            extra: tool2_extra,
        },
    ]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");

    assert_eq!(mapped["contents"].as_array().expect("contents").len(), 3);
    assert_eq!(mapped["contents"][2]["role"], "user");
    let parts = mapped["contents"][2]["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["functionResponse"]["name"], "lookup_weather");
    assert_eq!(parts[0]["functionResponse"]["response"]["temp"], 20);
    assert_eq!(parts[1]["functionResponse"]["name"], "lookup_time");
    assert_eq!(parts[1]["functionResponse"]["response"]["time"], "12:00");
}

#[test]
fn rejects_google_tool_result_without_a_preceding_matching_call() {
    let mut tool_extra = BTreeMap::new();
    tool_extra.insert("tool_call_id".to_string(), json!("call_later"));
    let mut assistant_extra = BTreeMap::new();
    assistant_extra.insert(
        "tool_calls".to_string(),
        json!([{
            "id": "call_later",
            "type": "function",
            "function": {
                "name": "lookup",
                "arguments": "{}"
            }
        }]),
    );
    let request = chat_request(vec![
        CoreChatMessage {
            role: "tool".to_string(),
            content: Value::String("result".to_string()),
            name: Some("lookup".to_string()),
            extra: tool_extra,
        },
        CoreChatMessage {
            role: "assistant".to_string(),
            content: Value::Null,
            name: None,
            extra: assistant_extra,
        },
    ]);

    let error = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
        .expect_err("future tool calls must not resolve earlier tool results");

    assert!(matches!(
        error,
        ProviderError::InvalidRequest(message)
            if message.contains("unknown `tool_call_id` `call_later`")
    ));
}

#[test]
fn omits_openai_none_tool_choice_for_google_payload() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("do not use tools".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("tool_choice".to_string(), json!("none"));

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");

    assert_eq!(
        mapped["toolConfig"]["functionCallingConfig"]["mode"],
        "NONE"
    );
    assert!(mapped.get("tools").is_none());
}

#[test]
fn maps_google_any_tool_choice_to_any_mode() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("use a tool".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("tool_choice".to_string(), json!("any"));

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");

    assert_eq!(mapped["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
}

#[test]
fn handles_google_parallel_tool_calls_locally() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("use a tool".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "tools".to_string(),
        json!([{
            "type": "function",
            "function": {"name": "lookup", "parameters": {"type": "object"}}
        }]),
    );
    request
        .extra
        .insert("parallel_tool_calls".to_string(), json!(false));

    let error = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
        .expect_err("disabled parallel calls are not supported");
    assert!(matches!(
        error,
        ProviderError::InvalidRequest(message)
            if message.contains("parallel_tool_calls: false")
    ));

    request
        .extra
        .insert("parallel_tool_calls".to_string(), json!(true));
    let mapped = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
        .expect("parallel calls are supported");
    assert!(mapped.get("parallel_tool_calls").is_none());
}

#[test]
fn normalizes_google_function_call_response_into_openai_tool_calls() {
    let response = json!({
        "responseId":"resp_123",
        "candidates":[{
            "index": 0,
            "content":{
                "parts":[{
                    "functionCall":{
                        "name":"lookup",
                        "args":{"city":"London"},
                        "id":"provider-call-123"
                    }
                }]
            },
            "finishReason":"STOP"
        }],
        "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":7,"totalTokenCount":12}
    });
    let normalized = normalize_google_response(&response, &context("google/gemini-2.0-flash"));

    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    let tool_calls = normalized["choices"][0]["message"]["tool_calls"]
        .as_array()
        .expect("tool_calls array");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"], "provider-call-123");
    assert_eq!(tool_calls[0]["function"]["name"], "lookup");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        "{\"city\":\"London\"}"
    );
}

#[test]
fn does_not_expose_malformed_google_function_calls_as_tool_calls() {
    let response = json!({
        "candidates":[{
            "content":{
                "parts":[{
                    "functionCall":{
                        "name":"lookup",
                        "args":{"city":"London"}
                    }
                }]
            },
            "finishReason":"MALFORMED_FUNCTION_CALL"
        }]
    });

    let normalized = normalize_google_response(&response, &context("google/gemini-2.0-flash"));

    assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    assert!(
        normalized["choices"][0]["message"]
            .get("tool_calls")
            .is_none()
    );
}
