use super::*;

/// A single-turn user request with the given OpenAI-shaped `extra` fields.
fn ping_request(extra: &[(&str, Value)]) -> CoreChatRequest {
    let mut request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: Value::String("ping".to_string()),
        name: None,
        extra: BTreeMap::new(),
    }]);
    for (key, value) in extra {
        request.extra.insert((*key).to_string(), value.clone());
    }
    request
}

/// Maps a ping request for `model` and returns its `generationConfig`.
fn generation_config(model: &str, extra: &[(&str, Value)]) -> Result<Value, ProviderError> {
    google_body(
        &ping_request(extra),
        &context(&format!("google/{model}")),
        false,
    )
    .map(|body| body["generationConfig"].clone())
}

fn thinking_config(model: &str, extra: &[(&str, Value)]) -> Value {
    generation_config(model, extra).expect("mapped")["thinkingConfig"].clone()
}

fn assert_invalid_request(error: ProviderError, keyword: &str) {
    match error {
        ProviderError::InvalidRequest(message) => {
            assert!(
                message.contains(keyword),
                "expected `{keyword}` in error `{message}`"
            );
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn maps_openai_request_to_google_payload() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {"type":"text","text":"Describe this"},
            {"type":"image_url","image_url":{"url":"gs://bucket/pic.png","mime_type":"image/png"}}
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);
    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(mapped["contents"][0]["role"], "user");
    assert_eq!(mapped["contents"][0]["parts"][0]["text"], "Describe this");
    assert_eq!(
        mapped["contents"][0]["parts"][1]["fileData"]["fileUri"],
        "gs://bucket/pic.png"
    );
    assert!(mapped.get("generationConfig").is_none());
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

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
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

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
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

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
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

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
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

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
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
fn infers_vertex_supported_video_mime_types() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([
            {"type": "video_url", "video_url": {"url": "https://media.example.invalid/clip.webm"}},
            {"type": "video_url", "video_url": {"url": "https://media.example.invalid/clip.mov"}},
            {"type": "video_url", "video_url": {"url": "https://media.example.invalid/clip.flv"}}
        ]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    let parts = mapped["contents"][0]["parts"].as_array().expect("parts");
    assert_eq!(parts[0]["fileData"]["mimeType"], "video/webm");
    assert_eq!(parts[1]["fileData"]["mimeType"], "video/quicktime");
    assert_eq!(parts[2]["fileData"]["mimeType"], "video/x-flv");
}

#[test]
fn maps_input_file_alias_to_google_file_data() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([{
            "type": "input_file",
            "input_file": {"url": "https://media.example.invalid/report.pdf"}
        }]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    assert_eq!(
        mapped["contents"][0]["parts"][0]["fileData"],
        json!({
            "fileUri": "https://media.example.invalid/report.pdf",
            "mimeType": "application/pdf"
        })
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
                    "url": "https://user:password@media.example.invalid/video.mp4",
                    "mime_type": "video/mp4"
                }
            }]),
            "must not include user credentials",
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
        let error = google_body(&request, &context("google/gemini-2.0-flash"), false)
            .expect_err("invalid media must be rejected");
        assert_invalid_request(error, expected_error);
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
        (
            json!({
                "url": "https://media.example.invalid/video.mp4",
                "mime_type": "video/"
            }),
            "must be a valid MIME type",
        ),
        (
            json!({
                "url": "https://media.example.invalid/video.mp4",
                "mime_type": "video/not a type"
            }),
            "must be a valid MIME type",
        ),
    ] {
        let request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!([{"type": "video_url", "video_url": media}]),
            name: None,
            extra: BTreeMap::new(),
        }]);
        let error = google_body(&request, &context("google/gemini-2.0-flash"), false)
            .expect_err("invalid video MIME type must be rejected");
        assert_invalid_request(error, expected_error);
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

    let mapped = google_body(&request, &context("google/gemini-2.0-flash"), false).expect("mapped");
    let parts = mapped["contents"][0]["parts"].as_array().expect("parts");
    assert_eq!(parts[0]["text"], "Compare the media");
    assert_eq!(parts[1]["fileData"]["fileUri"], "gs://bucket/image.png");
    assert_eq!(
        parts[2]["fileData"]["fileUri"],
        "https://media.example.invalid/video.mp4?signature=secret"
    );
}

#[test]
fn maps_sampling_keys_into_generation_config() {
    let config = generation_config(
        "gemini-2.0-flash",
        &[
            ("temperature", json!(0.7)),
            ("top_p", json!(0.9)),
            ("top_k", json!(40)),
            ("presence_penalty", json!(0.1)),
            ("frequency_penalty", json!(0.2)),
            ("seed", json!(42)),
            ("n", json!(3)),
            ("logprobs", json!(true)),
            ("top_logprobs", json!(5)),
            ("max_completion_tokens", json!(256)),
            ("stop", json!("END")),
        ],
    )
    .expect("mapped");

    assert_eq!(
        config,
        json!({
            "temperature": 0.7,
            "topP": 0.9,
            "topK": 40,
            "presencePenalty": 0.1,
            "frequencyPenalty": 0.2,
            "seed": 42,
            "candidateCount": 3,
            "responseLogprobs": true,
            "logprobs": 5,
            "maxOutputTokens": 256,
            "stopSequences": ["END"]
        })
    );
}

#[test]
fn passes_stop_array_through_and_keeps_openai_keys_out_of_body_root() {
    let mapped = google_body(
        &ping_request(&[
            ("stop", json!(["END", "STOP"])),
            ("temperature", json!(0.3)),
        ]),
        &context("google/gemini-2.0-flash"),
        false,
    )
    .expect("mapped");

    assert_eq!(
        mapped["generationConfig"]["stopSequences"],
        json!(["END", "STOP"])
    );
    assert!(mapped.get("stop").is_none());
    assert!(mapped.get("temperature").is_none());
}

#[test]
fn accepts_matching_max_tokens_aliases() {
    let config = generation_config(
        "gemini-2.0-flash",
        &[
            ("max_tokens", json!(100)),
            ("max_completion_tokens", json!(100)),
        ],
    )
    .expect("matching aliases are fine");
    assert_eq!(config["maxOutputTokens"], json!(100));

    let config =
        generation_config("gemini-2.0-flash", &[("max_tokens", json!(64))]).expect("mapped");
    assert_eq!(config["maxOutputTokens"], json!(64));
}

#[test]
fn rejects_conflicting_max_tokens_aliases() {
    let error = generation_config(
        "gemini-2.0-flash",
        &[
            ("max_tokens", json!(100)),
            ("max_completion_tokens", json!(200)),
        ],
    )
    .expect_err("differing max token aliases must be rejected");
    assert_invalid_request(error, "`max_completion_tokens` conflicts with `max_tokens`");
}

#[test]
fn drops_openai_only_fields_and_passes_unknown_fields_to_body_root() {
    let mapped = google_body(
        &ping_request(&[
            ("stream_options", json!({"include_usage": true})),
            ("user", json!("user-123")),
            ("store", json!(true)),
            ("logit_bias", json!({"50256": -100})),
            ("metadata", json!({"trace": "abc"})),
            ("service_tier", json!("default")),
            ("prompt_cache_key", json!("cache-1")),
            ("safety_identifier", json!("safe-1")),
            (
                "safetySettings",
                json!([{"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE"}]),
            ),
        ]),
        &context("google/gemini-2.0-flash"),
        false,
    )
    .expect("mapped");

    let rendered = mapped.to_string();
    for dropped in [
        "stream_options",
        "include_usage",
        "user-123",
        "\"store\"",
        "logit_bias",
        "50256",
        "\"metadata\"",
        "service_tier",
        "prompt_cache_key",
        "safety_identifier",
    ] {
        assert!(
            !rendered.contains(dropped),
            "`{dropped}` must not reach the vertex body: {rendered}"
        );
    }
    assert_eq!(
        mapped["safetySettings"][0]["category"],
        "HARM_CATEGORY_HARASSMENT"
    );
    assert!(mapped.get("generationConfig").is_none());
}

#[test]
fn maps_reasoning_effort_to_thinking_level_on_gemini_3_flash() {
    for (effort, level) in [
        // 3.7+ Flash has no MINIMAL level; the lowest supported level is used instead.
        ("minimal", "LOW"),
        ("low", "LOW"),
        ("medium", "MEDIUM"),
        ("high", "HIGH"),
        ("xhigh", "HIGH"),
        ("max", "HIGH"),
    ] {
        assert_eq!(
            thinking_config("gemini-3.7-flash", &[("reasoning_effort", json!(effort))]),
            json!({"thinkingLevel": level, "includeThoughts": true}),
            "effort {effort}"
        );
    }
    assert_eq!(
        thinking_config(
            "gemini-3.6-flash",
            &[("reasoning_effort", json!("minimal"))]
        ),
        json!({"thinkingLevel": "MINIMAL", "includeThoughts": true})
    );
}

#[test]
fn gemini_3_7_drops_deprecated_sampling_and_rejects_unsupported_fields() {
    // Ignored upstream: dropped rather than forwarded.
    let config = generation_config(
        "gemini-3.8-flash",
        &[
            ("temperature", json!(0.2)),
            ("top_p", json!(0.9)),
            ("top_k", json!(40)),
            ("seed", json!(7)),
            // No-op defaults are tolerated so stock OpenAI clients keep working.
            ("n", json!(1)),
            ("presence_penalty", json!(0)),
            ("frequency_penalty", json!(0.0)),
        ],
    )
    .expect("mapped");
    assert_eq!(config, json!({"seed": 7}));

    // Older models keep the full sampling surface.
    let config = generation_config(
        "gemini-3.6-flash",
        &[
            ("temperature", json!(0.2)),
            ("presence_penalty", json!(0.5)),
        ],
    )
    .expect("mapped");
    assert_eq!(config["temperature"], json!(0.2));
    assert_eq!(config["presencePenalty"], json!(0.5));

    // Non-default values Vertex would reject are refused locally with the field named.
    for (field, value) in [
        ("presence_penalty", json!(0.5)),
        ("frequency_penalty", json!(-1)),
        ("n", json!(2)),
    ] {
        let error = generation_config("gemini-3.7-flash", &[(field, value)])
            .expect_err("unsupported sampling field");
        assert_invalid_request(error, field);
    }
}

#[test]
fn collapses_thinking_levels_for_gemini_3_pro() {
    for (effort, level) in [
        ("minimal", "LOW"),
        ("low", "LOW"),
        ("medium", "HIGH"),
        ("high", "HIGH"),
    ] {
        assert_eq!(
            thinking_config(
                "gemini-3.1-pro-preview",
                &[("reasoning_effort", json!(effort))]
            ),
            json!({"thinkingLevel": level, "includeThoughts": true}),
            "effort {effort}"
        );
    }
}

#[test]
fn maps_reasoning_effort_to_thinking_budget_on_gemini_2_5() {
    for (effort, budget) in [
        ("minimal", 128),
        ("low", 2048),
        ("medium", 8192),
        ("high", 24_576),
        ("xhigh", 24_576),
    ] {
        assert_eq!(
            thinking_config("gemini-2.5-flash", &[("reasoning_effort", json!(effort))]),
            json!({"thinkingBudget": budget, "includeThoughts": true}),
            "effort {effort}"
        );
    }
    assert_eq!(
        thinking_config("gemini-2.5-pro", &[("reasoning_effort", json!("high"))]),
        json!({"thinkingBudget": 32_768, "includeThoughts": true})
    );
}

#[test]
fn disables_thoughts_for_effort_none() {
    assert_eq!(
        thinking_config("gemini-3.7-flash", &[("reasoning_effort", json!("none"))]),
        json!({"thinkingLevel": "LOW", "includeThoughts": false})
    );
    assert_eq!(
        thinking_config(
            "gemini-3.1-pro-preview",
            &[("reasoning_effort", json!("none"))]
        ),
        json!({"thinkingLevel": "LOW", "includeThoughts": false})
    );
    assert_eq!(
        thinking_config("gemini-2.5-flash", &[("reasoning_effort", json!("none"))]),
        json!({"thinkingBudget": 0, "includeThoughts": false})
    );
    // 2.5 Pro cannot disable thinking; it gets the 128-token floor instead of a rejected 0.
    assert_eq!(
        thinking_config("gemini-2.5-pro", &[("reasoning_effort", json!("none"))]),
        json!({"thinkingBudget": 128, "includeThoughts": false})
    );
    // `off` is the gateway's other spelling of disabled thinking.
    assert_eq!(
        thinking_config("gemini-3.7-flash", &[("reasoning_effort", json!("OFF"))]),
        json!({"thinkingLevel": "LOW", "includeThoughts": false})
    );
}

#[test]
fn both_generation_config_aliases_merge_instead_of_replacing() {
    let config = generation_config(
        "gemini-3.7-flash",
        &[
            ("generationConfig", json!({"seed": 7, "temperature": 0.1})),
            ("generation_config", json!({"topK": 5, "temperature": 0.9})),
        ],
    )
    .expect("mapped");

    assert_eq!(config["seed"], 7);
    assert_eq!(config["topK"], 5);
    // The later alias wins on overlap.
    assert_eq!(config["temperature"], 0.9);
}

#[test]
fn empty_user_content_array_gets_an_empty_text_part() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!([]),
        name: None,
        extra: BTreeMap::new(),
    }]);

    let body = google_body(&request, &context("google/gemini-3.7-flash"), false).expect("mapped");

    assert_eq!(body["contents"][0]["parts"], json!([{"text": ""}]));
}

#[test]
fn drops_additional_openai_only_fields_from_body_root() {
    let body = google_body(
        &ping_request(&[
            ("modalities", json!(["text"])),
            ("audio", json!({"voice": "alloy"})),
            ("prediction", json!({"type": "content", "content": "x"})),
            ("verbosity", json!("low")),
            ("web_search_options", json!({})),
        ]),
        &context("google/gemini-3.7-flash"),
        false,
    )
    .expect("mapped");
    let rendered = body.to_string();
    for key in [
        "modalities",
        "audio",
        "prediction",
        "verbosity",
        "web_search_options",
    ] {
        assert!(!rendered.contains(key), "{key} leaked into the body");
    }
}

#[test]
fn accepts_reasoning_object_effort() {
    assert_eq!(
        thinking_config(
            "gemini-3.7-flash",
            &[("reasoning", json!({"effort": "low"}))]
        ),
        json!({"thinkingLevel": "LOW", "includeThoughts": true})
    );
    // Same value in both places is not a conflict.
    assert_eq!(
        thinking_config(
            "gemini-3.7-flash",
            &[
                ("reasoning", json!({"effort": "high"})),
                ("reasoning_effort", json!("high")),
            ]
        ),
        json!({"thinkingLevel": "HIGH", "includeThoughts": true})
    );
    // `reasoning` without an effort leaves thinking untouched.
    let config = generation_config(
        "gemini-3.7-flash",
        &[("reasoning", json!({"summary": "auto"}))],
    )
    .expect("mapped");
    assert!(config.get("thinkingConfig").is_none());
}

#[test]
fn rejects_conflicting_reasoning_effort_sources() {
    let error = generation_config(
        "gemini-3.7-flash",
        &[
            ("reasoning", json!({"effort": "low"})),
            ("reasoning_effort", json!("high")),
        ],
    )
    .expect_err("differing efforts must be rejected");
    assert_invalid_request(
        error,
        "`reasoning_effort` conflicts with `reasoning.effort`",
    );
}

#[test]
fn rejects_invalid_reasoning_effort_values() {
    let error = generation_config(
        "gemini-3.7-flash",
        &[("reasoning_effort", json!("extreme"))],
    )
    .expect_err("unknown effort must be rejected");
    assert_invalid_request(error, "unsupported `reasoning_effort` `extreme`");

    let error = generation_config("gemini-3.7-flash", &[("reasoning_effort", json!(3))])
        .expect_err("non-string effort must be rejected");
    assert_invalid_request(error, "`reasoning_effort` must be a string");

    let error = generation_config("gemini-3.7-flash", &[("reasoning", json!("high"))])
        .expect_err("non-object reasoning must be rejected");
    assert_invalid_request(error, "`reasoning` must be an object");
}

#[test]
fn rejects_reasoning_effort_on_non_thinking_model() {
    for effort in ["low", "none"] {
        let error = generation_config("gemini-2.0-flash", &[("reasoning_effort", json!(effort))])
            .expect_err("gemini 2.0 has no thinking");
        assert_invalid_request(error, "`gemini-2.0-flash` does not support thinking");
    }
}

#[test]
fn omits_thinking_config_without_reasoning_effort() {
    let config =
        generation_config("gemini-3.7-flash", &[("temperature", json!(0.5))]).expect("mapped");
    assert!(config.get("thinkingConfig").is_none());
    assert!(
        generation_config("gemini-3.7-flash", &[])
            .expect("mapped")
            .is_null()
    );
}

#[test]
fn merges_caller_generation_config_with_mapped_sampling() {
    for alias in ["generationConfig", "generation_config"] {
        let config = generation_config(
            "gemini-3.7-flash",
            &[
                ("max_tokens", json!(100)),
                ("temperature", json!(0.9)),
                ("reasoning_effort", json!("low")),
                (alias, json!({"temperature": 0.2, "topK": 7})),
            ],
        )
        .expect("mapped");

        assert_eq!(
            config,
            json!({
                "maxOutputTokens": 100,
                "temperature": 0.2,
                "topK": 7,
                "thinkingConfig": {"thinkingLevel": "LOW", "includeThoughts": true}
            }),
            "alias {alias}"
        );
    }
}

#[test]
fn rejects_non_object_caller_generation_config() {
    let error = generation_config("gemini-3.7-flash", &[("generationConfig", json!("fast"))])
        .expect_err("non-object generationConfig must be rejected");
    assert_invalid_request(error, "`generationConfig` must be an object");
}

#[test]
fn rejects_reasoning_effort_alongside_caller_thinking_config() {
    let error = generation_config(
        "gemini-3.7-flash",
        &[
            ("reasoning_effort", json!("high")),
            (
                "generationConfig",
                json!({"thinkingConfig": {"thinkingLevel": "LOW"}}),
            ),
        ],
    )
    .expect_err("two thinking sources must be rejected");
    assert_invalid_request(error, "`generationConfig.thinkingConfig`");

    // Caller-only thinkingConfig is passed through untouched.
    let config = generation_config(
        "gemini-3.7-flash",
        &[(
            "generationConfig",
            json!({"thinkingConfig": {"thinkingLevel": "LOW"}}),
        )],
    )
    .expect("mapped");
    assert_eq!(config["thinkingConfig"], json!({"thinkingLevel": "LOW"}));
}

#[test]
fn maps_response_format_json_object() {
    let config = generation_config(
        "gemini-3.7-flash",
        &[("response_format", json!({"type": "json_object"}))],
    )
    .expect("mapped");
    assert_eq!(config, json!({"responseMimeType": "application/json"}));
}

#[test]
fn maps_response_format_json_schema() {
    let schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    });
    let config = generation_config(
        "gemini-3.7-flash",
        &[(
            "response_format",
            json!({
                "type": "json_schema",
                "json_schema": {"name": "answer", "strict": true, "schema": schema}
            }),
        )],
    )
    .expect("mapped");

    assert_eq!(config["responseMimeType"], "application/json");
    assert_eq!(config["responseJsonSchema"], schema);
    assert!(config.get("responseSchema").is_none());
}

#[test]
fn response_format_text_maps_to_nothing() {
    let config = generation_config(
        "gemini-3.7-flash",
        &[
            ("response_format", json!({"type": "text"})),
            ("seed", json!(1)),
        ],
    )
    .expect("mapped");
    assert_eq!(config, json!({"seed": 1}));
}

#[test]
fn rejects_unsupported_or_malformed_response_format() {
    let error = generation_config(
        "gemini-3.7-flash",
        &[("response_format", json!({"type": "xml"}))],
    )
    .expect_err("unknown response_format type must be rejected");
    assert_invalid_request(error, "`response_format.type` must be");

    let error = generation_config(
        "gemini-3.7-flash",
        &[(
            "response_format",
            json!({"type": "json_schema", "json_schema": {"name": "x"}}),
        )],
    )
    .expect_err("json_schema without a schema must be rejected");
    assert_invalid_request(
        error,
        "`response_format.json_schema.schema` must be an object",
    );

    let error = generation_config(
        "gemini-3.7-flash",
        &[("response_format", json!("json_object"))],
    )
    .expect_err("non-object response_format must be rejected");
    assert_invalid_request(error, "`response_format` must be an object");
}

#[test]
fn rejects_response_format_alongside_caller_response_mime_type() {
    let error = generation_config(
        "gemini-3.7-flash",
        &[
            ("response_format", json!({"type": "json_object"})),
            (
                "generationConfig",
                json!({"responseMimeType": "text/plain"}),
            ),
        ],
    )
    .expect_err("two response format sources must be rejected");
    assert_invalid_request(error, "`response_format` conflicts");
}

#[test]
fn route_extra_body_deep_merges_into_generation_config() {
    let mut context = context("google/gemini-3.7-flash");
    context.extra_body.insert(
        "generationConfig".to_string(),
        json!({"topK": 5, "temperature": 0.0}),
    );
    context
        .extra_body
        .insert("labels".to_string(), json!({"team": "search"}));

    let mapped = google_body(
        &ping_request(&[
            ("temperature", json!(0.8)),
            ("max_tokens", json!(50)),
            ("reasoning_effort", json!("medium")),
        ]),
        &context,
        false,
    )
    .expect("mapped");

    assert_eq!(
        mapped["generationConfig"],
        json!({
            "temperature": 0.0,
            "topK": 5,
            "maxOutputTokens": 50,
            "thinkingConfig": {"thinkingLevel": "MEDIUM", "includeThoughts": true}
        })
    );
    assert_eq!(mapped["labels"], json!({"team": "search"}));
}

#[test]
fn rejects_google_streaming_multiple_candidates_from_n() {
    let mut request = ping_request(&[("n", json!(2))]);
    request.stream = true;

    let error = google_body(&request, &context("google/gemini-2.0-flash"), true)
        .expect_err("streaming n>1 should be rejected");
    assert_invalid_request(error, "single candidate");
}

#[test]
fn rejects_google_streamed_function_call_arguments() {
    let request = ping_request(&[("tool_choice", json!("auto"))]);
    let mut context = context("google/gemini-3.7-flash");
    context.extra_body.insert(
        "toolConfig".to_string(),
        json!({
            "functionCallingConfig": {
                "streamFunctionCallArguments": true
            }
        }),
    );

    let error = google_body(&request, &context, true)
        .expect_err("partial function argument streaming must be rejected");
    assert_invalid_request(error, "streamFunctionCallArguments");
}

#[test]
fn rejects_google_streaming_multiple_candidates_from_route_override() {
    let mut request = ping_request(&[]);
    request.stream = true;
    let mut context = context("google/gemini-2.0-flash");
    context.extra_body.insert(
        "generationConfig".to_string(),
        json!({ "candidateCount": 2 }),
    );

    let error = google_body(&request, &context, true).expect_err("route override should win");
    assert_invalid_request(error, "single candidate");
}

#[test]
fn route_override_cannot_add_google_stream_field() {
    let mut context = context("google/gemini-2.0-flash");
    context.extra_body.insert("stream".to_string(), json!(true));

    let mapped =
        google_body(&ping_request(&[("stream", json!(true))]), &context, true).expect("mapped");

    assert!(mapped.get("stream").is_none());
}

#[test]
fn allows_google_non_streaming_multiple_candidates() {
    let mapped = google_body(
        &ping_request(&[("n", json!(2))]),
        &context("google/gemini-2.0-flash"),
        false,
    )
    .expect("non-stream n>1 remains allowed");

    assert_eq!(mapped["generationConfig"]["candidateCount"], json!(2));
}
