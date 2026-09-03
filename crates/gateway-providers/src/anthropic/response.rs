use gateway_core::ProviderRequestContext;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

pub fn normalize_anthropic_response(
    value: &Value,
    context: &ProviderRequestContext,
    provider_namespace: &str,
    usage_source: &str,
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
            provider_reasoning_metadata(provider_namespace, "anthropic_messages", thinking_blocks),
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

    if let Some(usage) = map_anthropic_usage(value, usage_source) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

pub fn extract_anthropic_thinking_blocks(blocks: &[Value]) -> Vec<Value> {
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

pub fn extract_anthropic_tool_calls(blocks: &[Value]) -> Vec<Value> {
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

pub fn map_anthropic_finish_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

pub fn provider_reasoning_metadata(
    provider_namespace: &str,
    source: &str,
    blocks: Vec<Value>,
) -> Value {
    let mut metadata = Map::new();
    let mut reasoning = Map::new();
    reasoning.insert("source".to_string(), Value::String(source.to_string()));
    reasoning.insert("blocks".to_string(), Value::Array(blocks));
    metadata.insert(
        provider_namespace.to_string(),
        Value::Object(Map::from_iter([(
            "reasoning".to_string(),
            Value::Object(reasoning),
        )])),
    );
    Value::Object(metadata)
}

pub fn map_anthropic_usage(value: &Value, usage_source: &str) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    map_anthropic_usage_object(usage, usage_source)
}

pub fn map_anthropic_stream_usage(value: &Value, usage_source: &str) -> Option<Value> {
    let usage = value
        .get("usage")
        .or_else(|| {
            value
                .get("message")
                .and_then(|message| message.get("usage"))
        })?
        .as_object()?;
    map_anthropic_usage_object(usage, usage_source)
}

fn map_anthropic_usage_object(usage: &Map<String, Value>, usage_source: &str) -> Option<Value> {
    let mut mapped = Map::new();
    if let Some(input) = usage.get("input_tokens") {
        mapped.insert("prompt_tokens".to_string(), input.clone());
    }
    if let Some(output) = usage.get("output_tokens") {
        mapped.insert("completion_tokens".to_string(), output.clone());
    }
    if let Some(total) = usage
        .get("total_tokens")
        .cloned()
        .or_else(|| anthropic_usage_total(usage).map(|total| json!(total)))
    {
        mapped.insert("total_tokens".to_string(), total);
    }
    mapped.insert(
        "usage_source".to_string(),
        Value::String(usage_source.to_string()),
    );
    mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    Some(Value::Object(mapped))
}

pub fn anthropic_usage_total(usage: &Map<String, Value>) -> Option<i64> {
    [
        "input_tokens",
        "output_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ]
    .into_iter()
    .try_fold(0_i64, |total, key| {
        total.checked_add(usage.get(key).and_then(Value::as_i64).unwrap_or(0))
    })
}

pub fn normalize_anthropic_thinking_delta(delta: &Map<String, Value>) -> Option<Value> {
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

pub fn normalize_anthropic_thinking_start(block: &Map<String, Value>) -> Option<Value> {
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
