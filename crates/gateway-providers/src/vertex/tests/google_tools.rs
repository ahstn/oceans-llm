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
    for choice in [json!("none"), json!({"type": "none"})] {
        let mut request = chat_request(vec![user("do not use tools")]);
        request
            .extra
            .insert("tools".to_string(), lookup_openai_tool());
        request
            .extra
            .insert("tool_choice".to_string(), choice.clone());

        let body = google_body(&request, &context("google/gemini-3.7-flash"), false)
            .unwrap_or_else(|error| panic!("tool_choice {choice}: {error:?}"));
        assert_eq!(
            body["toolConfig"]["functionCallingConfig"]["mode"], "NONE",
            "tool_choice {choice}"
        );
    }
}

#[test]
fn none_tool_choice_string_removes_tools_from_google_payload() {
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
fn empty_tool_call_arguments_map_to_an_empty_object() {
    let request = chat_request(vec![
        user("go"),
        assistant_tool_calls(json!([{
            "id": "call_1",
            "type": "function",
            "function": { "name": "lookup", "arguments": "  " }
        }])),
    ]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");
    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"]["args"],
        json!({})
    );
}

#[test]
fn rejects_unsupported_tool_entries_instead_of_dropping_them() {
    let mut request = chat_request(vec![user("search")]);
    request
        .extra
        .insert("tools".to_string(), json!([{ "type": "retrieval" }]));

    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("unknown tool shapes are rejected");
    invalid_request_containing(error, "tool");
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

#[test]
fn idless_parallel_results_follow_call_ids_even_when_function_names_match() {
    let request = chat_request(vec![
        user("compare cities"),
        assistant_tool_calls(json!([
            { "id": "london", "type": "function", "function": { "name": "lookup", "arguments": "{\"city\":\"London\"}" } },
            { "id": "paris", "type": "function", "function": { "name": "lookup", "arguments": "{\"city\":\"Paris\"}" } }
        ])),
        tool_result("paris", "{\"city\":\"Paris\",\"temperature\":21}"),
        tool_result("london", "{\"city\":\"London\",\"temperature\":16}"),
        user("summarize"),
    ]);
    let body = google_body(&request, &context("google/gemini-2.5-flash"), false).expect("mapped");
    let results = &body["contents"][2]["parts"];
    assert_eq!(results[0]["functionResponse"]["response"]["city"], "London");
    assert_eq!(results[1]["functionResponse"]["response"]["city"], "Paris");
    assert!(results[0]["functionResponse"].get("id").is_none());
    assert_eq!(body["contents"][3]["parts"][0]["text"], "summarize");
}

#[test]
fn idless_incomplete_result_groups_are_not_fabricated_or_moved_across_turns() {
    let request = chat_request(vec![
        user("compare"),
        assistant_tool_calls(json!([
            lookup_tool_call("first"),
            lookup_tool_call("second")
        ])),
        tool_result("second", "{\"result\":\"second\"}"),
        user("pause"),
        tool_result("first", "{\"result\":\"first\"}"),
    ]);
    let body = google_body(&request, &context("google/gemini-2.5-flash"), false).expect("mapped");
    assert_eq!(body["contents"][2]["parts"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"]["response"]["result"],
        "second"
    );
    assert_eq!(body["contents"][3]["parts"][0]["text"], "pause");
    assert_eq!(
        body["contents"][4]["parts"][0]["functionResponse"]["response"]["result"],
        "first"
    );
}

fn ordered_signed_response() -> Value {
    json!({ "candidates": [{
        "content": { "parts": [
            { "text": "consider", "thought": true, "thoughtSignature": "sig-thought" },
            { "text": "before", "thoughtSignature": "sig-before" },
            { "functionCall": { "name": "lookup", "args": { "city": "London" } }, "thoughtSignature": "sig-call" },
            { "futureServerPart": { "opaque": [1, 2] }, "thoughtSignature": "sig-future" },
            { "text": "after", "thoughtSignature": "sig-after" }
        ] },
        "finishReason": "STOP"
    }] })
}

#[test]
fn json_replay_preserves_ordered_signed_parts_and_opaque_future_parts() {
    let response = ordered_signed_response();
    let normalized =
        normalize_google_response(&response, &context("google/gemini-2.5-flash")).unwrap();
    let message: CoreChatMessage =
        serde_json::from_value(normalized["choices"][0]["message"].clone()).unwrap();
    let id = message.extra["tool_calls"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let request = chat_request(vec![user("look up"), message, tool_result(&id, "{}")]);
    let body = google_body(&request, &context("google/gemini-2.5-flash"), false).expect("replayed");
    assert_eq!(
        body["contents"][1]["parts"],
        response["candidates"][0]["content"]["parts"]
    );
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"]["name"],
        "lookup"
    );
}

#[test]
fn native_replay_keeps_existing_model_function_id_policy_for_generated_chat_ids() {
    let response = json!({ "candidates": [{
        "content": { "parts": [{
            "functionCall": { "name": "lookup", "args": {} }, "thoughtSignature": "sig-call"
        }] },
        "finishReason": "STOP"
    }] });
    let normalized =
        normalize_google_response(&response, &context("google/gemini-3.7-flash")).unwrap();
    let message: CoreChatMessage =
        serde_json::from_value(normalized["choices"][0]["message"].clone()).unwrap();
    let id = message.extra["tool_calls"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let request = chat_request(vec![message, tool_result(&id, "{}")]);
    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("replayed");
    assert_eq!(body["contents"][0]["parts"][0]["functionCall"]["id"], id);
    assert_eq!(
        body["contents"][1]["parts"][0]["functionResponse"]["id"],
        id
    );
    assert_eq!(
        body["contents"][0]["parts"][0]["thoughtSignature"],
        "sig-call"
    );
}

#[test]
fn replay_metadata_cannot_override_edited_public_text_reasoning_or_calls() {
    let normalized = normalize_google_response(
        &ordered_signed_response(),
        &context("google/gemini-2.5-flash"),
    )
    .unwrap();
    let original = normalized["choices"][0]["message"].clone();
    let mut edits = Vec::new();
    let mut edited = original.clone();
    edited["content"] = json!("different prompt");
    edits.push(edited);
    let mut edited = original.clone();
    edited["reasoning_content"] = json!("different reasoning");
    edits.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"][0]["id"] = json!("different-id");
    edits.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"][0]["function"]["name"] = json!("delete_everything");
    edits.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"][0]["function"]["arguments"] = json!("{\"city\":\"Paris\"}");
    edits.push(edited);
    let mut edited = original.clone();
    edited["provider_metadata"]["gcp_vertex"]["native_content"]["parts"][1]["text"] =
        json!("smuggled prompt");
    edits.push(edited);
    for edited in edits {
        let message: CoreChatMessage = serde_json::from_value(edited).unwrap();
        let request = chat_request(vec![user("look up"), message]);
        let error = google_body(&request, &context("google/gemini-2.5-flash"), false)
            .expect_err("stale or forged metadata must not win over Chat");
        invalid_request_containing(error, "replay metadata");
    }
}

#[test]
fn replay_native_function_ids_cannot_be_rebound_by_forged_metadata() {
    let response = json!({ "candidates": [{
        "content": { "parts": [{
            "functionCall": { "id": "provider-id", "name": "lookup", "args": {} },
            "thoughtSignature": "signed"
        }] },
        "finishReason": "STOP"
    }] });
    let normalized =
        normalize_google_response(&response, &context("google/gemini-3.7-flash")).unwrap();
    let mut message = normalized["choices"][0]["message"].clone();
    message["tool_calls"][0]["id"] = json!("forged-id");
    message["provider_metadata"]["gcp_vertex"]["native_content"]["tool_call_ids"][0] =
        json!("forged-id");
    let request = chat_request(vec![serde_json::from_value(message).unwrap()]);
    let error = google_body(&request, &context("google/gemini-3.7-flash"), false)
        .expect_err("the ID on the native call must also match the public ID");
    invalid_request_containing(error, "replay metadata");
}

#[test]
fn google_replay_ignores_foreign_vertex_anthropic_native_content() {
    let mut extra = BTreeMap::new();
    extra.insert(
        "provider_metadata".into(),
        json!({ "gcp_vertex": {
        "native_content": { "source": "anthropic_messages", "blocks": [
            { "type": "text", "text": "earlier answer" }
        ] }
    } }),
    );
    let request = chat_request(vec![
        user("hello"),
        assistant(json!("earlier answer"), extra),
        user("continue"),
    ]);
    let body = google_body(&request, &context("google/gemini-2.5-flash"), false)
        .expect("foreign provider history uses its public Chat projection");
    assert_eq!(
        body["contents"][1]["parts"],
        json!([{ "text": "earlier answer" }])
    );
}

#[test]
fn strict_function_control_is_rejected_before_tool_choice_can_discard_it() {
    for choice in [
        json!("auto"),
        json!("required"),
        json!("none"),
        json!({ "type": "function", "function": { "name": "lookup" } }),
    ] {
        let mut request = chat_request(vec![user("lookup")]);
        let mut tools = lookup_openai_tool();
        tools[0]["function"]["strict"] = json!(true);
        request.extra.insert("tools".to_string(), tools);
        request.extra.insert("tool_choice".to_string(), choice);
        let mut ctx = context("google/gemini-3.7-flash");
        ctx.extra_body.insert(
            "toolConfig".to_string(),
            json!({ "functionCallingConfig": { "mode": "AUTO" } }),
        );
        let error = google_body(&request, &ctx, false).expect_err("strict is not guaranteed");
        invalid_request_containing(error, "`strict: true` is not supported");
    }
}

#[test]
fn route_tool_overrides_cannot_silently_erase_requested_strict_semantics() {
    let mut request = chat_request(vec![user("lookup")]);
    let mut tools = lookup_openai_tool();
    tools[0]["function"]["strict"] = json!(true);
    request.extra.insert("tools".into(), tools);
    let mut ctx = context("google/gemini-2.5-flash");
    ctx.extra_body.insert("tools".into(), json!([]));
    let error = google_body(&request, &ctx, false).expect_err("strict cannot silently disappear");
    invalid_request_containing(error, "`strict: true` is not supported");
}

#[test]
fn non_strict_function_schema_preserves_const_and_auto_control() {
    let mut request = chat_request(vec![user("lookup")]);
    request.extra.insert(
        "tools".to_string(),
        json!([{
            "type": "function",
            "function": { "name": "lookup", "strict": false, "parameters": {
                "type": "object", "properties": { "city": { "const": "London" } }
            } }
        }]),
    );
    request
        .extra
        .insert("tool_choice".to_string(), json!("auto"));
    let body = google_body(&request, &context("google/gemini-2.5-flash"), false)
        .expect("non-strict mapping");
    assert_eq!(
        body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["properties"]["city"],
        json!({ "const": "London" })
    );
    assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
}
