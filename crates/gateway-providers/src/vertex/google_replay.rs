use gateway_core::CoreChatMessage;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    error::VertexAdapterError,
    google_content::{FunctionIdPolicy, map_google_parts, parse_openai_tool_arguments},
};

/// Field the OpenAI shape carries a Gemini thought signature in: on the message, and on each tool
/// call under `extra_content.google`, the shape Google's own OpenAI-compatible endpoints use
/// (<https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/thinking/thought-signatures>).
pub(super) const THOUGHT_SIGNATURE_FIELD: &str = "thought_signature";

/// `reasoning_details[].format` OpenRouter gives Gemini reasoning
/// (<https://openrouter.ai/docs/guides/best-practices/reasoning-tokens>).
const GEMINI_REASONING_FORMAT: &str = "google-gemini-v1";
const ENCRYPTED_REASONING_TYPE: &str = "reasoning.encrypted";

/// One candidate's parts folded into the OpenAI message shape.
#[derive(Debug, Default)]
pub(super) struct CandidateParts {
    pub text: String,
    pub reasoning: String,
    /// Signature Gemini 3 attaches to a non-function part; replayed on the next turn.
    pub thought_signature: Option<Value>,
    /// Tool calls; a signed `functionCall` part puts its signature in `extra_content`.
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
            let signature = part
                .get("thoughtSignature")
                .filter(|value| is_signature(value));
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
            if let Some(signature) = signature {
                parts.thought_signature = Some(signature.clone());
            }
        }
        parts
    }

    /// Every signature in the candidate as `reasoning_details`: one entry per signed call, then
    /// the text-part signature.
    pub(super) fn reasoning_details(&self) -> Vec<Value> {
        self.tool_calls
            .iter()
            .filter_map(tool_call_reasoning_detail)
            .chain(
                self.thought_signature
                    .as_ref()
                    .map(|signature| signature_reasoning_detail(None, signature)),
            )
            .collect()
    }
}

fn is_signature(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|signature| !signature.is_empty())
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
        call.insert(
            "extra_content".to_string(),
            json!({ "google": { THOUGHT_SIGNATURE_FIELD: signature } }),
        );
    }
    Value::Object(call)
}

/// The `reasoning_details` entry for a tool call built from a signed `functionCall` part.
pub(super) fn tool_call_reasoning_detail(call: &Value) -> Option<Value> {
    let signature = call
        .get("extra_content")?
        .get("google")?
        .get(THOUGHT_SIGNATURE_FIELD)?;
    Some(signature_reasoning_detail(call.get("id"), signature))
}

/// A Gemini signature in OpenRouter's `reasoning_details` shape, which Pi, OMP and OpenCode keep
/// and send back. `call_id` binds it to the tool call whose part was signed; a text-part
/// signature has none.
pub(super) fn signature_reasoning_detail(call_id: Option<&Value>, signature: &Value) -> Value {
    let mut detail = json!({
        "type": ENCRYPTED_REASONING_TYPE,
        "data": signature,
        "format": GEMINI_REASONING_FORMAT
    });
    if let Some(id) = call_id {
        detail["id"] = id.clone();
    }
    detail
}

/// Signature a client sends back on an assistant tool call: Google's
/// `extra_content.google.thought_signature`, the top-level field earlier gateway versions
/// returned, or the `reasoning_details` entry bound to the call ID.
pub(super) fn replayed_call_signature<'a>(
    message: &'a CoreChatMessage,
    call: &'a Map<String, Value>,
) -> Option<&'a Value> {
    let extra_content = call
        .get("extra_content")
        .and_then(|extra| extra.get("google"))
        .and_then(|google| google.get(THOUGHT_SIGNATURE_FIELD));
    [
        extra_content,
        call.get(THOUGHT_SIGNATURE_FIELD),
        call.get("thoughtSignature"),
    ]
    .into_iter()
    .flatten()
    .find(|signature| is_signature(signature))
    .or_else(|| {
        let id = call.get("id").and_then(Value::as_str)?;
        replayed_detail_signature(message, Some(id))
    })
}

/// Text-part signature a client sends back at message level: `thought_signature`,
/// `provider_metadata.gcp_vertex.thought_signature`, or the `reasoning_details` entry that no
/// tool call owns.
pub(super) fn replayed_message_signature(message: &CoreChatMessage) -> Option<&Value> {
    let metadata = message
        .extra
        .get("provider_metadata")
        .and_then(|metadata| metadata.get("gcp_vertex"))
        .and_then(|metadata| metadata.get(THOUGHT_SIGNATURE_FIELD));
    [message.extra.get(THOUGHT_SIGNATURE_FIELD), metadata]
        .into_iter()
        .flatten()
        .find(|signature| is_signature(signature))
        .or_else(|| replayed_detail_signature(message, None))
}

/// Gemini signature in an encrypted `reasoning_details` entry whose `id` is `call_id`. Entries
/// from other model families never qualify, whatever their ID.
fn replayed_detail_signature<'a>(
    message: &'a CoreChatMessage,
    call_id: Option<&str>,
) -> Option<&'a Value> {
    let details = message.extra.get("reasoning_details")?.as_array()?;
    details.iter().rev().find_map(|detail| {
        let id = match detail.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) => Some(id.as_str()),
            Some(_) => return None,
        };
        let gemini = detail.get("type").and_then(Value::as_str) == Some(ENCRYPTED_REASONING_TYPE)
            && detail.get("format").and_then(Value::as_str) == Some(GEMINI_REASONING_FORMAT);
        (gemini && id == call_id)
            .then(|| detail.get("data"))
            .flatten()
            .filter(|signature| is_signature(signature))
    })
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
