use super::*;

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
            br#"{"contentBlockIndex":1,"delta":{"toolUse":{"input":"{\"city\":"}}}"#,
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStop")],
            br#"{"stopReason":"tool_use"}"#,
        ),
    ];

    let transcript = collect_bedrock_stream(frames).await;

    assert!(transcript.contains(r#""tool_calls":[{"function":{"arguments":"","name":"get_weather"},"id":"tool_123","index":1,"type":"function"}]"#));
    assert!(
        transcript.contains(r#""tool_calls":[{"function":{"arguments":"{\"city\":"},"index":1}]"#)
    );
    assert!(transcript.contains(r#""finish_reason":"tool_calls""#));
    assert!(transcript.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn normalizes_reasoning_signature_redaction_text_and_tool_deltas_from_converse_stream() {
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
            br#"{"contentBlockIndex":0,"delta":{"reasoningContent":{"text":"summarized stream reasoning"}}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":0,"delta":{"reasoningContent":{"signature":"sig-stream"}}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":0,"delta":{"reasoningContent":{"data":"cmVkYWN0ZWQ="}}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":1,"delta":{"text":"Final "}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockStart"),
            ],
            br#"{"contentBlockIndex":2,"start":{"toolUse":{"toolUseId":"tool_123","name":"get_weather"}}}"#,
        ),
        eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"contentBlockIndex":2,"delta":{"toolUse":{"input":"{\"city\":\"London\"}"}}}"#,
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStop")],
            br#"{"stopReason":"tool_use"}"#,
        ),
    ];

    let transcript = collect_bedrock_stream(frames).await;

    assert!(transcript.contains(r#""source":"bedrock_converse_stream""#));
    assert!(transcript.contains(r#""type":"reasoning_text""#));
    assert!(transcript.contains(r#""text":"summarized stream reasoning""#));
    assert!(transcript.contains(r#""type":"reasoning_signature""#));
    assert!(transcript.contains(r#""signature":"sig-stream""#));
    assert!(transcript.contains(r#""type":"redacted_reasoning""#));
    assert!(transcript.contains(r#""data":"cmVkYWN0ZWQ=""#));
    assert!(transcript.contains(r#""delta":{"content":"Final "}"#));
    assert!(transcript.contains(r#""name":"get_weather""#));
    assert!(transcript.contains(r#""finish_reason":"tool_calls""#));
    assert!(!transcript.contains(r#""content":"summarized stream reasoning""#));
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
