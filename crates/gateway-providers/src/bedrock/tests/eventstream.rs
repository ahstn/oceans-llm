use super::*;

fn stream_events(transcript: &str) -> Vec<Value> {
    transcript
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

fn converse_event(kind: &str, payload: Value) -> Vec<u8> {
    eventstream_frame(
        &[(":message-type", "event"), (":event-type", kind)],
        payload.to_string().as_bytes(),
    )
}

#[tokio::test]
async fn eventstream_valid_usage_prefix_survives_later_crc_failure_independent_of_chunking() {
    for prelude_error in [false, true] {
        for reason in ["end_turn", "malformed_tool_use"] {
            let stop = converse_event("messageStop", json!({"stopReason":reason}));
            let usage = converse_event(
                "metadata",
                json!({"usage":{"inputTokens":8,"outputTokens":3}}),
            );
            let mut corrupt = converse_event(
                "contentBlockDelta",
                json!({
                    "contentBlockIndex":0,"delta":{"text":"CORRUPT_PAYLOAD_MUST_NOT_APPEAR"}
                }),
            );
            if prelude_error {
                corrupt[8] ^= 1;
                let crc_offset = corrupt.len() - 4;
                let crc = fixture_crc32(&corrupt[..crc_offset]);
                corrupt[crc_offset..].copy_from_slice(&crc.to_be_bytes());
            } else {
                *corrupt.last_mut().unwrap() ^= 1;
            }
            let later = converse_event(
                "metadata",
                json!({"usage":{"inputTokens":900,"outputTokens":900}}),
            );
            let combined = [stop.clone(), usage.clone(), corrupt.clone(), later.clone()].concat();
            let coalesced = collect_bedrock_stream(vec![combined]).await;
            let split =
                collect_bedrock_stream(vec![stop, usage.clone(), corrupt.clone(), later]).await;
            let accounting_and_errors = |transcript: &str| {
                stream_events(transcript)
                    .into_iter()
                    .filter(|event| event.get("usage").is_some() || event.get("error").is_some())
                    .map(|event| json!({"usage":event.get("usage"),"error":event.get("error")}))
                    .collect::<Vec<_>>()
            };
            let observed = accounting_and_errors(&coalesced);
            assert_eq!(observed, accounting_and_errors(&split));
            assert_eq!(observed.len(), 2);
            assert_eq!(observed[0]["usage"]["total_tokens"], 11);
            assert_eq!(
                observed[1]["error"]["code"],
                "bedrock_eventstream_parse_error"
            );
            assert!(
                observed[1]["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(if prelude_error {
                        "prelude checksum mismatch"
                    } else {
                        "message checksum mismatch"
                    })
            );
            for transcript in [&coalesced, &split] {
                assert!(!transcript.contains("CORRUPT_PAYLOAD_MUST_NOT_APPEAR"));
                assert!(!transcript.contains("[DONE]"));
            }
            let mut decoder = BedrockEventStreamDecoder::default();
            assert_eq!(
                decoder
                    .push_bytes(&[usage.clone(), corrupt].concat())
                    .unwrap()
                    .len(),
                1
            );
            assert!(decoder.take_error().is_some());
            assert!(
                decoder.push_bytes(&usage).is_err(),
                "failed decoder accepted more bytes"
            );
            assert!(decoder.finish().is_err());
        }
    }
}

#[tokio::test]
async fn converse_partial_tool_json_preserves_truncation_filter_or_deferred_error_and_usage() {
    for (reason, expected) in [
        ("max_tokens", Some("length")),
        ("model_context_window_exceeded", Some("length")),
        ("guardrail_intervened", Some("content_filter")),
        ("content_filtered", Some("content_filter")),
        ("tool_use", None),
        ("end_turn", None),
    ] {
        let transcript = collect_bedrock_stream(vec![
            converse_event("contentBlockStart", json!({"contentBlockIndex":3,"start":{"toolUse":{"toolUseId":"a","name":"lookup"}}})),
            converse_event("contentBlockDelta", json!({"contentBlockIndex":3,"delta":{"toolUse":{"input":"{\"city\":"}}})),
            converse_event("messageStop", json!({"stopReason":reason})),
            converse_event("metadata", json!({"usage":{"inputTokens":8,"outputTokens":3}})),
        ]).await;
        let events = stream_events(&transcript);
        let usage_index = events
            .iter()
            .position(|event| event.get("usage").is_some())
            .unwrap();
        assert_eq!(events[usage_index]["usage"]["total_tokens"], 11);
        assert!(events.iter().all(|event| {
            event["choices"][0]["delta"]["provider_metadata"]["aws_bedrock"]
                .get("native_content")
                .is_none()
        }));
        if let Some(expected) = expected {
            assert!(
                events
                    .iter()
                    .any(|event| event["choices"][0]["finish_reason"] == expected)
            );
            assert!(
                events.iter().all(|event| event.get("error").is_none()),
                "{transcript}"
            );
            assert!(transcript.ends_with("data: [DONE]\n\n"));
        } else {
            let error_index = events
                .iter()
                .position(|event| event.get("error").is_some())
                .unwrap();
            assert!(usage_index < error_index);
            assert_eq!(
                events[error_index]["error"]["code"],
                "bedrock_conversestream_replay_error"
            );
            assert!(
                events
                    .iter()
                    .all(|event| event["choices"][0]["finish_reason"].is_null())
            );
            assert!(!transcript.contains("[DONE]"));
        }
    }
}

#[test]
fn validates_independent_ieee_checksums_and_rejects_each_corruption() {
    assert_eq!(fixture_crc32(b"123456789"), 0xcbf4_3926);
    let frame = converse_event("messageStop", json!({"stopReason":"end_turn"}));
    assert_eq!(
        BedrockEventStreamDecoder::default()
            .push_bytes(&frame)
            .unwrap()
            .len(),
        1
    );
    let mut bad_prelude = frame.clone();
    bad_prelude[8] ^= 1;
    let crc_offset = bad_prelude.len() - 4;
    let valid_message_crc = fixture_crc32(&bad_prelude[..crc_offset]);
    bad_prelude[crc_offset..].copy_from_slice(&valid_message_crc.to_be_bytes());
    let error = BedrockEventStreamDecoder::default()
        .push_bytes(&bad_prelude)
        .unwrap_err();
    assert!(error.to_string().contains("prelude checksum mismatch"));
    let mut bad_message = frame.clone();
    *bad_message.last_mut().unwrap() ^= 1;
    let error = BedrockEventStreamDecoder::default()
        .push_bytes(&bad_message)
        .unwrap_err();
    assert!(error.to_string().contains("message checksum mismatch"));
    let mut bad_payload = frame;
    bad_payload[12] ^= 1;
    let error = BedrockEventStreamDecoder::default()
        .push_bytes(&bad_payload)
        .unwrap_err();
    assert!(error.to_string().contains("message checksum mismatch"));
}

#[test]
fn accepts_all_legal_typed_headers_and_rejects_truncated_values() {
    let values = [
        (0, vec![]),
        (1, vec![]),
        (2, vec![0xff]),
        (3, vec![0, 1]),
        (4, vec![0, 0, 0, 2]),
        (5, 3_i64.to_be_bytes().to_vec()),
        (6, vec![0, 2, 0xff, 0x80]),
        (7, vec![0, 2, b'o', b'k']),
        (8, 1_700_000_000_123_i64.to_be_bytes().to_vec()),
        (9, vec![0xab; 16]),
    ];
    let mut headers = Vec::new();
    for (kind, bytes) in &values {
        headers.extend_from_slice(&[1, b'h', *kind]);
        headers.extend_from_slice(bytes);
    }
    headers.extend_from_slice(&[11]);
    headers.extend_from_slice(b":event-type");
    headers.extend_from_slice(&[7, 0, 11]);
    headers.extend_from_slice(b"messageStop");
    let events = BedrockEventStreamDecoder::default()
        .push_bytes(&eventstream_frame_with_headers(&headers, b"{}"))
        .unwrap();
    assert_eq!(events[0].event_type.as_deref(), Some("messageStop"));
    for (kind, mut bytes) in values.into_iter().filter(|(kind, _)| *kind >= 2) {
        bytes.pop();
        let mut header = vec![1, b'h', kind];
        header.extend(bytes);
        assert!(
            BedrockEventStreamDecoder::default()
                .push_bytes(&eventstream_frame_with_headers(&header, b"{}"))
                .is_err(),
            "type {kind}"
        );
    }
    for header in [vec![1, b'h', 10], vec![4, b'a'], vec![1, b'h', 6, 0]] {
        assert!(
            BedrockEventStreamDecoder::default()
                .push_bytes(&eventstream_frame_with_headers(&header, b"{}"))
                .is_err()
        );
    }
}

#[test]
fn eventstream_keeps_sixteen_mebibyte_resource_cap() {
    let length = 16 * 1024 * 1024 + 1_u32;
    let mut prelude = Vec::new();
    prelude.extend_from_slice(&length.to_be_bytes());
    prelude.extend_from_slice(&0_u32.to_be_bytes());
    prelude.extend_from_slice(&fixture_crc32(&prelude).to_be_bytes());
    let error = BedrockEventStreamDecoder::default()
        .push_bytes(&prelude)
        .unwrap_err();
    assert!(error.to_string().contains("exceeds limit"));
}

#[tokio::test]
async fn sparse_interleaved_tool_blocks_keep_dense_stable_chat_ordinals() {
    let transcript = collect_bedrock_stream(vec![
        converse_event(
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{"text":"tools"}}),
        ),
        converse_event(
            "contentBlockStart",
            json!({"contentBlockIndex":3,"start":{"toolUse":{"toolUseId":"a","name":"first"}}}),
        ),
        converse_event(
            "contentBlockStart",
            json!({"contentBlockIndex":9,"start":{"toolUse":{"toolUseId":"b","name":"second"}}}),
        ),
        converse_event(
            "contentBlockDelta",
            json!({"contentBlockIndex":9,"delta":{"toolUse":{"input":"{"}}}),
        ),
        converse_event(
            "contentBlockDelta",
            json!({"contentBlockIndex":3,"delta":{"toolUse":{"input":"{\"n\":1}"}}}),
        ),
        converse_event(
            "contentBlockDelta",
            json!({"contentBlockIndex":9,"delta":{"toolUse":{"input":"}"}}}),
        ),
        converse_event("messageStop", json!({"stopReason":"tool_use"})),
    ])
    .await;
    let events = stream_events(&transcript);
    let calls = events
        .iter()
        .filter_map(|event| event["choices"][0]["delta"]["tool_calls"].as_array())
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(
        calls
            .iter()
            .map(|call| call["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 1, 0, 1]
    );
    let native = &events.last().unwrap()["choices"][0]["delta"]["provider_metadata"]["aws_bedrock"]
        ["native_content"]["blocks"];
    assert_eq!(native[1]["toolUse"]["input"], json!({"n":1}));
    assert_eq!(native[2]["toolUse"]["input"], json!({}));
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn malformed_generation_emits_later_usage_then_error_without_success() {
    for reason in ["malformed_tool_use", "malformed_model_output"] {
        let transcript = collect_bedrock_stream(vec![
            converse_event(
                "messageStop",
                json!({
                    "stopReason":reason, "additionalModelResponseFields":{"diagnostic":"kept"}
                }),
            ),
            converse_event(
                "metadata",
                json!({"usage":{"inputTokens":8,"outputTokens":3}}),
            ),
        ])
        .await;
        let events = stream_events(&transcript);
        let usage_index = events
            .iter()
            .position(|event| event.get("usage").is_some())
            .unwrap();
        let error_index = events
            .iter()
            .position(|event| event.get("error").is_some())
            .unwrap();
        assert!(usage_index < error_index);
        assert_eq!(events[usage_index]["usage"]["total_tokens"], 11);
        assert_eq!(events[error_index]["error"]["code"], reason);
        assert_eq!(
            events[0]["choices"][0]["delta"]["provider_metadata"]["aws_bedrock"]["additionalModelResponseFields"],
            json!({"diagnostic":"kept"})
        );
        assert!(
            events
                .iter()
                .all(|event| event["choices"][0]["finish_reason"].is_null())
        );
        assert!(!transcript.contains("[DONE]"));
    }
}

#[tokio::test]
async fn converse_stream_classifies_body_timeout() {
    let error = crate::http::tests::timed_out_body_error().await;
    let upstream = futures_util::stream::once(async move { Err(error) });
    let chunks = normalize_bedrock_converse_stream(upstream, context("test"))
        .collect::<Vec<_>>()
        .await;
    let output = chunks
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect::<String>();
    assert!(output.contains("Upstream response timed out"), "{output}");
    assert!(!output.contains("[DONE]"));
}

#[test]
fn decodes_fragmented_eventstream_frames() {
    let frame = eventstream_frame(
        &[
            (":message-type", "event"),
            (":event-type", "contentBlockDelta"),
        ],
        br#"{"delta":{"text":"hel"}}"#,
    );
    let mut decoder = BedrockEventStreamDecoder::default();

    assert!(decoder.push_bytes(&frame[..7]).expect("first").is_empty());
    let events = decoder.push_bytes(&frame[7..]).expect("second");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message_type.as_deref(), Some("event"));
    assert_eq!(events[0].event_type.as_deref(), Some("contentBlockDelta"));
    assert_eq!(
        events[0].payload,
        Bytes::from_static(br#"{"delta":{"text":"hel"}}"#)
    );
    decoder.finish().expect("complete");
}

#[test]
fn rejects_malformed_eventstream_lengths() {
    let mut decoder = BedrockEventStreamDecoder::default();
    let error = decoder
        .push_bytes(&[0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0])
        .expect_err("malformed")
        .to_string();

    assert!(error.contains("invalid aws_bedrock EventStream frame length"));
}

#[tokio::test]
async fn normalizes_text_finish_usage_and_done_from_converse_stream() {
    let frames = vec![
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStart")],
            br#"{"role":"assistant","conversationId":"conv-123"}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":0,"delta":{"text":"Hello "}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":0,"delta":{"text":"world"}}"#,
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStop")],
            br#"{"stopReason":"end_turn"}"#,
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "metadata")],
            br#"{"usage":{"inputTokens":11,"outputTokens":4,"totalTokens":15},"metrics":{"latencyMs":10}}"#,
        ),
    ];

    let transcript = collect_bedrock_stream(frames).await;

    assert!(transcript.contains(r#""id":"chatcmpl-conv-123""#));
    assert!(transcript.contains(r#""delta":{"role":"assistant"}"#));
    assert!(transcript.contains(r#""delta":{"content":"Hello "}"#));
    assert!(transcript.contains(r#""delta":{"content":"world"}"#));
    assert!(transcript.contains(r#""finish_reason":"stop""#));
    assert!(transcript.contains(r#""prompt_tokens":11"#));
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn normalizes_tool_deltas_from_converse_stream() {
    let frames = vec![
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStart")],
            br#"{"role":"assistant"}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockStart"),
            ],
            br#"{"contentBlockIndex":1,"start":{"toolUse":{"toolUseId":"tool_123","name":"get_weather"}}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":1,"delta":{"toolUse":{"input":"{\"city\":\"London\"}"}}}"#,
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStop")],
            br#"{"stopReason":"tool_use"}"#,
        ),
    ];

    let transcript = collect_bedrock_stream(frames).await;

    let calls = transcript
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["index"], 0);
    assert_eq!(calls[1]["index"], 0);
    assert_eq!(calls[1]["function"]["arguments"], r#"{"city":"London"}"#);
    assert!(transcript.contains(r#""finish_reason":"tool_calls""#));
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn normalizes_omitted_thinking_signature_before_text_from_converse_stream() {
    let frames = vec![
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStart")],
            br#"{"role":"assistant"}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":0,"delta":{"reasoningContent":{"signature":"sig-omitted"}}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":1,"delta":{"text":"The answer is 42."}}"#,
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStop")],
            br#"{"stopReason":"end_turn"}"#,
        ),
    ];

    let transcript = collect_bedrock_stream(frames).await;

    assert!(transcript.contains(r#""type":"reasoning_signature""#));
    assert!(transcript.contains(r#""signature":"sig-omitted""#));
    assert!(transcript.contains(r#""delta":{"content":"The answer is 42."}"#));
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn emits_structured_error_for_exception_event_without_done() {
    let frames = vec![eventstream_frame(
        &[
            (":message-type", "exception"),
            (":exception-type", "throttlingException"),
        ],
        br#"{"message":"rate limited"}"#,
    )];

    let transcript = collect_bedrock_stream(frames).await;

    assert!(transcript.contains(r#""code":"throttlingException""#));
    assert!(transcript.contains(r#""message":"rate limited""#));
    assert!(!transcript.contains("[DONE]"));
}

#[tokio::test]
async fn emits_structured_error_for_incomplete_frame_without_done() {
    let frame = eventstream_frame(
        &[(":message-type", "event"), (":event-type", "messageStart")],
        br#"{"role":"assistant"}"#,
    );
    let truncated = frame[..frame.len() - 3].to_vec();

    let transcript = collect_bedrock_stream(vec![truncated]).await;

    assert!(transcript.contains(r#""code":"bedrock_eventstream_finalization_error""#));
    assert!(transcript.contains("incomplete aws_bedrock EventStream frame"));
    assert!(!transcript.contains("[DONE]"));
}
