use super::*;

pub(super) fn normalize_converse_response(
    value: &Value,
    context: &ProviderRequestContext,
) -> Value {
    let id = value
        .get("responseId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = OffsetDateTime::now_utc().unix_timestamp();
    let blocks = value
        .get("output")
        .and_then(|output| output.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let content = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    let tool_calls = extract_tool_calls(blocks);
    let reasoning_blocks = extract_bedrock_reasoning_blocks(blocks);
    let finish_reason = value
        .get("stopReason")
        .and_then(Value::as_str)
        .map(map_stop_reason)
        .unwrap_or("stop");

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(content));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    if !reasoning_blocks.is_empty() {
        message.insert(
            "provider_metadata".to_string(),
            bedrock_reasoning_metadata("bedrock_converse", reasoning_blocks),
        );
    }

    let mut completion = Map::new();
    completion.insert("id".to_string(), Value::String(id));
    completion.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    completion.insert("created".to_string(), Value::Number(created.into()));
    completion.insert(
        "model".to_string(),
        Value::String(context.model_key.clone()),
    );
    completion.insert(
        "provider_model".to_string(),
        Value::String(context.upstream_model.clone()),
    );
    completion.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

pub(super) fn normalize_anthropic_messages_response(
    value: &Value,
    context: &ProviderRequestContext,
) -> Value {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = OffsetDateTime::now_utc().unix_timestamp();
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let content = blocks
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                block.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");
    let tool_calls = extract_anthropic_tool_calls(blocks);
    let thinking_blocks = extract_anthropic_thinking_blocks(blocks);
    let finish_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(map_stop_reason)
        .unwrap_or("stop");

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(content));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    if !thinking_blocks.is_empty() {
        message.insert(
            "provider_metadata".to_string(),
            bedrock_reasoning_metadata("anthropic_messages", thinking_blocks),
        );
    }

    let mut completion = Map::new();
    completion.insert("id".to_string(), Value::String(id));
    completion.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    completion.insert("created".to_string(), Value::Number(created.into()));
    completion.insert(
        "model".to_string(),
        Value::String(context.model_key.clone()),
    );
    completion.insert(
        "provider_model".to_string(),
        Value::String(context.upstream_model.clone()),
    );
    completion.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

pub(super) fn normalize_anthropic_messages_stream<S>(
    upstream: S,
    context: ProviderRequestContext,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let created = OffsetDateTime::now_utc().unix_timestamp();
        let mut parser = SseEventParser::default();
        let mut saw_payload_event = false;
        let mut stream_failed = false;
        let mut id = format!("chatcmpl-{}", Uuid::new_v4().simple());
        let mut sent_role = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_anthropic_messages_stream_error",
                        &error.to_string(),
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

                if event_type == "message_start" {
                    if let Some(message_id) = value
                        .get("message")
                        .and_then(|message| message.get("id"))
                        .and_then(Value::as_str)
                    {
                        id = message_id.to_string();
                    }
                    if !sent_role {
                        yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                            &id,
                            created,
                            &context,
                            json!({"role": "assistant"}),
                            None,
                            None,
                        )).unwrap_or_else(|_| "{}".to_string())));
                        sent_role = true;
                    }
                    continue;
                }

                if event_type == "content_block_delta" {
                    let Some(delta) = value.get("delta").and_then(Value::as_object) else {
                        continue;
                    };
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            if !sent_role {
                                yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                                    &id,
                                    created,
                                    &context,
                                    json!({"role": "assistant"}),
                                    None,
                                    None,
                                )).unwrap_or_else(|_| "{}".to_string())));
                                sent_role = true;
                            }
                            let text = delta.get("text").and_then(Value::as_str).unwrap_or_default();
                            yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                                &id,
                                created,
                                &context,
                                json!({"content": text}),
                                None,
                                None,
                            )).unwrap_or_else(|_| "{}".to_string())));
                        }
                        Some("thinking_delta") | Some("signature_delta") => {
                            let metadata = bedrock_reasoning_metadata(
                                "anthropic_messages_stream",
                                vec![Value::Object(delta.clone())],
                            );
                            yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                                &id,
                                created,
                                &context,
                                json!({"provider_metadata": metadata}),
                                None,
                                None,
                            )).unwrap_or_else(|_| "{}".to_string())));
                        }
                        Some("input_json_delta") => {
                            let partial_json = delta
                                .get("partial_json")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let index = value
                                .get("index")
                                .and_then(Value::as_u64)
                                .and_then(|value| u32::try_from(value).ok())
                                .unwrap_or(0);
                            yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                                &id,
                                created,
                                &context,
                                json!({"tool_calls": [{
                                    "index": index,
                                    "function": {"arguments": partial_json}
                                }]}),
                                None,
                                None,
                            )).unwrap_or_else(|_| "{}".to_string())));
                        }
                        _ => {}
                    }
                    continue;
                }

                if event_type == "message_delta" {
                    let finish_reason = value
                        .get("delta")
                        .and_then(|delta| delta.get("stop_reason"))
                        .and_then(Value::as_str)
                        .map(map_stop_reason);
                    let usage = value.get("usage").cloned();
                    if finish_reason.is_some() || usage.is_some() {
                        yield Ok(render_sse_event_chunk(None, &serde_json::to_string(&anthropic_stream_chunk(
                            &id,
                            created,
                            &context,
                            json!({}),
                            finish_reason,
                            usage,
                        )).unwrap_or_else(|_| "{}".to_string())));
                    }
                }
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

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

pub(super) fn anthropic_stream_chunk(
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
            "delta": delta,
            "finish_reason": finish_reason
        }]
    });
    if let Some(usage) = usage
        && let Some(object) = chunk.as_object_mut()
    {
        object.insert("usage".to_string(), usage);
    }
    chunk
}

pub(super) fn extract_tool_calls(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| {
            let tool_use = block.get("toolUse")?;
            let id = tool_use.get("toolUseId").and_then(Value::as_str)?;
            let name = tool_use.get("name").and_then(Value::as_str)?;
            let arguments = tool_use
                .get("input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments.to_string()
                }
            }))
        })
        .collect()
}

pub(super) fn extract_anthropic_tool_calls(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                return None;
            }
            let id = block.get("id").and_then(Value::as_str)?;
            let name = block.get("name").and_then(Value::as_str)?;
            let arguments = block
                .get("input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments.to_string()
                }
            }))
        })
        .collect()
}

pub(super) fn extract_anthropic_thinking_blocks(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                let mut normalized = Map::new();
                normalized.insert("type".to_string(), Value::String("thinking".to_string()));
                normalized.insert(
                    "thinking".to_string(),
                    block
                        .get("thinking")
                        .cloned()
                        .unwrap_or_else(|| Value::String(String::new())),
                );
                if let Some(signature) = block.get("signature").cloned() {
                    normalized.insert("signature".to_string(), signature);
                }
                Some(Value::Object(normalized))
            }
            Some("redacted_thinking") => {
                let mut normalized = Map::new();
                normalized.insert(
                    "type".to_string(),
                    Value::String("redacted_thinking".to_string()),
                );
                if let Some(data) = block.get("data").cloned() {
                    normalized.insert("data".to_string(), data);
                }
                Some(Value::Object(normalized))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn extract_bedrock_reasoning_blocks(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| block.get("reasoningContent"))
        .filter_map(normalize_bedrock_reasoning_content)
        .collect()
}

pub(super) fn normalize_bedrock_reasoning_content(reasoning: &Value) -> Option<Value> {
    if let Some(reasoning_text) = reasoning.get("reasoningText").and_then(Value::as_object) {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("reasoning_text".to_string()),
        );
        normalized.insert(
            "text".to_string(),
            reasoning_text
                .get("text")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
        );
        if let Some(signature) = reasoning_text.get("signature").cloned() {
            normalized.insert("signature".to_string(), signature);
        }
        return Some(Value::Object(normalized));
    }

    if let Some(text) = reasoning.get("text").cloned() {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("reasoning_text".to_string()),
        );
        normalized.insert("text".to_string(), text);
        if let Some(signature) = reasoning.get("signature").cloned() {
            normalized.insert("signature".to_string(), signature);
        }
        return Some(Value::Object(normalized));
    }

    if let Some(signature) = reasoning.get("signature").cloned() {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("reasoning_signature".to_string()),
        );
        normalized.insert("signature".to_string(), signature);
        return Some(Value::Object(normalized));
    }

    if let Some(data) = reasoning
        .get("redactedContent")
        .or_else(|| reasoning.get("data"))
        .cloned()
    {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("redacted_reasoning".to_string()),
        );
        normalized.insert("data".to_string(), data);
        return Some(Value::Object(normalized));
    }

    if let Some(redacted) = reasoning.get("redactedReasoning") {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("redacted_reasoning".to_string()),
        );
        if let Some(data) = redacted.get("data").cloned() {
            normalized.insert("data".to_string(), data);
        }
        return Some(Value::Object(normalized));
    }

    None
}

pub(super) fn bedrock_reasoning_metadata(source: &str, blocks: Vec<Value>) -> Value {
    json!({
        "aws_bedrock": {
            "reasoning": {
                "source": source,
                "blocks": blocks
            }
        }
    })
}

pub(super) fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" | "model_context_window_exceeded" => "length",
        "tool_use" => "tool_calls",
        "guardrail_intervened" | "content_filtered" | "refusal" => "content_filter",
        "malformed_model_output" | "malformed_tool_use" => "stop",
        _ => "stop",
    }
}

pub(super) fn map_usage(value: &Value) -> Option<Value> {
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
