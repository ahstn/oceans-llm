use super::*;

#[test]
fn parses_google_streamed_json_objects() {
    let mut parser = JsonObjectParser::default();
    let part_a = br#"{"candidates":[{"content":{"parts":[{"text":"he"}]}}]}
{"candidates":[{"content":{"parts":[{"text":"ll"#;
    let part_b = br#"o"}]},"finishReason":"STOP"}]}"#;
    let first = parser.push_bytes(part_a).expect("first");
    assert_eq!(first.len(), 1);
    let second = parser.push_bytes(part_b).expect("second");
    assert_eq!(second.len(), 1);
}

#[test]
fn parses_google_streamed_json_objects_with_split_utf8_codepoint() {
    let mut parser = JsonObjectParser::default();
    let payload = format!(
        r#"{{"candidates":[{{"content":{{"parts":[{{"text":"{}"}}]}}}}]}}"#,
        "👋"
    );
    let split = payload.find('👋').expect("emoji position") + 2;
    let first = parser
        .push_bytes(&payload.as_bytes()[..split])
        .expect("first chunk");
    assert!(first.is_empty());
    let second = parser
        .push_bytes(&payload.as_bytes()[split..])
        .expect("second chunk");
    assert_eq!(second.len(), 1);
    assert_eq!(
        super::extract_google_candidate_text(&second[0]["candidates"][0]),
        "👋"
    );
}

#[test]
fn parses_anthropic_sse_events() {
    let mut parser = SseEventParser::default();
    let input = br#"event: message_start
data: {"type":"message_start","message":{"role":"assistant"}}

event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}

event: vertex_event
data: {"type":"vertex_event"}

"#;
    let events = parser.push_bytes(input).expect("events");
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event.as_deref(), Some("message_start"));
    assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
    assert_eq!(events[2].event.as_deref(), Some("vertex_event"));
}

#[test]
fn parses_anthropic_sse_events_with_crlf_and_chunked_boundaries() {
    let mut parser = SseEventParser::default();
    let part_a = b"event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"role\":\"assistant\"}}\r\n\r";
    let part_b = b"\nevent: message_stop\r\ndata: {\"type\":\"message_stop\"}\r\n\r\n";

    let first = parser.push_bytes(part_a).expect("events a");
    assert!(first.is_empty());
    let second = parser.push_bytes(part_b).expect("events b");
    assert_eq!(second.len(), 2);
    assert_eq!(second[0].event.as_deref(), Some("message_start"));
    assert_eq!(second[1].event.as_deref(), Some("message_stop"));
}

#[test]
fn parses_anthropic_sse_events_with_split_utf8_codepoint() {
    let mut parser = SseEventParser::default();
    let payload = format!(
        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"{}\"}}}}\n\n",
        "👋"
    );
    let split = payload.find('👋').expect("emoji position") + 2;
    let first = parser
        .push_bytes(&payload.as_bytes()[..split])
        .expect("first chunk");
    assert!(first.is_empty());
    let second = parser
        .push_bytes(&payload.as_bytes()[split..])
        .expect("second chunk");
    assert_eq!(second.len(), 1);
    let payload: Value = serde_json::from_str(&second[0].data).expect("event payload");
    assert_eq!(payload["delta"]["text"], "👋");
}

#[tokio::test]
async fn google_stream_normalization_emits_done() {
    let upstream = stream::iter(vec![
        Ok(Bytes::from(
            r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}"#,
        )),
        Ok(Bytes::from(r#"{"candidates":[{"finishReason":"STOP"}]}"#)),
    ]);
    let stream = super::normalize_google_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();
    assert!(rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn google_stream_emits_usage_and_one_terminal_finish_chunk() {
    let upstream = stream::iter(vec![
        Ok(Bytes::from(
            r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}"#,
        )),
        Ok(Bytes::from(
            r#"{"candidates":[{"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2,"totalTokenCount":6}}"#,
        )),
        Ok(Bytes::from(
            r#"{"candidates":[{"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":2,"totalTokenCount":6}}"#,
        )),
    ]);
    let stream = super::normalize_google_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let rendered = stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(
        rendered.contains(r#""usage":{"completion_tokens":2,"prompt_tokens":4,"total_tokens":6}"#)
    );
    assert_eq!(rendered.matches(r#""finish_reason":"stop""#).count(), 1);
}

#[tokio::test]
async fn google_stream_normalization_stops_after_parse_error() {
    let upstream = stream::iter(vec![Ok(Bytes::from_static(&[0x80]))]);
    let stream = super::normalize_google_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();
    assert!(rendered.contains("google_stream_parse_error"));
    assert!(!rendered.contains(r#""finish_reason":"stop""#));
    assert!(!rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn google_stream_normalization_normalizes_tool_calls() {
    let upstream = stream::iter(vec![
        Ok(Bytes::from(
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":{"city":"London"}}}]}}]}"#,
        )),
        Ok(Bytes::from(r#"{"candidates":[{"finishReason":"STOP"}]}"#)),
    ]);
    let stream = super::normalize_google_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();
    assert!(rendered.contains("\"tool_calls\""));
    assert!(rendered.contains("\"name\":\"lookup\""));
    assert!(rendered.contains("\"arguments\":\"{\\\"city\\\":\\\"London\\\"}\""));
    assert!(rendered.contains("\"finish_reason\":\"tool_calls\""));
    assert!(rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn google_stream_preserves_tool_call_indexes_and_thought_signatures() {
    let upstream = stream::iter(vec![
        Ok(Bytes::from(
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"weather","args":{"city":"London"}},"thoughtSignature":"sig-weather"}]}}]}"#,
        )),
        Ok(Bytes::from(
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"time","args":{"city":"London"}},"thoughtSignature":"sig-time"}]}}]}"#,
        )),
        Ok(Bytes::from(r#"{"candidates":[{"finishReason":"STOP"}]}"#)),
    ]);
    let stream = super::normalize_google_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(rendered.contains("\"index\":0"));
    assert!(rendered.contains("\"index\":1"));
    assert!(rendered.contains("\"thought_signature\":\"sig-weather\""));
    assert!(rendered.contains("\"thought_signature\":\"sig-time\""));
    assert_eq!(
        rendered.matches("\"finish_reason\":\"tool_calls\"").count(),
        1
    );
}

#[tokio::test]
async fn google_stream_does_not_emit_malformed_function_calls() {
    let upstream = stream::iter(vec![Ok(Bytes::from(
        r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":{"city":"London"}}}]},"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#,
    ))]);
    let stream = super::normalize_google_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(!rendered.contains("\"tool_calls\""));
    assert!(rendered.contains("\"finish_reason\":\"stop\""));
}

#[tokio::test]
async fn anthropic_stream_normalization_stops_after_parse_error() {
    let upstream = stream::iter(vec![Ok(Bytes::from_static(&[0x80]))]);
    let stream = super::normalize_anthropic_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();
    assert!(rendered.contains("anthropic_sse_parse_error"));
    assert!(!rendered.contains(r#""finish_reason":"stop""#));
    assert!(!rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn anthropic_stream_preserves_thinking_and_signature_metadata() {
    let upstream = stream::iter(vec![Ok(Bytes::from(concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\"}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-stream\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"visible\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n"
    )))]);
    let stream = super::normalize_anthropic_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(rendered.contains("\"content\":\"visible\""));
    assert!(rendered.contains("\"provider_metadata\""));
    assert!(rendered.contains("\"gcp_vertex\""));
    assert!(rendered.contains("\"thinking_delta\""));
    assert!(rendered.contains("\"signature_delta\""));
    assert!(rendered.contains("\"sig-stream\""));
    assert_eq!(rendered.matches("\"role\":\"assistant\"").count(), 1);
    assert!(rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn anthropic_stream_preserves_redacted_thinking_start_metadata() {
    let upstream = stream::iter(vec![Ok(Bytes::from(concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\"}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"encrypted-redacted\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"visible\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n"
    )))]);
    let stream = super::normalize_anthropic_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(rendered.contains("\"content\":\"visible\""));
    assert!(rendered.contains("\"provider_metadata\""));
    assert!(rendered.contains("\"redacted_thinking\""));
    assert!(rendered.contains("\"encrypted-redacted\""));
    assert_eq!(rendered.matches("\"role\":\"assistant\"").count(), 1);
    assert!(rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn anthropic_stream_normalizes_tool_use_deltas() {
    let upstream = stream::iter(vec![Ok(Bytes::from(concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\"}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"lookup\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"London\\\"}\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n"
    )))]);
    let stream = super::normalize_anthropic_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();

    assert!(rendered.contains("\"tool_calls\""));
    assert!(rendered.contains("\"id\":\"toolu_123\""));
    assert!(rendered.contains("\"name\":\"lookup\""));
    assert!(rendered.contains("\"arguments\":\"{\\\"city\\\":\""));
    assert!(rendered.contains("\"arguments\":\"\\\"London\\\"}\""));
    assert!(rendered.contains("\"finish_reason\":\"tool_calls\""));
    assert!(rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn anthropic_stream_preserves_usage_events() {
    let upstream = stream::iter(vec![Ok(Bytes::from(concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":9,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n"
    )))]);
    let stream = super::normalize_anthropic_stream(
        upstream,
        "chatcmpl-test".to_string(),
        1,
        "fast".to_string(),
    );
    let bytes: Vec<_> = stream.collect().await;
    let rendered = bytes
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect::<String>();
    let events = openai_stream_events(&rendered);

    assert!(events.iter().any(|event| {
        event["usage"]["prompt_tokens"] == json!(9)
            && event["usage"]["completion_tokens"] == json!(0)
            && event["usage"]["total_tokens"] == json!(9)
    }));
    assert!(events.iter().any(|event| {
        event["choices"][0]["finish_reason"] == json!("stop")
            && event["usage"]["prompt_tokens"] == json!(9)
            && event["usage"]["completion_tokens"] == json!(2)
            && event["usage"]["total_tokens"] == json!(11)
    }));
}
