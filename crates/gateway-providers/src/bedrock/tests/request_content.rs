use super::*;

#[test]
fn maps_text_chat_request_to_converse_body() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![
            message("system", "Be terse."),
            message("developer", "Prefer SI units."),
            message("user", "Hello"),
        ],
        stream: false,
        extra: BTreeMap::from([
            ("max_completion_tokens".to_string(), json!(128)),
            ("temperature".to_string(), json!(0.2)),
            ("top_p".to_string(), json!(0.9)),
            ("stop".to_string(), json!(["END"])),
        ]),
    };

    let body = map_chat_request_to_converse(
        &request,
        &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
    )
    .expect("mapped");

    assert_eq!(
        body,
        json!({
            "system": [{"text":"Be terse."},{"text":"Prefer SI units."}],
            "messages": [{
                "role": "user",
                "content": [{"text": "Hello"}]
            }],
            "inferenceConfig": {
                "maxTokens": 128,
                "temperature": 0.2,
                "topP": 0.9,
                "stopSequences": ["END"]
            }
        })
    );
}

#[test]
fn maps_text_chat_request_to_anthropic_messages_invoke_body() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![
            message("system", "Be terse."),
            message("developer", "Prefer SI units."),
            message("user", "Hello"),
        ],
        stream: false,
        extra: BTreeMap::from([
            ("max_completion_tokens".to_string(), json!(128)),
            ("temperature".to_string(), json!(0.2)),
            ("top_p".to_string(), json!(0.9)),
            ("stop".to_string(), json!(["END"])),
            (
                "anthropic_beta".to_string(),
                json!(["token-efficient-tools-2025-02-19"]),
            ),
        ]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
    )
    .expect("mapped");

    assert_eq!(
        body,
        json!({
            "anthropic_version": "bedrock-2023-05-31",
            "anthropic_beta": ["token-efficient-tools-2025-02-19"],
            "system": "Be terse.\nPrefer SI units.",
            "messages": [{
                "role": "user",
                "content": [{"type": "text", "text": "Hello"}]
            }],
            "max_tokens": 128,
            "temperature": 0.2,
            "top_p": 0.9,
            "stop_sequences": ["END"]
        })
    );
}

#[test]
fn maps_converse_base64_image_blocks_and_rejects_remote_urls() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {
                    "type": "image_url",
                    "image_url": {
                        "url": "data:image/png;base64,aW1hZ2U="
                    }
                },
                {"type": "text", "text": "Describe it"}
            ]),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: true,
        extra: BTreeMap::new(),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    assert_eq!(
        body["messages"][0]["content"][0],
        json!({
            "image": {
                "format": "png",
                "source": {
                    "bytes": "aW1hZ2U="
                }
            }
        })
    );

    let remote = CoreChatRequest {
        messages: vec![CoreChatMessage {
            content: json!([{
                "type": "image_url",
                "image_url": {"url": "https://example.test/image.png"}
            }]),
            ..message("user", "")
        }],
        ..request
    };
    let error = map_chat_request_to_converse(&remote, &context("amazon.nova-pro-v1:0"))
        .expect_err("remote image rejected")
        .to_string();
    assert!(error.contains("remote image URLs are not supported"));
}

#[test]
fn rejects_unknown_bedrock_converse_request_fields() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("top_k".to_string(), json!(10))]),
    };

    let error = map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0"))
        .expect_err("unknown field rejected")
        .to_string();
    assert!(error.contains("unsupported request field(s)"));
    assert!(error.contains("top_k"));
    assert!(error.contains("additionalModelRequestFields"));
}

#[test]
fn rejects_unknown_anthropic_messages_request_fields() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(64)),
            ("unknown_anthropic_option".to_string(), json!(true)),
        ]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect_err("unknown field rejected")
    .to_string();
    assert!(error.contains("unsupported request field(s)"));
    assert!(error.contains("unknown_anthropic_option"));
    assert!(error.contains("extra_body"));
}

#[test]
fn maps_anthropic_base64_image_blocks_and_rejects_remote_urls() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "aW1hZ2U="
                    }
                },
                {"type": "text", "text": "Describe it"}
            ]),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };

    let body = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect("mapped");
    assert_eq!(
        body["messages"][0]["content"][0],
        json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "aW1hZ2U="
            }
        })
    );

    let remote = CoreChatRequest {
        messages: vec![CoreChatMessage {
            content: json!([{
                "type": "image_url",
                "image_url": {"url": "https://example.test/image.png"}
            }]),
            ..message("user", "")
        }],
        ..request
    };
    let error = map_chat_request_to_anthropic_messages(
        &remote,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect_err("remote image rejected")
    .to_string();
    assert!(error.contains("remote image URLs are not supported"));
}

#[test]
fn rejects_anthropic_messages_without_max_tokens() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect_err("max tokens rejected")
    .to_string();
    assert!(error.contains("requires `max_tokens` or `max_completion_tokens`"));
}

#[test]
fn gates_anthropic_messages_streaming_mapping() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &request,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect_err("streaming gated")
    .to_string();
    assert!(error.contains("streaming is gated"));
}

#[test]
fn rejects_unsupported_role_deterministically() {
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("critic", "Nope")],
        stream: false,
        extra: BTreeMap::new(),
    };

    let error =
        map_chat_request_to_converse(&request, &context("anthropic.claude-3-haiku-20240307-v1:0"))
            .expect_err("role rejected")
            .to_string();
    assert!(error.contains("unsupported message role `critic`"));
}
