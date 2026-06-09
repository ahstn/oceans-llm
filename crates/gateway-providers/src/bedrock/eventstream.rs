use super::*;

#[derive(Debug, Clone)]
pub(super) struct BedrockEvent {
    pub(super) message_type: Option<String>,
    pub(super) event_type: Option<String>,
    pub(super) exception_type: Option<String>,
    pub(super) payload: Bytes,
}

#[derive(Debug, Default)]
pub(super) struct BedrockEventStreamDecoder {
    buffer: Vec<u8>,
}

impl BedrockEventStreamDecoder {
    const PRELUDE_LEN: usize = 12;
    const MESSAGE_CRC_LEN: usize = 4;
    const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

    pub(super) fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<BedrockEvent>, ProviderError> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            if self.buffer.len() < Self::PRELUDE_LEN {
                break;
            }

            let total_len = u32::from_be_bytes(self.buffer[0..4].try_into().unwrap()) as usize;
            let headers_len = u32::from_be_bytes(self.buffer[4..8].try_into().unwrap()) as usize;

            if total_len < Self::PRELUDE_LEN + Self::MESSAGE_CRC_LEN {
                return Err(ProviderError::Transport(format!(
                    "invalid aws_bedrock EventStream frame length `{total_len}`"
                )));
            }
            if total_len > Self::MAX_FRAME_LEN {
                return Err(ProviderError::Transport(format!(
                    "aws_bedrock EventStream frame length `{total_len}` exceeds limit"
                )));
            }
            let payload_start = Self::PRELUDE_LEN.checked_add(headers_len).ok_or_else(|| {
                ProviderError::Transport(
                    "aws_bedrock EventStream headers length overflow".to_string(),
                )
            })?;
            let payload_end = total_len
                .checked_sub(Self::MESSAGE_CRC_LEN)
                .ok_or_else(|| {
                    ProviderError::Transport(
                        "aws_bedrock EventStream payload length underflow".to_string(),
                    )
                })?;
            if payload_start > payload_end {
                return Err(ProviderError::Transport(format!(
                    "aws_bedrock EventStream headers length `{headers_len}` exceeds frame payload"
                )));
            }
            if self.buffer.len() < total_len {
                break;
            }

            let frame = self.buffer.drain(..total_len).collect::<Vec<_>>();
            let headers = parse_eventstream_headers(&frame[Self::PRELUDE_LEN..payload_start])?;
            let payload = Bytes::copy_from_slice(&frame[payload_start..payload_end]);
            events.push(BedrockEvent {
                message_type: headers.get(":message-type").cloned(),
                event_type: headers.get(":event-type").cloned(),
                exception_type: headers.get(":exception-type").cloned(),
                payload,
            });
        }

        Ok(events)
    }

    pub(super) fn finish(&self) -> Result<(), ProviderError> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            Err(ProviderError::Transport(format!(
                "stream ended with incomplete aws_bedrock EventStream frame ({} bytes buffered)",
                self.buffer.len()
            )))
        }
    }
}

pub(super) fn parse_eventstream_headers(
    headers: &[u8],
) -> Result<BTreeMap<String, String>, ProviderError> {
    let mut cursor = headers;
    let mut parsed = BTreeMap::new();

    while cursor.has_remaining() {
        if cursor.remaining() < 1 {
            return Err(ProviderError::Transport(
                "malformed aws_bedrock EventStream header name length".to_string(),
            ));
        }
        let name_len = cursor.get_u8() as usize;
        if cursor.remaining() < name_len + 1 {
            return Err(ProviderError::Transport(
                "malformed aws_bedrock EventStream header".to_string(),
            ));
        }
        let name = std::str::from_utf8(&cursor[..name_len]).map_err(|error| {
            ProviderError::Transport(format!(
                "aws_bedrock EventStream header name was not utf8: {error}"
            ))
        })?;
        cursor.advance(name_len);
        let value_type = cursor.get_u8();

        let value = match value_type {
            7 => {
                if cursor.remaining() < 2 {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream string header length".to_string(),
                    ));
                }
                let value_len = cursor.get_u16() as usize;
                if cursor.remaining() < value_len {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream string header value".to_string(),
                    ));
                }
                let value = std::str::from_utf8(&cursor[..value_len]).map_err(|error| {
                    ProviderError::Transport(format!(
                        "aws_bedrock EventStream string header was not utf8: {error}"
                    ))
                })?;
                cursor.advance(value_len);
                value.to_string()
            }
            0 => "true".to_string(),
            1 => "false".to_string(),
            other => {
                return Err(ProviderError::Transport(format!(
                    "unsupported aws_bedrock EventStream header value type `{other}`"
                )));
            }
        };
        parsed.insert(name.to_string(), value);
    }

    Ok(parsed)
}

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
                if let Some(tool_use) = payload
                    .get("start")
                    .and_then(|start| start.get("toolUse"))
                    .and_then(Value::as_object)
                {
                    let index = payload
                        .get("contentBlockIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
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
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "content": text
                        }),
                        Value::Null,
                    )));
                } else if let Some(tool_use) = delta.get("toolUse").and_then(Value::as_object) {
                    let index = payload
                        .get("contentBlockIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
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
                let finish_reason = payload
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .map(map_stop_reason)
                    .unwrap_or("stop");
                actions.push(BedrockStreamAction::Chunk(
                    self.delta_chunk(json!({}), Value::String(finish_reason.to_string())),
                ));
                self.saw_terminal = true;
            }
            "metadata" => {
                if let Some(usage) = map_stream_usage(&payload) {
                    actions.push(BedrockStreamAction::Chunk(self.usage_chunk(usage)));
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

    pub(super) fn delta_chunk(&self, delta: Value, finish_reason: Value) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "provider_model": self.provider_model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }]
        })
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

pub(super) fn map_stream_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let prompt = usage
        .get("inputTokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("outputTokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = usage
        .get("totalTokens")
        .or_else(|| usage.get("total_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(prompt + completion);

    let mut mapped = Map::new();
    mapped.insert("prompt_tokens".to_string(), Value::Number(prompt.into()));
    mapped.insert(
        "completion_tokens".to_string(),
        Value::Number(completion.into()),
    );
    mapped.insert("total_tokens".to_string(), Value::Number(total.into()));
    mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    Some(Value::Object(mapped))
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
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let events = match decoder.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "bedrock_eventstream_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
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

        if !stream_failed && !normalizer.saw_terminal {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_missing_terminal_event",
                "upstream Bedrock ConverseStream ended without messageStop",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}
