use gateway_core::protocol::anthropic::AnthropicMessagesRequest;
use gateway_core::{CoreChatRequest, ProviderRequestContext, anthropic_messages_request_to_core};
use serde_json::{Value, json};

use super::request::{AnthropicRequestOptions, map_anthropic_request};

fn context() -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "request-test".into(),
        model_key: "claude".into(),
        provider_key: "zen".into(),
        upstream_model: "claude-sonnet-4-6".into(),
        owner_user_id: None,
        expected_provider_credential_id: None,
        extra_headers: Default::default(),
        extra_body: Default::default(),
        request_headers: Default::default(),
        compatibility: Default::default(),
    }
}

fn chat(extra: Value) -> CoreChatRequest {
    let mut request: CoreChatRequest = serde_json::from_value(json!({
        "model": "claude", "messages": [{"role": "user", "content": "hello"}]
    }))
    .unwrap();
    request.extra.extend(
        extra
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    request
}

fn native_messages_request(extra: Value) -> CoreChatRequest {
    let mut payload = json!({
        "model": "claude", "max_tokens": 128000,
        "messages": [{"role": "user", "content": "hello"}]
    });
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let native: AnthropicMessagesRequest = serde_json::from_value(payload).unwrap();
    anthropic_messages_request_to_core(&native)
}

#[test]
fn native_opus_5_5_preserves_adaptive_display_and_all_efforts() {
    let mut context = context();
    context.upstream_model = "claude-opus-5-5".into();
    for display in [json!("summarized"), json!("omitted"), Value::Null] {
        for effort in ["low", "medium", "high", "xhigh", "max"] {
            let thinking = json!({"type": "adaptive", "display": display});
            let output_config = json!({"effort": effort});
            let request = native_messages_request(json!({
                "thinking": thinking, "output_config": output_config
            }));
            for stream in [false, true] {
                let body = map_anthropic_request(
                    &request,
                    &context,
                    stream,
                    &AnthropicRequestOptions::default(),
                )
                .unwrap();
                assert_eq!(body["thinking"], thinking);
                assert_eq!(body["output_config"], output_config);
                assert_eq!(body["max_tokens"], 128000);
                assert_eq!(body["stream"], stream);
            }
        }
    }
}

#[test]
fn native_opus_5_5_leaves_omitted_thinking_and_effort_to_upstream_defaults() {
    let mut context = context();
    context.upstream_model = "claude-opus-5-5".into();
    let format = json!({
        "type": "json_schema", "schema": {"type": "object", "properties": {}}
    });
    for controls in [
        json!({}),
        json!({"thinking": {"type": "adaptive"}}),
        json!({"output_config": {"effort": "high"}}),
        json!({"output_config": {"effort": null}}),
        json!({"output_config": {"format": format}}),
        json!({"thinking": {"type": "adaptive"}, "output_config": {"format": format}}),
    ] {
        let request = native_messages_request(controls.clone());
        for stream in [false, true] {
            let body = map_anthropic_request(
                &request,
                &context,
                stream,
                &AnthropicRequestOptions::default(),
            )
            .unwrap();
            assert_eq!(body.get("thinking"), controls.get("thinking"));
            assert_eq!(body.get("output_config"), controls.get("output_config"));
        }
    }
}

#[test]
fn native_effort_preserves_independent_thinking_modes() {
    for (model, thinking) in [
        ("claude-opus-5", json!({"type": "disabled"})),
        ("claude-sonnet-4-6", json!({"type": "disabled"})),
        ("claude-opus-4-5", json!({"type": "disabled"})),
        ("claude-sonnet-5-5", json!({"type": "between_tools"})),
        (
            "claude-opus-4-5",
            json!({"type": "enabled", "budget_tokens": 4096,
            "display": "omitted"}),
        ),
    ] {
        let mut context = context();
        context.upstream_model = model.into();
        let request = native_messages_request(json!({
            "thinking": thinking, "output_config": {"effort": "high"}
        }));
        for stream in [false, true] {
            let body = map_anthropic_request(
                &request,
                &context,
                stream,
                &AnthropicRequestOptions::default(),
            )
            .unwrap();
            assert_eq!(body["thinking"], thinking, "{model}");
            assert_eq!(body["output_config"], json!({"effort": "high"}), "{model}");
        }
    }
}

#[test]
fn native_thinking_off_preserves_null_effort_and_normalizes_unused_fields() {
    for (model, mode) in [
        ("claude-opus-5", "disabled"),
        ("claude-sonnet-5-5", "between_tools"),
    ] {
        let mut context = context();
        context.upstream_model = model.into();
        let request = native_messages_request(json!({
            "thinking": {"type": mode, "display": "unused", "budget_tokens": null},
            "output_config": {"effort": null}
        }));
        for stream in [false, true] {
            let body = map_anthropic_request(
                &request,
                &context,
                stream,
                &AnthropicRequestOptions::default(),
            )
            .unwrap();
            assert_eq!(body["thinking"], json!({"type": mode}));
            assert_eq!(body["output_config"], json!({"effort": null}));
        }
    }
}

#[test]
fn native_thinking_preserves_beta_display_and_future_fields() {
    let mut context = context();
    context.upstream_model = "claude-opus-5-5".into();
    context.extra_headers.insert(
        "anthropic-beta".into(),
        json!("thinking-display-updates-2026-08-18"),
    );
    let thinking = json!({
        "type": "adaptive", "display": "updates", "future_thinking_option": true
    });
    let request = native_messages_request(json!({"thinking": thinking}));
    let body = map_anthropic_request(
        &request,
        &context,
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(body["thinking"], thinking);
    assert!(body.get("output_config").is_none());
}

#[test]
fn reasoning_alias_rejects_null_output_config() {
    let request = chat(json!({"reasoning_effort": "high", "output_config": null}));
    let error = map_anthropic_request(
        &request,
        &context(),
        false,
        &AnthropicRequestOptions::default(),
    )
    .expect_err("output_config must be an object");
    assert!(matches!(
        error,
        gateway_core::ProviderError::InvalidRequest(message)
            if message == super::error::AnthropicAdapterError::InvalidOutputConfig.to_string()
    ));
}

#[test]
fn manual_only_models_preserve_legacy_minimal_reasoning_alias_with_budget() {
    let mut context = context();
    context.upstream_model = "claude-sonnet-4-5".into();
    let request = chat(json!({"reasoning_effort": "minimal", "reasoning_budget_tokens": 1024}));
    let body = map_anthropic_request(
        &request,
        &context,
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(
        body["thinking"],
        json!({"type": "enabled", "budget_tokens": 1024})
    );
    assert!(body.get("output_config").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn native_opus_5_5_reports_invalid_display_separately_from_manual_budgets() {
    let mut context = context();
    context.upstream_model = "claude-opus-5-5".into();
    for display in [json!("unsupported"), json!(true), json!(42)] {
        let request = native_messages_request(json!({
            "thinking": {"type": "adaptive", "display": display}
        }));
        let error = map_anthropic_request(
            &request,
            &context,
            false,
            &AnthropicRequestOptions::default(),
        )
        .expect_err("invalid display must be rejected")
        .to_string();
        assert!(error.contains("thinking.display"), "{error}");
        assert!(!error.contains("budget"), "{error}");
    }

    let request = native_messages_request(json!({
        "thinking": {"type": "adaptive", "display": "summarized", "budget_tokens": 1024}
    }));
    let error = map_anthropic_request(
        &request,
        &context,
        false,
        &AnthropicRequestOptions::default(),
    )
    .expect_err("adaptive thinking must reject a manual budget")
    .to_string();
    assert!(error.contains("budget"), "{error}");
}

#[test]
fn native_system_cache_boundaries_survive_translation() {
    let system = json!([
        {"type": "text", "text": "stable", "cache_control": {"type": "ephemeral", "ttl": "1h"}},
        {"type": "text", "text": "session", "cache_control": {"type": "ephemeral", "ttl": "5m"}},
        {"type": "text", "text": "uncached suffix"}
    ]);
    let native: AnthropicMessagesRequest = serde_json::from_value(json!({
        "model": "claude", "system": system, "max_tokens": 100,
        "messages": [{"role": "user", "content": "hello"}]
    }))
    .unwrap();
    let request = anthropic_messages_request_to_core(&native);
    for stream in [false, true] {
        let body = map_anthropic_request(
            &request,
            &context(),
            stream,
            &AnthropicRequestOptions::default(),
        )
        .unwrap();
        assert_eq!(body["system"], system);
    }
}

#[test]
fn chat_system_and_developer_strings_become_ordered_blocks() {
    let request = serde_json::from_value(json!({"model": "claude", "messages": [
        {"role": "system", "content": "first"}, {"role": "developer", "content": "second"},
        {"role": "user", "content": "hello"}
    ]}))
    .unwrap();
    let body = map_anthropic_request(
        &request,
        &context(),
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(
        body["system"],
        json!([{"type": "text", "text": "first"}, {"type": "text", "text": "second"}])
    );
}

#[test]
fn maps_chat_controls_after_route_overrides() {
    let request = chat(
        json!({"stop": "old", "parallel_tool_calls": true, "store": false,
            "stream_options": {"include_usage": true},
            "tools": [{"type": "function", "function": {"name": "lookup", "parameters": {"type": "object"}}}]
        }),
    );
    let mut context = context();
    context
        .extra_body
        .insert("stop".into(), json!(["END", "STOP"]));
    context
        .extra_body
        .insert("parallel_tool_calls".into(), json!(false));
    let body = map_anthropic_request(
        &request,
        &context,
        true,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(body["stop_sequences"], json!(["END", "STOP"]));
    assert_eq!(
        body["tool_choice"],
        json!({"type": "auto", "disable_parallel_tool_use": true})
    );
    for key in ["stop", "parallel_tool_calls", "store", "stream_options"] {
        assert!(body.get(key).is_none(), "{key}");
    }
}

#[test]
fn validates_system_blocks_and_preserves_explicit_overrides() {
    let mut request = chat(json!({}));
    request.messages.insert(
        0,
        serde_json::from_value(json!({"role": "system", "content": [
            {"type": "input_text", "text": "cached", "cache_control": {"type": "ephemeral"}}
        ]}))
        .unwrap(),
    );
    let body = map_anthropic_request(
        &request,
        &context(),
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(
        body["system"],
        json!([{ "type": "text", "text": "cached", "cache_control": {"type": "ephemeral"} }])
    );
    let mut context = context();
    context.extra_body.insert(
        "system".into(),
        json!([{"type": "text", "text": "override"}]),
    );
    let body = map_anthropic_request(
        &request,
        &context,
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(body["system"], context.extra_body["system"]);
    for invalid in [
        json!([{"type": "image"}]),
        json!([{"type": "text", "text": 42}]),
        json!(["text"]),
    ] {
        request.messages[0].content = invalid;
        assert!(
            map_anthropic_request(
                &request,
                &context,
                false,
                &AnthropicRequestOptions::default()
            )
            .is_err()
        );
    }
}

#[test]
fn stop_empty_and_identical_native_controls_are_supported() {
    for stop in [json!([]), json!(["END"])] {
        let request = chat(json!({"stop": stop, "stop_sequences": stop,
            "parallel_tool_calls": false, "stream_options": {"include_usage": false}}));
        let body = map_anthropic_request(
            &request,
            &context(),
            false,
            &AnthropicRequestOptions::default(),
        )
        .unwrap();
        assert_eq!(body["stop_sequences"], stop);
        assert!(
            body.get("tool_choice").is_none(),
            "no tools should not create tool_choice"
        );
    }
}

#[test]
fn maps_stop_and_parallel_controls_without_losing_tool_choice() {
    for (choice, expected) in [
        (
            json!("auto"),
            json!({"type": "auto", "disable_parallel_tool_use": true}),
        ),
        (
            json!("required"),
            json!({"type": "any", "disable_parallel_tool_use": true}),
        ),
        (
            json!({"type": "function", "function": {"name": "lookup"}}),
            json!({"type": "tool", "name": "lookup", "disable_parallel_tool_use": true}),
        ),
        (json!("none"), json!({"type": "none"})),
        (
            Value::Null,
            json!({"type": "auto", "disable_parallel_tool_use": true}),
        ),
    ] {
        let request = chat(json!({"stop": "END", "parallel_tool_calls": false,
            "tool_choice": choice, "tools": [{"name": "lookup", "input_schema": {"type": "object"}}]}));
        let body = map_anthropic_request(
            &request,
            &context(),
            false,
            &AnthropicRequestOptions::default(),
        )
        .unwrap();
        assert_eq!(body["stop_sequences"], json!(["END"]));
        assert_eq!(body["tool_choice"], expected);
    }
}

#[test]
fn rejects_unsupported_or_conflicting_chat_controls() {
    for extra in [
        json!({"stop": 12}),
        json!({"stop": ["END", null]}),
        json!({"stop": "END", "stop_sequences": ["DIFFERENT"]}),
        json!({"store": true}),
        json!({"store": "false"}),
        json!({"stream_options": {"unsupported": true}}),
        json!({"stream_options": {"include_usage": "yes"}}),
        json!({"parallel_tool_calls": "false"}),
        json!({"parallel_tool_calls": false, "tools": [{"name": "lookup"}],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": false}}),
    ] {
        assert!(
            map_anthropic_request(
                &chat(extra.clone()),
                &context(),
                false,
                &AnthropicRequestOptions::default()
            )
            .is_err(),
            "{extra}"
        );
    }
}

#[test]
fn preserves_native_controls_and_omits_null_chat_hints() {
    let request = chat(
        json!({"stop": null, "stop_sequences": ["END"], "store": null,
        "stream_options": null, "parallel_tool_calls": null,
        "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}}),
    );
    let body = map_anthropic_request(
        &request,
        &context(),
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(body["stop_sequences"], json!(["END"]));
    assert_eq!(
        body["tool_choice"],
        json!({"type": "auto", "disable_parallel_tool_use": true})
    );
    for key in ["stop", "parallel_tool_calls", "store", "stream_options"] {
        assert!(body.get(key).is_none());
    }
}
