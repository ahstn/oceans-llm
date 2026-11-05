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
    pending_error: Option<ProviderError>,
    failed: bool,
}

impl BedrockEventStreamDecoder {
    const PRELUDE_LEN: usize = 12;
    const MESSAGE_CRC_LEN: usize = 4;
    const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

    pub(super) fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<BedrockEvent>, ProviderError> {
        if self.failed {
            return Err(self.take_error().unwrap_or_else(|| {
                ProviderError::Transport(
                    "aws_bedrock EventStream decoder already failed".to_string(),
                )
            }));
        }
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            match self.decode_frame() {
                Ok(Some(event)) => events.push(event),
                Ok(None) => break,
                Err(error) => {
                    self.failed = true;
                    if events.is_empty() {
                        return Err(error);
                    }
                    self.pending_error = Some(error);
                    break;
                }
            }
        }
        Ok(events)
    }

    fn decode_frame(&mut self) -> Result<Option<BedrockEvent>, ProviderError> {
        if self.buffer.len() < Self::PRELUDE_LEN {
            return Ok(None);
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
            ProviderError::Transport("aws_bedrock EventStream headers length overflow".to_string())
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
        let prelude_crc = u32::from_be_bytes(self.buffer[8..12].try_into().unwrap());
        if crc32fast::hash(&self.buffer[..8]) != prelude_crc {
            return Err(ProviderError::Transport(
                "aws_bedrock EventStream prelude checksum mismatch".to_string(),
            ));
        }
        if self.buffer.len() < total_len {
            return Ok(None);
        }
        let message_crc =
            u32::from_be_bytes(self.buffer[payload_end..total_len].try_into().unwrap());
        if crc32fast::hash(&self.buffer[..payload_end]) != message_crc {
            return Err(ProviderError::Transport(
                "aws_bedrock EventStream message checksum mismatch".to_string(),
            ));
        }
        let frame = self.buffer.drain(..total_len).collect::<Vec<_>>();
        let headers = parse_eventstream_headers(&frame[Self::PRELUDE_LEN..payload_start])?;
        let payload = Bytes::copy_from_slice(&frame[payload_start..payload_end]);
        Ok(Some(BedrockEvent {
            message_type: headers.get(":message-type").cloned(),
            event_type: headers.get(":event-type").cloned(),
            exception_type: headers.get(":exception-type").cloned(),
            payload,
        }))
    }

    pub(super) fn take_error(&mut self) -> Option<ProviderError> {
        self.pending_error.take()
    }

    pub(super) fn finish(&mut self) -> Result<(), ProviderError> {
        if let Some(error) = self.take_error() {
            Err(error)
        } else if self.failed {
            Err(ProviderError::Transport(
                "aws_bedrock EventStream decoder already failed".to_string(),
            ))
        } else if self.buffer.is_empty() {
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
            2..=6 | 8 | 9 => {
                let value_len = match value_type {
                    2 => 1,
                    3 => 2,
                    4 => 4,
                    5 | 8 => 8,
                    9 => 16,
                    6 => {
                        if cursor.remaining() < 2 {
                            return Err(ProviderError::Transport(
                                "malformed aws_bedrock EventStream byte header length".to_string(),
                            ));
                        }
                        cursor.get_u16() as usize
                    }
                    _ => unreachable!(),
                };
                if cursor.remaining() < value_len {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream typed header value".to_string(),
                    ));
                }
                cursor.advance(value_len);
                continue;
            }
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
    tool_block_indexes: BTreeMap<u64, usize>,
    native_blocks: BTreeMap<u64, Value>,
    tool_inputs: BTreeMap<u64, String>,
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
            native_blocks: BTreeMap::new(),
            tool_inputs: BTreeMap::new(),
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
        if self.generation_failure.is_some() && event_type != "metadata" {
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
                if let Some(start) = payload.get("start").and_then(Value::as_object)
                    && !start.is_empty()
                {
                    self.native_blocks
                        .insert(block_index, Value::Object(start.clone()));
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
                    self.native_blocks
                        .insert(block_index, json!({"toolUse": tool_use}));
                    self.tool_inputs.insert(block_index, String::new());
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
                self.record_delta(&payload, delta)?;
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
                if malformed {
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
                let mut replayable = !malformed;
                if replayable {
                    for (index, input) in &self.tool_inputs {
                        match serde_json::from_str::<Value>(input) {
                            Ok(input) => {
                                self.native_blocks.get_mut(index).expect("started tool")["toolUse"]
                                    ["input"] = input;
                            }
                            Err(error) => {
                                replayable = false;
                                if !matches!(finish_reason, "length" | "content_filter") {
                                    self.generation_failure = Some((
                                        "bedrock_conversestream_replay_error".to_string(),
                                        format!("invalid aws_bedrock streamed tool input: {error}"),
                                    ));
                                }
                                break;
                            }
                        }
                    }
                }
                if replayable {
                    delta["provider_metadata"]["aws_bedrock"]["native_content"] = json!({
                        "source": "bedrock_converse",
                        "blocks": self.native_blocks.values().collect::<Vec<_>>(),
                    });
                }
                delta["provider_metadata"]["aws_bedrock"]["reasoning"] = json!({
                    "source": "bedrock_converse_stream",
                    "blocks": self.native_blocks.values()
                        .filter_map(|block| block.get("reasoningContent"))
                        .filter_map(normalize_bedrock_reasoning_content).collect::<Vec<_>>(),
                });
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

    fn record_delta(&mut self, payload: &Value, delta: &Value) -> Result<(), ProviderError> {
        let index = payload
            .get("contentBlockIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if let Some(input) = delta
            .get("toolUse")
            .and_then(|tool| tool.get("input"))
            .and_then(Value::as_str)
        {
            self.tool_inputs
                .get_mut(&index)
                .ok_or_else(|| {
                    ProviderError::Transport(format!(
                        "aws_bedrock tool delta references unstarted content block `{index}`"
                    ))
                })?
                .push_str(input);
        } else if let Some(text) = delta.get("text").and_then(Value::as_str) {
            let block = self
                .native_blocks
                .entry(index)
                .or_insert_with(|| json!({"text": ""}));
            append_native_string(&mut block["text"], text)?;
        } else if let Some(reasoning) = delta.get("reasoningContent").and_then(Value::as_object) {
            let block = self
                .native_blocks
                .entry(index)
                .or_insert_with(|| json!({"reasoningContent": {}}));
            for (key, value) in reasoning {
                match key.as_str() {
                    "text" | "signature" => {
                        append_native_string(
                            &mut block["reasoningContent"]["reasoningText"][key],
                            value.as_str().ok_or_else(|| {
                                ProviderError::Transport(
                                    "aws_bedrock reasoning delta must be a string".to_string(),
                                )
                            })?,
                        )?;
                    }
                    "redactedContent" | "data" => {
                        let previous = block["reasoningContent"]["redactedContent"]
                            .as_str()
                            .unwrap_or_default();
                        let mut bytes = BASE64_STANDARD.decode(previous).map_err(|error| {
                            ProviderError::Transport(format!(
                                "invalid Bedrock redacted reasoning: {error}"
                            ))
                        })?;
                        bytes.extend(
                            BASE64_STANDARD
                                .decode(value.as_str().unwrap_or_default())
                                .map_err(|error| {
                                    ProviderError::Transport(format!(
                                        "invalid Bedrock redacted reasoning: {error}"
                                    ))
                                })?,
                        );
                        block["reasoningContent"]["redactedContent"] =
                            json!(BASE64_STANDARD.encode(bytes));
                    }
                    _ => {
                        block["reasoningContent"][key] = value.clone();
                    }
                }
            }
        } else if let Some(object) = delta.as_object() {
            let block = self.native_blocks.entry(index).or_insert_with(|| json!({}));
            block
                .as_object_mut()
                .expect("native content block")
                .extend(object.clone());
        }
        Ok(())
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

fn append_native_string(value: &mut Value, part: &str) -> Result<(), ProviderError> {
    if value.is_null() {
        *value = Value::String(String::new());
    }
    let Value::String(text) = value else {
        return Err(ProviderError::Transport(
            "aws_bedrock native string delta does not match its content block".to_string(),
        ));
    };
    text.push_str(part);
    Ok(())
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

            if !stream_failed && let Some(error) = decoder.take_error() {
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

        if !stream_failed && !normalizer.saw_terminal {
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
