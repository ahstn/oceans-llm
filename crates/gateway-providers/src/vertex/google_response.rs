use gateway_core::{ProviderRequestContext, ServiceTier};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    error::VertexAdapterError,
    google_replay::{CandidateParts, THOUGHT_SIGNATURE_FIELD, native_parts_metadata},
};
use crate::service_tier::report_served_tier;

/// Missing terminal state is permitted only while streaming. Failure and unknown reasons are
/// upstream errors, never successful stops (even if the candidate also includes function calls).
pub(super) fn candidate_finish_reason(
    candidate: &Value,
) -> Result<Option<&'static str>, VertexAdapterError> {
    let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) else {
        return Ok(None);
    };
    let message = candidate
        .get("finishMessage")
        .and_then(Value::as_str)
        .unwrap_or(reason);
    let mapped = match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY"
        | "RECITATION"
        | "LANGUAGE"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "MODEL_ARMOR"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT" => "content_filter",
        "MALFORMED_FUNCTION_CALL" => {
            return Err(VertexAdapterError::MalformedFunctionCall(
                message.to_string(),
            ));
        }
        _ => {
            return Err(VertexAdapterError::UnsuccessfulFinishReason(format!(
                "{reason}: {message}"
            )));
        }
    };
    Ok(Some(mapped))
}

/// Gemini's chosen candidate is not necessarily the first (or even present) in topCandidates.
pub(super) fn map_google_logprobs(candidate: &Value) -> Option<Value> {
    let result = candidate.get("logprobsResult")?;
    let chosen = result.get("chosenCandidates")?.as_array()?;
    let top = result.get("topCandidates").and_then(Value::as_array);
    let token = |candidate: &Value| {
        Some(json!({
            "token": candidate.get("token")?.as_str()?,
            "logprob": candidate.get("logProbability")?.as_f64()?,
            "bytes": Value::Null
        }))
    };
    let content: Option<Vec<_>> = chosen
        .iter()
        .enumerate()
        .map(|(index, chosen)| {
            let mut chosen = token(chosen)?;
            let alternatives: Option<Vec<_>> = top
                .and_then(|top| top.get(index))
                .and_then(|entry| entry.get("candidates"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(token)
                .collect();
            chosen["top_logprobs"] = json!(alternatives?);
            Some(chosen)
        })
        .collect();
    Some(json!({ "content": content? }))
}

/// `promptFeedback.blockReason` is set when the prompt itself was refused; no candidates follow.
pub(super) fn prompt_block_reason(value: &Value) -> Option<&str> {
    value
        .get("promptFeedback")?
        .get("blockReason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty() && *reason != "BLOCK_REASON_UNSPECIFIED")
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
        let mapped = candidate_finish_reason(candidate)?.ok_or(
            VertexAdapterError::IncompleteResponse("missing candidate finishReason"),
        )?;
        let parts = CandidateParts::from_candidate(candidate);
        let finish_reason = if mapped == "stop" && !parts.tool_calls.is_empty() {
            "tool_calls"
        } else {
            mapped
        };

        let mut metadata = native_parts_metadata(
            parts.native_parts,
            parts
                .tool_calls
                .iter()
                .map(|call| call["id"].clone())
                .collect(),
            parts.thought_signature.clone(),
        );
        metadata["gcp_vertex"]["finish_reason"] = candidate["finishReason"].clone();
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
            message.insert(THOUGHT_SIGNATURE_FIELD.to_string(), signature);
        }
        message.insert("provider_metadata".to_string(), metadata);
        if !parts.tool_calls.is_empty() {
            message.insert("tool_calls".to_string(), Value::Array(parts.tool_calls));
        }

        let mut choice = json!({
            "index": candidate.get("index").and_then(Value::as_i64).unwrap_or(index as i64),
            "message": Value::Object(message),
            "finish_reason": finish_reason
        });
        if let Some(logprobs) = map_google_logprobs(candidate) {
            choice["logprobs"] = logprobs;
        }
        choices.push(choice);
    }

    if choices.is_empty() {
        let Some(block_reason) = prompt_block_reason(value) else {
            return Err(VertexAdapterError::IncompleteResponse(
                "no candidates or promptFeedback.blockReason",
            ));
        };
        choices.push(json!({
            "index": 0,
            "message": {
                "role": "assistant", "content": Value::Null,
                "provider_metadata": { "gcp_vertex": { "prompt_block_reason": block_reason } }
            },
            "finish_reason": "content_filter"
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
    let mut completion = Value::Object(completion);
    report_served_tier(&mut completion, google_served_tier(value));
    Ok(completion)
}

/// The capacity grade `usageMetadata.trafficType` reports: `ON_DEMAND`, `ON_DEMAND_FLEX`, or
/// `ON_DEMAND_PRIORITY`. Provisioned Throughput bills outside per-token rates and reads as none.
pub(super) fn google_served_tier(value: &Value) -> Option<ServiceTier> {
    value
        .get("usageMetadata")?
        .get("trafficType")?
        .as_str()
        .and_then(ServiceTier::from_served)
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
