use super::*;

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
        let mut parser = JsonObjectParser::default();
        let mut role_emitted = false;
        let mut finish_emitted = false;
        let mut stream_failed = false;
        let mut stream_has_tool_calls = false;
        let mut next_tool_call_index = 0usize;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_google_stream_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };
            let objects = match parser.push_bytes(&chunk) {
                Ok(objects) => objects,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("google_stream_parse_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            for object in objects {
                let candidate = object
                    .get("candidates")
                    .and_then(Value::as_array)
                    .and_then(|candidates| candidates.first());
                let text = candidate
                    .map(extract_google_candidate_text)
                    .unwrap_or_default();
                let google_finish_reason = candidate
                    .and_then(|candidate| candidate.get("finishReason"))
                    .and_then(Value::as_str);
                let tool_calls = if google_finish_reason == Some("MALFORMED_FUNCTION_CALL") {
                    Vec::new()
                } else {
                    candidate
                        .map(extract_google_candidate_tool_calls)
                        .unwrap_or_default()
                };
                if !tool_calls.is_empty() {
                    stream_has_tool_calls = true;
                }
                let upstream_finish_reason = google_finish_reason.map(map_google_finish_reason);
                if !text.is_empty() {
                    let delta = openai_chunk(
                        &stream_id,
                        created,
                        &model,
                        (!role_emitted).then_some("assistant"),
                        Some(&text),
                        None,
                    );
                    yield Ok(openai_sse_chunk(&delta));
                    role_emitted = true;
                }

                for tool_call in tool_calls {
                    let mut delta = Map::new();
                    if !role_emitted {
                        delta.insert("role".to_string(), Value::String("assistant".to_string()));
                        role_emitted = true;
                    }
                    let mut call_delta = Map::new();
                    call_delta.insert("index".to_string(), json!(next_tool_call_index));
                    next_tool_call_index += 1;
                    if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                        call_delta.insert("id".to_string(), json!(id));
                    }
                    call_delta.insert("type".to_string(), json!("function"));
                    if let Some(function) = tool_call.get("function").and_then(Value::as_object) {
                        call_delta.insert("function".to_string(), Value::Object(function.clone()));
                    }
                    if let Some(signature) = tool_call.get("thought_signature") {
                        call_delta.insert("thought_signature".to_string(), signature.clone());
                    }
                    delta.insert("tool_calls".to_string(), Value::Array(vec![Value::Object(call_delta)]));

                    let chunk = openai_delta_chunk(
                        &stream_id,
                        created,
                        &model,
                        Value::Object(delta),
                        None,
                    );
                    yield Ok(openai_sse_chunk(&chunk));
                }

                if let Some(reason) = upstream_finish_reason {
                    let finish_reason = if stream_has_tool_calls {
                        "tool_calls"
                    } else {
                        reason
                    };
                    // Emits the single terminal finish reason when upstream signals candidate completion.
                    let finish = openai_chunk(
                        &stream_id,
                        created,
                        &model,
                        None,
                        None,
                        Some(finish_reason),
                    );
                    yield Ok(openai_sse_chunk(&finish));
                    finish_emitted = true;
                }
            }
        }

        if !stream_failed && !finish_emitted {
            let finish_reason = if stream_has_tool_calls {
                "tool_calls"
            } else {
                "stop"
            };
            let finish = openai_chunk(
                &stream_id,
                created,
                &model,
                None,
                None,
                Some(finish_reason),
            );
            yield Ok(openai_sse_chunk(&finish));
        }
        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

pub(super) fn normalize_anthropic_stream<S>(
    upstream: S,
    stream_id: String,
    created: i64,
    model: String,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    // Split plan: move Anthropic SSE state, usage merging, and event mapping into
    // a focused Vertex Anthropic stream module when this normalizer next grows.
    Box::pin(stream! {
        let mut parser = SseEventParser::default();
        let mut role_emitted = false;
        let mut finish_emitted = false;
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
                    Ok(value) => value,
                    Err(_) => continue,
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
                        if let Some(usage) = map_anthropic_stream_usage(&payload) {
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
                            .and_then(|delta| delta.get("type"))
                            .and_then(Value::as_str);
                        if delta_type == Some("text_delta")
                            && let Some(text) = delta
                                .and_then(|delta| delta.get("text"))
                                .and_then(Value::as_str)
                                .filter(|text| !text.is_empty())
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
                                block_index.and_then(|index| tool_block_indexes.get(&index).copied()),
                                delta
                                    .and_then(|delta| delta.get("partial_json"))
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
                                vertex_reasoning_metadata(
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
                                        .unwrap_or("toolu_vertex"),
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
                                vertex_reasoning_metadata(
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
                        let usage = map_anthropic_stream_usage(&payload)
                            .map(|usage| merge_openai_stream_usage(&mut latest_usage, &usage));
                        if let Some(reason) = payload
                            .get("delta")
                            .and_then(Value::as_object)
                            .and_then(|delta| delta.get("stop_reason"))
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
                    "message_stop" if !finish_emitted => {
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
                    "error" => {
                        let message = payload
                            .get("error")
                            .and_then(Value::as_object)
                            .and_then(|error| error.get("message"))
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

        if !stream_failed && !finish_emitted {
            let finish = openai_chunk(
                &stream_id,
                created,
                &model,
                None,
                None,
                Some("stop"),
            );
            yield Ok(openai_sse_chunk(&finish));
        }
        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

#[derive(Debug, Clone, Default)]
pub(super) struct JsonObjectParser {
    utf8: Utf8ChunkDecoder,
    buffer: String,
}

impl JsonObjectParser {
    pub(super) fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<Value>, ProviderError> {
        let text = self.utf8.push_bytes(chunk)?;
        self.buffer.push_str(&text);

        let mut parsed = Vec::new();
        let mut consumed_until = 0usize;
        let mut object_start = None;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let bytes = self.buffer.as_bytes();

        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }

            match byte {
                b'"' => in_string = true,
                b'{' => {
                    if depth == 0 {
                        object_start = Some(index);
                    }
                    depth += 1;
                }
                b'}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0
                        && let Some(start) = object_start.take()
                    {
                        let end = index + 1;
                        let object_json = &self.buffer[start..end];
                        let value: Value = serde_json::from_str(object_json).map_err(|error| {
                            ProviderError::Transport(format!(
                                "failed parsing streamed google JSON object: {error}"
                            ))
                        })?;
                        parsed.push(value);
                        consumed_until = end;
                    }
                }
                _ => {}
            }

            index += 1;
        }

        if consumed_until > 0 {
            self.buffer.drain(..consumed_until);
        }

        Ok(parsed)
    }
}

fn openai_chunk(
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
            .map(|reason| Value::String(reason.to_string()))
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

fn openai_delta_chunk(
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

fn openai_usage_chunk(id: &str, created: i64, model: &str, usage: Value) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [],
        "usage": usage
    })
}

fn openai_sse_chunk(value: &Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}
