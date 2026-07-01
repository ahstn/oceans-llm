use super::*;

#[test]
fn maps_adaptive_only_claude_reasoning_effort_to_adaptive_thinking() {
    for upstream_model in [
        "global.anthropic.claude-fable-5",
        "global.anthropic.claude-opus-4-7",
        "global.anthropic.claude-opus-4-8",
        "global.anthropic.claude-sonnet-5",
    ] {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Think carefully")],
            stream: false,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(4096)),
                ("reasoning_effort".to_string(), json!("xhigh")),
                ("temperature".to_string(), json!(1.0)),
            ]),
        };

        let body = map_chat_request_to_anthropic_messages(&request, &context(upstream_model))
            .expect("mapped");

        assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(body["output_config"], json!({ "effort": "xhigh" }));
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("temperature").is_none());
    }
}

#[test]
fn maps_opus_and_sonnet_4_6_reasoning_effort_to_adaptive_thinking() {
    for upstream_model in [
        "us.anthropic.claude-opus-4-6-v1:0",
        "us.anthropic.claude-sonnet-4-6-v1:0",
    ] {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Think carefully")],
            stream: false,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(4096)),
                ("reasoning_effort".to_string(), json!("medium")),
            ]),
        };

        let body = map_chat_request_to_anthropic_messages(&request, &context(upstream_model))
            .expect("mapped");

        assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(body["output_config"], json!({ "effort": "medium" }));
        assert!(body.get("reasoning_effort").is_none());
    }
}

#[test]
fn maps_older_claude_reasoning_budget_to_manual_thinking() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            (
                "reasoning".to_string(),
                json!({ "effort": "high", "budget_tokens": 1024 }),
            ),
        ]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-sonnet-4-5-v1:0"),
    )
    .expect("mapped");

    assert_eq!(
        body["thinking"],
        json!({ "type": "enabled", "budget_tokens": 1024 })
    );
    assert!(body.get("output_config").is_none());
    assert!(body.get("reasoning").is_none());
}

#[test]
fn maps_opus_4_5_reasoning_effort_to_bedrock_effort_beta() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("reasoning_effort".to_string(), json!("medium")),
        ]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-opus-4-5-v1:0"),
    )
    .expect("mapped");

    assert!(body.get("thinking").is_none());
    assert_eq!(body["output_config"], json!({ "effort": "medium" }));
    assert_eq!(body["anthropic_beta"], json!(["effort-2025-11-24"]));
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn ignores_null_reasoning_effort_for_anthropic_mapping() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("reasoning_effort".to_string(), Value::Null),
            ("reasoning".to_string(), json!({ "effort": null })),
            ("output_config".to_string(), json!({ "effort": null })),
        ]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("global.anthropic.claude-opus-4-7"),
    )
    .expect("mapped");

    assert!(body.get("thinking").is_none());
    assert!(body.get("output_config").is_none());
    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("reasoning").is_none());
}

#[test]
fn validates_native_output_config_effort_for_anthropic_mapping() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("output_config".to_string(), json!({ "effort": "medium" })),
            ("reasoning_budget_tokens".to_string(), json!(1024)),
        ]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-opus-4-5-v1:0"),
    )
    .expect("mapped");

    assert_eq!(body["output_config"], json!({ "effort": "medium" }));
    assert_eq!(body["anthropic_beta"], json!(["effort-2025-11-24"]));
}

#[test]
fn rejects_native_output_config_effort_for_manual_only_anthropic_mapping() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("output_config".to_string(), json!({ "effort": "medium" })),
        ]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-sonnet-4-5-v1:0"),
    )
    .expect_err("manual-only effort rejected")
    .to_string();

    assert!(error.contains("output_config.effort"));
}

#[test]
fn maps_claude_converse_reasoning_effort_to_additional_model_request_fields() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("reasoning_effort".to_string(), json!("high")),
            (
                "additionalModelRequestFields".to_string(),
                json!({ "trace": "enabled" }),
            ),
        ]),
    };

    let body =
        map_chat_request_to_converse(&request, &context("us.anthropic.claude-sonnet-4-6-v1:0"))
            .expect("mapped");

    assert_eq!(
        body["additionalModelRequestFields"],
        json!({
            "trace": "enabled",
            "thinking": {
                "type": "adaptive",
                "effort": "high"
            }
        })
    );
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn maps_adaptive_only_claude_converse_reasoning_effort() {
    for upstream_model in [
        "global.anthropic.claude-fable-5",
        "global.anthropic.claude-opus-4-8",
        "global.anthropic.claude-sonnet-5",
    ] {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Think carefully")],
            stream: true,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(4096)),
                ("reasoning_effort".to_string(), json!("xhigh")),
            ]),
        };

        let body =
            map_chat_request_to_converse(&request, &context(upstream_model)).expect("mapped");

        assert_eq!(
            body["additionalModelRequestFields"],
            json!({
                "thinking": {
                    "type": "adaptive",
                    "effort": "xhigh"
                }
            })
        );
        assert!(body.get("reasoning_effort").is_none());
    }
}

#[test]
fn maps_older_claude_converse_reasoning_budget_to_manual_thinking() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            (
                "reasoning".to_string(),
                json!({ "effort": "high", "budget_tokens": 1024 }),
            ),
        ]),
    };

    let body = map_chat_request_to_converse(&request, &context("anthropic.claude-haiku-4-5-v1:0"))
        .expect("mapped");

    assert_eq!(
        body["additionalModelRequestFields"],
        json!({
            "thinking": {
                "type": "enabled",
                "budget_tokens": 1024
            }
        })
    );
    assert!(body.get("reasoning").is_none());
}

#[test]
fn maps_opus_4_5_converse_reasoning_effort_with_manual_budget() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            (
                "reasoning".to_string(),
                json!({ "effort": "medium", "budget_tokens": 1024 }),
            ),
        ]),
    };

    let body = map_chat_request_to_converse(&request, &context("anthropic.claude-opus-4-5-v1:0"))
        .expect("mapped");

    assert_eq!(
        body["additionalModelRequestFields"],
        json!({
            "thinking": {
                "type": "enabled",
                "budget_tokens": 1024,
                "effort": "medium"
            }
        })
    );
}

#[test]
fn rejects_opus_4_5_converse_reasoning_effort_without_manual_budget() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("reasoning_effort".to_string(), json!("medium")),
        ]),
    };

    let error = map_chat_request_to_converse(&request, &context("anthropic.claude-opus-4-5-v1:0"))
        .expect_err("budget required")
        .to_string();

    assert!(error.contains("manual thinking budget"));
}

#[test]
fn rejects_conflicting_claude_converse_reasoning_effort() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("reasoning_effort".to_string(), json!("high")),
            (
                "additionalModelRequestFields".to_string(),
                json!({
                    "thinking": {
                        "type": "adaptive",
                        "effort": "low"
                    }
                }),
            ),
        ]),
    };

    let error =
        map_chat_request_to_converse(&request, &context("us.anthropic.claude-opus-4-6-v1:0"))
            .expect_err("conflict rejected")
            .to_string();

    assert!(error.contains("additionalModelRequestFields.thinking.effort"));
}

#[test]
fn rejects_opus_4_7_converse_non_default_sampling_fields() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(64)),
            ("temperature".to_string(), json!(0.2)),
        ]),
    };

    let error =
        map_chat_request_to_converse(&request, &context("global.anthropic.claude-opus-4-7"))
            .expect_err("sampling rejected")
            .to_string();

    assert!(error.contains("temperature"));
    assert!(error.contains("non-default"));
}

#[test]
fn rejects_opus_4_7_converse_additional_model_top_k() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(64)),
            (
                "additionalModelRequestFields".to_string(),
                json!({ "top_k": 50 }),
            ),
        ]),
    };

    let error =
        map_chat_request_to_converse(&request, &context("global.anthropic.claude-opus-4-7"))
            .expect_err("top_k rejected")
            .to_string();

    assert!(error.contains("top_k"));
    assert!(error.contains("adaptive-only Claude models"));
}

#[test]
fn rejects_opus_4_7_converse_additional_model_top_k_without_inference_config() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::from([(
            "additionalModelRequestFields".to_string(),
            json!({
                "thinking": {"type": "adaptive"},
                "top_k": 50
            }),
        )]),
    };

    let error =
        map_chat_request_to_converse(&request, &context("global.anthropic.claude-opus-4-7"))
            .expect_err("top_k rejected without inferenceConfig")
            .to_string();

    assert!(error.contains("top_k"));
    assert!(error.contains("adaptive-only Claude models"));
}

#[test]
fn rejects_adaptive_only_manual_thinking_budget() {
    for upstream_model in [
        "global.anthropic.claude-fable-5",
        "global.anthropic.claude-opus-4-8",
        "global.anthropic.claude-sonnet-5",
    ] {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Think carefully")],
            stream: false,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(4096)),
                (
                    "thinking".to_string(),
                    json!({ "type": "enabled", "budget_tokens": 1024 }),
                ),
            ]),
        };

        let error = map_chat_request_to_anthropic_messages(&request, &context(upstream_model))
            .expect_err("manual thinking rejected")
            .to_string();

        assert!(error.contains("thinking.type: enabled"));
        assert!(error.contains(upstream_model));
    }
}

#[test]
fn rejects_native_manual_thinking_without_budget() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("thinking".to_string(), json!({ "type": "enabled" })),
        ]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-haiku-4-5-v1:0"),
    )
    .expect_err("manual thinking requires budget")
    .to_string();

    assert!(error.contains("thinking.type: enabled"));
    assert!(error.contains("budget_tokens"));
}

#[test]
fn rejects_converse_manual_thinking_without_budget() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: true,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            (
                "additionalModelRequestFields".to_string(),
                json!({ "thinking": { "type": "enabled" } }),
            ),
        ]),
    };

    let error = map_chat_request_to_converse(&request, &context("anthropic.claude-haiku-4-5-v1:0"))
        .expect_err("manual thinking requires budget")
        .to_string();

    assert!(error.contains("additionalModelRequestFields.thinking.type: enabled"));
    assert!(error.contains("budget_tokens"));
}

#[test]
fn rejects_older_claude_adaptive_thinking() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("thinking".to_string(), json!({ "type": "adaptive" })),
        ]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-haiku-4-5-v1:0"),
    )
    .expect_err("adaptive thinking rejected")
    .to_string();

    assert!(error.contains("thinking.type: adaptive"));
    assert!(error.contains("is not supported"));
}

#[test]
fn rejects_opus_4_7_non_default_sampling_fields() {
    for field in ["temperature", "top_p", "top_k"] {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Hello")],
            stream: false,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(64)),
                (field.to_string(), json!(0.2)),
            ]),
        };

        let error = map_chat_request_to_anthropic_messages(
            &request,
            &context("global.anthropic.claude-opus-4-7"),
        )
        .expect_err("sampling rejected")
        .to_string();

        assert!(error.contains(field));
        assert!(error.contains("non-default"));
    }
}

#[test]
fn rejects_conflicting_reasoning_and_output_config_effort() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Think carefully")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(4096)),
            ("reasoning_effort".to_string(), json!("medium")),
            ("output_config".to_string(), json!({ "effort": "high" })),
        ]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("global.anthropic.claude-opus-4-7"),
    )
    .expect_err("conflict rejected")
    .to_string();

    assert!(error.contains("conflicts with `output_config.effort`"));
}
