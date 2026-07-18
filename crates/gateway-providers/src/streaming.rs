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
        let mut saw_output_event = false;
        let mut saw_terminal_event = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        'upstream: while let Some(chunk) = upstream.next().await {
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
                    saw_terminal_event = true;
                    break 'upstream;
                }

                if data.is_empty() {
                    continue;
                }

                let value = match serde_json::from_str::<Value>(data) {
                    Ok(value) if value.is_object() => value,
                    Ok(_) => {
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_payload_parse_error",
                            "Chat Completions SSE payload must be a JSON object",
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_payload_parse_error",
                            &format!("invalid Chat Completions SSE JSON payload: {error}"),
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                };
                saw_payload_event = true;
                let (normalized_data, semantics) =
                    normalize_openai_compat_chat_sse_data(value);
                saw_output_event |= semantics.has_output;
                saw_terminal_event |= semantics.is_terminal;
                yield Ok(render_sse_event_chunk(event.event.as_deref(), &normalized_data));

                if semantics.is_error {
                    stream_failed = true;
                    break 'upstream;
                }
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

        if !stream_failed && !saw_terminal_event {
            let message = if saw_output_event {
                "upstream Chat Completions stream ended after output without `[DONE]` or a finish_reason"
            } else {
                "upstream Chat Completions stream ended without `[DONE]` or a finish_reason"
            };
            yield Ok(openai_sse_error_chunk(
                "openai_compat_premature_eof",
                message,
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

#[derive(Debug, Default)]
struct ChatSseSemantics {
    has_output: bool,
    is_terminal: bool,
    is_error: bool,
}

fn normalize_openai_compat_chat_sse_data(mut value: Value) -> (String, ChatSseSemantics) {
    let semantics = chat_sse_semantics(&value);
    normalize_openai_compat_chunk_value(&mut value);
    (value.to_string(), semantics)
}

pub(crate) fn normalize_openai_compat_responses_stream<S>(upstream: S) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut saw_output_event = false;
        let mut saw_terminal_event = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        'upstream: while let Some(chunk) = upstream.next().await {
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
                if data.is_empty() {
                    continue;
                }

                let parsed = match serde_json::from_str::<Value>(data) {
                    Ok(value) if value.is_object() => value,
                    Ok(_) => {
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_responses_payload_parse_error",
                            "Responses SSE payload must be a JSON object",
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_responses_payload_parse_error",
                            &format!("invalid Responses SSE JSON payload: {error}"),
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                };
                saw_payload_event = true;
                if is_chat_completions_chunk(&parsed) {
                    yield Ok(openai_sse_error_chunk(
                        "openai_compat_responses_protocol_mismatch",
                        "received a Chat Completions chunk from a Responses endpoint",
                    ));
                    stream_failed = true;
                    break 'upstream;
                }

                let event_type = responses_event_type(event.event.as_deref(), Some(&parsed));
                if let Some(error) =
                    responses_event_type_mismatch(event.event.as_deref(), Some(&parsed))
                {
                    yield Ok(openai_sse_error_chunk(
                        "openai_compat_responses_protocol_error",
                        &error,
                    ));
                    stream_failed = true;
                    break 'upstream;
                }

                if event_type.is_some_and(responses_event_has_output) {
                    saw_output_event = true;
                }

                if matches!(event_type, Some("error" | "response.failed"))
                    || is_top_level_stream_error(&parsed)
                {
                    yield Ok(render_sse_event_chunk(event.event.as_deref(), &event.data));
                    stream_failed = true;
                    break 'upstream;
                }

                if matches!(
                    event_type,
                    Some("response.completed" | "response.incomplete")
                ) {
                    if let Some(error) =
                        validate_responses_terminal_status(event_type, Some(&parsed))
                    {
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_responses_protocol_error",
                            &error,
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                    saw_terminal_event = true;
                }

                yield Ok(render_sse_event_chunk(event.event.as_deref(), &event.data));
                if saw_terminal_event {
                    break 'upstream;
                }
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

        if !stream_failed && !saw_terminal_event {
            let message = if saw_output_event {
                "upstream Responses stream ended after output without response.completed or response.incomplete"
            } else {
                "upstream Responses stream ended without response.completed or response.incomplete"
            };
            yield Ok(openai_sse_error_chunk(
                "openai_compat_responses_premature_eof",
                message,
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

fn chat_sse_semantics(value: &Value) -> ChatSseSemantics {
    let Some(object) = value.as_object() else {
        return ChatSseSemantics::default();
    };
    let choices = object.get("choices").and_then(Value::as_array);
    ChatSseSemantics {
        has_output: choices.is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .is_some_and(|delta| {
                        delta.iter().any(|(key, value)| {
                            key != "role"
                                && !value.is_null()
                                && value.as_str().is_none_or(|text| !text.is_empty())
                        })
                    })
            })
        }),
        is_terminal: choices.is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("finish_reason")
                    .and_then(Value::as_str)
                    .is_some_and(|reason| !reason.is_empty())
            })
        }),
        is_error: is_top_level_stream_error(value),
    }
}

fn is_top_level_stream_error(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("type").and_then(Value::as_str) == Some("error")
        || object.get("error").is_some_and(|error| !error.is_null())
}

fn is_chat_completions_chunk(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("choices"))
}

fn responses_event_type<'a>(
    event_name: Option<&'a str>,
    value: Option<&'a Value>,
) -> Option<&'a str> {
    value
        .and_then(Value::as_object)
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .or(event_name)
}

fn responses_event_type_mismatch(
    event_name: Option<&str>,
    value: Option<&Value>,
) -> Option<String> {
    let data_type = value
        .and_then(Value::as_object)
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str);
    match (event_name, data_type) {
        (Some(event_name), Some(data_type))
            if (event_name.starts_with("response.") || event_name == "error")
                && event_name != data_type =>
        {
            Some(format!(
                "SSE event `{event_name}` conflicts with payload type `{data_type}`"
            ))
        }
        _ => None,
    }
}

fn responses_event_has_output(event_type: &str) -> bool {
    event_type.starts_with("response.output_")
        || matches!(
            event_type,
            "response.output_text.delta"
                | "response.reasoning_text.delta"
                | "response.function_call_arguments.delta"
        )
}

fn validate_responses_terminal_status(
    event_type: Option<&str>,
    value: Option<&Value>,
) -> Option<String> {
    let expected_status = match event_type {
        Some("response.completed") => "completed",
        Some("response.incomplete") => "incomplete",
        _ => return None,
    };
    let status = value
        .and_then(Value::as_object)
        .and_then(|object| object.get("response"))
        .and_then(Value::as_object)
        .and_then(|response| response.get("status"))
        .and_then(Value::as_str);
    status
        .filter(|status| *status != expected_status)
        .map(|status| {
            format!(
                "terminal event `{}` conflicts with response status `{status}`",
                event_type.unwrap_or_default()
            )
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

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    async fn render(stream: ProviderStream) -> String {
        let chunks = stream.collect::<Vec<_>>().await;
        chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.expect("stream chunk").to_vec()).expect("utf8"))
            .collect()
    }

    fn upstream(
        transcript: &'static str,
    ) -> impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> {
        stream::iter([Ok(Bytes::from_static(transcript.as_bytes()))])
    }

    #[tokio::test]
    async fn chat_accepts_finish_reason_as_terminal_and_appends_done() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"content\":\"hi\""));
        assert!(rendered.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn chat_rejects_premature_eof_after_output() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_premature_eof\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn chat_rejects_non_string_finish_reason_as_terminal() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":false}]}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_premature_eof\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn chat_rejects_malformed_payload_even_when_done_follows() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: not-json\n\ndata: [DONE]\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_payload_parse_error\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn chat_ignores_named_events_without_payload_data() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "event: ping\ndata:\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        )))
        .await;

        assert!(!rendered.contains("event: ping"));
        assert!(rendered.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn chat_surfaces_top_level_error_without_done() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: {\"error\":{\"type\":\"server_error\",\"message\":\"boom\"}}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"message\":\"boom\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_accepts_completed_and_incomplete_terminal_events() {
        for (event_type, status) in [
            ("response.completed", "completed"),
            ("response.incomplete", "incomplete"),
        ] {
            let transcript = format!(
                "event: {event_type}\ndata: {{\"type\":\"{event_type}\",\"response\":{{\"status\":\"{status}\"}}}}\n\n"
            );
            let rendered = render(normalize_openai_compat_responses_stream(stream::iter([
                Ok::<Bytes, reqwest::Error>(Bytes::from(transcript)),
            ])))
            .await;

            assert!(rendered.contains(event_type));
            assert!(rendered.ends_with("data: [DONE]\n\n"));
        }
    }

    #[tokio::test]
    async fn responses_surfaces_failed_terminal_without_done() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"boom\"}}}\n\n",
        )))
        .await;

        assert!(rendered.contains("response.failed"));
        assert!(rendered.contains("\"message\":\"boom\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_surfaces_pre_output_error_without_done() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "event: error\ndata: {\"type\":\"error\",\"code\":\"rate_limit_exceeded\",\"message\":\"slow down\"}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"rate_limit_exceeded\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_rejects_chat_completions_protocol_mismatch() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"delta\":{\"content\":\"wrong API\"}}]}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_responses_protocol_mismatch\""));
        assert!(!rendered.contains("wrong API"));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_rejects_malformed_terminal_payload() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "event: response.completed\ndata: not-json\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_responses_payload_parse_error\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_ignores_named_events_without_payload_data() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "event: ping\ndata:\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
        )))
        .await;

        assert!(!rendered.contains("event: ping"));
        assert!(rendered.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn responses_rejects_premature_eof_after_output() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_responses_premature_eof\""));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn responses_rejects_conflicting_terminal_status() {
        let rendered = render(normalize_openai_compat_responses_stream(upstream(
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\"}}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_responses_protocol_error\""));
        assert!(!rendered.contains("data: [DONE]"));
    }
}
