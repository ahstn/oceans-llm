use std::collections::{BTreeMap, HashMap};

use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderError};
use serde_json::{Map, Value, json};

use super::{
    error::VertexAdapterError,
    google_content::{
        FunctionIdPolicy, function_call_part, map_google_parts, map_google_tool_result_part,
        message_content_as_text, parse_openai_tool_arguments, record_google_tool_names,
    },
    google_replay::{THOUGHT_SIGNATURE_FIELD, validated_native_parts},
};

pub(super) fn map_google_contents(
    request: &CoreChatRequest,
    function_ids: FunctionIdPolicy,
    body: &mut Map<String, Value>,
) -> Result<(), ProviderError> {
    let mut contents = Vec::new();
    let mut system_lines = Vec::new();
    let mut known_tool_names = BTreeMap::new();
    let mut pending_tool_parts = Vec::new();
    let mut tool_call_order = Vec::new();

    for message in &request.messages {
        if message.role == "tool" {
            let part = map_google_tool_result_part(message, &known_tool_names, function_ids)?;
            pending_tool_parts.push((
                message.extra["tool_call_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                part,
            ));
            continue;
        }

        if !pending_tool_parts.is_empty() {
            contents.push(json!({
                "role": "user",
                "parts": ordered_tool_results(
                    std::mem::take(&mut pending_tool_parts), &tool_call_order, function_ids
                )
            }));
        }

        match message.role.as_str() {
            "system" | "developer" => {
                system_lines.push(message_content_as_text(&message.content)?);
            }
            "user" => {
                let mut parts =
                    map_google_parts(&message.content, Some(&known_tool_names), function_ids)?;
                if parts.is_empty() {
                    // Vertex rejects a content entry with no parts.
                    parts.push(json!({ "text": "" }));
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            "assistant" => {
                let parts = map_google_assistant_parts(message, function_ids)?;
                record_google_tool_names(message, &mut known_tool_names);
                tool_call_order = message
                    .extra
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|call| call.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect();
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            other => {
                return Err(VertexAdapterError::UnsupportedMessageRole(other.to_string()).into());
            }
        }
    }

    if !pending_tool_parts.is_empty() {
        contents.push(json!({
            "role": "user",
            "parts": ordered_tool_results(pending_tool_parts, &tool_call_order, function_ids)
        }));
    }
    if contents.is_empty() {
        return Err(VertexAdapterError::EmptyMessages.into());
    }
    body.insert("contents".to_string(), Value::Array(contents));

    if !system_lines.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({ "parts": [{ "text": system_lines.join("\n\n") }] }),
        );
    }
    Ok(())
}

/// Older Gemini models correlate parallel results by position, not ID or function name.
/// Only a complete, unambiguous group can be safely reordered; other history stays untouched.
fn ordered_tool_results(
    mut parts: Vec<(String, Value)>,
    call_order: &[String],
    function_ids: FunctionIdPolicy,
) -> Vec<Value> {
    if function_ids == FunctionIdPolicy::Omit && parts.len() >= 2 && parts.len() == call_order.len()
    {
        let mut ranks = HashMap::with_capacity(call_order.len());
        let unique_calls = call_order
            .iter()
            .enumerate()
            .all(|(rank, id)| ranks.insert(id.as_str(), (rank, false)).is_none());
        let bijection = unique_calls
            && parts
                .iter()
                .all(|(id, _)| match ranks.get_mut(id.as_str()) {
                    Some((_, seen)) if !*seen => {
                        *seen = true;
                        true
                    }
                    _ => false,
                });
        if bijection {
            parts.sort_unstable_by_key(|(id, _)| ranks[id.as_str()].0);
        }
    }
    parts.into_iter().map(|(_, part)| part).collect()
}

/// Maps an assistant turn to `model` parts, replaying the text-part thought signature Gemini 3
/// returned on the previous turn (`thought_signature` on the message, or
/// `provider_metadata.gcp_vertex.thought_signature`).
fn map_google_assistant_parts(
    message: &CoreChatMessage,
    function_ids: FunctionIdPolicy,
) -> Result<Vec<Value>, VertexAdapterError> {
    if let Some(metadata) = message
        .extra
        .get("provider_metadata")
        .and_then(|metadata| metadata.get("gcp_vertex"))
        .and_then(|metadata| metadata.get("native_content"))
        .filter(|metadata| {
            metadata.get("source").and_then(Value::as_str) == Some("google_generate_content")
        })
    {
        return validated_native_parts(message, metadata, function_ids);
    }
    let mut parts = match &message.content {
        Value::Null => Vec::new(),
        Value::String(text) if text.is_empty() => Vec::new(),
        other => map_google_parts(other, None, function_ids)?,
    };

    if let Some(signature) = message_thought_signature(message) {
        match parts
            .iter_mut()
            .rev()
            .find(|part| part.get("text").is_some())
        {
            Some(text_part) => {
                text_part["thoughtSignature"] = signature.clone();
            }
            None => parts.push(json!({ "text": "", "thoughtSignature": signature })),
        }
    }

    if let Some(tool_calls) = message.extra.get("tool_calls") {
        let calls = tool_calls
            .as_array()
            .ok_or(VertexAdapterError::InvalidToolCalls)?;
        for call in calls {
            let object = call
                .as_object()
                .ok_or(VertexAdapterError::InvalidToolCallEntry)?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(VertexAdapterError::UnsupportedToolCallType);
            }
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or(VertexAdapterError::MissingToolCallFunction)?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or(VertexAdapterError::MissingToolCallFunctionName)?;
            let args = parse_openai_tool_arguments(function)?;
            let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
            let signature = object
                .get(THOUGHT_SIGNATURE_FIELD)
                .or_else(|| object.get("thoughtSignature"));
            parts.push(function_call_part(id, name, args, function_ids, signature));
        }
    }

    if parts.is_empty() {
        parts.push(json!({ "text": "" }));
    }
    Ok(parts)
}

fn message_thought_signature(message: &CoreChatMessage) -> Option<&Value> {
    message
        .extra
        .get(THOUGHT_SIGNATURE_FIELD)
        .or_else(|| {
            message
                .extra
                .get("provider_metadata")?
                .get("gcp_vertex")?
                .get(THOUGHT_SIGNATURE_FIELD)
        })
        .filter(|signature| signature.as_str().is_some_and(|s| !s.is_empty()))
}
