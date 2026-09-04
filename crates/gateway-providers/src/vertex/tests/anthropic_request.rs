use super::*;

fn user(text: &str) -> CoreChatMessage {
    CoreChatMessage {
        role: "user".to_string(),
        content: Value::String(text.to_string()),
        name: None,
        extra: BTreeMap::new(),
    }
}

fn beta_header(value: &str) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    headers.insert("anthropic-beta".to_string(), value.to_string());
    headers
}

#[test]
fn maps_openai_request_to_vertex_anthropic_payload() {
    let request = chat_request(vec![
        CoreChatMessage {
            role: "system".to_string(),
            content: Value::String("be concise".to_string()),
            name: None,
            extra: BTreeMap::new(),
        },
        user("ping"),
    ]);

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

    assert_eq!(mapped["anthropic_version"], "vertex-2023-10-16");
    assert!(mapped.get("model").is_none());
    assert_eq!(mapped["max_tokens"], 4096);
    assert_eq!(mapped["stream"], false);
    assert_eq!(mapped["messages"][0]["role"], "user");
    assert_eq!(mapped["messages"][0]["content"], "ping");
    assert_eq!(
        mapped["system"],
        json!([{"type": "text", "text": "be concise"}])
    );
    assert!(mapped.get("anthropic_beta").is_none());
}

#[test]
fn aliases_max_completion_tokens_to_max_tokens() {
    let mut request = chat_request(vec![user("ping")]);
    request
        .extra
        .insert("max_completion_tokens".to_string(), json!(512));

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

    assert_eq!(mapped["max_tokens"], 512);
    assert!(mapped.get("max_completion_tokens").is_none());
}

#[test]
fn rejects_conflicting_max_tokens_fields() {
    let mut request = chat_request(vec![user("ping")]);
    request
        .extra
        .insert("max_completion_tokens".to_string(), json!(512));
    request.extra.insert("max_tokens".to_string(), json!(1024));

    let error = anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect_err("conflicting max tokens");

    assert!(matches!(error, ProviderError::InvalidRequest(_)));
    assert!(error.to_string().contains("max_completion_tokens"));
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
        user("weather?"),
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
                "strict": true,
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
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

    assert_eq!(mapped["tools"][0]["name"], "lookup");
    assert_eq!(mapped["tools"][0]["description"], "Look up weather");
    assert_eq!(
        mapped["tools"][0]["input_schema"],
        json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"]
        })
    );
    assert!(mapped["tools"][0].get("parameters").is_none());
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
fn maps_string_tool_choices_for_anthropic_payload() {
    let cases = [
        ("auto", json!({"type": "auto"})),
        ("required", json!({"type": "any"})),
        ("none", json!({"type": "none"})),
    ];
    for (choice, expected) in cases {
        let mut request = chat_request(vec![user("tool choice")]);
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
            .insert("tool_choice".to_string(), json!(choice));

        let mapped =
            anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect(choice);

        assert_eq!(mapped["tool_choice"], expected, "tool_choice {choice}");
    }
}

#[test]
fn maps_openai_image_parts_to_anthropic_base64_blocks() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {"type": "text", "text": "describe"},
            {
                "type": "image_url",
                "image_url": {"url": "data:image/png;base64,QUJD"}
            },
            {
                "type": "image",
                "source": {"type": "base64", "media_type": "image/jpeg", "data": "REVG"}
            }
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

    let content = mapped["messages"][0]["content"]
        .as_array()
        .expect("content array");
    assert_eq!(content.len(), 3);
    assert_eq!(content[0], json!({"type": "text", "text": "describe"}));
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["type"], "base64");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(content[1]["source"]["data"], "QUJD");
    assert_eq!(
        content[2],
        json!({
            "type": "image",
            "source": {"type": "base64", "media_type": "image/jpeg", "data": "REVG"}
        })
    );
}

#[test]
fn rejects_remote_image_urls_for_anthropic_payload() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "image_url",
            "image_url": {"url": "https://example.invalid/cat.png"}
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let error = anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect_err("remote image urls are not supported");

    assert!(matches!(error, ProviderError::InvalidRequest(_)));
}

#[test]
fn route_override_cannot_replace_anthropic_messages_or_stream_mode() {
    let mut request = chat_request(vec![user("mapped message")]);
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

    let mapped = anthropic_body(&request, &context, true).expect("mapped");

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
        user("weather?"),
        CoreChatMessage {
            role: "assistant".to_string(),
            content: Value::Null,
            name: None,
            extra: assistant_extra,
        },
    ]);

    let error = anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false)
        .expect_err("malformed arguments should fail");

    assert!(matches!(error, ProviderError::InvalidRequest(_)));
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

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

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

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

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
        user("start"),
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

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

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
            },
            {
                "id": "toolu_2",
                "type": "function",
                "function": {
                    "name": "duplicate",
                    "arguments": "{}"
                }
            }
        ]),
    );
    let request = chat_request(vec![
        user("start"),
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

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

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

// --- anthropic-beta header -> `anthropic_beta` body array ---------------------------------

#[test]
fn moves_route_beta_header_into_body_array() {
    let request = chat_request(vec![user("ping")]);
    let mut context = context("anthropic/claude-sonnet-4-6");
    context.extra_headers.insert(
        "Anthropic-Beta".to_string(),
        json!("interleaved-thinking-2025-05-14, files-api-2025-04-14 ,"),
    );

    let mapped = anthropic_body(&request, &context, false).expect("mapped");

    assert_eq!(
        mapped["anthropic_beta"],
        json!(["interleaved-thinking-2025-05-14", "files-api-2025-04-14"])
    );
}

#[test]
fn merges_provider_default_and_route_betas_with_caller_array_without_duplicates() {
    let mut request = chat_request(vec![user("ping")]);
    request.extra.insert(
        "anthropic_beta".to_string(),
        json!(["caller-beta-1", "files-api-2025-04-14"]),
    );
    let mut context = context("anthropic/claude-sonnet-4-6");
    context.extra_headers.insert(
        "anthropic-beta".to_string(),
        json!("files-api-2025-04-14,route-beta"),
    );
    let default_headers = beta_header("provider-beta,caller-beta-1");

    let mapped =
        map_vertex_anthropic_request(&request, &context, false, &default_headers).expect("mapped");

    assert_eq!(
        mapped["anthropic_beta"],
        json!([
            "caller-beta-1",
            "files-api-2025-04-14",
            "provider-beta",
            "route-beta"
        ])
    );
}

#[test]
fn accepts_comma_separated_caller_anthropic_beta_string() {
    let mut request = chat_request(vec![user("ping")]);
    request
        .extra
        .insert("anthropic_beta".to_string(), json!("beta-a, beta-b"));

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

    assert_eq!(mapped["anthropic_beta"], json!(["beta-a", "beta-b"]));
}

#[test]
fn rejects_non_string_caller_anthropic_beta() {
    // An object, and an array with a non-string entry, both fail instead of being partially
    // applied.
    for value in [json!({"beta": true}), json!(["files-api-2025-04-14", 1])] {
        let mut request = chat_request(vec![user("ping")]);
        request
            .extra
            .insert("anthropic_beta".to_string(), value.clone());

        let error = match anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false) {
            Ok(_) => panic!("anthropic_beta {value} must fail"),
            Err(error) => error,
        };

        assert!(matches!(error, ProviderError::InvalidRequest(_)), "{value}");
        assert!(error.to_string().contains("anthropic_beta"), "{value}");
    }
}

#[test]
fn appends_effort_beta_for_manual_thinking_with_output_effort() {
    let mut request = chat_request(vec![user("think hard")]);
    request
        .extra
        .insert("reasoning_effort".to_string(), json!("high"));
    request.extra.insert(
        "thinking".to_string(),
        json!({"type": "enabled", "budget_tokens": 2048}),
    );

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-opus-4-5"), false).expect("mapped");

    assert_eq!(
        mapped["thinking"],
        json!({"type": "enabled", "budget_tokens": 2048})
    );
    assert_eq!(mapped["output_config"]["effort"], "high");
    assert!(mapped.get("reasoning_effort").is_none());
    assert_eq!(mapped["anthropic_beta"], json!(["effort-2025-11-24"]));
}

#[test]
fn does_not_duplicate_effort_beta_already_configured_as_header() {
    let mut request = chat_request(vec![user("think hard")]);
    request
        .extra
        .insert("reasoning_effort".to_string(), json!("high"));
    request.extra.insert(
        "thinking".to_string(),
        json!({"type": "enabled", "budget_tokens": 2048}),
    );
    let default_headers = beta_header("effort-2025-11-24");

    let mapped = map_vertex_anthropic_request(
        &request,
        &context("anthropic/claude-opus-4-5"),
        false,
        &default_headers,
    )
    .expect("mapped");

    assert_eq!(mapped["anthropic_beta"], json!(["effort-2025-11-24"]));
}

#[test]
fn omits_effort_beta_for_adaptive_thinking_models() {
    let mut request = chat_request(vec![user("think hard")]);
    request
        .extra
        .insert("reasoning_effort".to_string(), json!("high"));

    let mapped =
        anthropic_body(&request, &context("anthropic/claude-sonnet-4-6"), false).expect("mapped");

    assert_eq!(mapped["thinking"], json!({"type": "adaptive"}));
    assert_eq!(mapped["output_config"]["effort"], "high");
    assert!(mapped.get("anthropic_beta").is_none());
}

// --- context_management gating ---------------------------------------------------------------

#[test]
fn omits_context_management_when_beta_only_appears_in_caller_request_headers() {
    let mut request = chat_request(vec![user("start")]);
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

    let mapped = anthropic_body(&request, &context, false).expect("mapped");

    assert!(mapped.get("context_management").is_none());
    assert!(mapped.get("anthropic_beta").is_none());
}

#[test]
fn omits_route_owned_context_management_without_matching_beta_header() {
    let request = chat_request(vec![user("start")]);
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

    let mapped = anthropic_body(&request, &context, false).expect("mapped");

    assert!(mapped.get("context_management").is_none());
}

#[test]
fn allows_route_owned_context_management_with_matching_route_beta_header() {
    let mut request = chat_request(vec![user("start")]);
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

    let mapped = anthropic_body(&request, &context, false).expect("mapped");

    assert_eq!(
        mapped["context_management"],
        context.extra_body["context_management"]
    );
    assert_eq!(
        mapped["anthropic_beta"],
        json!(["context-management-2025-06-27"])
    );
}

#[test]
fn allows_context_management_with_matching_provider_default_beta_header() {
    let mut request = chat_request(vec![user("start")]);
    request.extra.insert(
        "context_management".to_string(),
        json!({"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]}),
    );
    let default_headers = beta_header("context-management-2025-06-27");

    let mapped = map_vertex_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-6"),
        false,
        &default_headers,
    )
    .expect("mapped");

    assert_eq!(
        mapped["context_management"],
        json!({"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]})
    );
    assert_eq!(
        mapped["anthropic_beta"],
        json!(["context-management-2025-06-27"])
    );
}
