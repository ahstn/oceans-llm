use std::collections::BTreeMap;

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{ProviderError, ProviderRequestContext, ProviderStream};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::converse_replay::{ConverseReplayStream, normalize_bedrock_reasoning_content};
use super::eventstream::{BedrockEvent, BedrockEventStreamDecoder};
use super::response::{bedrock_reasoning_metadata, map_stop_reason, map_usage};
use super::service_tier::converse_served_tier;
use crate::service_tier::report_served_tier;
use crate::streaming::{done_sse_chunk, openai_sse_error_chunk, render_sse_event_chunk};

#[derive(Debug)]
pub(super) enum BedrockStreamAction {
    Chunk(Value),
    Error { code: String, message: String },
}

#[derive(Debug)]
pub(super) struct BedrockConverseStreamNormalizer {
    id: String,
    created: i64,
    model: String,
    provider_model: String,
    role_sent: bool,
    saw_payload: bool,
    saw_terminal: bool,
    tool_block_indexes: BTreeMap<u64, usize>,
    replay: ConverseReplayStream,
    generation_failure: Option<(String, String)>,
}

impl BedrockConverseStreamNormalizer {
    pub(super) fn new(context: &ProviderRequestContext) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4().simple()),
            created: OffsetDateTime::now_utc().unix_timestamp(),
            model: context.model_key.clone(),
            provider_model: context.upstream_model.clone(),
            role_sent: false,
            saw_payload: false,
            saw_terminal: false,
            tool_block_indexes: BTreeMap::new(),
            replay: ConverseReplayStream::default(),
            generation_failure: None,
        }
    }

    pub(super) fn process_event(
        &mut self,
        event: BedrockEvent,
    ) -> Result<Vec<BedrockStreamAction>, ProviderError> {
        if event.message_type.as_deref() == Some("exception") || event.exception_type.is_some() {
            let code = event
                .exception_type
                .or(event.event_type)
                .unwrap_or_else(|| "bedrock_eventstream_exception".to_string());
            let message = bedrock_event_payload_message(&event.payload)
                .unwrap_or_else(|| "aws_bedrock EventStream exception".to_string());
            return Ok(vec![BedrockStreamAction::Error { code, message }]);
        }

        let event_type = event.event_type.as_deref().ok_or_else(|| {
            ProviderError::Transport(
                "aws_bedrock EventStream event is missing :event-type".to_string(),
            )
        })?;

        let payload: Value = serde_json::from_slice(&event.payload).map_err(|error| {
            ProviderError::Transport(format!(
                "invalid JSON payload in aws_bedrock `{event_type}` EventStream event: {error}"
            ))
        })?;

        self.saw_payload = true;
        if self.generation_failure.is_some() && !matches!(event_type, "messageStop" | "metadata") {
            return Ok(Vec::new());
        }
        let mut actions = Vec::new();
        match event_type {
            "messageStart" => {
                if let Some(conversation_id) = payload.get("conversationId").and_then(Value::as_str)
                {
                    self.id = format!("chatcmpl-{conversation_id}");
                }
                if !self.role_sent {
                    let role = payload
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("assistant");
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "role": role
                        }),
                        Value::Null,
                    )));
                    self.role_sent = true;
                }
            }
            "contentBlockStart" => {
                let block_index = payload
                    .get("contentBlockIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if let Err(error) = self.replay.record_start(block_index, payload.get("start")) {
                    self.fail_replay(error.to_string());
                    return Ok(actions);
                }
                if let Some(tool_use) = payload
                    .get("start")
                    .and_then(|start| start.get("toolUse"))
                    .and_then(Value::as_object)
                {
                    let block_index = payload
                        .get("contentBlockIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let next_index = self.tool_block_indexes.len();
                    let index = *self
                        .tool_block_indexes
                        .entry(block_index)
                        .or_insert(next_index);
                    self.replay.start_tool_input(block_index);
                    let id = tool_use
                        .get("toolUseId")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let name = tool_use
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "tool_calls": [{
                                "index": index,
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": ""
                                }
                            }]
                        }),
                        Value::Null,
                    )));
                }
            }
            "contentBlockDelta" => {
                let delta = payload.get("delta").ok_or_else(|| {
                    ProviderError::Transport(
                        "aws_bedrock contentBlockDelta event is missing `delta`".to_string(),
                    )
                })?;
                if let Err(error) = self.replay.record_delta(&payload, delta) {
                    self.fail_replay(error.to_string());
                    return Ok(actions);
                }
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "content": text
                        }),
                        Value::Null,
                    )));
                } else if let Some(tool_use) = delta.get("toolUse").and_then(Value::as_object) {
                    let block_index = payload
                        .get("contentBlockIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let index = self.tool_block_indexes.get(&block_index).copied().ok_or_else(|| {
                        ProviderError::Transport(format!(
                            "aws_bedrock tool delta references unstarted content block `{block_index}`"
                        ))
                    })?;
                    let input = tool_use
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "tool_calls": [{
                                "index": index,
                                "function": {
                                    "arguments": input
                                }
                            }]
                        }),
                        Value::Null,
                    )));
                } else if let Some(reasoning) = delta.get("reasoningContent")
                    && let Some(block) = normalize_bedrock_reasoning_content(reasoning)
                {
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "provider_metadata": bedrock_reasoning_metadata(
                                "bedrock_converse_stream",
                                vec![block],
                            )
                        }),
                        Value::Null,
                    )));
                }
            }
            "contentBlockStop" => {}
            "messageStop" => {
                let stop_reason = payload.get("stopReason").and_then(Value::as_str);
                let malformed = matches!(
                    stop_reason,
                    Some("malformed_tool_use" | "malformed_model_output")
                );
                if malformed && self.generation_failure.is_none() {
                    self.generation_failure = stop_reason.map(|reason| {
                        (
                            reason.to_string(),
                            format!("aws_bedrock generation failed: {reason}"),
                        )
                    });
                }
                let finish_reason = payload
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .map(map_stop_reason)
                    .unwrap_or("stop");
                let mut delta = json!({
                    "provider_metadata": {"aws_bedrock": {
                        "stop_reason": payload.get("stopReason").cloned().unwrap_or(Value::Null)
                    }}
                });
                if let Some(fields) = payload.get("additionalModelResponseFields") {
                    delta["provider_metadata"]["aws_bedrock"]["additionalModelResponseFields"] =
                        fields.clone();
                }
                if let Some(message) = self.replay.attach_terminal(
                    &mut delta,
                    finish_reason,
                    self.generation_failure.is_none(),
                ) {
                    self.fail_replay(message);
                }
                actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                    delta,
                    if self.generation_failure.is_some() {
                        Value::Null
                    } else {
                        Value::String(finish_reason.to_string())
                    },
                )));
                self.saw_terminal = true;
            }
            "metadata" => {
                if let Some(usage) = map_usage(&payload) {
                    let mut chunk = self.usage_chunk(usage);
                    report_served_tier(&mut chunk, converse_served_tier(&payload));
                    actions.push(BedrockStreamAction::Chunk(chunk));
                }
            }
            other => {
                return Err(ProviderError::Transport(format!(
                    "unsupported aws_bedrock ConverseStream event `{other}`"
                )));
            }
        }

        Ok(actions)
    }

    fn fail_replay(&mut self, message: String) {
        self.generation_failure
            .get_or_insert_with(|| ("bedrock_conversestream_replay_error".to_string(), message));
    }

    pub(super) fn delta_chunk(&self, delta: Value, finish_reason: Value) -> Value {
        let mut chunk = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "provider_model": self.provider_model,
            "choices": [{
                "index": 0,
                "delta": null,
                "finish_reason": finish_reason
            }]
        });
        chunk["choices"][0]["delta"] = delta;
        chunk
    }

    pub(super) fn usage_chunk(&self, usage: Value) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "provider_model": self.provider_model,
            "choices": [],
            "usage": usage
        })
    }
}

pub(super) fn bedrock_event_payload_message(payload: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.get("Message"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| value.as_str().map(str::to_string))
        })
}

pub(super) fn normalize_bedrock_converse_stream<S>(
    upstream: S,
    context: ProviderRequestContext,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut decoder = BedrockEventStreamDecoder::default();
        let mut normalizer = BedrockConverseStreamNormalizer::new(&context);
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_bedrock_eventstream_error",
                        &crate::http::stream_read_error_message(&error),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let batch = match decoder.push_bytes(&chunk) {
                Ok(batch) => batch,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "bedrock_eventstream_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in batch.events {
                let actions = match normalizer.process_event(event) {
                    Ok(actions) => actions,
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk(
                            "bedrock_conversestream_normalization_error",
                            &error.to_string(),
                        ));
                        stream_failed = true;
                        break;
                    }
                };

                for action in actions {
                    match action {
                        BedrockStreamAction::Chunk(value) => {
                            yield Ok(render_sse_event_chunk(None, &value.to_string()));
                        }
                        BedrockStreamAction::Error { code, message } => {
                            yield Ok(openai_sse_error_chunk(&code, &message));
                            stream_failed = true;
                            break;
                        }
                    }
                }

                if stream_failed {
                    break;
                }
            }

            if !stream_failed && let Some(error) = batch.error {
                yield Ok(openai_sse_error_chunk(
                    "bedrock_eventstream_parse_error",
                    &error.to_string(),
                ));
                stream_failed = true;
            }

            if stream_failed {
                break;
            }
        }

        if !stream_failed && let Err(error) = decoder.finish() {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !normalizer.saw_payload {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_empty_stream",
                "upstream stream ended without Bedrock EventStream payload events",
            ));
            stream_failed = true;
        }

        if !stream_failed
            && !normalizer.saw_terminal
            && normalizer.generation_failure.is_none()
        {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_missing_terminal_event",
                "upstream Bedrock ConverseStream ended without messageStop",
            ));
            stream_failed = true;
        }

        if !stream_failed && let Some((code, message)) = normalizer.generation_failure {
            yield Ok(openai_sse_error_chunk(&code, &message));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}
