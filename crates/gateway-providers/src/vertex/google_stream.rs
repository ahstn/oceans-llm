use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderStream, ServiceTier, SseEventParser};
use serde_json::{Map, Value, json};

use super::{
    error::VertexAdapterError,
    google_replay::{CandidateParts, THOUGHT_SIGNATURE_FIELD, native_parts_metadata},
    google_response::{
        candidate_finish_reason, google_served_tier, inline_error, map_google_logprobs,
        map_google_usage, prompt_block_reason,
    },
};
use crate::{
    anthropic::streaming::{
        merge_openai_stream_usage, openai_chunk, openai_delta_chunk, openai_sse_chunk,
    },
    service_tier::report_served_tier,
    streaming::done_sse_chunk,
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
    served_tier: Option<ServiceTier>,
    native_parts: Vec<Value>,
    tool_call_ids: Vec<Value>,
    thought_signature: Option<Value>,
    native_finish_reason: Option<String>,
    prompt_block_reason: Option<String>,
    terminal_error: Option<VertexAdapterError>,
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
            served_tier: None,
            native_parts: Vec::new(),
            tool_call_ids: Vec::new(),
            thought_signature: None,
            native_finish_reason: None,
            prompt_block_reason: None,
            terminal_error: None,
        }
    }

    /// Converts one streamed `GenerateContentResponse` into zero or more chunks.
    pub(super) fn on_response(&mut self, object: &Value) -> Vec<Value> {
        if let Some(usage) = map_google_usage(object) {
            merge_openai_stream_usage(&mut self.latest_usage, &usage);
        }
        self.served_tier = google_served_tier(object).or(self.served_tier);
        // A failed generation can be followed by a usage-only frame. Keep consuming accounting
        // information, but never emit subsequent content or reinterpret the failure as success.
        if self.terminal_error.is_some() {
            return Vec::new();
        }
        if let Some(error) = inline_error(object) {
            self.terminal_error = Some(error);
            return Vec::new();
        }

        let Some(candidate) = object
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            if let Some(reason) = prompt_block_reason(object) {
                self.finish_reason = Some("content_filter");
                self.prompt_block_reason = Some(reason.to_string());
            }
            return Vec::new();
        };
        let terminal = match candidate_finish_reason(candidate) {
            Ok(terminal) => terminal,
            Err(error) => {
                self.native_finish_reason = candidate
                    .get("finishReason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.terminal_error = Some(error);
                return Vec::new();
            }
        };

        let parts = CandidateParts::from_candidate(candidate);
        self.native_parts.extend(parts.native_parts);
        let mut chunks = Vec::new();
        if !parts.reasoning.is_empty() {
            chunks.push(self.delta_chunk(json!({ "reasoning_content": parts.reasoning })));
        }
        if !parts.text.is_empty() {
            chunks.push(self.delta_chunk(json!({ "content": parts.text })));
        }
        if let Some(signature) = parts.thought_signature {
            self.thought_signature = Some(signature);
        }
        for tool_call in parts.tool_calls {
            self.tool_call_ids.push(tool_call["id"].clone());
            chunks.push(self.tool_call_chunk(tool_call));
        }

        if let Some(logprobs) = map_google_logprobs(candidate) {
            let mut chunk = self.delta_chunk(json!({}));
            chunk["choices"][0]["logprobs"] = logprobs;
            chunks.push(chunk);
        }
        if let Some(mapped) = terminal {
            // A later call must not erase an already observed truncation or filter terminal.
            if self.finish_reason.is_none_or(|reason| reason == "stop") {
                self.finish_reason = Some(mapped);
                self.native_finish_reason = candidate
                    .get("finishReason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }
        chunks
    }

    /// Terminal chunk carrying the resolved finish reason and the latest usage snapshot.
    ///
    /// Every Gemini stream ends with a candidate `finishReason` (or a prompt block with no
    /// candidates). A stream that closes without one was cut short, so the client must not
    /// treat the partial output as complete.
    pub(super) fn finish(&mut self) -> Result<Value, VertexAdapterError> {
        if let Some(error) = self.terminal_error.take() {
            return Err(error);
        }
        let finish_reason = self
            .finish_reason
            .ok_or(VertexAdapterError::StreamPrematureEof)?;
        let finish_reason = if finish_reason == "stop" && !self.tool_call_ids.is_empty() {
            "tool_calls"
        } else {
            finish_reason
        };
        let mut finish = openai_chunk(
            &self.stream_id,
            self.created,
            &self.model,
            None,
            None,
            Some(finish_reason),
        );
        finish["choices"][0]["delta"]["provider_metadata"] = self.replay_metadata();
        if let Some(usage) = &self.latest_usage {
            finish["usage"] = usage.clone();
            report_served_tier(&mut finish, self.served_tier);
        }
        Ok(finish)
    }

    fn replay_metadata(&self) -> Value {
        let mut metadata = native_parts_metadata(
            self.native_parts.clone(),
            self.tool_call_ids.clone(),
            self.thought_signature.clone(),
        );
        if let Some(reason) = &self.native_finish_reason {
            metadata["gcp_vertex"]["finish_reason"] = json!(reason);
        }
        if let Some(reason) = &self.prompt_block_reason {
            metadata["gcp_vertex"]["prompt_block_reason"] = json!(reason);
        }
        metadata
    }

    fn error_chunk(&self, code: &str, message: &str) -> Bytes {
        let mut error = json!({
            "error": { "message": message, "type": "upstream_error", "code": code }
        });
        if let Some(usage) = &self.latest_usage {
            error["usage"] = usage.clone();
            report_served_tier(&mut error, self.served_tier);
        }
        if !self.native_parts.is_empty() {
            let metadata = openai_delta_chunk(
                &self.stream_id,
                self.created,
                &self.model,
                json!({ "provider_metadata": self.replay_metadata() }),
                None,
            );
            return Bytes::from(format!("data: {metadata}\n\ndata: {error}\n\n"));
        }
        openai_sse_chunk(&error)
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
                    yield Ok(state.error_chunk("upstream_google_stream_error", &crate::http::stream_read_error_message(&error)));
                    return;
                }
            };
            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(state.error_chunk("google_stream_parse_error", &error.to_string()));
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
                        yield Ok(state.error_chunk("google_stream_parse_error", &error.to_string()));
                        return;
                    }
                };
                for chunk in state.on_response(&object) {
                    yield Ok(openai_sse_chunk(&chunk));
                }
            }
        }

        if let Err(error) = parser.finish() {
            yield Ok(state.error_chunk("google_stream_parse_error", &error.to_string()));
            return;
        }
        match state.finish() {
            Ok(chunk) => yield Ok(openai_sse_chunk(&chunk)),
            Err(error) => {
                let code = if matches!(&error, VertexAdapterError::StreamPrematureEof) {
                    "google_stream_premature_eof"
                } else {
                    "google_stream_error"
                };
                yield Ok(state.error_chunk(code, &error.to_string()));
                return;
            }
        }
        yield Ok(done_sse_chunk());
    })
}
