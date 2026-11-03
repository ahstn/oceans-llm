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
            loop {
                let Some(end) = frame_end(&pending, scanned) else {
                    scanned = pending.len().saturating_sub(3);
                    break;
                };
                let frame: Vec<_> = pending.drain(..end).collect();
                scanned = 0;
                if let Some(frame) = rewrite_message_start(&frame, &model) {
                    model_written = true;
                    yield Ok(axum::body::Bytes::from(frame));
                    if !pending.is_empty() { yield Ok(axum::body::Bytes::from(std::mem::take(&mut pending))); }
                    break;
                }
                yield Ok(axum::body::Bytes::from(frame));
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
        } else {
            None
        }
    })
}

fn rewrite_message_start(frame: &[u8], model: &str) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(frame).ok()?;
    let mut parser = gateway_core::SseEventParser::default();
    let event = parser.push_bytes(frame).ok()?.into_iter().next()?;
    let mut payload: Value = serde_json::from_str(&event.data).ok()?;
    if payload["type"] != "message_start" {
        return None;
    }
    payload
        .get_mut("message")?
        .as_object_mut()?
        .insert("model".into(), json!(model));
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut result = String::new();
    let mut written = false;
    for line in text.split_inclusive('\n') {
        if line.starts_with("data:") {
            if !written {
                result.push_str(&format!("data: {payload}\n"));
                written = true;
            }
        } else {
            result.push_str(line);
        }
    }
    Some(result.into_bytes())
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
    async fn native_model_rewrite_preserves_unknown_events_and_fragmented_frames() {
        let suffix = concat!(
            "id: opaque\nretry: 250\nevent: future_event\ndata: {\"type\":\"future_event\",\"text\":\"世界\"}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        );
        for newline in ["\n", "\r\n", "\r"] {
            let start = format!(
                "id: original{newline}event: message_start{newline}data: {{\"type\":\"message_start\",\"message\":{{\"model\":\"upstream\",\"future\":true}}}}{newline}{newline}"
            );
            let input = format!(": keepalive\n\n{start}{suffix}");
            for chunk_size in [1, 3, input.len()] {
                let chunks: Vec<_> = input
                    .as_bytes()
                    .chunks(chunk_size)
                    .map(|c| Ok(Bytes::copy_from_slice(c)))
                    .collect();
                let result = stream_with_model(
                    Box::pin(futures_util::stream::iter(chunks)),
                    "gateway-alias".into(),
                )
                .collect::<Vec<_>>()
                .await;
                let result: Vec<_> = result
                    .into_iter()
                    .flat_map(|c| c.unwrap().to_vec())
                    .collect();
                let text = String::from_utf8(result).unwrap();
                assert!(text.starts_with(": keepalive\n\n"));
                assert!(text.contains("id: original\n"));
                assert!(
                    text.ends_with(suffix),
                    "unknown frames must be byte-for-byte unchanged"
                );
                let mut parser = gateway_core::SseEventParser::default();
                let events = parser.push_bytes(text.as_bytes()).unwrap();
                let payload: Value = serde_json::from_str(&events[1].data).unwrap();
                assert_eq!(
                    payload["message"],
                    json!({"model":"gateway-alias","future":true})
                );
            }
        }
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
