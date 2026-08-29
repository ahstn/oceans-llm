use std::collections::BTreeMap;

use serde_json::json;

use super::{
    MAX_INLINE_REQUEST_BYTES, RequestLogPayloadCaptureMode, RequestLogPayloadPolicy,
    is_sensitive_json_key, mask_secret_leaf_values, parse_payload_path, redact_header_value,
    redact_json_value, redact_json_value_with_policy, sanitize_diagnostic_headers,
    truncate_large_payload_fields,
};

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
