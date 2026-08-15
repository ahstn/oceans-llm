use super::*;

#[test]
fn maps_openai_request_to_google_payload() {
    let request = CoreChatRequest {
        model: "fast".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([
                {"type":"text","text":"Describe this"},
                {"type":"image_url","image_url":{"url":"gs://bucket/pic.png","mime_type":"image/png"}}
            ]),
            name: None,
            extra: std::collections::BTreeMap::new(),
        }],
        stream: false,
        extra: std::collections::BTreeMap::new(),
    };
    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(mapped["contents"][0]["role"], "user");
    assert_eq!(mapped["contents"][0]["parts"][0]["text"], "Describe this");
    assert_eq!(
        mapped["contents"][0]["parts"][1]["fileData"]["fileUri"],
        "gs://bucket/pic.png"
    );
}

#[test]
fn maps_image_only_https_request_to_google_file_data() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "image_url",
            "image_url": {
                "url": "https://media.example.invalid/image.png?version=1",
                "mime_type": "image/png"
            }
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(
        mapped["contents"][0]["parts"][0]["fileData"],
        json!({
            "fileUri": "https://media.example.invalid/image.png?version=1",
            "mimeType": "image/png"
        })
    );
}

#[test]
fn preserves_existing_gs_file_mapping() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "file",
            "file": {
                "url": "gs://bucket/document.pdf",
                "mime_type": "application/pdf"
            }
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(
        mapped["contents"][0]["parts"][0]["fileData"],
        json!({
            "fileUri": "gs://bucket/document.pdf",
            "mimeType": "application/pdf"
        })
    );
}

#[test]
fn maps_signed_https_video_url_to_google_file_data() {
    let signed_url =
        "https://media.example.invalid/video.mp4?expires=1700000000&signature=%3Credacted%3E";
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "video_url",
            "video_url": {
                "url": signed_url,
                "mime_type": "video/mp4"
            }
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(
        mapped["contents"][0]["parts"][0],
        json!({
            "fileData": {
                "fileUri": signed_url,
                "mimeType": "video/mp4"
            }
        })
    );
}

#[test]
fn maps_generic_https_video_file_to_google_file_data() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "file",
            "file": {
                "url": "https://media.example.invalid/video.mp4",
                "mediaType": "video/mp4"
            }
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(
        mapped["contents"][0]["parts"][0]["fileData"],
        json!({
            "fileUri": "https://media.example.invalid/video.mp4",
            "mimeType": "video/mp4"
        })
    );
}

#[test]
fn infers_video_mime_from_signed_url_path_and_prefers_explicit_mime() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {
                "type": "video_url",
                "video_url": {
                    "url": "https://media.example.invalid/inferred.mp4?token=secret"
                }
            },
            {
                "type": "video_url",
                "video_url": {
                    "url": "https://media.example.invalid/explicit.mov?token=secret",
                    "media_type": "video/mp4"
                }
            }
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(
        mapped["contents"][0]["parts"][0]["fileData"]["mimeType"],
        "video/mp4"
    );
    assert_eq!(
        mapped["contents"][0]["parts"][1]["fileData"]["mimeType"],
        "video/mp4"
    );
}

#[test]
fn rejects_unsupported_media_uri_and_unknown_mime() {
    for (content, expected_error) in [
        (
            json!([{
                "type": "video_url",
                "video_url": {
                    "url": "file:///tmp/video.mp4",
                    "mime_type": "video/mp4"
                }
            }]),
            "URI scheme `file`",
        ),
        (
            json!([{
                "type": "video_url",
                "video_url": {
                    "url": "gs:bucket/video.mp4",
                    "mime_type": "video/mp4"
                }
            }]),
            "must include a host",
        ),
        (
            json!([{
                "type": "video_url",
                "video_url": {
                    "url": "https://media.example.invalid/video"
                }
            }]),
            "could not infer MIME type",
        ),
    ] {
        let request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content,
            name: None,
            extra: BTreeMap::new(),
        }]);
        let error = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
            .expect_err("invalid media must be rejected");
        assert!(
            matches!(
                error,
                ProviderError::InvalidRequest(message) if message.contains(expected_error)
            ),
            "expected error containing {expected_error}"
        );
    }
}

#[test]
fn rejects_video_image_mime_and_conflicting_mime_aliases() {
    for (media, expected_error) in [
        (
            json!({
                "url": "https://media.example.invalid/video.mp4",
                "mime_type": "image/png"
            }),
            "requires a video/ MIME type",
        ),
        (
            json!({
                "url": "https://media.example.invalid/video.mp4",
                "mime_type": "video/mp4",
                "mediaType": "video/mov"
            }),
            "MIME type fields conflict",
        ),
    ] {
        let request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([{"type": "video_url", "video_url": media}]),
            name: None,
            extra: BTreeMap::new(),
        }]);
        let error = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
            .expect_err("invalid video MIME type must be rejected");
        assert!(
            matches!(
                error,
                ProviderError::InvalidRequest(message) if message.contains(expected_error)
            ),
            "expected error containing {expected_error}"
        );
    }
}

#[test]
fn preserves_mixed_text_image_and_video_part_order() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {"type": "text", "text": "Compare the media"},
            {
                "type": "image_url",
                "image_url": {
                    "url": "gs://bucket/image.png",
                    "mime_type": "image/png"
                }
            },
            {
                "type": "input_video",
                "input_video": {
                    "url": "https://media.example.invalid/video.mp4?signature=secret"
                }
            }
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped =
        map_google_request(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    let parts = mapped["contents"][0]["parts"].as_array().expect("parts");
    assert_eq!(parts[0]["text"], "Compare the media");
    assert_eq!(parts[1]["fileData"]["fileUri"], "gs://bucket/image.png");
    assert_eq!(
        parts[2]["fileData"]["fileUri"],
        "https://media.example.invalid/video.mp4?signature=secret"
    );
}

#[test]
fn rejects_google_streaming_multiple_candidates_from_n() {
    let mut request = CoreChatRequest {
        model: "fast".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("ping".to_string()),
            name: None,
            extra: std::collections::BTreeMap::new(),
        }],
        stream: true,
        extra: std::collections::BTreeMap::new(),
    };
    request.extra.insert("n".to_string(), json!(2));

    let error = map_google_request(&request, &context("google/gemini-2.0-flash"), true)
        .expect_err("streaming n>1 should be rejected");

    match error {
        ProviderError::InvalidRequest(message) => {
            assert!(message.contains("single candidate"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn rejects_google_streamed_function_call_arguments() {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("use a tool".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    request
        .extra
        .insert("tool_choice".to_string(), json!("auto"));
    let mut context = context("google/gemini-3-flash");
    context.extra_body.insert(
        "toolConfig".to_string(),
        json!({
            "functionCallingConfig": {
                "streamFunctionCallArguments": true
            }
        }),
    );

    let error = map_google_request(&request, &context, true)
        .expect_err("partial function argument streaming must be rejected");

    assert!(matches!(
        error,
        ProviderError::InvalidRequest(message)
            if message.contains("streamFunctionCallArguments")
    ));
}

#[test]
fn rejects_google_streaming_multiple_candidates_from_route_override() {
    let request = CoreChatRequest {
        model: "fast".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("ping".to_string()),
            name: None,
            extra: std::collections::BTreeMap::new(),
        }],
        stream: true,
        extra: std::collections::BTreeMap::new(),
    };
    let mut context = context("google/gemini-2.0-flash");
    context.extra_body.insert(
        "generationConfig".to_string(),
        json!({ "candidateCount": 2 }),
    );

    let error =
        map_google_request(&request, &context, true).expect_err("route override should win");

    match error {
        ProviderError::InvalidRequest(message) => {
            assert!(message.contains("single candidate"));
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn allows_google_non_streaming_multiple_candidates() {
    let mut request = CoreChatRequest {
        model: "fast".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("ping".to_string()),
            name: None,
            extra: std::collections::BTreeMap::new(),
        }],
        stream: false,
        extra: std::collections::BTreeMap::new(),
    };
    request.extra.insert("n".to_string(), json!(2));

    let mapped = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
        .expect("non-stream n>1 remains allowed");

    assert_eq!(mapped["generationConfig"]["candidateCount"], json!(2));
}
