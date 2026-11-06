//! The public Messages edge retains native response metadata without forwarding
//! upstream cookies, connection headers, or replacing the gateway's request identity.
use super::*;

/// Rewrite only the gateway model identity in message_start. All other SSE frames,
/// including unknown events, IDs, retry directives and comments, pass through verbatim.
pub(super) fn stream_with_model(mut upstream: ProviderStream, model: String) -> ProviderStream {
    Box::pin(async_stream::stream! {
        let mut pending = Vec::new();
        let mut scanned = 0;
        let mut model_written = false;
        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk { Ok(chunk) => chunk, Err(error) => { yield Err(error); return; } };
            if model_written { yield Ok(chunk); continue; }
            pending.extend_from_slice(&chunk);
            let mut consumed = 0;
            loop {
                let Some(end) = frame_end(&pending, scanned) else {
                    scanned = pending.len().saturating_sub(3).max(consumed);
                    break;
                };
                if let Some(frame) = rewrite_message_start(&pending[consumed..end], &model) {
                    model_written = true;
                    let buffered = axum::body::Bytes::from(std::mem::take(&mut pending));
                    if consumed != 0 { yield Ok(buffered.slice(..consumed)); }
                    yield Ok(axum::body::Bytes::from(frame));
                    if end < buffered.len() { yield Ok(buffered.slice(end..)); }
                    break;
                }
                consumed = end;
                scanned = end;
            }
            // Release complete prefixes once per input chunk, without shifting the tail per frame.
            if !model_written && consumed != 0 {
                let buffered = axum::body::Bytes::from(std::mem::take(&mut pending));
                pending.extend_from_slice(&buffered[consumed..]);
                scanned = scanned.saturating_sub(consumed);
                yield Ok(buffered.slice(..consumed));
            }
        }
        if !pending.is_empty() { yield Ok(axum::body::Bytes::from(pending)); }
    })
}

fn frame_end(bytes: &[u8], from: usize) -> Option<usize> {
    (from..bytes.len().saturating_sub(1)).find_map(|index| {
        let tail = &bytes[index..];
        if tail.starts_with(b"\r\n\r\n") {
            Some(index + 4)
        } else if tail.starts_with(b"\n\n") || tail.starts_with(b"\r\r") {
            Some(index + 2)
        } else if tail.starts_with(b"\n\r") {
            Some(index + if tail.get(2) == Some(&b'\n') { 3 } else { 2 })
        } else {
            None
        }
    })
}

fn rewrite_message_start(frame: &[u8], model: &str) -> Option<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct MessageStart<'a> {
        #[serde(borrow, rename = "type")]
        kind: std::borrow::Cow<'a, str>,
        #[serde(borrow)]
        message: Message<'a>,
    }

    #[derive(serde::Deserialize)]
    struct Message<'a> {
        #[serde(borrow)]
        model: &'a serde_json::value::RawValue,
    }

    let text = std::str::from_utf8(frame).ok()?;
    let mut parser = gateway_core::SseEventParser::default();
    let event = parser.push_bytes(frame).ok()?.into_iter().next()?;
    let payload: MessageStart<'_> = serde_json::from_str(&event.data).ok()?;
    if payload.kind != "message_start" {
        return None;
    }
    let token = payload.message.model.get();
    let start = token.as_ptr() as usize - event.data.as_ptr() as usize;
    let replacement = serde_json::to_vec(model).ok()?;
    let token_range = start..start + token.len();
    let mut result = Vec::with_capacity(frame.len() + replacement.len());
    let mut data_offset = 0;
    let mut copied = 0;
    let mut written = false;

    // Map the parser's joined data back to the original bytes. Only JSON token
    // bytes are removed; data prefixes and line endings remain even when a
    // value spans several data lines. Match SseEventParser's whitespace handling.
    for line in text.split_inclusive(['\r', '\n']) {
        let Some(data) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        let end = data_offset + data.len();
        let overlap_start = token_range.start.max(data_offset);
        let overlap_end = token_range.end.min(end);
        if overlap_start < overlap_end {
            let frame_offset = data.as_ptr() as usize - text.as_ptr() as usize;
            let start = frame_offset + overlap_start - data_offset;
            result.extend_from_slice(&frame[copied..start]);
            if !written {
                result.extend_from_slice(&replacement);
                written = true;
            }
            copied = frame_offset + overlap_end - data_offset;
        }
        data_offset = end + 1;
    }
    result.extend_from_slice(&frame[copied..]);
    Some(result)
}

pub(super) fn apply_headers(response: &mut Response, headers: &BTreeMap<String, String>) {
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::try_from(name.as_str()),
            HeaderValue::from_str(value),
        ) {
            response.headers_mut().insert(name, value);
        }
    }
}

pub(super) fn upstream_error_response(error: &GatewayError) -> Option<Response> {
    let GatewayError::Provider(ProviderError::NativeMessagesHttp {
        status,
        body,
        headers,
    }) = error
    else {
        return None;
    };
    let mut response = Response::builder()
        .status(*status)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.clone()))
        .ok()?;
    apply_headers(&mut response, headers);
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Bytes, to_bytes};

    #[tokio::test]
    async fn native_model_rewrite_preserves_framing_and_all_other_json_bytes() {
        fn start_frame(model: &str, newlines: &[&str], multiline: bool) -> String {
            let payload = format!(
                r#"{{
  "type" : "message_start",
  "model": "upstream",
  "message": {{
    "id": "msg-opaque",
    "metadata": {{"model":"upstream","text":"\"model\":\"upstream\""}},
    "mo\u0064el" : {model},
    "content": [],
    "usage": {{"input_tokens":17,"cache_read_input_tokens":43}},
    "safeguard_results": {{"verdict":"allow","opaque":"世界"}},
    "future": {{"nested":["extension",{{"preserved":true}}]}}
  }},
  "future": {{"message":{{"model":"upstream"}}}}
}}"#
            );
            let payload = if multiline {
                payload
            } else {
                payload.replace('\n', " ")
            };
            let mut frame = String::new();
            for (index, line) in [
                "id: original",
                "retry: 250",
                ": start comment",
                "unknown-field: opaque",
                "event: message_start",
            ]
            .into_iter()
            .enumerate()
            {
                frame.push_str(line);
                frame.push_str(newlines[index % newlines.len()]);
            }
            let lines: Vec<_> = payload.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                frame.push_str(if index % 2 == 0 { "data: " } else { "data:" });
                frame.push_str(line);
                if index + 1 == lines.len() {
                    frame.push_str(newlines[0]);
                } else {
                    frame.push_str(newlines[index % newlines.len()]);
                    frame.push_str(": interleaved comment");
                    frame.push_str(newlines[(index + 1) % newlines.len()]);
                }
            }
            frame.push_str(newlines[0]);
            frame
        }

        let suffix = concat!(
            "id: opaque\nretry: 250\nevent: future_event\ndata: {\"type\":\"future_event\",\"text\":\"世界\"}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        );
        let model = "gateway-\"alias\"\\東京\n🌊";
        let replacement = serde_json::to_string(model).unwrap();
        let newline_styles: &[&[&str]] = &[&["\n"], &["\r\n"], &["\r"], &["\r\n", "\n", "\r"]];
        for newlines in newline_styles {
            for multiline in [false, true] {
                for token in [r#""upstream""#, r#""up\u0073tream \"quoted\" \\ 東京 🌊""#] {
                    let input = format!(
                        ": keepalive\n\n{}{suffix}",
                        start_frame(token, newlines, multiline)
                    );
                    let expected = format!(
                        ": keepalive\n\n{}{suffix}",
                        start_frame(&replacement, newlines, multiline)
                    );
                    let original_events = gateway_core::SseEventParser::default()
                        .push_bytes(input.as_bytes())
                        .unwrap();
                    assert_eq!(original_events.len(), 5);
                    for chunk_size in [1, 3, 7, input.len()] {
                        let chunks: Vec<_> = input
                            .as_bytes()
                            .chunks(chunk_size)
                            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                            .collect();
                        let output = stream_with_model(
                            Box::pin(futures_util::stream::iter(chunks)),
                            model.into(),
                        )
                        .collect::<Vec<_>>()
                        .await;
                        let output: Vec<_> = output
                            .into_iter()
                            .flat_map(|chunk| chunk.unwrap().to_vec())
                            .collect();
                        assert_eq!(
                            output,
                            expected.as_bytes(),
                            "only the complete model token may change"
                        );
                        let events = gateway_core::SseEventParser::default()
                            .push_bytes(&output)
                            .unwrap();
                        assert_eq!(events.len(), original_events.len());
                        for (index, (event, original)) in
                            events.iter().zip(&original_events).enumerate()
                        {
                            assert_eq!(event.event, original.event);
                            if original.data.is_empty() {
                                assert_eq!(event.data, original.data);
                                continue;
                            }
                            let mut expected: Value = serde_json::from_str(&original.data).unwrap();
                            if index == 1 {
                                expected["message"]["model"] = json!(model);
                            }
                            assert_eq!(
                                serde_json::from_str::<Value>(&event.data).unwrap(),
                                expected
                            );
                        }
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn native_model_rewrite_preserves_mixed_blank_line_delimiters() {
        for delimiter in ["\n\r", "\n\r\n", "\r\n\n", "\r\n\r", "\r\r\n"] {
            let input = format!(
                "event: message_start\r\ndata: {{\"type\":\"message_start\",\"message\":{{\"model\":\"upstream\",\"content\":[],\"future\":\"世界\"}}}}{delimiter}event: message_stop\r\ndata: {{\"type\":\"message_stop\"}}{delimiter}"
            );
            let expected = input.replace("\"upstream\"", "\"gateway\"");
            for chunk_size in [1, 2, 3, input.len()] {
                let chunks: Vec<_> = input
                    .as_bytes()
                    .chunks(chunk_size)
                    .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                    .collect();
                let output = stream_with_model(
                    Box::pin(futures_util::stream::iter(chunks)),
                    "gateway".into(),
                )
                .collect::<Vec<_>>()
                .await;
                let output: Vec<_> = output
                    .into_iter()
                    .flat_map(|chunk| chunk.unwrap().to_vec())
                    .collect();
                assert_eq!(output, expected.as_bytes(), "{delimiter:?}, {chunk_size}");
            }
        }
    }

    #[tokio::test]
    async fn native_model_rewrite_preserves_large_coalesced_and_fragmented_preambles() {
        let mut preamble = String::new();
        for index in 0..2048 {
            preamble.push_str(&format!(": keepalive {index}\r\n\r\n"));
            preamble.push_str("event: future\ndata: {\"type\":\"future\",\"opaque\":\"世界\"}\n\n");
        }
        let start = "event: message_start\rdata: {\"type\":\"message_start\",\"message\":{\"model\":\"upstream\",\"content\":[]}}\r\r";
        let suffix = "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let input = format!("{preamble}{start}{suffix}");
        let expected = format!(
            "{preamble}{}{suffix}",
            start.replace("\"upstream\"", "\"gateway\"")
        );
        for chunk_size in [7, 4093, input.len()] {
            let chunks = input
                .as_bytes()
                .chunks(chunk_size)
                .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
                .collect::<Vec<_>>();
            let output = stream_with_model(
                Box::pin(futures_util::stream::iter(chunks)),
                "gateway".into(),
            )
            .collect::<Vec<_>>()
            .await;
            let output = output
                .into_iter()
                .flat_map(|chunk| chunk.unwrap().to_vec())
                .collect::<Vec<_>>();
            assert_eq!(output, expected.as_bytes());
        }

        let unfinished = format!("{preamble}event: message_start\ndata: {{\"type\":");
        let stream =
            futures_util::stream::iter([Ok(Bytes::copy_from_slice(unfinished.as_bytes()))]);
        let output = stream_with_model(Box::pin(stream), "gateway".into())
            .collect::<Vec<_>>()
            .await;
        let output = output
            .into_iter()
            .flat_map(|chunk| chunk.unwrap().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(output, unfinished.as_bytes());
    }

    #[tokio::test]
    async fn native_http_errors_preserve_upstream_words_extensions_and_retry_headers() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"thinking: capability_rejected: synthetic","future_error":true}}"#;
        let response = super::super::anthropic_error_response(GatewayError::Provider(
            ProviderError::NativeMessagesHttp {
                status: 429,
                body: body.into(),
                headers: [
                    ("retry-after".into(), "3".into()),
                    ("x-should-retry".into(), "false".into()),
                ]
                .into(),
            },
        ));
        assert_eq!(response.status(), 429);
        assert_eq!(response.headers()["retry-after"], "3");
        assert_eq!(response.headers()["x-should-retry"], "false");
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            body
        );
    }
}
