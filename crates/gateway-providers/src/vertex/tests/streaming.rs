use super::*;

/// Runs the Google stream normalizer over raw upstream byte chunks and renders the output.
async fn render_google_stream(chunks: Vec<Bytes>) -> String {
    let upstream = stream::iter(chunks.into_iter().map(Ok::<Bytes, reqwest::Error>));
    normalize_google_stream(upstream, "chatcmpl-test".to_string(), 1, "fast".to_string())
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
        .collect()
}

/// Encodes each upstream object as its own SSE frame and delivers one frame per chunk.
async fn render_google_objects(objects: &[Value]) -> String {
    let chunks = objects
        .iter()
        .map(|object| Bytes::from(google_sse_frames(std::slice::from_ref(object))))
        .collect();
    render_google_stream(chunks).await
}

fn deltas(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter_map(|event| event["choices"].get(0).map(|choice| &choice["delta"]))
        .collect()
}

fn concat_delta_field(events: &[Value], field: &str) -> String {
    deltas(events)
        .into_iter()
        .filter_map(|delta| delta[field].as_str())
        .collect()
}

fn finish_events(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| !event["choices"][0]["finish_reason"].is_null())
        .collect()
}

fn text_object(text: &str) -> Value {
    json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": text }] } }] })
}

fn finish_object(reason: &str) -> Value {
    json!({ "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": reason }] })
}

#[tokio::test]
async fn google_stream_emits_text_deltas_role_once_and_terminates_with_done() {
    let rendered =
        render_google_objects(&[text_object("hel"), text_object("lo"), finish_object("STOP")])
            .await;
    let events = openai_stream_events(&rendered);

    assert!(rendered.ends_with("data: [DONE]\n\n"));
    assert_eq!(concat_delta_field(&events, "content"), "hello");
    let roles: Vec<_> = deltas(&events)
        .into_iter()
        .filter(|delta| delta.get("role").is_some())
        .collect();
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0]["role"], "assistant");
    for event in &events {
        assert_eq!(event["id"], "chatcmpl-test");
        assert_eq!(event["object"], "chat.completion.chunk");
        assert_eq!(event["created"], 1);
        assert_eq!(event["model"], "fast");
    }
    let finish = finish_events(&events);
    assert_eq!(finish.len(), 1);
    assert_eq!(finish[0]["choices"][0]["finish_reason"], "stop");
    assert_eq!(
        events.last().expect("last event")["choices"][0]["finish_reason"],
        "stop"
    );
}

#[tokio::test]
async fn google_stream_routes_thoughts_to_reasoning_content() {
    let rendered = render_google_objects(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [
            { "text": "thinking...", "thought": true }
        ] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [
            { "text": " still", "thought": true },
            { "text": "answer" }
        ] } }] }),
        finish_object("STOP"),
    ])
    .await;
    let events = openai_stream_events(&rendered);

    assert_eq!(
        concat_delta_field(&events, "reasoning_content"),
        "thinking... still"
    );
    assert_eq!(concat_delta_field(&events, "content"), "answer");
    assert!(deltas(&events).into_iter().all(|delta| {
        delta["content"]
            .as_str()
            .is_none_or(|c| !c.contains("thinking"))
    }));
}

#[tokio::test]
async fn google_stream_surfaces_text_thought_signature_as_provider_metadata() {
    let rendered = render_google_objects(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [
            { "text": "hello", "thoughtSignature": "sig-text" }
        ] } }] }),
        finish_object("STOP"),
    ])
    .await;
    let events = openai_stream_events(&rendered);

    let metadata: Vec<_> = deltas(&events)
        .into_iter()
        .filter_map(|delta| delta.get("provider_metadata"))
        .collect();
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0]["gcp_vertex"]["thought_signature"], "sig-text");
}

#[tokio::test]
async fn google_stream_tool_call_indexes_are_monotonic_and_keep_thought_signatures() {
    let rendered = render_google_objects(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [
            { "functionCall": { "id": "fc-weather", "name": "weather", "args": { "city": "London" } },
              "thoughtSignature": "sig-weather" }
        ] } }] }),
        json!({ "candidates": [{ "content": { "role": "model", "parts": [
            { "functionCall": { "name": "time", "args": { "city": "London" } },
              "thoughtSignature": "sig-time" },
            { "functionCall": { "name": "news", "args": {} } }
        ] } }] }),
        finish_object("STOP"),
    ])
    .await;
    let events = openai_stream_events(&rendered);

    let tool_calls: Vec<&Value> = deltas(&events)
        .into_iter()
        .filter_map(|delta| delta["tool_calls"].as_array())
        .flatten()
        .collect();
    assert_eq!(tool_calls.len(), 3);
    for (expected_index, call) in tool_calls.iter().enumerate() {
        assert_eq!(call["index"], expected_index as i64);
        assert_eq!(call["type"], "function");
        assert!(call["id"].as_str().is_some_and(|id| !id.is_empty()));
    }
    assert_eq!(tool_calls[0]["id"], "fc-weather");
    assert_eq!(tool_calls[0]["function"]["name"], "weather");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        r#"{"city":"London"}"#
    );
    assert_eq!(tool_calls[0]["thought_signature"], "sig-weather");
    assert_eq!(tool_calls[1]["function"]["name"], "time");
    assert_eq!(tool_calls[1]["thought_signature"], "sig-time");
    assert_eq!(tool_calls[2]["function"]["name"], "news");
    assert_eq!(tool_calls[2]["function"]["arguments"], "{}");
    assert!(tool_calls[2].get("thought_signature").is_none());

    let finish = finish_events(&events);
    assert_eq!(finish.len(), 1);
    assert_eq!(finish[0]["choices"][0]["finish_reason"], "tool_calls");
    assert!(rendered.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn google_stream_final_chunk_carries_usage_with_reasoning_tokens() {
    let rendered = render_google_objects(&[
        json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "a", "thought": true }] } }],
            "usageMetadata": { "promptTokenCount": 4, "thoughtsTokenCount": 3, "totalTokenCount": 7 }
        }),
        json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "b" }] }, "finishReason": "STOP" }],
            "usageMetadata": {
                "promptTokenCount": 4,
                "cachedContentTokenCount": 2,
                "candidatesTokenCount": 5,
                "thoughtsTokenCount": 3,
                "totalTokenCount": 12
            }
        }),
    ])
    .await;
    let events = openai_stream_events(&rendered);

    let with_usage: Vec<_> = events
        .iter()
        .filter(|event| event.get("usage").is_some())
        .collect();
    assert_eq!(
        with_usage.len(),
        1,
        "usage must ride only on the terminal chunk"
    );
    let finish = with_usage[0];
    assert_eq!(finish["choices"][0]["finish_reason"], "stop");
    let usage = &finish["usage"];
    assert_eq!(usage["prompt_tokens"], 4);
    assert_eq!(usage["completion_tokens"], 8);
    assert_eq!(usage["total_tokens"], 12);
    assert_eq!(usage["completion_tokens_details"]["reasoning_tokens"], 3);
    assert_eq!(usage["prompt_tokens_details"]["cached_tokens"], 2);
    assert_eq!(usage["usage_source"], "vertex_google");
    assert_eq!(usage["provider_usage"]["thoughtsTokenCount"], 3);
    assert_eq!(rendered.matches(r#""finish_reason":"stop""#).count(), 1);
}

#[tokio::test]
async fn google_stream_keeps_text_that_trails_the_finish_reason() {
    let rendered = render_google_objects(&[
        json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "hel" }] }, "finishReason": "STOP" }] }),
        text_object("lo"),
    ])
    .await;
    let events = openai_stream_events(&rendered);

    assert_eq!(concat_delta_field(&events, "content"), "hello");
    assert_eq!(
        events.last().expect("last")["choices"][0]["finish_reason"],
        "stop"
    );
}

#[tokio::test]
async fn google_stream_maps_upstream_finish_reasons() {
    for (upstream, expected) in [
        ("MAX_TOKENS", "length"),
        ("SAFETY", "content_filter"),
        ("RECITATION", "content_filter"),
    ] {
        let rendered = render_google_objects(&[text_object("x"), finish_object(upstream)]).await;
        let events = openai_stream_events(&rendered);
        let finish = finish_events(&events);
        assert_eq!(finish.len(), 1, "finishReason {upstream}");
        assert_eq!(
            finish[0]["choices"][0]["finish_reason"], expected,
            "finishReason {upstream}"
        );
    }
}

#[tokio::test]
async fn google_stream_blocked_prompt_finishes_with_content_filter() {
    let rendered = render_google_objects(&[json!({
        "promptFeedback": { "blockReason": "PROHIBITED_CONTENT" },
        "usageMetadata": { "promptTokenCount": 9, "totalTokenCount": 9 }
    })])
    .await;
    let events = openai_stream_events(&rendered);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["choices"][0]["finish_reason"], "content_filter");
    assert_eq!(events[0]["usage"]["prompt_tokens"], 9);
    assert!(rendered.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn google_stream_parses_crlf_frames_split_mid_utf8_and_mid_frame() {
    let frames = google_sse_frames(&[
        text_object("héllo"),
        text_object(" wörld"),
        finish_object("STOP"),
    ]);
    assert!(frames.contains("\r\n\r\n"));
    let bytes = frames.as_bytes();
    let split_at = bytes.iter().position(|b| *b == 0xC3).expect("é lead byte") + 1;
    assert!(
        std::str::from_utf8(&bytes[..split_at]).is_err(),
        "first chunk must end inside a multi-byte codepoint"
    );
    let mut chunks: Vec<Bytes> = vec![Bytes::copy_from_slice(&bytes[..split_at])];
    // Deliver the remainder one byte at a time to exercise every possible frame boundary.
    chunks.extend(
        bytes[split_at..]
            .iter()
            .map(|b| Bytes::copy_from_slice(&[*b])),
    );

    let rendered = render_google_stream(chunks).await;
    let events = openai_stream_events(&rendered);

    assert_eq!(concat_delta_field(&events, "content"), "héllo wörld");
    assert_eq!(finish_events(&events).len(), 1);
    assert!(rendered.ends_with("data: [DONE]\n\n"));
    assert!(!rendered.contains("google_stream_parse_error"));
}

#[tokio::test]
async fn google_stream_accepts_lf_only_frames_and_ignores_comments() {
    let raw = format!(
        ": keep-alive\n\ndata: {}\n\ndata: {}\n\n",
        text_object("hello"),
        finish_object("STOP")
    );
    let rendered = render_google_stream(vec![Bytes::from(raw)]).await;
    let events = openai_stream_events(&rendered);

    assert_eq!(concat_delta_field(&events, "content"), "hello");
    assert!(rendered.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn google_stream_truncated_final_frame_is_an_error_not_a_clean_stop() {
    // Upstream closes mid-frame: the delivered text is emitted, then the truncated tail is
    // surfaced as a parse error instead of a successful finish + [DONE].
    let raw = format!(
        "data: {}\n\ndata: {{\"candidates\": [{{\"content\": {{\"parts\": [{{\"text\": \"lost\"",
        text_object("kept")
    );
    let rendered = render_google_stream(vec![Bytes::from(raw)]).await;
    let events = openai_stream_events(&rendered);

    assert_eq!(concat_delta_field(&events, "content"), "kept");
    let error = events
        .iter()
        .find(|event| event.get("error").is_some())
        .expect("error chunk");
    assert_eq!(error["error"]["code"], "google_stream_parse_error");
    assert!(finish_events(&events).is_empty());
    assert!(!rendered.contains("[DONE]"));
}

#[tokio::test]
async fn google_stream_inline_error_emits_error_chunk_without_done() {
    let rendered = render_google_objects(&[
        text_object("partial"),
        json!({ "error": { "code": 429, "status": "RESOURCE_EXHAUSTED", "message": "quota exceeded" } }),
        text_object("never delivered"),
    ])
    .await;
    let events = openai_stream_events(&rendered);

    assert_eq!(concat_delta_field(&events, "content"), "partial");
    let error = events
        .iter()
        .find(|event| event.get("error").is_some())
        .expect("error chunk");
    assert_eq!(error["error"]["code"], "google_stream_error");
    assert_eq!(error["error"]["type"], "upstream_error");
    let message = error["error"]["message"].as_str().expect("message");
    assert!(message.contains("RESOURCE_EXHAUSTED"));
    assert!(message.contains("quota exceeded"));
    assert!(
        events
            .iter()
            .all(|event| event["choices"][0]["finish_reason"].is_null())
    );
    assert!(!rendered.contains("never delivered"));
    assert!(!rendered.contains("data: [DONE]"));
    assert!(rendered.ends_with(&format!("data: {error}\n\n")));
}

#[tokio::test]
async fn google_stream_malformed_function_call_emits_error_chunk_without_done() {
    let rendered = render_google_objects(&[json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "functionCall": { "name": "lookup", "args": {} } }] },
            "finishReason": "MALFORMED_FUNCTION_CALL",
            "finishMessage": "unparseable call"
        }]
    })])
    .await;
    let events = openai_stream_events(&rendered);

    assert!(!rendered.contains("\"tool_calls\""));
    let error = events
        .iter()
        .find(|event| event.get("error").is_some())
        .expect("error chunk");
    assert_eq!(error["error"]["code"], "google_stream_error");
    assert!(
        error["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("unparseable call"))
    );
    assert!(!rendered.contains("\"finish_reason\":\"stop\""));
    assert!(!rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn google_stream_stops_after_invalid_utf8() {
    let rendered = render_google_stream(vec![Bytes::from_static(b"data: \x80\n\n")]).await;

    assert!(rendered.contains("google_stream_parse_error"));
    assert!(!rendered.contains(r#""finish_reason":"stop""#));
    assert!(!rendered.contains("data: [DONE]"));
}

#[tokio::test]
async fn google_stream_stops_after_non_json_frame() {
    let rendered = render_google_stream(vec![Bytes::from("data: {not json\n\n")]).await;

    assert!(rendered.contains("google_stream_parse_error"));
    assert!(!rendered.contains("data: [DONE]"));
}

#[test]
fn google_stream_state_emits_nothing_for_usage_only_frames_and_folds_them_into_finish() {
    let mut state = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string());

    let chunks = state
        .on_response(&json!({
            "usageMetadata": { "promptTokenCount": 4, "candidatesTokenCount": 1, "totalTokenCount": 5 }
        }))
        .expect("usage frame");
    assert!(chunks.is_empty());

    let chunks = state
        .on_response(&json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "hi" }] }, "finishReason": "MAX_TOKENS" }],
            "usageMetadata": { "promptTokenCount": 4, "candidatesTokenCount": 2, "totalTokenCount": 6 }
        }))
        .expect("text frame");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(chunks[0]["choices"][0]["delta"]["content"], "hi");
    assert!(chunks[0]["choices"][0]["finish_reason"].is_null());
    assert!(chunks[0].get("usage").is_none());

    let finish = state.finish().expect("finish reason seen");
    assert_eq!(finish["choices"][0]["finish_reason"], "length");
    assert_eq!(finish["usage"]["completion_tokens"], 2);
    assert_eq!(finish["usage"]["total_tokens"], 6);
    assert_eq!(finish["usage"]["usage_source"], "vertex_google");
}

#[test]
fn google_stream_state_tool_calls_override_later_finish_reason() {
    let mut state = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string());

    state
        .on_response(
            &json!({ "candidates": [{ "content": { "role": "model", "parts": [
            { "functionCall": { "name": "weather", "args": {} } }
        ] } }] }),
        )
        .expect("tool call frame");
    state
        .on_response(&finish_object("STOP"))
        .expect("finish frame");

    assert_eq!(
        state.finish().expect("finish reason seen")["choices"][0]["finish_reason"],
        "tool_calls"
    );
}

#[test]
fn google_stream_state_without_finish_reason_is_a_premature_eof() {
    // No frames at all.
    let error = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string())
        .finish()
        .expect_err("no finish reason");
    assert!(matches!(error, VertexAdapterError::StreamPrematureEof));
    assert!(matches!(
        ProviderError::from(error),
        ProviderError::Transport(_)
    ));

    // Complete text frames but the connection closed before the `finishReason` frame.
    let mut state = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string());
    state
        .on_response(&text_object("partial"))
        .expect("text frame");
    assert!(matches!(
        state.finish(),
        Err(VertexAdapterError::StreamPrematureEof)
    ));
}

#[tokio::test]
async fn google_stream_closed_before_finish_reason_is_an_error_not_a_clean_stop() {
    // Upstream closes cleanly between frames: the delivered text is kept, but the client sees
    // an error chunk instead of `finish_reason: "stop"` + [DONE] for truncated output.
    let rendered = render_google_objects(&[text_object("kept")]).await;
    let events = openai_stream_events(&rendered);

    assert_eq!(concat_delta_field(&events, "content"), "kept");
    let error = events
        .iter()
        .find(|event| event.get("error").is_some())
        .expect("error chunk");
    assert_eq!(error["error"]["code"], "google_stream_premature_eof");
    assert!(finish_events(&events).is_empty());
    assert!(!rendered.contains("[DONE]"));
}
