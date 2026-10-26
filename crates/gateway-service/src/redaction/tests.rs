use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{
    MAX_INLINE_REQUEST_BYTES, PayloadSecretRedactor, RequestLogPayloadCaptureMode,
    RequestLogPayloadPolicy, is_sensitive_json_key, mask_secret_leaf_values, parse_payload_path,
    redact_header_value, redact_json_value, redact_json_value_with_policy,
    sanitize_diagnostic_headers, truncate_large_payload_fields,
    truncate_large_payload_fields_with_count,
};
use crate::payload_bounding::{bound_request_payload_after_known_fields, serialized_size};

#[test]
fn request_policy_enforces_the_absolute_inline_ceiling() {
    let policy = RequestLogPayloadPolicy::new(
        RequestLogPayloadCaptureMode::RedactedPayloads,
        MAX_INLINE_REQUEST_BYTES * 2,
        64 * 1024,
        128,
        Vec::new(),
    );

    assert_eq!(policy.request_max_bytes, MAX_INLINE_REQUEST_BYTES);
    assert_eq!(policy.response_max_bytes, 64 * 1024);
}

#[test]
fn redacts_nested_sensitive_json_keys() {
    let input = json!({
        "token": "raw",
        "raw_key": "gwk_public.secret",
        "generated_key": "gwk_generated.secret",
        "key_material": "secret material",
        "nested": {
            "password": "secret",
            "keep": "value"
        }
    });

    let redacted = redact_json_value(&input);
    assert_eq!(redacted["token"], "[REDACTED]");
    assert_eq!(redacted["raw_key"], "[REDACTED]");
    assert_eq!(redacted["generated_key"], "[REDACTED]");
    assert_eq!(redacted["key_material"], "[REDACTED]");
    assert_eq!(redacted["nested"]["password"], "[REDACTED]");
    assert_eq!(redacted["nested"]["keep"], "value");
}

#[test]
fn header_redaction_keeps_non_sensitive_values() {
    assert_eq!(redact_header_value("x-trace-id", "trace-1"), "trace-1");
    assert_eq!(redact_header_value("authorization", "secret"), "[REDACTED]");
}

#[test]
fn diagnostic_headers_keep_only_session_and_lineage_fields() {
    let headers = BTreeMap::from([
        ("Authorization".to_string(), "Bearer secret".to_string()),
        ("Session_Id".to_string(), "session-1".to_string()),
        ("X-Client-Secret".to_string(), "client-secret".to_string()),
        (
            "X-Client-Request-Id".to_string(),
            "request-lineage-1".to_string(),
        ),
        (
            "X-Codex-Turn-Metadata".to_string(),
            json!({
                "session_id": "codex-session",
                "thread_id": "thread-1",
                "turn_id": {"token": "nested-secret"},
                "token": "embedded-secret",
                "unrelated": "not diagnostic"
            })
            .to_string(),
        ),
    ]);

    let sanitized = sanitize_diagnostic_headers(&headers);

    assert_eq!(sanitized["Session_Id"], "session-1");
    assert_eq!(sanitized["X-Client-Request-Id"], "request-lineage-1");
    assert!(sanitized.get("Authorization").is_none());
    assert!(sanitized.get("X-Client-Secret").is_none());
    let codex_metadata: serde_json::Value = serde_json::from_str(
        sanitized["X-Codex-Turn-Metadata"]
            .as_str()
            .expect("metadata string"),
    )
    .expect("sanitized metadata JSON");
    assert_eq!(codex_metadata["session_id"], "codex-session");
    assert_eq!(codex_metadata["thread_id"], "thread-1");
    assert!(codex_metadata.get("turn_id").is_none());
    assert!(codex_metadata.get("token").is_none());
    assert!(codex_metadata.get("unrelated").is_none());
}

#[test]
fn mask_secret_leaf_values_preserves_shape_with_asterisked_scalars() {
    let input = json!({
        "api_key": "raw-key",
        "service_account": {
            "client_email": "svc@example.com",
            "nested": [
                {"private_key": "-----BEGIN PRIVATE KEY-----"},
                42,
                true,
                null
            ]
        }
    });

    let masked = mask_secret_leaf_values(&input);

    assert_eq!(masked["api_key"], "********");
    assert_eq!(masked["service_account"]["client_email"], "********");
    assert_eq!(
        masked["service_account"]["nested"][0]["private_key"],
        "********"
    );
    assert_eq!(masked["service_account"]["nested"][1], "********");
    assert_eq!(masked["service_account"]["nested"][2], "********");
    assert_eq!(
        masked["service_account"]["nested"][3],
        serde_json::Value::Null
    );
}

#[test]
fn sensitive_json_key_check_normalizes_separators() {
    assert!(is_sensitive_json_key("x-api-key"));
    assert!(is_sensitive_json_key("refresh_token"));
}

#[test]
fn parses_payload_paths_with_wildcards() {
    let path = parse_payload_path("body.messages.*.content").expect("path parses");
    assert_eq!(path.as_string(), "body.messages.*.content");
    assert!(parse_payload_path("body..messages").is_err());
    assert!(parse_payload_path("body.messages[0]").is_err());
}

#[test]
fn redacts_operator_configured_paths() {
    let policy = RequestLogPayloadPolicy::new(
        RequestLogPayloadCaptureMode::RedactedPayloads,
        1024,
        1024,
        10,
        vec![parse_payload_path("body.messages.*.metadata.internal").expect("path")],
    );
    let input = json!({
        "body": {
            "messages": [
                {"metadata": {"internal": "secret", "public": "kept"}}
            ]
        }
    });

    let redacted = redact_json_value_with_policy(&input, &policy);

    assert_eq!(
        redacted["body"]["messages"][0]["metadata"]["internal"],
        "[REDACTED]"
    );
    assert_eq!(
        redacted["body"]["messages"][0]["metadata"]["public"],
        "kept"
    );
}

#[test]
fn applies_the_secret_redactor_after_path_redaction() {
    let policy = RequestLogPayloadPolicy::default().with_secret_redactor(
        PayloadSecretRedactor::new(|value| {
            if let Some(content) = value.pointer_mut("/body/messages/0/content") {
                *content = json!("[REDACTED:test-rule]");
            }
        }),
    );
    let input = json!({
        "headers": {"authorization": "Bearer abc"},
        "body": {"messages": [{"content": "leaked"}]}
    });

    let redacted = redact_json_value_with_policy(&input, &policy);

    assert_eq!(redacted["headers"]["authorization"], "[REDACTED]");
    assert_eq!(
        redacted["body"]["messages"][0]["content"],
        "[REDACTED:test-rule]"
    );
}

#[test]
fn truncates_known_large_provider_fields_without_changing_shape() {
    let input = json!({
        "body": {
            "messages": [
                {
                    "content": [
                        {
                            "type": "input_audio",
                            "input_audio": {
                                "data": "a".repeat(400),
                                "format": "wav"
                            }
                        }
                    ]
                }
            ]
        }
    });

    let truncated = truncate_large_payload_fields(&input);

    assert_eq!(
        truncated["body"]["messages"][0]["content"][0]["input_audio"]["data"]["truncated"],
        true
    );
    assert_eq!(
        truncated["body"]["messages"][0]["content"][0]["input_audio"]["format"],
        "wav"
    );
}

#[test]
fn leaves_normal_remote_image_urls_unchanged() {
    let input = json!({
        "body": {
            "messages": [
                {
                    "content": [
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": "https://example.com/image.png"
                            }
                        }
                    ]
                }
            ]
        }
    });

    let truncated = truncate_large_payload_fields(&redact_json_value(&input));

    assert_eq!(
        truncated["body"]["messages"][0]["content"][0]["image_url"]["url"],
        "https://example.com/image.png"
    );
}

#[test]
fn redacts_signed_media_url_queries_from_retained_payloads() {
    let input = json!({
        "body": {
            "messages": [{
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://media.example.invalid/image.png?token=image-secret"
                        }
                    },
                    {
                        "type": "video_url",
                        "video_url": {
                            "url": "https://media.example.invalid/video.mp4?expires=1&signature=video-secret"
                        }
                    },
                    {
                        "type": "file",
                        "file": {
                            "url": "https://media.example.invalid/file.pdf?credential=file-secret"
                        }
                    },
                    {
                        "type": "input_file",
                        "input_file": {
                            "url": "https://media.example.invalid/input.pdf?signature=input-secret"
                        }
                    },
                    {
                        "type": "document",
                        "document": {
                            "url": "https://media.example.invalid/document.pdf?signature=document-secret"
                        }
                    }
                ]
            }]
        }
    });

    let redacted = redact_json_value(&input);
    let content = redacted["body"]["messages"][0]["content"]
        .as_array()
        .expect("content");
    assert_eq!(
        content[0]["image_url"]["url"],
        "https://media.example.invalid/image.png?<redacted>"
    );
    assert_eq!(
        content[1]["video_url"]["url"],
        "https://media.example.invalid/video.mp4?<redacted>"
    );
    assert_eq!(
        content[2]["file"]["url"],
        "https://media.example.invalid/file.pdf?<redacted>"
    );
    assert_eq!(
        content[3]["input_file"]["url"],
        "https://media.example.invalid/input.pdf?<redacted>"
    );
    assert_eq!(
        content[4]["document"]["url"],
        "https://media.example.invalid/document.pdf?<redacted>"
    );
    let retained = redacted.to_string();
    for secret in [
        "image-secret",
        "video-secret",
        "file-secret",
        "input-secret",
        "document-secret",
    ] {
        assert!(!retained.contains(secret));
    }
}

#[test]
fn redacts_direct_responses_media_urls_before_storage() {
    let input = json!({
        "body": {
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_image",
                        "image_url": "https://user:password@media.example.invalid/image.png?token=image-secret"
                    },
                    {
                        "type": "input_file",
                        "file_url": "https://media.example.invalid/file.pdf?signature=file-secret"
                    },
                    {
                        "type": "input_video",
                        "video_url": "https://media.example.invalid/video.mp4?signature=video-secret"
                    },
                    {
                        "type": "input_audio",
                        "audio_url": "https://media.example.invalid/audio.wav?signature=audio-secret"
                    }
                ]
            }]
        }
    });

    let redacted = redact_json_value(&input);
    let content = redacted["body"]["input"][0]["content"]
        .as_array()
        .expect("content");
    assert_eq!(
        content[0]["image_url"],
        "https://media.example.invalid/image.png?<redacted>"
    );
    assert_eq!(
        content[1]["file_url"],
        "https://media.example.invalid/file.pdf?<redacted>"
    );
    assert_eq!(
        content[2]["video_url"],
        "https://media.example.invalid/video.mp4?<redacted>"
    );
    assert_eq!(
        content[3]["audio_url"],
        "https://media.example.invalid/audio.wav?<redacted>"
    );
    let retained = redacted.to_string();
    for secret in [
        "password",
        "image-secret",
        "file-secret",
        "video-secret",
        "audio-secret",
    ] {
        assert!(!retained.contains(secret));
    }
}

#[test]
fn truncates_large_direct_responses_media_fields() {
    let input = json!({
        "body": {
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_image",
                        "image_url": "data:image/png;base64,".to_string() + &"A".repeat(400)
                    },
                    {
                        "type": "input_file",
                        "file_data": "A".repeat(400)
                    }
                ]
            }]
        }
    });

    let truncated = truncate_large_payload_fields(&input);

    assert_eq!(
        truncated["body"]["input"][0]["content"][0]["image_url"]["truncated"],
        true
    );
    assert_eq!(
        truncated["body"]["input"][0]["content"][1]["file_data"]["truncated"],
        true
    );
}

#[test]
fn redacts_media_url_userinfo_from_retained_payloads() {
    let input = json!({
        "body": {
            "messages": [{
                "content": [{
                    "type": "video_url",
                    "video_url": {
                        "url": "https://user:password@media.example.invalid/video.mp4?signature=secret"
                    }
                }]
            }]
        }
    });

    let redacted = redact_json_value(&input);
    assert_eq!(
        redacted["body"]["messages"][0]["content"][0]["video_url"]["url"],
        "https://media.example.invalid/video.mp4?<redacted>"
    );
    let retained = redacted.to_string();
    for secret in ["user", "password", "secret"] {
        assert!(!retained.contains(secret));
    }
}

#[test]
fn redacts_signed_image_generation_urls() {
    let input = json!({
        "body": {
            "input_references": [{
                "type": "image_url",
                "image_url": {"url": "https://user:password@media.example.invalid/ref.png?X-Amz-Signature=ref-secret"}
            }],
            "messages": [{
                "role": "assistant",
                "images": [{"image_url": {"url": "https://media.example.invalid/prev.png?sig=replay-secret"}}]
            }],
            "choices": [{
                "message": {
                    "images": [{"image_url": {"url": "https://media.example.invalid/out.png?sig=out-secret"}}]
                }
            }]
        },
        "events": [{
            "choices": [{
                "delta": {
                    "images": [{"image_url": {"url": "https://media.example.invalid/delta.png?sig=delta-secret"}}]
                }
            }]
        }]
    });

    let redacted = redact_json_value(&input);

    assert_eq!(
        redacted["body"]["input_references"][0]["image_url"]["url"],
        "https://media.example.invalid/ref.png?<redacted>"
    );
    let retained = redacted.to_string();
    for secret in ["password", "ref-secret", "replay-secret", "out-secret", "delta-secret"] {
        assert!(!retained.contains(secret), "{secret} retained");
    }
}

#[test]
fn redacts_signed_media_urls_echoed_in_error_messages() {
    let input = json!({
        "body": {
            "error": {
                "message": "Vertex rejected HTTPS://media.example.invalid/video.mp4?signature=error-secret while processing the request"
            }
        }
    });

    let redacted = redact_json_value(&input);

    assert_eq!(
        redacted["body"]["error"]["message"],
        "Vertex rejected HTTPS://media.example.invalid/video.mp4?<redacted> while processing the request"
    );
    assert!(!redacted.to_string().contains("error-secret"));
}

#[test]
fn truncates_vertex_gemini_inline_data_fields() {
    let input = json!({
        "body": {
            "contents": [
                {
                    "parts": [
                        {
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": "a".repeat(400)
                            }
                        }
                    ]
                }
            ]
        }
    });

    let truncated = truncate_large_payload_fields(&input);

    assert_eq!(
        truncated["body"]["contents"][0]["parts"][0]["inlineData"]["data"]["truncated"],
        true
    );
    assert_eq!(
        truncated["body"]["contents"][0]["parts"][0]["inlineData"]["mimeType"],
        "image/png"
    );
}

#[test]
fn truncates_vertex_anthropic_base64_source_data_fields() {
    let input = json!({
        "body": {
            "messages": [
                {
                    "content": [
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/jpeg",
                                "data": "a".repeat(400)
                            }
                        }
                    ]
                }
            ]
        }
    });

    let truncated = truncate_large_payload_fields(&input);

    assert_eq!(
        truncated["body"]["messages"][0]["content"][0]["source"]["data"]["truncated"],
        true
    );
    assert_eq!(
        truncated["body"]["messages"][0]["content"][0]["source"]["media_type"],
        "image/jpeg"
    );
}

#[test]
fn policy_metadata_records_secret_redaction_settings() {
    let plain = RequestLogPayloadPolicy::default();
    let scanning = RequestLogPayloadPolicy::default().with_secret_redactor(
        PayloadSecretRedactor::new(|_| {}).with_metadata(json!({"tiers": ["credentials"]})),
    );

    assert!(plain.metadata_value().get("secret_redaction").is_none());
    assert_eq!(
        scanning.metadata_value()["secret_redaction"],
        json!({"tiers": ["credentials"]})
    );
}

fn png_data_url(base64_bytes: usize) -> String {
    format!(
        "data:image/png;base64,iVBORw0KGgo{}",
        "A".repeat(base64_bytes)
    )
}

/// Fernet-style encrypted reasoning state, which uses the URL-safe base64 alphabet.
fn encrypted_reasoning_data(bytes: usize) -> String {
    let mut data = String::from("gAAAAAB");
    while data.len() < bytes {
        data.push_str("Zm9v-_YmFy");
    }
    data
}

fn assert_blob_truncated(value: &Value, original: &str) {
    assert_eq!(value["truncated"], true, "blob not truncated: {value}");
    assert_eq!(value["size_bytes"], original.len());
    assert_eq!(value["preview"], original[..96]);
}

/// Mirrors non-stream response logging: wrap, redact, reduce known large fields, and
/// require the result to fit `response_max_bytes` so the whole-response marker is unused.
fn logged_response_payload(payload: Value) -> Value {
    let policy = RequestLogPayloadPolicy::default();
    let stored = truncate_large_payload_fields(&redact_json_value_with_policy(&payload, &policy));
    let stored_size = serialized_size(&stored).expect("stored payload serializes");
    assert!(
        stored_size <= policy.response_max_bytes,
        "stored response is {stored_size} bytes and would hit the whole-payload fallback"
    );
    stored
}

#[test]
fn bounds_chat_image_response_without_whole_payload_fallback() {
    let image = png_data_url(1_500_000);
    let reasoning = encrypted_reasoning_data(1_000_000);
    let usage = json!({
        "prompt_tokens": 12,
        "completion_tokens": 1306,
        "total_tokens": 1318,
        "completion_tokens_details": {"reasoning_tokens": 16, "image_tokens": 1290}
    });
    let response = json!({
        "id": "gen-1",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "Here is your image.",
                "images": [{"type": "image_url", "image_url": {"url": image}}],
                "reasoning_details": [{
                    "type": "reasoning.encrypted",
                    "format": "google-gemini-v1",
                    "index": 0,
                    "data": reasoning
                }]
            }
        }],
        "usage": usage
    });

    let stored = logged_response_payload(json!({ "body": response }));

    let message = &stored["body"]["choices"][0]["message"];
    assert_blob_truncated(&message["images"][0]["image_url"]["url"], &image);
    assert_eq!(message["images"][0]["type"], "image_url");
    assert_blob_truncated(&message["reasoning_details"][0]["data"], &reasoning);
    assert_eq!(
        message["reasoning_details"][0]["type"],
        "reasoning.encrypted"
    );
    assert_eq!(message["content"], "Here is your image.");
    assert_eq!(stored["body"]["usage"], usage);
}

#[test]
fn bounds_images_api_response_b64_json_without_whole_payload_fallback() {
    let image = format!("iVBORw0KGgo{}", "A".repeat(1_260_000));
    let usage = json!({
        "prompt_tokens": 14,
        "completion_tokens": 229,
        "total_tokens": 243,
        "cost": 0,
        "is_byok": true,
        "cost_details": {"upstream_inference_cost": 0.00694},
        "completion_tokens_details": {"reasoning_tokens": 0, "image_tokens": 229}
    });
    let response = json!({
        "created": 1_792_995_439,
        "data": [{"b64_json": image, "media_type": "image/png"}],
        "usage": usage
    });

    let stored = logged_response_payload(json!({ "body": response }));

    assert_blob_truncated(&stored["body"]["data"][0]["b64_json"], &image);
    assert_eq!(stored["body"]["data"][0]["media_type"], "image/png");
    assert_eq!(stored["body"]["created"], 1_792_995_439);
    assert_eq!(stored["body"]["usage"], usage);
}

#[test]
fn bounds_streamed_image_and_reasoning_deltas() {
    let image = png_data_url(1_500_000);
    let reasoning = encrypted_reasoning_data(1_000_000);
    let usage = json!({"prompt_tokens": 12, "completion_tokens": 1306, "total_tokens": 1318});
    let payload = json!({
        "stream": true,
        "events": [
            {"choices": [{"index": 0, "delta": {
                "reasoning_details": [{"type": "reasoning.encrypted", "data": reasoning}]
            }}]},
            {"choices": [{"index": 0, "delta": {
                "role": "assistant",
                "images": [{"type": "image_url", "image_url": {"url": image}}]
            }}]},
            {"choices": [], "usage": usage}
        ],
        "usage": usage,
        "error": null
    });

    let stored = logged_response_payload(payload);

    let events = &stored["events"];
    assert_blob_truncated(
        &events[0]["choices"][0]["delta"]["reasoning_details"][0]["data"],
        &reasoning,
    );
    assert_blob_truncated(
        &events[1]["choices"][0]["delta"]["images"][0]["image_url"]["url"],
        &image,
    );
    assert_eq!(events[1]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(events[2]["usage"], usage);
    assert_eq!(stored["usage"], usage);
}

#[test]
fn bounds_multi_turn_image_edit_request_without_whole_payload_fallback() {
    let previous_image = png_data_url(1_500_000);
    let reasoning = encrypted_reasoning_data(1_000_000);
    let user_image = png_data_url(600_000);
    let request = json!({
        "model": "google/gemini-3.1-flash-lite-image",
        "modalities": ["image", "text"],
        "messages": [
            {"role": "user", "content": "Draw a red panda."},
            {
                "role": "assistant",
                "content": "Here is your red panda.",
                "images": [{"type": "image_url", "image_url": {"url": previous_image}}],
                "reasoning_details": [{"type": "reasoning.encrypted", "data": reasoning}]
            },
            {"role": "user", "content": [
                {"type": "text", "text": "Give it this hat."},
                {"type": "image_url", "image_url": {"url": user_image}}
            ]}
        ]
    });
    let policy = RequestLogPayloadPolicy::default();
    let redacted = redact_json_value_with_policy(&json!({"headers": {}, "body": request}), &policy);
    let (storage, known_large_fields) = truncate_large_payload_fields_with_count(&redacted);

    let (stored, truncated) = bound_request_payload_after_known_fields(
        storage,
        policy.request_max_bytes,
        serialized_size(&redacted).ok(),
        known_large_fields,
    );

    assert_eq!(known_large_fields, 3);
    assert!(truncated);
    assert!(stored.get("body").is_some(), "hard fallback: {stored}");
    assert_eq!(stored["truncation"]["known_large_fields_truncated"], 3);
    let messages = &stored["body"]["messages"];
    assert_blob_truncated(
        &messages[1]["images"][0]["image_url"]["url"],
        &previous_image,
    );
    assert_blob_truncated(&messages[1]["reasoning_details"][0]["data"], &reasoning);
    assert_eq!(messages[1]["content"], "Here is your red panda.");
    assert_blob_truncated(&messages[2]["content"][1]["image_url"]["url"], &user_image);
    assert_eq!(messages[2]["content"][0]["text"], "Give it this hat.");
    assert_eq!(stored["body"]["modalities"], json!(["image", "text"]));
}

#[test]
fn bounds_images_api_input_references() {
    let reference = png_data_url(800_000);
    let input = json!({"body": {
        "model": "openai/gpt-image-2.5-flare",
        "prompt": "make this scene look like a watercolor painting",
        "input_references": [
            {"type": "image_url", "image_url": {"url": reference}},
            {"type": "image_url", "image_url": {"url": "https://example.com/photo.jpg"}}
        ]
    }});

    let (truncated, count) = truncate_large_payload_fields_with_count(&input);

    assert_eq!(count, 1);
    let references = &truncated["body"]["input_references"];
    assert_blob_truncated(&references[0]["image_url"]["url"], &reference);
    assert_eq!(
        references[1]["image_url"]["url"],
        "https://example.com/photo.jpg"
    );
}

#[test]
fn url_safe_base64_is_only_treated_as_opaque_at_reasoning_paths() {
    let opaque = encrypted_reasoning_data(400);
    let input = json!({"body": {
        "data": [{"b64_json": opaque}],
        "choices": [{"message": {"reasoning_details": [
            {"type": "reasoning.encrypted", "data": opaque},
            {"type": "reasoning.encrypted", "data": "gAAAAAB-short_token"}
        ]}}]
    }});

    let (truncated, count) = truncate_large_payload_fields_with_count(&input);

    assert_eq!(count, 1);
    assert_eq!(truncated["body"]["data"][0]["b64_json"], opaque);
    let details = &truncated["body"]["choices"][0]["message"]["reasoning_details"];
    assert_blob_truncated(&details[0]["data"], &opaque);
    assert_eq!(details[1]["data"], "gAAAAAB-short_token");
}
