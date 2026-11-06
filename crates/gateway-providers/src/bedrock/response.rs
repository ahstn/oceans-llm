use gateway_core::{ProviderError, ProviderRequestContext};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::converse_replay::extract_bedrock_reasoning_blocks;

pub(super) fn normalize_converse_response(
    value: &Value,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    if let Some(reason @ ("malformed_tool_use" | "malformed_model_output")) =
        value.get("stopReason").and_then(Value::as_str)
    {
        return Err(ProviderError::PartialUsage {
            source: Box::new(ProviderError::Transport(format!(
                "aws_bedrock generation failed: {reason}"
            ))),
            provider_usage: map_usage(value),
        });
    }
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
    let metadata = message
        .entry("provider_metadata")
        .or_insert_with(|| json!({}));
    metadata["aws_bedrock"]["native_content"] = json!({
        "source": "bedrock_converse",
        "blocks": blocks,
    });
    metadata["aws_bedrock"]["stop_reason"] =
        value.get("stopReason").cloned().unwrap_or(Value::Null);
    if let Some(fields) = value.get("additionalModelResponseFields") {
        metadata["aws_bedrock"]["additionalModelResponseFields"] = fields.clone();
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

    Ok(Value::Object(completion))
}

pub(crate) fn normalize_anthropic_messages_response(
    value: &Value,
    context: &ProviderRequestContext,
    provider_namespace: &str,
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
            provider_reasoning_metadata(provider_namespace, "anthropic_messages", thinking_blocks),
        );
    }
    crate::anthropic::response::preserve_anthropic_message_metadata(
        &mut message,
        value,
        provider_namespace,
        "message",
    );

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

fn provider_reasoning_metadata(
    provider_namespace: &str,
    source: &str,
    blocks: Vec<Value>,
) -> Value {
    let mut metadata = Map::new();
    metadata.insert(
        provider_namespace.to_string(),
        json!({
            "reasoning": {
                "source": source,
                "blocks": blocks
            }
        }),
    );
    Value::Object(metadata)
}

pub(super) fn bedrock_reasoning_metadata(source: &str, blocks: Vec<Value>) -> Value {
    provider_reasoning_metadata("aws_bedrock", source, blocks)
}

pub(super) fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" | "model_context_window_exceeded" => "length",
        "tool_use" => "tool_calls",
        "guardrail_intervened" | "content_filtered" | "refusal" => "content_filter",
        _ => "stop",
    }
}

pub(super) fn map_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let input = usage
        .get("inputTokens")
        .or_else(|| usage.get("input_tokens"));
    let output = usage
        .get("outputTokens")
        .or_else(|| usage.get("output_tokens"));
    let total = usage
        .get("totalTokens")
        .or_else(|| usage.get("total_tokens"))
        .cloned()
        .or_else(|| {
            [
                input.and_then(Value::as_i64),
                output.and_then(Value::as_i64),
                usage
                    .get("cacheReadInputTokens")
                    .or_else(|| usage.get("cache_read_input_tokens"))
                    .and_then(Value::as_i64),
                usage
                    .get("cacheWriteInputTokens")
                    .or_else(|| usage.get("cache_write_input_tokens"))
                    .or_else(|| usage.get("cache_creation_input_tokens"))
                    .and_then(Value::as_i64),
            ]
            .into_iter()
            .try_fold(0_i64, |total, value| total.checked_add(value.unwrap_or(0)))
            .map(|total| Value::Number(total.into()))
        });

    let mut mapped = Map::new();
    if let Some(input) = input {
        mapped.insert("prompt_tokens".to_string(), input.clone());
    }
    if let Some(output) = output {
        mapped.insert("completion_tokens".to_string(), output.clone());
    }
    if let Some(total) = total {
        mapped.insert("total_tokens".to_string(), total);
    }
    mapped.insert(
        "usage_source".to_string(),
        Value::String("bedrock".to_string()),
    );
    mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    Some(Value::Object(mapped))
}
