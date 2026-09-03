use std::collections::BTreeMap;

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderStream, SseEventParser};
use serde_json::{Map, Value, json};

use super::response::{
    anthropic_usage_total, map_anthropic_finish_reason, map_anthropic_stream_usage,
    normalize_anthropic_thinking_delta, normalize_anthropic_thinking_start,
    provider_reasoning_metadata,
};
use crate::streaming::{done_sse_chunk, openai_sse_error_chunk};

pub fn normalize_anthropic_stream<S>(
    upstream: S,
    stream_id: String,
    created: i64,
    model: String,
    provider_namespace: &'static str,
    usage_source: &'static str,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut parser = SseEventParser::default();
        let mut role_emitted = false;
        let mut finish_emitted = false;
        let mut saw_message_stop = false;
        let mut stream_failed = false;
        let mut tool_block_indexes = BTreeMap::<i64, i64>::new();
        let mut latest_usage = None;
        futures_util::pin_mut!(upstream);

        'stream_loop: while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_anthropic_stream_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("anthropic_sse_parse_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
                    continue;
                }

                let payload: Value = match serde_json::from_str(&event.data) {
                    Ok(val) => val,
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk(
                            "anthropic_sse_json_error",
                            &error.to_string(),
                        ));
                        stream_failed = true;
                        break 'stream_loop;
                    }
                };
                let kind = payload
                    .get("type")
                    .and_then(Value::as_str)
                    .or(event.event.as_deref())
                    .unwrap_or_default();

                match kind {
                    "message_start" if !role_emitted => {
                        let mut delta = openai_chunk(
                            &stream_id,
                            created,
                            &model,
                            Some("assistant"),
                            None,
                            None,
                        );
                        if let Some(usage) = map_anthropic_stream_usage(&payload, usage_source) {
                            let usage = merge_openai_stream_usage(&mut latest_usage, &usage);
                            delta["usage"] = usage;
                        }
                        yield Ok(openai_sse_chunk(&delta));
                        role_emitted = true;
                    }
                    "content_block_delta" => {
                        let block_index = payload.get("index").and_then(Value::as_i64);
                        let delta = payload.get("delta").and_then(Value::as_object);
                        let delta_type = delta
                            .and_then(|d| d.get("type"))
                            .and_then(Value::as_str);
                        if delta_type == Some("text_delta")
                            && let Some(text) = delta
                                .and_then(|d| d.get("text"))
                                .and_then(Value::as_str)
                                .filter(|t| !t.is_empty())
                        {
                            let chunk = openai_chunk(
                                &stream_id,
                                created,
                                &model,
                                (!role_emitted).then_some("assistant"),
                                Some(text),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        } else if delta_type == Some("input_json_delta")
                            && let (Some(_block_index), Some(tool_call_index), Some(partial_json)) = (
                                block_index,
                                block_index.and_then(|idx| tool_block_indexes.get(&idx).copied()),
                                delta
                                    .and_then(|d| d.get("partial_json"))
                                    .and_then(Value::as_str),
                            )
                        {
                            let mut outbound_delta = Map::new();
                            if !role_emitted {
                                outbound_delta.insert(
                                    "role".to_string(),
                                    Value::String("assistant".to_string()),
                                );
                            }
                            outbound_delta.insert(
                                "tool_calls".to_string(),
                                json!([{
                                    "index": tool_call_index,
                                    "function": {"arguments": partial_json}
                                }]),
                            );
                            let chunk = openai_delta_chunk(
                                &stream_id,
                                created,
                                &model,
                                Value::Object(outbound_delta),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        } else if let Some(delta) = delta
                            && let Some(block) = normalize_anthropic_thinking_delta(delta)
                        {
                            let mut outbound_delta = Map::new();
                            if !role_emitted {
                                outbound_delta.insert(
                                    "role".to_string(),
                                    Value::String("assistant".to_string()),
                                );
                            }
                            outbound_delta.insert(
                                "provider_metadata".to_string(),
                                provider_reasoning_metadata(
                                    provider_namespace,
                                    "anthropic_messages_stream",
                                    vec![block],
                                ),
                            );
                            let chunk = openai_delta_chunk(
                                &stream_id,
                                created,
                                &model,
                                Value::Object(outbound_delta),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        }
                    }
                    "content_block_start" => {
                        if let Some(content_block) = payload
                            .get("content_block")
                            .and_then(Value::as_object)
                            && content_block.get("type").and_then(Value::as_str) == Some("tool_use")
                        {
                            let block_index = payload.get("index").and_then(Value::as_i64).unwrap_or(0);
                            let tool_call_index = i64::try_from(tool_block_indexes.len()).unwrap_or(i64::MAX);
                            tool_block_indexes.insert(block_index, tool_call_index);
                            let mut outbound_delta = Map::new();
                            if !role_emitted {
                                outbound_delta.insert(
                                    "role".to_string(),
                                    Value::String("assistant".to_string()),
                                );
                            }
                            outbound_delta.insert(
                                "tool_calls".to_string(),
                                json!([{
                                    "index": tool_call_index,
                                    "id": content_block
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .unwrap_or("toolu_anthropic"),
                                    "type": "function",
                                    "function": {
                                        "name": content_block
                                            .get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or("tool"),
                                        "arguments": ""
                                    }
                                }]),
                            );
                            let chunk = openai_delta_chunk(
                                &stream_id,
                                created,
                                &model,
                                Value::Object(outbound_delta),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        } else if let Some(block) = payload
                            .get("content_block")
                            .and_then(Value::as_object)
                            .and_then(normalize_anthropic_thinking_start)
                        {
                            let mut outbound_delta = Map::new();
                            if !role_emitted {
                                outbound_delta.insert(
                                    "role".to_string(),
                                    Value::String("assistant".to_string()),
                                );
                            }
                            outbound_delta.insert(
                                "provider_metadata".to_string(),
                                provider_reasoning_metadata(
                                    provider_namespace,
                                    "anthropic_messages_stream",
                                    vec![block],
                                ),
                            );
                            let chunk = openai_delta_chunk(
                                &stream_id,
                                created,
                                &model,
                                Value::Object(outbound_delta),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        }
                    }
                    "message_delta" => {
                        let usage = map_anthropic_stream_usage(&payload, usage_source)
                            .map(|u| merge_openai_stream_usage(&mut latest_usage, &u));
                        if let Some(reason) = payload
                            .get("delta")
                            .and_then(Value::as_object)
                            .and_then(|d| d.get("stop_reason"))
                            .and_then(Value::as_str)
                        {
                            let mut finish = openai_chunk(
                                &stream_id,
                                created,
                                &model,
                                None,
                                None,
                                Some(map_anthropic_finish_reason(reason)),
                            );
                            if let Some(usage) = usage {
                                finish["usage"] = usage;
                            }
                            yield Ok(openai_sse_chunk(&finish));
                            finish_emitted = true;
                        } else if let Some(usage) = usage {
                            yield Ok(openai_sse_chunk(&openai_usage_chunk(
                                &stream_id,
                                created,
                                &model,
                                usage,
                            )));
                        }
                    }
                    "message_stop" => {
                        saw_message_stop = true;
                        if !finish_emitted {
                            let finish = openai_chunk(
                                &stream_id,
                                created,
                                &model,
                                None,
                                None,
                                Some("stop"),
                            );
                            yield Ok(openai_sse_chunk(&finish));
                            finish_emitted = true;
                        }
                    }
                    "error" => {
                        let message = payload
                            .get("error")
                            .and_then(Value::as_object)
                            .and_then(|err| err.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("anthropic stream error");
                        yield Ok(openai_sse_error_chunk("anthropic_stream_error", message));
                        stream_failed = true;
                        break 'stream_loop;
                    }
                    _ => {}
                }
            }
        }

        if !stream_failed && let Err(error) = parser.finish() {
            yield Ok(openai_sse_error_chunk(
                "anthropic_sse_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !saw_message_stop {
            yield Ok(openai_sse_error_chunk(
                "anthropic_stream_truncated",
                "upstream Anthropic stream ended prematurely before message_stop event",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

pub fn openai_chunk(
    id: &str,
    created: i64,
    model: &str,
    role: Option<&str>,
    content: Option<&str>,
    finish_reason: Option<&str>,
) -> Value {
    let mut delta = Map::new();
    if let Some(role) = role {
        delta.insert("role".to_string(), Value::String(role.to_string()));
    }
    if let Some(content) = content {
        delta.insert("content".to_string(), Value::String(content.to_string()));
    }

    let mut choice = Map::new();
    choice.insert("index".to_string(), Value::Number(0.into()));
    choice.insert("delta".to_string(), Value::Object(delta));
    choice.insert(
        "finish_reason".to_string(),
        finish_reason
            .map(|r| Value::String(r.to_string()))
            .unwrap_or(Value::Null),
    );

    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [Value::Object(choice)]
    })
}

pub fn openai_delta_chunk(
    id: &str,
    created: i64,
    model: &str,
    delta: Value,
    finish_reason: Option<&str>,
) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason
        }]
    })
}

pub fn openai_usage_chunk(id: &str, created: i64, model: &str, usage: Value) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [],
        "usage": usage
    })
}

pub fn openai_sse_chunk(value: &Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

pub fn merge_openai_stream_usage(latest: &mut Option<Value>, usage: &Value) -> Value {
    let usage = openai_usage_with_known_fields(usage.clone(), latest.as_ref());
    *latest = Some(usage.clone());
    usage
}

fn openai_usage_with_known_fields(usage: Value, latest: Option<&Value>) -> Value {
    let prompt_tokens = merged_usage_counter(&usage, latest, "prompt_tokens");
    let completion_tokens = merged_usage_counter(&usage, latest, "completion_tokens");
    let mut object = latest
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(incoming) = usage.as_object() {
        merge_usage_maps(&mut object, incoming);
    }
    if let Some(prompt_tokens) = prompt_tokens {
        object.insert("prompt_tokens".to_string(), json!(prompt_tokens));
    }
    if let Some(completion_tokens) = completion_tokens {
        object.insert("completion_tokens".to_string(), json!(completion_tokens));
    }
    let total_tokens = if object
        .get("usage_source")
        .and_then(Value::as_str)
        .is_some_and(|src| src.contains("anthropic"))
    {
        object
            .get("provider_usage")
            .and_then(Value::as_object)
            .and_then(anthropic_usage_total)
    } else {
        prompt_tokens
            .zip(completion_tokens)
            .and_then(|(prompt, completion)| prompt.checked_add(completion))
            .or_else(|| merged_usage_counter(&usage, latest, "total_tokens"))
    };
    if let Some(total_tokens) = total_tokens {
        object.insert("total_tokens".to_string(), json!(total_tokens));
    }
    Value::Object(object)
}

fn merged_usage_counter(usage: &Value, latest: Option<&Value>, field: &str) -> Option<i64> {
    usage
        .get(field)
        .and_then(Value::as_i64)
        .or_else(|| latest.and_then(|l| l.get(field)).and_then(Value::as_i64))
}

fn merge_usage_maps(current: &mut Map<String, Value>, incoming: &Map<String, Value>) {
    for (key, incoming_value) in incoming {
        match (
            current.get_mut(key).and_then(Value::as_object_mut),
            incoming_value.as_object(),
        ) {
            (Some(current_nested), Some(incoming_nested)) => {
                merge_usage_maps(current_nested, incoming_nested);
            }
            _ => {
                current.insert(key.clone(), incoming_value.clone());
            }
        }
    }
}
