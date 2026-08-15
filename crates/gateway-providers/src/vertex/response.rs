use super::*;

pub(super) fn normalize_google_response(value: &Value, context: &ProviderRequestContext) -> Value {
    let id = value
        .get("responseId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = OffsetDateTime::now_utc().unix_timestamp();

    let mut choices = Vec::new();
    for (index, candidate) in value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let text = extract_google_candidate_text(candidate);
        let google_finish_reason = candidate.get("finishReason").and_then(Value::as_str);
        let tool_calls = if google_finish_reason == Some("MALFORMED_FUNCTION_CALL") {
            Vec::new()
        } else {
            extract_google_candidate_tool_calls(candidate)
        };
        let finish_reason = if !tool_calls.is_empty() {
            "tool_calls"
        } else {
            google_finish_reason
                .map(map_google_finish_reason)
                .unwrap_or("stop")
        };

        let mut message = Map::new();
        message.insert("role".to_string(), Value::String("assistant".to_string()));
        if !tool_calls.is_empty() {
            message.insert(
                "content".to_string(),
                if text.is_empty() {
                    Value::Null
                } else {
                    Value::String(text)
                },
            );
            message.insert("tool_calls".to_string(), Value::Array(tool_calls));
        } else {
            message.insert("content".to_string(), Value::String(text));
        }

        choices.push(json!({
            "index": candidate.get("index").and_then(Value::as_i64).unwrap_or(index as i64),
            "message": Value::Object(message),
            "finish_reason": finish_reason
        }));
    }

    if choices.is_empty() {
        choices.push(json!({
            "index": 0,
            "message": {"role":"assistant","content":""},
            "finish_reason": "stop"
        }));
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
    completion.insert("choices".to_string(), Value::Array(choices));

    if let Some(usage) = map_google_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}
pub(super) fn normalize_anthropic_response(
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
    let text = blocks
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
    let thinking_blocks = extract_anthropic_thinking_blocks(blocks);
    let finish_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(map_anthropic_finish_reason)
        .unwrap_or("stop");

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(text));
    if !thinking_blocks.is_empty() {
        message.insert(
            "provider_metadata".to_string(),
            vertex_reasoning_metadata("anthropic_messages", thinking_blocks),
        );
    }
    let tool_calls = extract_anthropic_tool_calls(blocks);
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
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
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_anthropic_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

fn extract_anthropic_thinking_blocks(blocks: &[Value]) -> Vec<Value> {
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

fn extract_anthropic_tool_calls(blocks: &[Value]) -> Vec<Value> {
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
                .unwrap_or_else(|| Value::Object(Map::new()))
                .to_string();
            Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments
                }
            }))
        })
        .collect()
}

pub(super) fn normalize_anthropic_thinking_delta(delta: &Map<String, Value>) -> Option<Value> {
    match delta.get("type").and_then(Value::as_str) {
        Some("thinking_delta") => {
            let mut normalized = Map::new();
            normalized.insert(
                "type".to_string(),
                Value::String("thinking_delta".to_string()),
            );
            normalized.insert(
                "thinking".to_string(),
                delta
                    .get("thinking")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
            );
            Some(Value::Object(normalized))
        }
        Some("signature_delta") => {
            let mut normalized = Map::new();
            normalized.insert(
                "type".to_string(),
                Value::String("signature_delta".to_string()),
            );
            if let Some(signature) = delta.get("signature").cloned() {
                normalized.insert("signature".to_string(), signature);
            }
            Some(Value::Object(normalized))
        }
        _ => None,
    }
}

pub(super) fn normalize_anthropic_thinking_start(block: &Map<String, Value>) -> Option<Value> {
    match block.get("type").and_then(Value::as_str) {
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
    }
}

pub(super) fn vertex_reasoning_metadata(source: &str, blocks: Vec<Value>) -> Value {
    json!({
        "gcp_vertex": {
            "reasoning": {
                "source": source,
                "blocks": blocks
            }
        }
    })
}

pub(super) fn map_google_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usageMetadata")?.as_object()?;
    Some(json!({
        "prompt_tokens": usage.get("promptTokenCount").and_then(Value::as_i64).unwrap_or(0),
        "completion_tokens": usage.get("candidatesTokenCount").and_then(Value::as_i64).unwrap_or(0),
        "total_tokens": usage.get("totalTokenCount").and_then(Value::as_i64).unwrap_or(0)
    }))
}

fn map_anthropic_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let prompt = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Some(json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion
    }))
}

pub(super) fn map_anthropic_stream_usage(value: &Value) -> Option<Value> {
    let usage = value
        .get("usage")
        .or_else(|| {
            value
                .get("message")
                .and_then(|message| message.get("usage"))
        })?
        .as_object()?;
    let mut mapped = Map::new();
    if let Some(prompt) = usage.get("input_tokens").and_then(Value::as_i64) {
        mapped.insert("prompt_tokens".to_string(), json!(prompt));
    }
    if let Some(completion) = usage.get("output_tokens").and_then(Value::as_i64) {
        mapped.insert("completion_tokens".to_string(), json!(completion));
    }
    if let Some(total) = usage.get("total_tokens").and_then(Value::as_i64) {
        mapped.insert("total_tokens".to_string(), json!(total));
    } else if let (Some(prompt), Some(completion)) = (
        mapped.get("prompt_tokens").and_then(Value::as_i64),
        mapped.get("completion_tokens").and_then(Value::as_i64),
    ) {
        mapped.insert("total_tokens".to_string(), json!(prompt + completion));
    }

    if mapped.is_empty() {
        None
    } else {
        Some(Value::Object(mapped))
    }
}

pub(super) fn merge_openai_stream_usage(latest: &mut Option<Value>, usage: &Value) -> Value {
    let usage = openai_usage_with_known_fields(usage.clone(), latest.as_ref());
    *latest = Some(usage.clone());
    usage
}

fn openai_usage_with_known_fields(usage: Value, latest: Option<&Value>) -> Value {
    let prompt_tokens = merged_usage_counter(&usage, latest, "prompt_tokens");
    let completion_tokens = merged_usage_counter(&usage, latest, "completion_tokens");
    let total_tokens = match (prompt_tokens, completion_tokens) {
        (Some(prompt), Some(completion)) => prompt.saturating_add(completion),
        _ => merged_usage_counter(&usage, latest, "total_tokens").unwrap_or(0),
    };

    let mut object = usage.as_object().cloned().unwrap_or_default();
    if let Some(prompt_tokens) = prompt_tokens {
        object.insert("prompt_tokens".to_string(), json!(prompt_tokens));
    }
    if let Some(completion_tokens) = completion_tokens {
        object.insert("completion_tokens".to_string(), json!(completion_tokens));
    }
    object.insert("total_tokens".to_string(), json!(total_tokens));
    Value::Object(object)
}

fn merged_usage_counter(usage: &Value, latest: Option<&Value>, field: &str) -> Option<i64> {
    let incoming = usage.get(field).and_then(Value::as_i64);
    let previous = latest
        .and_then(|latest| latest.get(field))
        .and_then(Value::as_i64);

    match (incoming, previous) {
        (Some(incoming), Some(previous)) => Some(incoming.max(previous)),
        (Some(incoming), None) => Some(incoming),
        (None, Some(previous)) => Some(previous),
        (None, None) => None,
    }
}

pub(super) fn extract_google_candidate_text(candidate: &Value) -> String {
    candidate
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

pub(super) fn extract_google_candidate_tool_calls(candidate: &Value) -> Vec<Value> {
    let mut tool_calls = Vec::new();
    let Some(parts) = candidate
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
    else {
        return tool_calls;
    };

    for part in parts {
        if let Some(function_call) = part.get("functionCall").and_then(Value::as_object) {
            let name = function_call
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = function_call
                .get("args")
                .map(|args| serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string()))
                .unwrap_or_else(|| "{}".to_string());
            let id = function_call
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
            let mut call_obj = json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": args
                }
            });
            if let Some(signature) = part
                .get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
            {
                call_obj["thought_signature"] = signature.clone();
            }
            tool_calls.push(call_obj);
        }
    }
    tool_calls
}

pub(super) fn map_google_finish_reason(reason: &str) -> &'static str {
    match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" => "content_filter",
        "MALFORMED_FUNCTION_CALL" => "stop",
        _ => "stop",
    }
}

pub(super) fn map_anthropic_finish_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "stop_sequence" => "stop",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}
