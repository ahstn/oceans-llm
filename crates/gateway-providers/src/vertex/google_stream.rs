use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderStream, SseEventParser};
use serde_json::{Map, Value, json};

use super::{
    error::VertexAdapterError,
    google_response::{
        CandidateParts, check_malformed_function_call, inline_error, map_google_finish_reason,
        map_google_usage, prompt_block_reason, thought_signature_metadata,
    },
    google_tools::THOUGHT_SIGNATURE_FIELD,
};
use crate::{
    anthropic::streaming::{
        merge_openai_stream_usage, openai_chunk, openai_delta_chunk, openai_sse_chunk,
    },
    streaming::{done_sse_chunk, openai_sse_error_chunk},
};

/// Folds `streamGenerateContent?alt=sse` events into OpenAI chat-completion chunks.
///
/// The terminal `finish_reason` chunk is emitted once at end of stream so late `usageMetadata`
/// and any text that trails the `finishReason` object are never dropped.
pub(super) struct GoogleStreamState {
    stream_id: String,
    created: i64,
    model: String,
    role_emitted: bool,
    next_tool_call_index: usize,
    finish_reason: Option<&'static str>,
    latest_usage: Option<Value>,
}

impl GoogleStreamState {
    pub(super) fn new(stream_id: String, created: i64, model: String) -> Self {
        Self {
            stream_id,
            created,
            model,
            role_emitted: false,
            next_tool_call_index: 0,
            finish_reason: None,
            latest_usage: None,
        }
    }

    /// Converts one streamed `GenerateContentResponse` into zero or more chunks.
    pub(super) fn on_response(&mut self, object: &Value) -> Result<Vec<Value>, VertexAdapterError> {
        if let Some(error) = inline_error(object) {
            return Err(error);
        }
        if let Some(usage) = map_google_usage(object) {
            merge_openai_stream_usage(&mut self.latest_usage, &usage);
        }

        let Some(candidate) = object
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            if prompt_block_reason(object).is_some() {
                self.finish_reason = Some("content_filter");
            }
            return Ok(Vec::new());
        };
        check_malformed_function_call(candidate)?;

        let parts = CandidateParts::from_candidate(candidate);
        let mut chunks = Vec::new();
        if !parts.reasoning.is_empty() {
            chunks.push(self.delta_chunk(json!({ "reasoning_content": parts.reasoning })));
        }
        if !parts.text.is_empty() {
            chunks.push(self.delta_chunk(json!({ "content": parts.text })));
        }
        if let Some(signature) = parts.thought_signature {
            chunks.push(self.delta_chunk(
                json!({ "provider_metadata": thought_signature_metadata(signature) }),
            ));
        }
        for tool_call in parts.tool_calls {
            chunks.push(self.tool_call_chunk(tool_call));
            self.finish_reason = Some("tool_calls");
        }

        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str)
            && self.finish_reason != Some("tool_calls")
        {
            self.finish_reason = Some(map_google_finish_reason(reason));
        }
        Ok(chunks)
    }

    /// Terminal chunk carrying the resolved finish reason and the latest usage snapshot.
    pub(super) fn finish(self) -> Value {
        let mut finish = openai_chunk(
            &self.stream_id,
            self.created,
            &self.model,
            None,
            None,
            Some(self.finish_reason.unwrap_or("stop")),
        );
        if let Some(usage) = self.latest_usage {
            finish["usage"] = usage;
        }
        finish
    }

    fn delta_chunk(&mut self, mut delta: Value) -> Value {
        if !self.role_emitted {
            delta["role"] = Value::String("assistant".to_string());
            self.role_emitted = true;
        }
        openai_delta_chunk(&self.stream_id, self.created, &self.model, delta, None)
    }

    fn tool_call_chunk(&mut self, mut tool_call: Value) -> Value {
        let mut call_delta = Map::new();
        call_delta.insert("index".to_string(), json!(self.next_tool_call_index));
        self.next_tool_call_index += 1;
        if let Some(call) = tool_call.as_object_mut() {
            for key in ["id", "type", "function", THOUGHT_SIGNATURE_FIELD] {
                if let Some(value) = call.remove(key) {
                    call_delta.insert(key.to_string(), value);
                }
            }
        }
        self.delta_chunk(json!({ "tool_calls": [Value::Object(call_delta)] }))
    }
}

pub(super) fn normalize_google_stream<S>(
    upstream: S,
    stream_id: String,
    created: i64,
    model: String,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut parser = SseEventParser::default();
        let mut state = GoogleStreamState::new(stream_id, created, model);
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_google_stream_error", &error.to_string()));
                    return;
                }
            };
            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("google_stream_parse_error", &error.to_string()));
                    return;
                }
            };
            for event in events {
                if event.data.trim().is_empty() {
                    continue;
                }
                let object: Value = match serde_json::from_str(&event.data) {
                    Ok(object) => object,
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk("google_stream_parse_error", &error.to_string()));
                        return;
                    }
                };
                match state.on_response(&object) {
                    Ok(chunks) => {
                        for chunk in chunks {
                            yield Ok(openai_sse_chunk(&chunk));
                        }
                    }
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk("google_stream_error", &error.to_string()));
                        return;
                    }
                }
            }
        }

        yield Ok(openai_sse_chunk(&state.finish()));
        yield Ok(done_sse_chunk());
    })
}
