use std::collections::HashSet;

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
    normalize_openai_compat_chat_stream(upstream, true)
}

/// With `forward_usage_chunk` false, the usage-only chunk (`choices: []` plus `usage`) is folded
/// into the final choice chunk. The gateway requested usage for its own ledger, so a client that
/// did not ask for `include_usage` gets no extra chunk, while the downstream usage collector
/// still reads the counts.
pub(crate) fn normalize_openai_compat_chat_stream<S>(
    upstream: S,
    forward_usage_chunk: bool,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut saw_output_event = false;
        let mut saw_terminal_event = false;
        let mut stream_failed = false;
        let mut seen_choices = HashSet::new();
        let mut finished_choices = HashSet::new();
        // The final choice chunk, held back while a usage-only chunk may still fold into it.
        let mut held: Option<HeldChatChunk> = None;
        // A usage-only chunk that came before the final choice chunk it folds into.
        let mut early_usage: Option<HeldChatChunk> = None;
        futures_util::pin_mut!(upstream);

        'upstream: while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
                        yield Ok(pending);
                    }
                    yield Ok(openai_sse_error_chunk(
                        "upstream_openai_compat_stream_error",
                        &crate::http::stream_read_error_message(&error),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
                        yield Ok(pending);
                    }
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
                        for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
                            yield Ok(pending);
                        }
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_payload_parse_error",
                            "Chat Completions SSE payload must be a JSON object",
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                    Err(error) => {
                        for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
                            yield Ok(pending);
                        }
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_payload_parse_error",
                            &format!("invalid Chat Completions SSE JSON payload: {error}"),
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                };
                let semantics = match chat_sse_semantics(&value) {
                    Ok(semantics) => semantics,
                    Err(error) => {
                        for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
                            yield Ok(pending);
                        }
                        yield Ok(openai_sse_error_chunk(
                            "openai_compat_protocol_error",
                            &error,
                        ));
                        stream_failed = true;
                        break 'upstream;
                    }
                };
                saw_payload_event = true;
                saw_output_event |= semantics.has_output;
                for choice in semantics.choices {
                    seen_choices.insert(choice.key);
                    if choice.is_terminal {
                        finished_choices.insert(choice.key);
                    }
                }
                saw_terminal_event =
                    !seen_choices.is_empty() && seen_choices.is_subset(&finished_choices);
                if !forward_usage_chunk {
                    if !semantics.is_error && is_usage_only_chunk(&value) {
                        match held.as_mut() {
                            Some((_, final_chunk)) => {
                                fold_usage_chunk(final_chunk, value);
                                continue;
                            }
                            None if !saw_terminal_event => {
                                early_usage = Some((event.event, value));
                                continue;
                            }
                            None => {}
                        }
                    } else {
                        // Usage held for a final chunk must precede an error, which ends the stream.
                        if semantics.is_error {
                            for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
                                yield Ok(pending);
                            }
                        } else if let Some(final_chunk) = render_held_chat_chunk(&mut held) {
                            yield Ok(final_chunk);
                        }
                        if saw_terminal_event && !semantics.is_error {
                            let mut final_chunk = value;
                            if let Some((_, usage_chunk)) = early_usage.take() {
                                fill_from_early_usage_chunk(&mut final_chunk, usage_chunk);
                            }
                            held = Some((event.event, final_chunk));
                            continue;
                        }
                    }
                }
                let normalized_data = normalize_openai_compat_chat_sse_data(value);
                yield Ok(render_sse_event_chunk(event.event.as_deref(), &normalized_data));

                if semantics.is_error {
                    stream_failed = true;
                    break 'upstream;
                }
            }
        }

        // Usage the stream never got to fold still reaches the ledger.
        for pending in take_pending_chat_chunks(&mut held, &mut early_usage) {
            yield Ok(pending);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ChatChoiceKey {
    Index(u64),
    Position(usize),
}

#[derive(Debug)]
struct ChatChoiceSemantics {
    key: ChatChoiceKey,
    is_terminal: bool,
}

#[derive(Debug, Default)]
struct ChatSseSemantics {
    has_output: bool,
    is_error: bool,
    choices: Vec<ChatChoiceSemantics>,
}

fn normalize_openai_compat_chat_sse_data(mut value: Value) -> String {
    normalize_openai_compat_chunk_value(&mut value);
    value.to_string()
}

/// A Chat Completions chunk and its SSE event name.
type HeldChatChunk = (Option<String>, Value);

fn render_held_chat_chunk(held: &mut Option<HeldChatChunk>) -> Option<Bytes> {
    held.take().map(|(event, value)| {
        render_sse_event_chunk(
            event.as_deref(),
            &normalize_openai_compat_chat_sse_data(value),
        )
    })
}

/// The held final chunk, then a usage-only chunk that never found a final chunk to fold
/// into, for a stream about to end.
fn take_pending_chat_chunks(
    held: &mut Option<HeldChatChunk>,
    early_usage: &mut Option<HeldChatChunk>,
) -> Vec<Bytes> {
    render_held_chat_chunk(held)
        .into_iter()
        .chain(render_held_chat_chunk(early_usage))
        .collect()
}

/// The chunk `stream_options.include_usage` adds after the last choice chunk.
fn is_usage_only_chunk(value: &Value) -> bool {
    value.get("usage").is_some_and(Value::is_object)
        && value
            .get("choices")
            .is_none_or(|choices| choices.as_array().is_some_and(Vec::is_empty))
}

/// Moves `usage` onto the final choice chunk; other top-level fields such as `service_tier`
/// fill gaps without overriding what the final chunk already reports.
fn fold_usage_chunk(final_chunk: &mut Value, usage_chunk: Value) {
    let (Some(target), Value::Object(source)) = (final_chunk.as_object_mut(), usage_chunk) else {
        return;
    };
    for (key, value) in source {
        if key == "usage" {
            target.insert(key, value);
        } else if key != "choices" {
            target.entry(key).or_insert(value);
        }
    }
}

/// Folds a usage-only chunk that came before the final choice chunk. The final chunk is
/// later, so a `usage` object or other field it already reports wins. A `usage: null`
/// placeholder is not a report.
fn fill_from_early_usage_chunk(final_chunk: &mut Value, usage_chunk: Value) {
    let (Some(target), Value::Object(source)) = (final_chunk.as_object_mut(), usage_chunk) else {
        return;
    };
    for (key, value) in source {
        if key == "usage" && target.get("usage").is_some_and(Value::is_null) {
            target.insert(key, value);
        } else if key != "choices" {
            target.entry(key).or_insert(value);
        }
    }
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
                        &crate::http::stream_read_error_message(&error),
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

fn chat_sse_semantics(value: &Value) -> Result<ChatSseSemantics, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "Chat Completions SSE payload must be a JSON object".to_string())?;
    if is_top_level_stream_error(value) {
        return Ok(ChatSseSemantics {
            is_error: true,
            ..ChatSseSemantics::default()
        });
    }
    if object.get("choices").is_none() && object.get("usage").is_some_and(Value::is_object) {
        return Ok(ChatSseSemantics::default());
    }

    let choices = object
        .get("choices")
        .ok_or_else(|| {
            "received a non-Chat Completions payload from a Chat Completions endpoint".to_string()
        })?
        .as_array()
        .ok_or_else(|| "Chat Completions SSE `choices` must be an array".to_string())?;
    let mut semantics = ChatSseSemantics::default();
    for (position, choice) in choices.iter().enumerate() {
        let choice = choice
            .as_object()
            .ok_or_else(|| "Chat Completions SSE choices must be objects".to_string())?;
        let key = match choice.get("index") {
            None => ChatChoiceKey::Position(position),
            Some(index) => ChatChoiceKey::Index(index.as_u64().ok_or_else(|| {
                "Chat Completions SSE choice `index` must be a non-negative integer".to_string()
            })?),
        };
        let is_terminal = match choice.get("finish_reason") {
            None | Some(Value::Null) => false,
            Some(Value::String(reason)) if !reason.is_empty() => true,
            Some(Value::String(_)) => {
                return Err(
                    "Chat Completions SSE choice `finish_reason` must not be empty".to_string(),
                );
            }
            Some(_) => {
                return Err(
                    "Chat Completions SSE choice `finish_reason` must be a string or null"
                        .to_string(),
                );
            }
        };
        semantics.has_output |=
            choice
                .get("delta")
                .and_then(Value::as_object)
                .is_some_and(|delta| {
                    delta.iter().any(|(key, value)| {
                        key != "role"
                            && !value.is_null()
                            && value.as_str().is_none_or(|text| !text.is_empty())
                    })
                });
        semantics
            .choices
            .push(ChatChoiceSemantics { key, is_terminal });
    }
    Ok(semantics)
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
    match status {
        Some(status) if status == expected_status => None,
        Some(status) => Some(format!(
            "terminal event `{}` conflicts with response status `{status}`",
            event_type.unwrap_or_default()
        )),
        None => Some(format!(
            "terminal event `{}` must include string response status `{expected_status}`",
            event_type.unwrap_or_default()
        )),
    }
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
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":false}]}\n\ndata: [DONE]\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_protocol_error\""));
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
    async fn chat_rejects_responses_protocol_mismatch() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\ndata: [DONE]\n\n",
        )))
        .await;

        assert!(rendered.contains("\"code\":\"openai_compat_protocol_error\""));
        assert!(!rendered.contains("response.completed"));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn chat_requires_every_seen_choice_to_finish_before_eof() {
        let incomplete = render(normalize_openai_compat_stream(upstream(
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"},{\"index\":1,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
        )))
        .await;
        assert!(incomplete.contains("\"code\":\"openai_compat_premature_eof\""));
        assert!(!incomplete.contains("data: [DONE]"));

        let complete = render(normalize_openai_compat_stream(upstream(
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"},{\"index\":1,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":1,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        )))
        .await;
        assert!(complete.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn chat_accepts_proxy_usage_chunk_without_choices() {
        let rendered = render(normalize_openai_compat_stream(upstream(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n",
        )))
        .await;

        assert!(rendered.contains("\"usage\""));
        assert!(rendered.ends_with("data: [DONE]\n\n"));
    }

    const GATEWAY_REQUESTED_USAGE_TRANSCRIPT: &str = "data: {\"id\":\"c\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n\
         data: {\"id\":\"c\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
         data: {\"id\":\"c\",\"choices\":[],\"service_tier\":\"default\",\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n\
         data: [DONE]\n\n";

    fn rendered_events(rendered: &str) -> Vec<&str> {
        rendered
            .split("\n\n")
            .filter(|event| !event.is_empty())
            .map(|event| event.strip_prefix("data: ").expect("data event"))
            .collect()
    }

    #[tokio::test]
    async fn chat_folds_usage_chunk_into_final_chunk_when_client_did_not_ask() {
        let rendered = render(normalize_openai_compat_chat_stream(
            upstream(GATEWAY_REQUESTED_USAGE_TRANSCRIPT),
            false,
        ))
        .await;

        let events = rendered_events(&rendered);
        assert_eq!(events.len(), 3, "{rendered}");
        assert!(!rendered.contains("\"choices\":[]"), "{rendered}");
        let final_chunk: Value = serde_json::from_str(events[1]).expect("final chunk");
        assert_eq!(final_chunk["choices"][0]["finish_reason"], "stop");
        assert_eq!(final_chunk["usage"]["total_tokens"], 3);
        assert_eq!(final_chunk["service_tier"], "default");
        assert_eq!(events[2], "[DONE]");
    }

    #[tokio::test]
    async fn chat_forwards_usage_chunk_when_client_asked() {
        let rendered = render(normalize_openai_compat_chat_stream(
            upstream(GATEWAY_REQUESTED_USAGE_TRANSCRIPT),
            true,
        ))
        .await;

        let events = rendered_events(&rendered);
        assert_eq!(events.len(), 4, "{rendered}");
        let final_chunk: Value = serde_json::from_str(events[1]).expect("final chunk");
        assert!(final_chunk.get("usage").is_none(), "{final_chunk}");
        let usage_chunk: Value = serde_json::from_str(events[2]).expect("usage chunk");
        assert_eq!(usage_chunk["choices"], json!([]));
        assert_eq!(usage_chunk["usage"]["total_tokens"], 3);
    }

    #[tokio::test]
    async fn chat_flushes_held_final_chunk_before_a_later_error() {
        let rendered = render(normalize_openai_compat_chat_stream(
            upstream(
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: not-json\n\n",
            ),
            false,
        ))
        .await;

        let finish = rendered
            .find("\"finish_reason\":\"stop\"")
            .expect("final chunk");
        let error = rendered
            .find("openai_compat_payload_parse_error")
            .expect("error chunk");
        assert!(finish < error, "{rendered}");
    }

    #[tokio::test]
    async fn chat_final_chunk_usage_wins_over_an_early_usage_chunk() {
        let rendered = render(normalize_openai_compat_chat_stream(
            upstream(
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":0,\"total_tokens\":2}}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":5,\"total_tokens\":7}}\n\n\
                 data: [DONE]\n\n",
            ),
            false,
        ))
        .await;

        let events = rendered_events(&rendered);
        assert_eq!(events.len(), 2, "{rendered}");
        let final_chunk: Value = serde_json::from_str(events[0]).expect("final chunk");
        assert_eq!(final_chunk["usage"]["total_tokens"], 7, "{rendered}");

        // `usage: null` is a placeholder, not a report, so the early usage still lands.
        let rendered = render(normalize_openai_compat_chat_stream(
            upstream(
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n\
                 data: [DONE]\n\n",
            ),
            false,
        ))
        .await;
        let final_chunk: Value =
            serde_json::from_str(rendered_events(&rendered)[0]).expect("final chunk");
        assert_eq!(final_chunk["usage"]["total_tokens"], 3, "{rendered}");
    }

    #[tokio::test]
    async fn chat_flushes_early_usage_before_an_error() {
        for transcript in [
            concat!(
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n",
                "data: not-json\n\n"
            ),
            concat!(
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n",
                "data: {\"error\":{\"message\":\"boom\"}}\n\n"
            ),
        ] {
            let rendered = render(normalize_openai_compat_chat_stream(
                upstream(transcript),
                false,
            ))
            .await;

            let usage = rendered.find("\"usage\"").expect("usage chunk");
            let error = rendered.find("\"error\"").expect("error chunk");
            assert!(usage < error, "{rendered}");
        }
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

    #[tokio::test]
    async fn responses_requires_string_terminal_status() {
        for response in ["{}", "{\"status\":123}"] {
            let transcript = format!(
                "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{response}}}\n\n"
            );
            let rendered = render(normalize_openai_compat_responses_stream(stream::iter([
                Ok::<Bytes, reqwest::Error>(Bytes::from(transcript)),
            ])))
            .await;

            assert!(rendered.contains("\"code\":\"openai_compat_responses_protocol_error\""));
            assert!(!rendered.contains("data: [DONE]"));
        }
    }
}
