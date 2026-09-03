use super::*;

fn user(text: &str) -> CoreChatMessage {
    CoreChatMessage {
        role: "user".to_string(),
        content: Value::String(text.to_string()),
        name: None,
        extra: BTreeMap::new(),
    }
}

fn assistant(content: Value, extra: BTreeMap<String, Value>) -> CoreChatMessage {
    CoreChatMessage {
        role: "assistant".to_string(),
        content,
        name: None,
        extra,
    }
}

fn assistant_tool_calls(tool_calls: Value) -> CoreChatMessage {
    let mut extra = BTreeMap::new();
    extra.insert("tool_calls".to_string(), tool_calls);
    assistant(Value::Null, extra)
}

fn tool_result(tool_call_id: &str, content: &str) -> CoreChatMessage {
    let mut extra = BTreeMap::new();
    extra.insert("tool_call_id".to_string(), json!(tool_call_id));
    CoreChatMessage {
        role: "tool".to_string(),
        content: Value::String(content.to_string()),
        name: None,
        extra,
    }
}

fn lookup_tool_call(id: &str) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": { "name": "lookup", "arguments": "{\"city\":\"London\"}" }
    })
}

fn lookup_openai_tool() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "lookup",
            "description": "Look up weather",
            "strict": true,
            "parameters": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
                "additionalProperties": false
            }
        }
    }])
}

fn invalid_request_containing(error: ProviderError, needle: &str) {
    match error {
        ProviderError::InvalidRequest(message) => assert!(
            message.contains(needle),
            "expected `{needle}` in `{message}`"
        ),
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
}

#[test]
fn maps_openai_tools_tool_calls_and_tool_results_to_google_payload() {
    let mut request = chat_request(vec![
        user("weather?"),
        assistant_tool_calls(json!([lookup_tool_call("call_123")])),
        tool_result("call_123", "{\"temp\": 20}"),
    ]);
    request
        .extra
        .insert("tools".to_string(), lookup_openai_tool());
    request.extra.insert(
        "tool_choice".to_string(),
        json!({"type":"function","function":{"name":"lookup"}}),
    );

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    let declaration = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(declaration["name"], "lookup");
    assert_eq!(declaration["description"], "Look up weather");
    assert_eq!(
        declaration["parametersJsonSchema"],
        json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": false
        })
    );
    assert!(declaration.get("parameters").is_none());
    assert!(declaration.get("strict").is_none());
    assert_eq!(
        body["toolConfig"]["functionCallingConfig"],
        json!({ "mode": "ANY", "allowedFunctionNames": ["lookup"] })
    );

    assert_eq!(body["contents"][1]["role"], "model");
    let model_parts = body["contents"][1]["parts"].as_array().expect("parts");
    assert_eq!(model_parts.len(), 1, "no empty text part beside tool calls");
    assert_eq!(
        model_parts[0]["functionCall"],
        json!({ "id": "call_123", "name": "lookup", "args": { "city": "London" } })
    );

    assert_eq!(body["contents"][2]["role"], "user");
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"],
        json!({ "id": "call_123", "name": "lookup", "response": { "temp": 20 } })
    );
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn maps_anthropic_tools_calls_and_results_to_google_payload() {
    let mut request = chat_request(vec![
        user("weather?"),
        assistant(
            json!([{
                "type": "tool_use",
                "id": "toolu_123",
                "name": "lookup",
                "input": {"city": "London"}
            }]),
            BTreeMap::new(),
        ),
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

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    let declaration = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(declaration["parametersJsonSchema"]["required"][0], "city");
    assert!(declaration.get("input_schema").is_none());
    assert_eq!(
        body["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
        "lookup"
    );
    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"],
        json!({ "id": "toolu_123", "name": "lookup", "args": { "city": "London" } })
    );
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"],
        json!({ "id": "toolu_123", "name": "lookup", "response": { "temp": 20 } })
    );
}

#[test]
fn rejects_anthropic_tools_without_input_schema() {
    let mut request = chat_request(vec![user("weather?")]);
    request
        .extra
        .insert("tools".to_string(), json!([{ "name": "lookup" }]));

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("anthropic tools need a schema");
    invalid_request_containing(error, "input_schema");
}

#[test]
fn omits_function_ids_for_models_before_gemini_3_5() {
    let request = chat_request(vec![
        user("weather?"),
        assistant_tool_calls(json!([lookup_tool_call("call_123")])),
        tool_result("call_123", "{\"temp\": 20}"),
    ]);

    let body = google_body(&request, &context("google/gemini-2.5-flash"), false).expect("mapped");

    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"],
        json!({ "name": "lookup", "args": { "city": "London" } })
    );
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"],
        json!({ "name": "lookup", "response": { "temp": 20 } })
    );
}

#[test]
fn maps_openai_tool_results_without_name_and_coalesces_parallel_turns() {
    let request = chat_request(vec![
        user("check both"),
        assistant_tool_calls(json!([
            {
                "id": "call_1",
                "type": "function",
                "function": { "name": "lookup_weather", "arguments": "{\"city\":\"London\"}" }
            },
            {
                "id": "call_2",
                "type": "function",
                "function": { "name": "lookup_time", "arguments": "{\"city\":\"London\"}" }
            }
        ])),
        tool_result("call_1", "{\"temp\": 20}"),
        tool_result("call_2", "{\"time\": \"12:00\"}"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    assert_eq!(body["contents"].as_array().expect("contents").len(), 3);
    assert_eq!(body["contents"][2]["role"], "user");
    let parts = body["contents"][2]["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["functionResponse"]["id"], "call_1");
    assert_eq!(parts[0]["functionResponse"]["name"], "lookup_weather");
    assert_eq!(parts[0]["functionResponse"]["response"]["temp"], 20);
    assert_eq!(parts[1]["functionResponse"]["id"], "call_2");
    assert_eq!(parts[1]["functionResponse"]["name"], "lookup_time");
    assert_eq!(parts[1]["functionResponse"]["response"]["time"], "12:00");
}

#[test]
fn wraps_non_object_tool_results_in_output() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([
            lookup_tool_call("call_1"),
            lookup_tool_call("call_2")
        ])),
        tool_result("call_1", "plain text result"),
        tool_result("call_2", "[1, 2]"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    let parts = body["contents"][2]["parts"].as_array().expect("parts");
    assert_eq!(
        parts[0]["functionResponse"]["response"],
        json!({ "output": "plain text result" })
    );
    assert_eq!(
        parts[1]["functionResponse"]["response"],
        json!({ "output": [1, 2] })
    );
}

#[test]
fn rejects_google_tool_result_without_a_preceding_matching_call() {
    let request = chat_request(vec![
        tool_result("call_later", "result"),
        assistant_tool_calls(json!([{
            "id": "call_later",
            "type": "function",
            "function": { "name": "lookup", "arguments": "{}" }
        }])),
    ]);

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("future tool calls must not resolve earlier tool results");
    invalid_request_containing(error, "unknown tool call id `call_later`");
}

#[test]
fn rejects_tool_messages_without_tool_call_id() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([lookup_tool_call("call_1")])),
        CoreChatMessage {
            role: "tool".to_string(),
            content: Value::String("result".to_string()),
            name: Some("lookup".to_string()),
            extra: BTreeMap::new(),
        },
    ]);

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("tool messages need tool_call_id");
    invalid_request_containing(error, "tool_call_id");
}

#[test]
fn maps_string_tool_choices_to_function_calling_modes() {
    for (choice, mode) in [
        ("auto", "AUTO"),
        ("required", "ANY"),
        ("any", "ANY"),
        ("validated", "VALIDATED"),
    ] {
        let mut request = chat_request(vec![user("use a tool")]);
        request
            .extra
            .insert("tools".to_string(), lookup_openai_tool());
        request
            .extra
            .insert("tool_choice".to_string(), json!(choice));

        let body = google_body(&request, &context("google/gemini-3.7-flash"), false)
            .unwrap_or_else(|error| panic!("tool_choice {choice}: {error:?}"));

        assert_eq!(
            body["toolConfig"]["functionCallingConfig"],
            json!({ "mode": mode }),
            "tool_choice {choice}"
        );
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"], "lookup",
            "tool_choice {choice} keeps tools"
        );
    }
}

#[test]
fn none_tool_choice_disables_tools_for_google_payload() {
    let mut request = chat_request(vec![user("do not use tools")]);
    request
        .extra
        .insert("tools".to_string(), lookup_openai_tool());
    request
        .extra
        .insert("tool_choice".to_string(), json!("none"));

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    assert_eq!(
        body["toolConfig"]["functionCallingConfig"],
        json!({ "mode": "NONE" })
    );
    assert!(body.get("tools").is_none());
}

#[test]
fn rejects_unsupported_tool_choice() {
    let mut request = chat_request(vec![user("go")]);
    request
        .extra
        .insert("tool_choice".to_string(), json!("banana"));

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("unknown tool_choice");
    invalid_request_containing(error, "unsupported tool_choice");
}

#[test]
fn handles_google_parallel_tool_calls_locally() {
    let mut request = chat_request(vec![user("use a tool")]);
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

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("disabled parallel calls are not supported");
    invalid_request_containing(error, "parallel_tool_calls: false");

    request
        .extra
        .insert("parallel_tool_calls".to_string(), json!(true));
    let body = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect("parallel calls are supported");
    assert!(body.get("parallel_tool_calls").is_none());
    assert_eq!(
        body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"],
        json!({"type": "object"})
    );
}

#[test]
fn replays_tool_call_thought_signature_onto_function_call_part() {
    let request = chat_request(vec![
        user("weather?"),
        assistant_tool_calls(json!([
            {
                "id": "call_1",
                "type": "function",
                "thought_signature": "sig-1",
                "function": { "name": "lookup", "arguments": "{\"city\":\"London\"}" }
            },
            {
                "id": "call_2",
                "type": "function",
                "function": { "name": "lookup", "arguments": "{\"city\":\"Paris\"}" }
            }
        ])),
        tool_result("call_1", "{}"),
        tool_result("call_2", "{}"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    let parts = body["contents"][1]["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0]["thoughtSignature"], "sig-1");
    assert_eq!(parts[0]["functionCall"]["id"], "call_1");
    assert!(parts[1].get("thoughtSignature").is_none());
    assert_eq!(parts[1]["functionCall"]["args"]["city"], "Paris");
}

#[test]
fn replays_message_thought_signature_onto_text_part() {
    let mut extra = BTreeMap::new();
    extra.insert("thought_signature".to_string(), json!("sig-text"));
    extra.insert(
        "tool_calls".to_string(),
        json!([lookup_tool_call("call_1")]),
    );
    let request = chat_request(vec![
        user("weather?"),
        assistant(Value::String("Checking.".to_string()), extra),
        tool_result("call_1", "{}"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    let parts = body["contents"][1]["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[0],
        json!({ "text": "Checking.", "thoughtSignature": "sig-text" })
    );
    assert!(
        parts[1].get("thoughtSignature").is_none(),
        "message-level signature must not leak onto the function call"
    );
}

#[test]
fn replays_provider_metadata_thought_signature_onto_text_part() {
    let mut extra = BTreeMap::new();
    extra.insert(
        "provider_metadata".to_string(),
        json!({ "gcp_vertex": { "thought_signature": "sig-meta" } }),
    );
    let request = chat_request(vec![
        user("hi"),
        assistant(Value::String("Hello.".to_string()), extra),
        user("again"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    assert_eq!(
        body["contents"][1]["parts"],
        json!([{ "text": "Hello.", "thoughtSignature": "sig-meta" }])
    );
}

#[test]
fn message_thought_signature_without_text_gets_a_signed_empty_text_part() {
    let mut extra = BTreeMap::new();
    extra.insert("thought_signature".to_string(), json!("sig-only"));
    extra.insert(
        "tool_calls".to_string(),
        json!([lookup_tool_call("call_1")]),
    );
    let request = chat_request(vec![
        user("weather?"),
        assistant(Value::Null, extra),
        tool_result("call_1", "{}"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    let parts = body["contents"][1]["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts[0],
        json!({ "text": "", "thoughtSignature": "sig-only" })
    );
    assert_eq!(parts[1]["functionCall"]["id"], "call_1");
}

#[test]
fn ignores_empty_message_thought_signature() {
    let mut extra = BTreeMap::new();
    extra.insert("thought_signature".to_string(), json!(""));
    let request = chat_request(vec![
        user("hi"),
        assistant(Value::String("Hello.".to_string()), extra),
        user("again"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    assert_eq!(body["contents"][1]["parts"], json!([{ "text": "Hello." }]));
}

#[test]
fn tool_call_arguments_default_to_empty_object_when_absent() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([{
            "id": "call_1",
            "type": "function",
            "function": { "name": "ping" }
        }])),
        tool_result("call_1", "{}"),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"]["args"],
        json!({})
    );
}

#[test]
fn rejects_tool_call_arguments_that_are_not_json_strings() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([{
            "id": "call_1",
            "type": "function",
            "function": { "name": "lookup", "arguments": { "city": "London" } }
        }])),
    ]);

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("object arguments are not the OpenAI wire shape");
    invalid_request_containing(error, "must be a JSON string");
}

#[test]
fn rejects_tool_call_arguments_that_are_malformed_json() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([{
            "id": "call_1",
            "type": "function",
            "function": { "name": "lookup", "arguments": "{\"city\": " }
        }])),
    ]);

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("truncated arguments are rejected");
    invalid_request_containing(error, "must contain valid JSON");
}

#[test]
fn rejects_non_function_tool_calls() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([{
            "id": "call_1",
            "type": "custom",
            "custom": { "name": "lookup", "input": "x" }
        }])),
    ]);

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("only function tool calls are mapped");
    invalid_request_containing(error, "only function tool_calls");
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
                    },
                    "thoughtSignature":"sig-call"
                }]
            },
            "finishReason":"STOP"
        }],
        "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":7,"totalTokenCount":12}
    });

    let normalized = normalize_google_response(&response, &context("google/gemini-3.7-flash"))
        .expect("normalized");

    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    let tool_calls = normalized["choices"][0]["message"]["tool_calls"]
        .as_array()
        .expect("tool_calls array");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"], "provider-call-123");
    assert_eq!(tool_calls[0]["type"], "function");
    assert_eq!(tool_calls[0]["function"]["name"], "lookup");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        "{\"city\":\"London\"}"
    );
    assert_eq!(tool_calls[0]["thought_signature"], "sig-call");
}

#[test]
fn synthesizes_tool_call_ids_when_google_omits_them() {
    let response = json!({
        "candidates":[{
            "content":{ "parts":[{ "functionCall":{ "name":"lookup", "args":{} } }] },
            "finishReason":"STOP"
        }]
    });

    let normalized = normalize_google_response(&response, &context("google/gemini-2.5-flash"))
        .expect("normalized");

    let id = normalized["choices"][0]["message"]["tool_calls"][0]["id"]
        .as_str()
        .expect("tool call id");
    assert!(id.starts_with("call_") && id.len() > "call_".len());
}

#[test]
fn malformed_google_function_calls_are_rejected() {
    let response = json!({
        "candidates":[{
            "content":{
                "parts":[{ "functionCall":{ "name":"lookup", "args":{"city":"London"} } }]
            },
            "finishReason":"MALFORMED_FUNCTION_CALL"
        }]
    });

    let error = normalize_google_response(&response, &context("google/gemini-3.7-flash"))
        .expect_err("malformed function calls must not be exposed as tool calls");
    assert!(matches!(
        error,
        VertexAdapterError::MalformedFunctionCall(_)
    ));
    assert!(matches!(
        ProviderError::from(error),
        ProviderError::Transport(_)
    ));
}
