use gateway_core::ProviderRequestContext;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{error::VertexAdapterError, google_tools::THOUGHT_SIGNATURE_FIELD};

/// One candidate's parts folded into the OpenAI message shape.
#[derive(Debug, Default)]
pub(super) struct CandidateParts {
    pub text: String,
    pub reasoning: String,
    /// Signature Gemini 3 attaches to a non-function part; replayed on the next turn.
    pub thought_signature: Option<Value>,
    pub tool_calls: Vec<Value>,
}

impl CandidateParts {
    pub(super) fn from_candidate(candidate: &Value) -> Self {
        let mut parts = Self::default();
        let Some(items) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        else {
            return parts;
        };
        for part in items {
            let signature = part.get("thoughtSignature");
            if let Some(function_call) = part.get("functionCall") {
                parts
                    .tool_calls
                    .push(tool_call_from_function_call(function_call, signature));
                continue;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    parts.reasoning.push_str(text);
                } else {
                    parts.text.push_str(text);
                }
            }
            if let Some(signature) = signature.filter(|s| s.as_str().is_some_and(|s| !s.is_empty()))
            {
                parts.thought_signature = Some(signature.clone());
            }
        }
        parts
    }
}

fn tool_call_from_function_call(function_call: &Value, signature: Option<&Value>) -> Value {
    let name = function_call
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = function_call
        .get("args")
        .map_or_else(|| "{}".to_string(), Value::to_string);
    let id = function_call.get("id").and_then(Value::as_str).map_or_else(
        || format!("call_{}", Uuid::new_v4().simple()),
        str::to_string,
    );
    let mut call = Map::new();
    call.insert("id".to_string(), Value::String(id));
    call.insert("type".to_string(), Value::String("function".to_string()));
    call.insert(
        "function".to_string(),
        json!({ "name": name, "arguments": arguments }),
    );
    if let Some(signature) = signature {
        call.insert(THOUGHT_SIGNATURE_FIELD.to_string(), signature.clone());
    }
    Value::Object(call)
}

pub(super) fn thought_signature_metadata(signature: Value) -> Value {
    json!({ "gcp_vertex": { THOUGHT_SIGNATURE_FIELD: signature } })
}

/// Vertex reports these as candidate `finishReason`s; anything not listed maps to `stop`.
pub(super) fn map_google_finish_reason(reason: &str) -> &'static str {
    match reason {
        "MAX_TOKENS" => "length",
        "SAFETY"
        | "RECITATION"
        | "LANGUAGE"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "OTHER" => "content_filter",
        _ => "stop",
    }
}

/// `MALFORMED_FUNCTION_CALL` means the model emitted an unparseable call; Vertex drops it and
/// the turn has no usable content, so it surfaces as a retryable upstream failure.
pub(super) fn check_malformed_function_call(candidate: &Value) -> Result<(), VertexAdapterError> {
    if candidate.get("finishReason").and_then(Value::as_str) == Some("MALFORMED_FUNCTION_CALL") {
        let message = candidate
            .get("finishMessage")
            .and_then(Value::as_str)
            .unwrap_or("finishReason MALFORMED_FUNCTION_CALL");
        return Err(VertexAdapterError::MalformedFunctionCall(
            message.to_string(),
        ));
    }
    Ok(())
}

/// `promptFeedback.blockReason` is set when the prompt itself was refused; no candidates follow.
pub(super) fn prompt_block_reason(value: &Value) -> Option<&str> {
    value
        .get("promptFeedback")?
        .get("blockReason")
        .and_then(Value::as_str)
}

/// In-band error objects arrive with HTTP 200 on streams (for example `RESOURCE_EXHAUSTED`).
pub(super) fn inline_error(value: &Value) -> Option<VertexAdapterError> {
    let error = value.get("error")?.as_object()?;
    let status = error
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(VertexAdapterError::StreamError(format!(
        "{status} ({code}): {message}"
    )))
}

pub(super) fn normalize_google_response(
    value: &Value,
    context: &ProviderRequestContext,
) -> Result<Value, VertexAdapterError> {
    if let Some(error) = inline_error(value) {
        return Err(error);
    }
    let id = value.get("responseId").and_then(Value::as_str).map_or_else(
        || format!("chatcmpl-{}", Uuid::new_v4().simple()),
        str::to_string,
    );
    let created = OffsetDateTime::now_utc().unix_timestamp();

    let mut choices = Vec::new();
    for (index, candidate) in value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        check_malformed_function_call(candidate)?;
        let parts = CandidateParts::from_candidate(candidate);
        let finish_reason = if parts.tool_calls.is_empty() {
            candidate
                .get("finishReason")
                .and_then(Value::as_str)
                .map_or("stop", map_google_finish_reason)
        } else {
            "tool_calls"
        };

        let mut message = Map::new();
        message.insert("role".to_string(), Value::String("assistant".to_string()));
        let content = if parts.text.is_empty() && !parts.tool_calls.is_empty() {
            Value::Null
        } else {
            Value::String(parts.text)
        };
        message.insert("content".to_string(), content);
        if !parts.reasoning.is_empty() {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(parts.reasoning),
            );
        }
        if let Some(signature) = parts.thought_signature {
            message.insert(THOUGHT_SIGNATURE_FIELD.to_string(), signature.clone());
            message.insert(
                "provider_metadata".to_string(),
                thought_signature_metadata(signature),
            );
        }
        if !parts.tool_calls.is_empty() {
            message.insert("tool_calls".to_string(), Value::Array(parts.tool_calls));
        }

        choices.push(json!({
            "index": candidate.get("index").and_then(Value::as_i64).unwrap_or(index as i64),
            "message": Value::Object(message),
            "finish_reason": finish_reason
        }));
    }

    if choices.is_empty() {
        let finish_reason = if prompt_block_reason(value).is_some() {
            "content_filter"
        } else {
            "stop"
        };
        choices.push(json!({
            "index": 0,
            "message": { "role": "assistant", "content": Value::Null },
            "finish_reason": finish_reason
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
    Ok(Value::Object(completion))
}

/// Maps `usageMetadata` to OpenAI usage. Gemini's `candidatesTokenCount` excludes thinking
/// tokens while OpenAI's `completion_tokens` includes them, so thoughts are added back and
/// also reported under `completion_tokens_details.reasoning_tokens`.
pub(super) fn map_google_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usageMetadata")?.as_object()?;
    let count = |key: &str| usage.get(key).and_then(Value::as_i64);
    let mut mapped = Map::new();
    if let Some(prompt) = count("promptTokenCount") {
        mapped.insert("prompt_tokens".to_string(), json!(prompt));
    }
    let thoughts = count("thoughtsTokenCount");
    match (count("candidatesTokenCount"), thoughts) {
        (Some(candidates), thoughts) => {
            mapped.insert(
                "completion_tokens".to_string(),
                json!(candidates.saturating_add(thoughts.unwrap_or(0))),
            );
        }
        (None, Some(thoughts)) => {
            mapped.insert("completion_tokens".to_string(), json!(thoughts));
        }
        (None, None) => {}
    }
    if let Some(total) = count("totalTokenCount") {
        mapped.insert("total_tokens".to_string(), json!(total));
    }
    if let Some(cached) = count("cachedContentTokenCount") {
        mapped.insert(
            "prompt_tokens_details".to_string(),
            json!({ "cached_tokens": cached }),
        );
    }
    if let Some(thoughts) = thoughts {
        mapped.insert(
            "completion_tokens_details".to_string(),
            json!({ "reasoning_tokens": thoughts }),
        );
    }
    mapped.insert(
        "usage_source".to_string(),
        Value::String("vertex_google".to_string()),
    );
    mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    Some(Value::Object(mapped))
}
