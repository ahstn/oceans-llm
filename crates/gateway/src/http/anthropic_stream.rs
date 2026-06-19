use std::collections::BTreeMap;

use async_stream::stream;
use axum::body::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    ProviderStream, SseEventParser, protocol::anthropic::openai_finish_reason_to_anthropic,
};
use serde_json::{Value, json};

#[derive(Debug, Default)]
struct AnthropicStreamState {
    message_started: bool,
    next_block_index: i64,
    text_block_index: Option<i64>,
    tool_block_indexes: BTreeMap<i64, i64>,
    latest_usage: Option<Value>,
    pending_stop_reason: Option<String>,
}

pub(super) fn anthropic_messages_stream_from_openai(
    upstream: ProviderStream,
    model: String,
) -> ProviderStream {
    Box::pin(stream! {
        let mut parser = SseEventParser::default();
        let mut state = AnthropicStreamState::default();
        let mut failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Err(error);
                    failed = true;
                    break;
                }
            };
            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Err(error);
                    failed = true;
                    break;
                }
            };

            for event in events {
                let payload = event.data.trim();
                if payload.is_empty() || payload == "[DONE]" {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(payload) else {
                    continue;
                };
                if let Some(outbound) = anthropic_error_event_from_openai_chunk(&value) {
                    yield Ok(outbound);
                    failed = true;
                    break;
                }
                for outbound in anthropic_events_from_openai_chunk(&value, &model, &mut state) {
                    yield Ok(outbound);
                }
            }
            if failed {
                break;
            }
        }

        if !failed {
            if let Err(error) = parser.finish() {
                yield Err(error);
            } else {
                for outbound in finish_anthropic_stream_blocks(&mut state) {
                    yield Ok(outbound);
                }
                if let Some(outbound) = pending_anthropic_message_delta(&mut state) {
                    yield Ok(outbound);
                }
                yield Ok(anthropic_sse_chunk("message_stop", json!({"type":"message_stop"})));
            }
        }
    })
}

fn anthropic_events_from_openai_chunk(
    value: &Value,
    model: &str,
    state: &mut AnthropicStreamState,
) -> Vec<Bytes> {
    let mut events = Vec::new();
    let usage = anthropic_stream_usage_from_openai(value.get("usage"));
    if let Some(usage) = usage.as_ref() {
        merge_anthropic_stream_usage(&mut state.latest_usage, usage);
    }
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    else {
        if state.message_started {
            append_anthropic_usage_delta(usage, state, &mut events);
        }
        return events;
    };

    if !state.message_started {
        let message = json!({
            "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_oceans_stream"),
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        events.push(anthropic_sse_chunk(
            "message_start",
            json!({"type":"message_start","message":message}),
        ));
        state.message_started = true;
    }
    let delta = choice.get("delta").and_then(Value::as_object);

    if let Some(content) = delta
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
    {
        let index = *state.text_block_index.get_or_insert_with(|| {
            let index = state.next_block_index;
            state.next_block_index += 1;
            events.push(anthropic_sse_chunk(
                "content_block_start",
                json!({
                    "type":"content_block_start",
                    "index": index,
                    "content_block": {"type":"text","text":""}
                }),
            ));
            index
        });
        events.push(anthropic_sse_chunk(
            "content_block_delta",
            json!({
                "type":"content_block_delta",
                "index": index,
                "delta": {"type":"text_delta","text":content}
            }),
        ));
    }

    if let Some(tool_calls) = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(Value::as_array)
    {
        for tool_call in tool_calls {
            append_anthropic_tool_delta(tool_call, state, &mut events);
        }
    }

    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        events.extend(finish_anthropic_stream_blocks(state));
        state.pending_stop_reason = Some(openai_finish_reason_to_anthropic(reason).to_string());
    } else {
        append_anthropic_usage_delta(usage, state, &mut events);
    }

    events
}

fn anthropic_error_event_from_openai_chunk(value: &Value) -> Option<Bytes> {
    let error = value.get("error")?.as_object()?;
    let error_type = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("api_error");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("stream failed");
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or(error_type);

    Some(anthropic_sse_chunk(
        "error",
        json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
                "code": code
            }
        }),
    ))
}

fn append_anthropic_usage_delta(
    usage: Option<Value>,
    state: &AnthropicStreamState,
    events: &mut Vec<Bytes>,
) {
    let Some(usage) = usage else {
        return;
    };
    if state.pending_stop_reason.is_some() {
        return;
    }
    events.push(anthropic_sse_chunk(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {},
            "usage": usage_with_known_fields(usage, state.latest_usage.as_ref())
        }),
    ));
}

fn anthropic_stream_usage_from_openai(value: Option<&Value>) -> Option<Value> {
    let usage = value?.as_object()?;
    let mut anthropic_usage = serde_json::Map::new();
    if let Some(input_tokens) = usage.get("prompt_tokens").and_then(Value::as_i64) {
        anthropic_usage.insert("input_tokens".to_string(), json!(input_tokens));
    }
    if let Some(output_tokens) = usage.get("completion_tokens").and_then(Value::as_i64) {
        anthropic_usage.insert("output_tokens".to_string(), json!(output_tokens));
    }
    if anthropic_usage.is_empty() {
        None
    } else {
        Some(Value::Object(anthropic_usage))
    }
}

fn merge_anthropic_stream_usage(latest: &mut Option<Value>, usage: &Value) {
    let usage = usage_with_known_fields(usage.clone(), latest.as_ref());
    *latest = Some(usage);
}

fn usage_with_known_fields(usage: Value, latest: Option<&Value>) -> Value {
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            latest
                .and_then(|latest| latest.get("input_tokens"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            latest
                .and_then(|latest| latest.get("output_tokens"))
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);

    let mut object = usage.as_object().cloned().unwrap_or_default();
    object.insert("input_tokens".to_string(), json!(input_tokens));
    object.insert("output_tokens".to_string(), json!(output_tokens));
    Value::Object(object)
}

fn fallback_anthropic_stream_usage() -> Value {
    json!({
        "input_tokens": 0,
        "output_tokens": 0
    })
}

fn pending_anthropic_message_delta(state: &mut AnthropicStreamState) -> Option<Bytes> {
    let stop_reason = state.pending_stop_reason.take()?;
    Some(anthropic_sse_chunk(
        "message_delta",
        json!({
            "type":"message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            },
            "usage": state.latest_usage.clone().unwrap_or_else(fallback_anthropic_stream_usage)
        }),
    ))
}

fn append_anthropic_tool_delta(
    tool_call: &Value,
    state: &mut AnthropicStreamState,
    events: &mut Vec<Bytes>,
) {
    let openai_index = tool_call.get("index").and_then(Value::as_i64).unwrap_or(0);
    if !state.tool_block_indexes.contains_key(&openai_index) {
        stop_open_text_block(state, events);
        let index = state.next_block_index;
        state.next_block_index += 1;
        state.tool_block_indexes.insert(openai_index, index);
        events.push(anthropic_sse_chunk(
            "content_block_start",
            json!({
                "type":"content_block_start",
                "index": index,
                "content_block": {
                    "type":"tool_use",
                    "id": tool_call.get("id").and_then(Value::as_str).unwrap_or("toolu_oceans"),
                    "name": tool_call
                        .get("function")
                        .and_then(Value::as_object)
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool"),
                    "input": {}
                }
            }),
        ));
    }
    let Some(index) = state.tool_block_indexes.get(&openai_index).copied() else {
        return;
    };
    if let Some(arguments) = tool_call
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("arguments"))
        .and_then(Value::as_str)
        .filter(|arguments| !arguments.is_empty())
    {
        events.push(anthropic_sse_chunk(
            "content_block_delta",
            json!({
                "type":"content_block_delta",
                "index": index,
                "delta": {"type":"input_json_delta","partial_json":arguments}
            }),
        ));
    }
}

fn stop_open_text_block(state: &mut AnthropicStreamState, events: &mut Vec<Bytes>) {
    if let Some(index) = state.text_block_index.take() {
        events.push(content_block_stop(index));
    }
}

fn finish_anthropic_stream_blocks(state: &mut AnthropicStreamState) -> Vec<Bytes> {
    let mut indexes = Vec::new();
    if let Some(index) = state.text_block_index.take() {
        indexes.push(index);
    }
    indexes.extend(std::mem::take(&mut state.tool_block_indexes).into_values());
    indexes.sort_unstable();

    indexes.into_iter().map(content_block_stop).collect()
}

fn content_block_stop(index: i64) -> Bytes {
    anthropic_sse_chunk(
        "content_block_stop",
        json!({"type":"content_block_stop","index":index}),
    )
}

fn anthropic_sse_chunk(event: &str, value: Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {value}\n\n"))
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use futures_util::{StreamExt, stream};
    use gateway_core::ProviderStream;
    use gateway_service::StreamResponseCollector;
    use serde_json::json;

    use super::anthropic_messages_stream_from_openai;

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_emits_messages_sse() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"toolu_1\",\"type\":\"function\",\"function\":{\"name\":\"noop\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        )))]));
        let rendered = render_stream(upstream).await;

        assert!(rendered.contains("event: message_start"));
        assert!(rendered.contains("\"text\":\"hi\""));
        assert!(rendered.contains("\"type\":\"text_delta\""));
        assert!(rendered.contains("\"type\":\"tool_use\""));
        assert!(rendered.contains("\"partial_json\":\"{}\""));
        assert!(rendered.contains("\"stop_reason\":\"tool_use\""));
        assert!(rendered.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_stops_text_before_tool_block() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"toolu_1\",\"type\":\"function\",\"function\":{\"name\":\"noop\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"
        )))]));
        let rendered = render_stream(upstream).await;

        let text_stop = rendered
            .find("\"index\":0,\"type\":\"content_block_stop\"")
            .expect("text stop");
        let tool_start = rendered
            .find("\"index\":1,\"type\":\"content_block_start\"")
            .expect("tool start");

        assert!(text_stop < tool_start);
    }

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_finishes_blocks_in_index_order() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"toolu_1\",\"type\":\"function\",\"function\":{\"name\":\"first\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"after\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"
        )))]));
        let rendered = render_stream(upstream).await;

        let tool_stop = rendered
            .find("\"index\":0,\"type\":\"content_block_stop\"")
            .expect("tool stop");
        let text_stop = rendered
            .find("\"index\":1,\"type\":\"content_block_stop\"")
            .expect("text stop");

        assert!(tool_stop < text_stop);
    }

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_emits_error_without_success_stop() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(
            "data: {\"error\":{\"message\":\"upstream failed\",\"type\":\"upstream_error\",\"code\":\"upstream_bad\"}}\n\n",
        ))]));

        let rendered = render_stream(upstream).await;

        assert!(rendered.contains("event: error"));
        assert!(rendered.contains("\"message\":\"upstream failed\""));
        assert!(rendered.contains("\"code\":\"upstream_bad\""));
        assert!(!rendered.contains("event: message_start"));
        assert!(!rendered.contains("event: message_stop"));
    }

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_preserves_usage_for_collector() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4,\"total_tokens\":7}}\n\n",
            "data: [DONE]\n\n"
        )))]));

        let rendered = render_stream(upstream).await;
        let mut collector = StreamResponseCollector::default();
        collector.observe_chunk(rendered.as_bytes());

        assert!(rendered.contains("event: message_delta"));
        assert!(rendered.contains("\"input_tokens\":3"));
        assert!(rendered.contains("\"output_tokens\":4"));
        assert_eq!(
            collector.usage(),
            Some(&json!({"input_tokens": 3, "output_tokens": 4}))
        );
    }

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_final_delta_has_fallback_usage() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        )))]));

        let rendered = render_stream(upstream).await;

        assert!(rendered.contains("event: message_delta"));
        assert!(rendered.contains("\"stop_reason\":\"end_turn\""));
        assert!(rendered.contains("\"usage\":{\"input_tokens\":0,\"output_tokens\":0}"));
    }

    #[tokio::test]
    async fn anthropic_messages_stream_adapter_merges_partial_usage_for_final_delta() {
        let upstream: ProviderStream = Box::pin(stream::iter(vec![Ok(Bytes::from(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":0,\"total_tokens\":11}}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )))]));

        let rendered = render_stream(upstream).await;

        assert!(rendered.contains("\"usage\":{\"input_tokens\":11,\"output_tokens\":3}"));
    }

    async fn render_stream(upstream: ProviderStream) -> String {
        anthropic_messages_stream_from_openai(upstream, "claude".to_string())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
            .collect::<String>()
    }
}
