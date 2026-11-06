use std::collections::BTreeMap;

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderStream, SseEventParser};
use serde_json::{Map, Value, json};

use super::replay::{AnthropicReplayStream, ReplayError};
use super::response::{
    anthropic_usage_total, attach_owned_anthropic_terminal_replay, map_anthropic_finish_reason,
    map_anthropic_stream_usage, preserve_anthropic_message_metadata,
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
        let mut tool_initial_arguments = BTreeMap::<i64, String>::new();
        let mut latest_usage = None;
        let mut replay = AnthropicReplayStream::default();
        let mut replay_error = None;
        let mut incomplete_terminal = false;
        futures_util::pin_mut!(upstream);

        'stream_loop: while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_anthropic_stream_error", &crate::http::stream_read_error_message(&error)));
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

                let mut payload: Value = match serde_json::from_str(&event.data) {
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
                if payload.get("type").is_none()
                    && let Some(kind) = event.event.as_deref()
                    && let Some(object) = payload.as_object_mut()
                {
                    object.insert("type".into(), json!(kind));
                }
                let kind = payload
                    .get("type")
                    .and_then(Value::as_str)
                    .or(event.event.as_deref())
                    .unwrap_or_default();

                if replay_error.is_none()
                    && let Err(error) = replay.observe(&payload)
                {
                    replay_error = Some(error);
                    replay = AnthropicReplayStream::default();
                    tool_initial_arguments.clear();
                }
                // A malformed generated block can still be billable. Drain final usage before
                // reporting failure, but never publish a successful finish or partial replay.
                if replay_error.is_some() {
                    let mut state = Map::new();
                    let mut finish_reason = None;
                    if kind == "message_delta" {
                        preserve_anthropic_message_metadata(
                            &mut state, &payload["delta"], provider_namespace, "message",
                        );
                        let terminal = payload["delta"]["stop_reason"].as_str()
                            .map(map_anthropic_finish_reason);
                        if matches!(terminal, Some("length" | "content_filter"))
                            && replay_error.is_some_and(ReplayError::is_incomplete)
                        {
                            incomplete_terminal = true;
                            finish_reason = terminal;
                            finish_emitted = true;
                        }
                    }
                    let usage = map_anthropic_stream_usage(&payload, usage_source)
                        .map(|usage| merge_openai_stream_usage(&mut latest_usage, &usage));
                    if !state.is_empty() || finish_reason.is_some() {
                        let mut chunk = openai_delta_chunk(
                            &stream_id, created, &model, Value::Object(state), finish_reason,
                        );
                        if let Some(usage) = usage {
                            chunk["usage"] = usage;
                        }
                        yield Ok(openai_sse_chunk(&chunk));
                    } else if let Some(usage) = usage {
                        yield Ok(openai_sse_chunk(&openai_usage_chunk(
                            &stream_id, created, &model, usage,
                        )));
                    }
                    if kind == "message_stop" {
                        saw_message_stop = true;
                        break 'stream_loop;
                    }
                    continue;
                }
                let mut metadata_delta = Map::new();
                if kind == "message_delta" {
                    preserve_anthropic_message_metadata(
                        &mut metadata_delta,
                        &payload,
                        provider_namespace,
                        "message_delta",
                    );
                }
                if !metadata_delta.is_empty() {
                    yield Ok(openai_sse_chunk(&openai_delta_chunk(
                        &stream_id, created, &model, Value::Object(metadata_delta), None,
                    )));
                }

                let mut content_chunk = None;
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
                        preserve_anthropic_message_metadata(
                            delta["choices"][0]["delta"].as_object_mut().expect("delta object"),
                            &payload["message"],
                            provider_namespace,
                            "message_start",
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
                            content_chunk = Some(chunk);
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
                            if let Some(index) = block_index {
                                tool_initial_arguments.remove(&index);
                            }
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
                            content_chunk = Some(chunk);
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
                            tool_initial_arguments.insert(block_index, content_block.get("input")
                                .cloned().unwrap_or_else(|| json!({})).to_string());
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
                            content_chunk = Some(chunk);
                            role_emitted = true;
                        } else if let Some(text) = payload.get("content_block")
                            .filter(|block| block["type"] == "text")
                            .and_then(|block| block.get("text")).and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            content_chunk = Some(openai_chunk(
                                &stream_id, created, &model,
                                (!role_emitted).then_some("assistant"), Some(text), None,
                            ));
                            role_emitted = true;
                        }
                    }
                    "content_block_stop" => {
                        if let Some(index) = payload.get("index").and_then(Value::as_i64)
                            && let Some(arguments) = tool_initial_arguments.remove(&index)
                            && let Some(tool_index) = tool_block_indexes.get(&index)
                        {
                            content_chunk = Some(openai_delta_chunk(
                                &stream_id, created, &model,
                                json!({"tool_calls": [{"index": tool_index,
                                    "function": {"arguments": arguments}}]}), None,
                            ));
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
                            attach_owned_anthropic_terminal_replay(
                                finish["choices"][0]["delta"].as_object_mut().expect("delta object"),
                                replay.take_message(),
                                provider_namespace,
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
                            let mut finish = openai_chunk(
                                &stream_id,
                                created,
                                &model,
                                None,
                                None,
                                Some("stop"),
                            );
                            attach_owned_anthropic_terminal_replay(
                                finish["choices"][0]["delta"].as_object_mut().expect("delta object"),
                                replay.take_message(),
                                provider_namespace,
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
                if matches!(kind, "content_block_start" | "content_block_delta" | "content_block_stop") {
                    let mut chunk = content_chunk.unwrap_or_else(|| openai_delta_chunk(
                        &stream_id, created, &model, json!({}), None,
                    ));
                    chunk["choices"][0]["delta"]["provider_metadata"][provider_namespace]["native_event"] = payload;
                    yield Ok(openai_sse_chunk(&chunk));
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

        if !stream_failed && !incomplete_terminal && let Some(error) = replay_error {
            yield Ok(openai_sse_error_chunk("anthropic_replay_error", error.as_str()));
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
    let native_usage = object.get("provider_usage").and_then(Value::as_object);
    let total_tokens = if native_usage.is_some_and(|usage| {
        usage.contains_key("input_tokens") || usage.contains_key("output_tokens")
    }) {
        native_usage.and_then(anthropic_usage_total)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn openrouter_stream_totals_include_cached_tokens_across_usage_events() {
        let events = [
            json!({"type":"message_start","message":{"usage":{
                "input_tokens":3,"output_tokens":0,
                "cache_read_input_tokens":100,"cache_creation_input_tokens":7
            }}}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"output_tokens":2}}),
            json!({"type":"message_stop"}),
        ];
        let upstream = futures_util::stream::iter(events.map(|event| {
            Ok(Bytes::from(format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )))
        }));
        let chunks = normalize_anthropic_stream(
            upstream,
            "review-stream".into(),
            0,
            "claude".into(),
            "anthropic_compat",
            "openrouter",
        )
        .collect::<Vec<_>>()
        .await;
        let mut parser = SseEventParser::default();
        let mut final_usage = None;
        for chunk in chunks {
            for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
                if let Ok(value) = serde_json::from_str::<Value>(&event.data)
                    && let Some(usage) = value.get("usage")
                {
                    final_usage = Some(usage.clone());
                }
            }
        }
        let usage = final_usage.unwrap();
        assert_eq!(usage["total_tokens"], 112);
        assert_eq!(usage["provider_usage"]["input_tokens"], 3);
        assert_eq!(usage["provider_usage"]["cache_read_input_tokens"], 100);
        assert_eq!(usage["provider_usage"]["cache_creation_input_tokens"], 7);
        assert_eq!(usage["completion_tokens"], 2);
    }
}
