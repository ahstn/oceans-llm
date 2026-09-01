use super::*;

#[test]
fn maps_vertex_adaptive_only_claude_reasoning_effort_to_adaptive_thinking() {
    for upstream_model in [
        "anthropic/claude-fable-5",
        "anthropic/claude-opus-4-7",
        "anthropic/claude-opus-4-8",
        "anthropic/claude-sonnet-5",
    ] {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert("model".to_string(), json!("fast"));
        request
            .extra
            .insert("reasoning_effort".to_string(), json!("xhigh"));
        request.extra.insert("temperature".to_string(), json!(1.0));
        request.extra.insert("top_p".to_string(), json!(1.0));

        let mapped =
            map_anthropic_request(&request, &context(upstream_model), false).expect("mapped");

        assert_eq!(mapped["anthropic_version"], "vertex-2023-10-16");
        assert_eq!(mapped["thinking"], json!({ "type": "adaptive" }));
        assert!(mapped.get("output_config").is_none());
        assert!(mapped.get("reasoning_effort").is_none());
        assert!(mapped.get("model").is_none());
        assert!(mapped.get("temperature").is_none());
        assert!(mapped.get("top_p").is_none());
    }
}

#[test]
fn ignores_null_reasoning_effort_for_vertex_anthropic_mapping() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("hello".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("reasoning_effort".to_string(), Value::Null);
    request
        .extra
        .insert("reasoning".to_string(), json!({ "effort": null }));
    request
        .extra
        .insert("output_config".to_string(), json!({ "effort": null }));

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-opus-4-7"), false)
        .expect("mapped");

    assert!(mapped.get("thinking").is_none());
    assert!(mapped.get("output_config").is_none());
    assert!(mapped.get("reasoning_effort").is_none());
    assert!(mapped.get("reasoning").is_none());
}

#[test]
fn validates_native_output_config_effort_for_vertex_anthropic_mapping() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "output_config".to_string(),
        json!({ "effort": "xhigh", "metadata": {"source": "caller"} }),
    );

    let mapped = map_anthropic_request(&request, &context("anthropic/claude-opus-4-7"), false)
        .expect("mapped");

    assert_eq!(mapped["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(
        mapped["output_config"],
        json!({ "metadata": {"source": "caller"} })
    );
}

#[test]
fn rejects_native_output_config_effort_for_vertex_manual_only_models() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("output_config".to_string(), json!({ "effort": "medium" }));
    request
        .extra
        .insert("reasoning_budget_tokens".to_string(), json!(1024));

    let error = map_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-5@20250929"),
        false,
    )
    .expect_err("manual-only effort rejected")
    .to_string();

    assert!(error.contains("output_config.effort"));
}

#[test]
fn ignores_null_vertex_thinking_budget_alias_before_reasoning_budget() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("thinking_budget_tokens".to_string(), Value::Null);
    request
        .extra
        .insert("reasoning_budget_tokens".to_string(), json!(1024));

    let mapped = map_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-5@20250929"),
        false,
    )
    .expect("mapped");

    assert_eq!(
        mapped["thinking"],
        json!({ "type": "enabled", "budget_tokens": 1024 })
    );
    assert!(mapped.get("thinking_budget_tokens").is_none());
    assert!(mapped.get("reasoning_budget_tokens").is_none());
}

#[test]
fn rejects_conflicting_vertex_thinking_budget_aliases() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("thinking_budget_tokens".to_string(), json!(2048));
    request
        .extra
        .insert("reasoning_budget_tokens".to_string(), json!(1024));

    let error = map_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-5@20250929"),
        false,
    )
    .expect_err("conflicting budgets rejected")
    .to_string();

    assert!(error.contains("thinking_budget_tokens"));
    assert!(error.contains("reasoning_budget_tokens"));
}

#[test]
fn rejects_conflicting_nested_and_top_level_reasoning_budgets() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("reasoning_budget_tokens".to_string(), json!(1024));
    request
        .extra
        .insert("reasoning".to_string(), json!({"budget_tokens": 2048}));

    let error = map_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-5@20250929"),
        false,
    )
    .expect_err("conflicting budgets rejected")
    .to_string();

    assert!(error.contains("reasoning.budget_tokens"));
    assert!(error.contains("reasoning_budget_tokens"));
}

#[test]
fn maps_vertex_opus_and_sonnet_4_6_reasoning_effort_to_adaptive_thinking() {
    for model in ["anthropic/claude-opus-4-6", "anthropic/claude-sonnet-4-6"] {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request
            .extra
            .insert("reasoning_effort".to_string(), json!("high"));

        let mapped = map_anthropic_request(&request, &context(model), false).expect("mapped");

        assert_eq!(mapped["thinking"], json!({ "type": "adaptive" }));
        assert!(mapped.get("output_config").is_none());
        assert!(mapped.get("reasoning_effort").is_none());
    }
}

#[test]
fn maps_vertex_opus_4_5_reasoning_effort_with_manual_budget() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "reasoning".to_string(),
        json!({ "effort": "medium", "budget_tokens": 2048 }),
    );

    let mapped = map_anthropic_request(
        &request,
        &context("anthropic/claude-opus-4-5@20251101"),
        false,
    )
    .expect("mapped");

    assert_eq!(
        mapped["thinking"],
        json!({ "type": "enabled", "budget_tokens": 2048 })
    );
    assert!(mapped.get("output_config").is_none());
    assert!(mapped.get("reasoning").is_none());
}

#[test]
fn maps_vertex_older_claude_reasoning_budget_to_manual_thinking() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request.extra.insert(
        "reasoning".to_string(),
        json!({ "effort": "medium", "budget_tokens": 1024 }),
    );

    let mapped = map_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-5@20250929"),
        false,
    )
    .expect("mapped");

    assert_eq!(
        mapped["thinking"],
        json!({ "type": "enabled", "budget_tokens": 1024 })
    );
    assert!(mapped.get("output_config").is_none());
    assert!(mapped.get("reasoning").is_none());
}

#[test]
fn rejects_vertex_adaptive_only_manual_thinking_budget() {
    for upstream_model in [
        "anthropic/claude-fable-5",
        "anthropic/claude-opus-4-7",
        "anthropic/claude-opus-4-8",
        "anthropic/claude-sonnet-5",
    ] {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert(
            "thinking".to_string(),
            json!({ "type": "enabled", "budget_tokens": 4096 }),
        );

        let error = map_anthropic_request(&request, &context(upstream_model), false)
            .expect_err("manual thinking should be rejected");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("thinking.type: enabled"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}

#[test]
fn rejects_vertex_near_match_adaptive_only_claude_ids() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("reasoning_effort".to_string(), json!("high"));

    let error = map_anthropic_request(&request, &context("anthropic/claude-sonnet-50"), false)
        .expect_err("near-match model should not be adaptive-only")
        .to_string();

    assert!(error.contains("manual thinking budget"));
}

#[test]
fn rejects_vertex_extra_body_that_bypasses_anthropic_validation() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    let mut context = context("anthropic/claude-opus-4-7");
    context
        .extra_body
        .insert("temperature".to_string(), json!(0.2));

    let error = map_anthropic_request(&request, &context, false)
        .expect_err("route extra_body should be validated after merge");

    match error {
        ProviderError::InvalidRequest(message) => {
            assert!(message.contains("temperature"));
            assert!(message.contains("adaptive-only Claude models"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn rejects_vertex_native_manual_thinking_without_budget() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("thinking".to_string(), json!({ "type": "enabled" }));

    let error = map_anthropic_request(
        &request,
        &context("anthropic/claude-sonnet-4-5@20250929"),
        false,
    )
    .expect_err("native manual thinking requires a budget");

    match error {
        ProviderError::InvalidRequest(message) => {
            assert!(message.contains("thinking.type: enabled"));
            assert!(message.contains("budget_tokens"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn rejects_vertex_older_claude_adaptive_thinking() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("think carefully".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("thinking".to_string(), json!({ "type": "adaptive" }));

    let error = map_anthropic_request(
        &request,
        &context("anthropic/claude-haiku-4-5@20251001"),
        false,
    )
    .expect_err("adaptive thinking should be rejected");

    match error {
        ProviderError::InvalidRequest(message) => {
            assert!(message.contains("thinking.type: adaptive"));
        }
        other => panic!("unexpected error: {other}"),
    }
}
