use std::collections::BTreeMap;

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderRequestContext, ProviderStream, SseEventParser};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::replay::AnthropicReplayStream;
use super::response::{anthropic_usage_total, extract_anthropic_thinking_blocks};
use crate::streaming::{done_sse_chunk, openai_sse_error_chunk, render_sse_event_chunk};

pub(crate) fn normalize_anthropic_messages_stream<S>(
    upstream: S,
    context: ProviderRequestContext,
    provider_namespace: &'static str,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let created = OffsetDateTime::now_utc().unix_timestamp();
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut saw_message_stop = false;
        let mut stream_failed = false;
        let mut id = format!("chatcmpl-{}", Uuid::new_v4().simple());
        let mut sent_role = false;
        let mut tool_block_indexes = BTreeMap::<u64, u32>::new();
        let mut initial_tool_inputs = BTreeMap::<u64, Value>::new();
        let mut latest_usage = None;
        let mut replay = AnthropicReplayStream::default();
        let mut native_content_sent = false;
        let mut replay_error = None;
        let mut replay_error_has_expected_terminal = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_anthropic_messages_stream_error",
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
                        "anthropic_messages_sse_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                let data = event.data.trim();
                if data == "[DONE]" || data.is_empty() {
                    continue;
                }
                saw_payload_event = true;

                let value = match serde_json::from_str::<Value>(data) {
                    Ok(value) => value,
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk(
                            "anthropic_messages_sse_json_error",
                            &error.to_string(),
                        ));
                        stream_failed = true;
                        break;
                    }
                };
                let event_type = event
                    .event
                    .as_deref()
                    .or_else(|| value.get("type").and_then(Value::as_str))
                    .unwrap_or_default();

                if event_type == "message_stop" {
                    saw_message_stop = true;
                }
                if event_type == "error" {
                    let error = value.get("error").unwrap_or(&value);
                    let code = error
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("anthropic_messages_stream_error");
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("upstream Anthropic Messages stream failed");
                    yield Ok(openai_sse_error_chunk(code, message));
                    stream_failed = true;
                    break;
                }

                if replay_error.is_none()
                    && let Err(message) = replay.observe(&value)
                {
                    replay_error = Some(message);
                    replay = AnthropicReplayStream::default();
                    initial_tool_inputs.clear();
                }
                if let Some(message) = replay_error {
                    let finish_reason = value.get("delta")
                        .and_then(|delta| delta.get("stop_reason"))
                        .and_then(Value::as_str).map(map_stop_reason);
                    let accepted_finish = finish_reason.filter(|reason| {
                        matches!(*reason, "length" | "content_filter")
                            && message.is_incomplete()
                    });
                    if finish_reason.is_some() {
                        replay_error_has_expected_terminal = accepted_finish.is_some();
                    }
                    let usage = map_anthropic_stream_usage(&value)
                        .map(|usage| merge_openai_stream_usage(&mut latest_usage, &usage));
                    if usage.is_some() || accepted_finish.is_some() {
                        let mut chunk = anthropic_stream_chunk(
                            &id, created, &context, json!({}), accepted_finish, usage,
                        );
                        if accepted_finish.is_none() {
                            chunk["choices"] = json!([]);
                        }
                        yield Ok(render_sse_event_chunk(None, &chunk.to_string()));
                    }
                    continue;
                }
                if matches!(event_type, "content_block_start" | "content_block_delta" | "content_block_stop") {
                    let mut outbound_delta = Map::new();
                    match event_type {
                        "content_block_start" => {
                            if let Some(content_block) = value.get("content_block").and_then(Value::as_object) {
                                if content_block.get("type").and_then(Value::as_str) == Some("text")
                                    && let Some(text) = content_block.get("text").and_then(Value::as_str)
                                    && !text.is_empty()
                                {
                                    outbound_delta.insert("content".into(), json!(text));
                                }
                                if content_block.get("type").and_then(Value::as_str) == Some("tool_use") {
                                    if !sent_role {
                                        yield Ok(render_sse_event_chunk(None, &anthropic_stream_chunk(
                                            &id, created, &context, json!({"role":"assistant"}), None, None,
                                        ).to_string()));
                                        sent_role = true;
                                    }
                                    let block_index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
                                    let index = u32::try_from(tool_block_indexes.len()).unwrap_or(u32::MAX);
                                    tool_block_indexes.insert(block_index, index);
                                    initial_tool_inputs.insert(
                                        block_index, content_block.get("input").cloned().unwrap_or_else(|| json!({})),
                                    );
                                    outbound_delta.insert("tool_calls".into(), json!([{
                                        "index":index,
                                        "id":content_block.get("id").and_then(Value::as_str).unwrap_or_default(),
                                        "type":"function",
                                        "function":{
                                            "name":content_block.get("name").and_then(Value::as_str).unwrap_or_default(),
                                            "arguments":""
                                        }
                                    }]));
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = value.get("delta").and_then(Value::as_object) {
                                match delta.get("type").and_then(Value::as_str) {
                                    Some("text_delta") => {
                                        if !sent_role {
                                            yield Ok(render_sse_event_chunk(None, &anthropic_stream_chunk(
                                                &id, created, &context, json!({"role":"assistant"}), None, None,
                                            ).to_string()));
                                            sent_role = true;
                                        }
                                        outbound_delta.insert("content".into(), json!(
                                            delta.get("text").and_then(Value::as_str).unwrap_or_default()
                                        ));
                                    }
                                    Some("input_json_delta") => {
                                        if let Some(block_index) = value.get("index").and_then(Value::as_u64) {
                                            initial_tool_inputs.remove(&block_index);
                                        }
                                        let partial_json = delta.get("partial_json")
                                            .and_then(Value::as_str).unwrap_or_default();
                                        let index = value.get("index").and_then(Value::as_u64)
                                            .and_then(|block_index| tool_block_indexes.get(&block_index).copied())
                                            .unwrap_or(0);
                                        outbound_delta.insert("tool_calls".into(), json!([{
                                            "index":index,"function":{"arguments":partial_json}
                                        }]));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "content_block_stop" => {
                            if let Some(block_index) = value.get("index").and_then(Value::as_u64)
                                && let Some(input) = initial_tool_inputs.remove(&block_index)
                                && let Some(index) = tool_block_indexes.get(&block_index)
                            {
                                outbound_delta.insert("tool_calls".into(), json!([{
                                    "index":index,"function":{"arguments":input.to_string()}
                                }]));
                            }
                        }
                        _ => unreachable!(),
                    }
                    let mut chunk = anthropic_stream_chunk(
                        &id, created, &context, Value::Object(outbound_delta), None, None,
                    );
                    chunk["choices"][0]["delta"]["provider_metadata"][provider_namespace]["native_event"] = value;
                    yield Ok(render_sse_event_chunk(None, &chunk.to_string()));
                    continue;
                }
                if event_type == "message_stop" && !native_content_sent {
                    let mut delta = Map::new();
                    preserve_anthropic_replay_metadata(&mut delta, replay.take_message(), provider_namespace);
                    yield Ok(render_sse_event_chunk(None, &anthropic_stream_chunk(
                        &id, created, &context, Value::Object(delta), None, None,
                    ).to_string()));
                    native_content_sent = true;
                }

                if event_type == "message_start" {
                    if let Some(message_id) = value
                        .get("message")
                        .and_then(|message| message.get("id"))
                        .and_then(Value::as_str)
                    {
                        id = message_id.to_string();
                    }
                    let usage = map_anthropic_stream_usage(&value)
                        .map(|usage| merge_openai_stream_usage(&mut latest_usage, &usage));
                    if !sent_role {
                        let mut delta = Map::new();
                        delta.insert("role".to_string(), json!("assistant"));
                        crate::anthropic::response::preserve_anthropic_message_metadata(
                            &mut delta, &value["message"], provider_namespace, "message_start",
                        );
                        yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                            &id,
                            created,
                            &context,
                            Value::Object(delta),
                            None,
                            usage,
                        )).unwrap_or_else(|_| "{}".to_string())));
                        sent_role = true;
                    }
                    continue;
                }

                if event_type == "message_delta" {
                    let finish_reason = value
                        .get("delta")
                        .and_then(|delta| delta.get("stop_reason"))
                        .and_then(Value::as_str)
                        .map(map_stop_reason);
                    let usage = map_anthropic_stream_usage(&value)
                        .map(|usage| merge_openai_stream_usage(&mut latest_usage, &usage));
                    let mut delta = Map::new();
                    crate::anthropic::response::preserve_anthropic_message_metadata(
                        &mut delta, &value, provider_namespace, "message_delta",
                    );
                    if finish_reason.is_some() && !native_content_sent {
                        preserve_anthropic_replay_metadata(&mut delta, replay.take_message(), provider_namespace);
                        native_content_sent = true;
                    }
                    if finish_reason.is_some() || usage.is_some() || !delta.is_empty() {
                        yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                            &id,
                            created,
                            &context,
                            Value::Object(delta),
                            finish_reason,
                            usage,
                        )).unwrap_or_else(|_| "{}".to_string())));
                    }
                }
            }

            if stream_failed {
                break;
            }
        }

        if !stream_failed && let Err(error) = parser.finish() {
            yield Ok(openai_sse_error_chunk(
                "anthropic_messages_sse_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !saw_payload_event {
            yield Ok(openai_sse_error_chunk(
                "anthropic_messages_empty_stream",
                "upstream Anthropic Messages stream ended without SSE payload events",
            ));
            stream_failed = true;
        }

        if !stream_failed && !replay_error_has_expected_terminal
            && let Some(message) = replay_error
        {
            yield Ok(openai_sse_error_chunk("anthropic_messages_replay_error", message.as_str()));
            stream_failed = true;
        }

        if !stream_failed && !saw_message_stop {
            yield Ok(openai_sse_error_chunk(
                "anthropic_messages_stream_truncated",
                "upstream Anthropic Messages stream ended prematurely before message_stop event",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

fn preserve_anthropic_replay_metadata(
    delta: &mut Map<String, Value>,
    message: Value,
    provider_namespace: &str,
) {
    let thinking = message
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| extract_anthropic_thinking_blocks(blocks))
        .unwrap_or_default();
    if !thinking.is_empty() {
        let metadata = delta
            .entry("provider_metadata")
            .or_insert_with(|| json!({}));
        let mut reasoning = json!({"source":"anthropic_messages"});
        reasoning["blocks"] = Value::Array(thinking);
        metadata[provider_namespace]["reasoning"] = reasoning;
    }
    crate::anthropic::response::attach_owned_anthropic_terminal_replay(
        delta,
        message,
        provider_namespace,
    );
}

fn anthropic_stream_chunk(
    id: &str,
    created: i64,
    context: &ProviderRequestContext,
    delta: Value,
    finish_reason: Option<&str>,
    usage: Option<Value>,
) -> Value {
    let mut chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": context.model_key,
        "provider_model": context.upstream_model,
        "choices": [{
            "index": 0,
            "delta": null,
            "finish_reason": finish_reason
        }]
    });
    chunk["choices"][0]["delta"] = delta;
    if let Some(usage) = usage
        && let Some(object) = chunk.as_object_mut()
    {
        object.insert("usage".to_string(), usage);
    }
    chunk
}

fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" | "model_context_window_exceeded" => "length",
        "tool_use" => "tool_calls",
        "guardrail_intervened" | "content_filtered" | "refusal" => "content_filter",
        _ => "stop",
    }
}

fn map_anthropic_stream_usage(value: &Value) -> Option<Value> {
    let usage = value
        .get("usage")
        .or_else(|| {
            value
                .get("message")
                .and_then(|message| message.get("usage"))
        })?
        .as_object()?;
    let mut mapped = Map::new();
    if let Some(prompt) = usage.get("input_tokens").and_then(Value::as_i64) {
        mapped.insert("prompt_tokens".to_string(), Value::Number(prompt.into()));
    }
    if let Some(completion) = usage.get("output_tokens").and_then(Value::as_i64) {
        mapped.insert(
            "completion_tokens".to_string(),
            Value::Number(completion.into()),
        );
    }
    if !usage.is_empty() {
        mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    }
    if mapped.is_empty() {
        None
    } else {
        Some(Value::Object(mapped))
    }
}

fn merge_openai_stream_usage(latest: &mut Option<Value>, usage: &Value) -> Value {
    let prompt = usage
        .get("prompt_tokens")
        .or_else(|| latest.as_ref().and_then(|usage| usage.get("prompt_tokens")))
        .and_then(Value::as_i64);
    let completion = usage
        .get("completion_tokens")
        .or_else(|| {
            latest
                .as_ref()
                .and_then(|usage| usage.get("completion_tokens"))
        })
        .and_then(Value::as_i64);
    let mut merged = usage.as_object().cloned().unwrap_or_default();
    let mut provider_usage = latest
        .as_ref()
        .and_then(|usage| usage.get("provider_usage"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(incoming) = usage.get("provider_usage").and_then(Value::as_object) {
        provider_usage.extend(incoming.clone());
    }
    if !provider_usage.is_empty() {
        merged.insert("provider_usage".to_string(), Value::Object(provider_usage));
    }
    if let Some(prompt) = prompt {
        merged.insert("prompt_tokens".to_string(), Value::Number(prompt.into()));
    }
    if let Some(completion) = completion {
        merged.insert(
            "completion_tokens".to_string(),
            Value::Number(completion.into()),
        );
    }
    if prompt.is_some()
        && completion.is_some()
        && let Some(provider_usage) = merged.get("provider_usage").and_then(Value::as_object)
    {
        // Explicit unknown clears an earlier valid total in consumers merging usage snapshots.
        let total = anthropic_usage_total(provider_usage)
            .map(|total| Value::Number(total.into()))
            .unwrap_or(Value::Null);
        merged.insert("total_tokens".to_string(), total);
    }
    let merged = Value::Object(merged);
    *latest = Some(merged.clone());
    merged
}
