use gateway_core::CoreChatMessage;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    error::VertexAdapterError,
    google_content::{FunctionIdPolicy, map_google_parts, parse_openai_tool_arguments},
};

/// Field the OpenAI shape carries a Gemini thought signature in, on both tool calls and messages.
pub(super) const THOUGHT_SIGNATURE_FIELD: &str = "thought_signature";

/// One candidate's parts folded into the OpenAI message shape.
#[derive(Debug, Default)]
pub(super) struct CandidateParts {
    pub text: String,
    pub reasoning: String,
    /// Signature Gemini 3 attaches to a non-function part; replayed on the next turn.
    pub thought_signature: Option<Value>,
    pub tool_calls: Vec<Value>,
    pub native_parts: Vec<Value>,
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
        parts.native_parts = items.clone();
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

/// Native parts are opaque replay data; call IDs bind ID-less parts to the public tool calls.
pub(super) fn native_parts_metadata(
    parts: Vec<Value>,
    tool_call_ids: Vec<Value>,
    signature: Option<Value>,
) -> Value {
    let mut metadata = json!({ "gcp_vertex": {} });
    if !parts.is_empty() {
        metadata["gcp_vertex"]["native_content"] = json!({
            "source": "google_generate_content",
            "parts": parts,
            "tool_call_ids": tool_call_ids
        });
    }
    if let Some(signature) = signature {
        metadata["gcp_vertex"][THOUGHT_SIGNATURE_FIELD] = signature;
    }
    metadata
}

/// Metadata is caller-controlled. Never restore a hidden prompt or call that differs from Chat.
pub(super) fn validated_native_parts(
    message: &CoreChatMessage,
    metadata: &Value,
    function_ids: FunctionIdPolicy,
) -> Result<Vec<Value>, VertexAdapterError> {
    let mismatch = || VertexAdapterError::ReplayProjectionMismatch;
    let parts = metadata
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(mismatch)?;
    let ids = metadata
        .get("tool_call_ids")
        .and_then(Value::as_array)
        .ok_or_else(mismatch)?;
    let public_parts = if message.content.is_null() {
        Vec::new()
    } else {
        map_google_parts(&message.content, None, function_ids)?
    };
    let mut public_text = String::new();
    for part in public_parts {
        public_text.push_str(
            part.get("text")
                .and_then(Value::as_str)
                .ok_or_else(mismatch)?,
        );
    }
    let public_calls = match message.extra.get("tool_calls") {
        None => &[][..],
        Some(calls) => calls.as_array().ok_or_else(mismatch)?.as_slice(),
    };
    let public_reasoning = match message.extra.get("reasoning_content") {
        None | Some(Value::Null) => "",
        Some(reasoning) => reasoning.as_str().ok_or_else(mismatch)?,
    };
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut call_index = 0;
    for part in parts {
        if !part.is_object() {
            return Err(mismatch());
        }
        if let Some(call) = part.get("functionCall") {
            let public = public_calls.get(call_index).ok_or_else(mismatch)?;
            let id = ids
                .get(call_index)
                .and_then(Value::as_str)
                .ok_or_else(mismatch)?;
            let function = public
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(mismatch)?;
            if public.get("type").and_then(Value::as_str) != Some("function")
                || public.get("id").and_then(Value::as_str) != Some(id)
                || call
                    .get("id")
                    .is_some_and(|native| native.as_str() != Some(id))
                || call.get("name").and_then(Value::as_str).is_none()
                || call.get("name") != function.get("name")
                || call.get("args").cloned().unwrap_or_else(|| json!({}))
                    != parse_openai_tool_arguments(function)?
            {
                return Err(mismatch());
            }
            call_index += 1;
        } else if let Some(part_text) = part.get("text") {
            let part_text = part_text.as_str().ok_or_else(mismatch)?;
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                reasoning.push_str(part_text);
            } else {
                text.push_str(part_text);
            }
        }
    }
    if text != public_text
        || reasoning != public_reasoning
        || call_index != public_calls.len()
        || call_index != ids.len()
    {
        return Err(mismatch());
    }
    let mut parts = parts.clone();
    let mut call_index = 0;
    for part in &mut parts {
        if let Some(call) = part.get_mut("functionCall").and_then(Value::as_object_mut) {
            match function_ids {
                FunctionIdPolicy::Include => {
                    call.insert("id".to_string(), ids[call_index].clone());
                }
                FunctionIdPolicy::Omit => {
                    call.remove("id");
                }
            }
            call_index += 1;
        }
    }
    Ok(parts)
}
