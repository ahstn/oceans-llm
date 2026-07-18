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
fn maps_openai_file_content_to_bedrock_document() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {
                    "type": "input_file",
                    "file": {
                        "file_data": "data:application/pdf;base64,cGRm",
                        "filename": "reports/Board_Packet!!.pdf"
                    }
                },
                {"type": "input_text", "text": "Summarize the document"}
            ]),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::new(),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    assert_eq!(
        body["messages"][0]["content"][0],
        json!({
            "document": {
                "format": "pdf",
                "name": "Board Packet",
                "source": {"bytes": "cGRm"}
            }
        })
    );
}

#[test]
fn rejects_images_and_documents_in_invalid_message_contexts() {
    let cases = [
        (
            "assistant",
            json!([
                {
                    "type": "input_file",
                    "file": {
                        "file_data": "data:application/pdf;base64,cGRm",
                        "filename": "report.pdf"
                    }
                },
                {"type": "input_text", "text": "Summary"}
            ]),
            "document content is only supported in user messages",
        ),
        (
            "assistant",
            json!([{
                "type": "image_url",
                "image_url": {"url": "data:image/png;base64,aW1hZ2U="}
            }]),
            "image content is only supported in user messages",
        ),
        (
            "user",
            json!([{
                "type": "input_file",
                "file": {
                    "file_data": "data:application/pdf;base64,cGRm",
                    "filename": "report.pdf"
                }
            }]),
            "messages containing documents must also contain text",
        ),
    ];

    for (role, content, expected_error) in cases {
        let request = CoreChatRequest {
            model: "nova".to_string(),
            messages: vec![CoreChatMessage {
                role: role.to_string(),
                content,
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra: BTreeMap::new(),
        };

        let error = map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0"))
            .expect_err("invalid media context rejected")
            .to_string();
        assert!(error.contains(expected_error), "{error}");
    }
}

#[test]
fn maps_validated_request_scoped_converse_controls() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([
            (
                "requestMetadata".to_string(),
                json!({"tenant": "acme", "cost:center": "research"}),
            ),
            (
                "performanceConfig".to_string(),
                json!({"latency": "optimized"}),
            ),
            (
                "guardrailConfig".to_string(),
                json!({
                    "guardrailIdentifier": "guardrail123",
                    "guardrailVersion": "DRAFT",
                    "trace": "enabled_full"
                }),
            ),
            (
                "additionalModelResponseFieldPaths".to_string(),
                json!(["/stop_sequence", "/nested~1field"]),
            ),
        ]),
    };

    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    assert_eq!(
        body["requestMetadata"],
        json!({"tenant": "acme", "cost:center": "research"})
    );
    assert_eq!(body["performanceConfig"], json!({"latency": "optimized"}));
    assert_eq!(
        body["guardrailConfig"],
        json!({
            "guardrailIdentifier": "guardrail123",
            "guardrailVersion": "DRAFT",
            "trace": "enabled_full"
        })
    );
    assert_eq!(
        body["additionalModelResponseFieldPaths"],
        json!(["/stop_sequence", "/nested~1field"])
    );
}

#[test]
fn validates_stream_specific_guardrail_controls() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::from([(
            "guardrailConfig".to_string(),
            json!({
                "guardrailIdentifier": "guardrail123",
                "guardrailVersion": "1",
                "streamProcessingMode": "async"
            }),
        )]),
    };
    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    assert_eq!(
        body["guardrailConfig"]["streamProcessingMode"],
        json!("async")
    );

    let non_streaming = CoreChatRequest {
        stream: false,
        ..request
    };
    let error = map_chat_request_to_converse(&non_streaming, &context("amazon.nova-pro-v1:0"))
        .expect_err("stream-only field rejected")
        .to_string();
    assert!(error.contains("streamProcessingMode"));
}

#[test]
fn rejects_invalid_request_scoped_converse_controls() {
    let too_many_metadata = Value::Object(
        (0..17)
            .map(|index| (format!("key{index}"), json!("value")))
            .collect(),
    );
    for (field, value, expected) in [
        ("requestMetadata", too_many_metadata, "at most 16 entries"),
        (
            "requestMetadata",
            json!({"invalid!key": "value"}),
            "keys must be 1-256",
        ),
        (
            "performanceConfig",
            json!({"latency": "fastest"}),
            "standard",
        ),
        (
            "guardrailConfig",
            json!({"guardrailVersion": "01"}),
            "positive version",
        ),
        (
            "guardrailConfig",
            json!({
                "guardrailIdentifier":
                    "arn:aws-:bedrock:us-east-1:123456789012:guardrail/abc"
            }),
            "Bedrock guardrail ARN",
        ),
        (
            "additionalModelResponseFieldPaths",
            json!(["not/a/pointer"]),
            "RFC 6901",
        ),
        (
            "additionalModelResponseFieldPaths",
            json!(["/invalid~2escape"]),
            "RFC 6901",
        ),
    ] {
        let request = CoreChatRequest {
            model: "nova".to_string(),
            messages: vec![message("user", "Hello")],
            stream: false,
            extra: BTreeMap::from([(field.to_string(), value)]),
        };
        let error = map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0"))
            .expect_err("invalid control rejected")
            .to_string();
        assert!(
            error.contains(expected),
            "expected `{expected}` in error for {field}: {error}"
        );
    }
}

#[test]
fn static_route_extra_body_still_overrides_validated_request_controls() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("requestMetadata".to_string(), json!({"source": "request"}))]),
    };
    let mut route_context = context("amazon.nova-pro-v1:0");
    route_context
        .extra_body
        .insert("requestMetadata".to_string(), json!({"source": "route"}));

    let body = map_chat_request_to_converse(&request, &route_context).expect("mapped");
    assert_eq!(body["requestMetadata"], json!({"source": "route"}));
}

#[test]
fn accepts_snake_case_converse_controls_and_rejects_alias_conflicts() {
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("request_metadata".to_string(), json!({"source": "request"}))]),
    };
    let body =
        map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0")).expect("mapped");
    assert_eq!(body["requestMetadata"], json!({"source": "request"}));

    let conflicting = CoreChatRequest {
        extra: BTreeMap::from([
            ("requestMetadata".to_string(), json!({"source": "camel"})),
            ("request_metadata".to_string(), json!({"source": "snake"})),
        ]),
        ..request
    };
    let error = map_chat_request_to_converse(&conflicting, &context("amazon.nova-pro-v1:0"))
        .expect_err("conflicting aliases rejected")
        .to_string();
    assert!(error.contains("conflicts with `request_metadata`"));
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
fn rejects_anthropic_base64_image_blocks_without_supported_media_type() {
    let missing_media_type = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([{
                "type": "image",
                "source": {
                    "type": "base64",
                    "data": "aW1hZ2U="
                }
            }]),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };

    let error = map_chat_request_to_anthropic_messages(
        &missing_media_type,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect_err("missing media type rejected")
    .to_string();
    assert!(error.contains("must include `media_type`"));

    let unsupported_media_type = CoreChatRequest {
        messages: vec![CoreChatMessage {
            content: json!([{
                "type": "image",
                "image_url": {
                    "source": {
                        "type": "base64",
                        "media_type": "application/pdf",
                        "data": "aW1hZ2U="
                    }
                }
            }]),
            ..message("user", "")
        }],
        ..missing_media_type
    };

    let error = map_chat_request_to_anthropic_messages(
        &unsupported_media_type,
        &context("anthropic.claude-3-haiku-20240307-v1:0"),
    )
    .expect_err("unsupported media type rejected")
    .to_string();
    assert!(error.contains("unsupported image media type `application/pdf`"));
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
