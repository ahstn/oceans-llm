use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderStream, SseEventParser};
use serde_json::json;
use serde_json::{Map, Value};

pub(crate) fn openai_sse_error_chunk(kind: &str, message: &str) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        json!({
            "error": {
                "message": message,
                "type": "upstream_error",
                "code": kind
            }
        })
    ))
}

pub(crate) fn done_sse_chunk() -> Bytes {
    Bytes::from("data: [DONE]\n\n")
}

pub(crate) fn render_sse_event_chunk(event: Option<&str>, data: &str) -> Bytes {
    let mut rendered = String::new();
    if let Some(event) = event {
        rendered.push_str("event: ");
        rendered.push_str(event);
        rendered.push('\n');
    }

    for line in data.split('\n') {
        rendered.push_str("data: ");
        rendered.push_str(line);
        rendered.push('\n');
    }
    rendered.push('\n');

    Bytes::from(rendered)
}

pub(crate) fn normalize_openai_compat_stream<S>(upstream: S) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_openai_compat_stream_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "openai_compat_sse_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                let data = event.data.trim();
                if data == "[DONE]" {
                    continue;
                }

                if data.is_empty() && event.event.is_none() {
                    continue;
                }

                saw_payload_event = true;
                let normalized_data = normalize_openai_compat_sse_data(&event.data);
                yield Ok(render_sse_event_chunk(event.event.as_deref(), &normalized_data));
            }
        }

        if !stream_failed && let Err(error) = parser.finish() {
            yield Ok(openai_sse_error_chunk(
                "openai_compat_sse_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !saw_payload_event {
            yield Ok(openai_sse_error_chunk(
                "openai_compat_empty_stream",
                "upstream stream ended without SSE payload events",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

fn normalize_openai_compat_sse_data(data: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(data) else {
        return data.to_string();
    };

    normalize_openai_compat_chunk_value(&mut value);
    serde_json::to_string(&value).unwrap_or_else(|_| data.to_string())
}

pub(crate) fn normalize_openai_compat_responses_stream<S>(upstream: S) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_openai_compat_responses_stream_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "openai_compat_responses_sse_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                let data = event.data.trim();
                if data == "[DONE]" {
                    continue;
                }

                if data.is_empty() && event.event.is_none() {
                    continue;
                }

                saw_payload_event = true;
                yield Ok(render_sse_event_chunk(event.event.as_deref(), &event.data));
            }
        }

        if !stream_failed && let Err(error) = parser.finish() {
            yield Ok(openai_sse_error_chunk(
                "openai_compat_responses_sse_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !saw_payload_event {
            yield Ok(openai_sse_error_chunk(
                "openai_compat_responses_empty_stream",
                "upstream responses stream ended without SSE payload events",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

fn normalize_openai_compat_chunk_value(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let mut usage_from_choice = None;
    if let Some(choices) = object.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            let Some(choice_object) = choice.as_object_mut() else {
                continue;
            };

            if usage_from_choice.is_none()
                && let Some(usage) = choice_object.get("usage").filter(|usage| !usage.is_null())
            {
                usage_from_choice = Some(usage.clone());
            }

            if let Some(delta) = choice_object
                .get_mut("delta")
                .and_then(Value::as_object_mut)
            {
                normalize_openai_compat_delta_reasoning(delta);
            }
        }
    }

    if !object.contains_key("usage")
        && let Some(usage) = usage_from_choice
    {
        object.insert("usage".to_string(), usage);
    }
}

fn normalize_openai_compat_delta_reasoning(delta: &mut Map<String, Value>) {
    if delta.contains_key("reasoning") {
        return;
    }

    for field in ["reasoning_content", "reasoning_text"] {
        if let Some(value) = delta
            .get(field)
            .filter(|value| value.as_str().is_some_and(|text| !text.is_empty()) || !value.is_null())
        {
            delta.insert("reasoning".to_string(), value.clone());
            return;
        }
    }
}
