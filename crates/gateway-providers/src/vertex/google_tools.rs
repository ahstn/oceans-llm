use std::collections::BTreeMap;

use gateway_core::CoreChatMessage;
use serde_json::{Map, Value, json};

use super::{error::VertexAdapterError, google_request::map_google_parts};

/// Whether `functionCall.id` / `functionResponse.id` are emitted. Vertex rejects them on models
/// older than Gemini 3.5, so the request mapper decides per model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FunctionIdPolicy {
    Include,
    Omit,
}

/// Field the OpenAI shape carries a Gemini thought signature in, on both tool calls and messages.
pub(super) const THOUGHT_SIGNATURE_FIELD: &str = "thought_signature";

pub(super) fn map_google_anthropic_tool_use_part(
    object: &Map<String, Value>,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolUseField("id"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolUseField("name"))?;
    let args = object
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    Ok(function_call_part(id, name, args, function_ids, None))
}

pub(super) fn map_google_anthropic_tool_result_part(
    object: &Map<String, Value>,
    known_tool_names: &BTreeMap<String, String>,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let tool_call_id = object
        .get("tool_use_id")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolResultField("tool_use_id"))?;
    let tool_name = known_tool_names
        .get(tool_call_id)
        .ok_or_else(|| VertexAdapterError::UnknownToolCallId(tool_call_id.to_string()))?;
    let content = object
        .get("content")
        .ok_or(VertexAdapterError::MissingToolResultField("content"))?;
    map_google_function_response_part(tool_call_id, tool_name, content, function_ids)
}

pub(super) fn record_google_tool_names(
    message: &CoreChatMessage,
    known_tool_names: &mut BTreeMap<String, String>,
) {
    if let Some(tool_calls) = message.extra.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            if let Some(call) = call.as_object()
                && let (Some(id), Some(name)) = (
                    call.get("id").and_then(Value::as_str),
                    call.get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str),
                )
            {
                known_tool_names.insert(id.to_string(), name.to_string());
            }
        }
    }

    if let Value::Array(blocks) = &message.content {
        for block in blocks {
            if let Some(block) = block.as_object()
                && block.get("type").and_then(Value::as_str) == Some("tool_use")
                && let (Some(id), Some(name)) = (
                    block.get("id").and_then(Value::as_str),
                    block.get("name").and_then(Value::as_str),
                )
            {
                known_tool_names.insert(id.to_string(), name.to_string());
            }
        }
    }
}

/// Maps an assistant turn to `model` parts, replaying the text-part thought signature Gemini 3
/// returned on the previous turn (`thought_signature` on the message, or
/// `provider_metadata.gcp_vertex.thought_signature`).
pub(super) fn map_google_assistant_parts(
    message: &CoreChatMessage,
    function_ids: FunctionIdPolicy,
) -> Result<Vec<Value>, VertexAdapterError> {
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

fn function_call_part(
    id: &str,
    name: &str,
    args: Value,
    function_ids: FunctionIdPolicy,
    signature: Option<&Value>,
) -> Value {
    let mut function_call = Map::new();
    if function_ids == FunctionIdPolicy::Include && !id.is_empty() {
        function_call.insert("id".to_string(), Value::String(id.to_string()));
    }
    function_call.insert("name".to_string(), Value::String(name.to_string()));
    function_call.insert("args".to_string(), args);

    let mut part = Map::new();
    part.insert("functionCall".to_string(), Value::Object(function_call));
    if let Some(signature) = signature {
        part.insert("thoughtSignature".to_string(), signature.clone());
    }
    Value::Object(part)
}

pub(super) fn parse_openai_tool_arguments(
    function: &Map<String, Value>,
) -> Result<Value, VertexAdapterError> {
    let Some(arguments) = function.get("arguments") else {
        return Ok(Value::Object(Map::new()));
    };
    let arguments = arguments
        .as_str()
        .ok_or(VertexAdapterError::InvalidToolCallArguments)?;
    serde_json::from_str(arguments).map_err(VertexAdapterError::MalformedToolCallArguments)
}

pub(super) fn map_google_tool_result_part(
    message: &CoreChatMessage,
    known_tool_names: &BTreeMap<String, String>,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let tool_call_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolCallId)?;
    let tool_name = known_tool_names
        .get(tool_call_id)
        .ok_or_else(|| VertexAdapterError::UnknownToolCallId(tool_call_id.to_string()))?;
    map_google_function_response_part(tool_call_id, tool_name, &message.content, function_ids)
}

fn map_google_function_response_part(
    tool_call_id: &str,
    tool_name: &str,
    content: &Value,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let raw_text = super::google_request::message_content_as_text(content)?;
    let response = match serde_json::from_str::<Value>(&raw_text) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(other) => json!({ "output": other }),
        Err(_) => json!({ "output": raw_text }),
    };

    let mut function_response = Map::new();
    if function_ids == FunctionIdPolicy::Include {
        function_response.insert("id".to_string(), Value::String(tool_call_id.to_string()));
    }
    function_response.insert("name".to_string(), Value::String(tool_name.to_string()));
    function_response.insert("response".to_string(), response);
    Ok(json!({ "functionResponse": function_response }))
}

pub(super) fn convert_openai_tools_for_google(
    body: &mut Map<String, Value>,
) -> Result<(), VertexAdapterError> {
    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    match body.remove("parallel_tool_calls") {
        None | Some(Value::Bool(true)) | Some(Value::Null) => {}
        Some(Value::Bool(false)) if has_tools => {
            return Err(VertexAdapterError::ParallelToolCallsDisabled);
        }
        Some(Value::Bool(false)) => {}
        Some(_) => return Err(VertexAdapterError::InvalidParallelToolCalls),
    }

    if let Some(tool_choice) = body.remove("tool_choice")
        && let Some(config) = function_calling_config(&tool_choice)?
    {
        let disables_tools = config.get("mode").and_then(Value::as_str) == Some("NONE");
        let tool_config = body
            .entry("toolConfig".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let tool_config = tool_config
            .as_object_mut()
            .ok_or(VertexAdapterError::InvalidFunctionCallingConfig)?;
        let existing = tool_config
            .entry("functionCallingConfig".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        existing
            .as_object_mut()
            .ok_or(VertexAdapterError::InvalidFunctionCallingConfig)?
            .extend(config);
        if disables_tools {
            body.remove("tools");
            return Ok(());
        }
    }

    if let Some(tools) = body.remove("tools") {
        let declarations = convert_openai_function_declarations(&tools)?;
        if !declarations.is_empty() {
            body.insert(
                "tools".to_string(),
                json!([{ "functionDeclarations": declarations }]),
            );
        }
    }
    Ok(())
}

fn function_calling_config(
    tool_choice: &Value,
) -> Result<Option<Map<String, Value>>, VertexAdapterError> {
    let mode = |mode: &str| {
        let mut config = Map::new();
        config.insert("mode".to_string(), Value::String(mode.to_string()));
        config
    };
    let forced = |name: &str| {
        let mut config = mode("ANY");
        config.insert("allowedFunctionNames".to_string(), json!([name]));
        config
    };
    let config = match tool_choice {
        Value::Null => return Ok(None),
        Value::String(choice) => match choice.as_str() {
            "none" => mode("NONE"),
            "auto" => mode("AUTO"),
            "required" | "any" => mode("ANY"),
            "validated" => mode("VALIDATED"),
            _ => return Err(VertexAdapterError::UnsupportedToolChoice),
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => mode("AUTO"),
            Some("any") => mode("ANY"),
            Some("tool") => forced(
                object
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or(VertexAdapterError::MissingToolChoiceName)?,
            ),
            Some("function") => forced(
                object
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                    .ok_or(VertexAdapterError::MissingToolChoiceFunctionName)?,
            ),
            _ => return Err(VertexAdapterError::UnsupportedToolChoice),
        },
        _ => return Err(VertexAdapterError::UnsupportedToolChoice),
    };
    Ok(Some(config))
}

/// Converts OpenAI (`{type:function, function:{...}}`) or Anthropic (`{name, input_schema}`)
/// tool definitions into Gemini function declarations. Schemas are sent as
/// `parametersJsonSchema`, which accepts full JSON Schema; the legacy `parameters` field is an
/// OpenAPI subset that rejects `$defs`, `additionalProperties`, and similar keywords.
fn convert_openai_function_declarations(value: &Value) -> Result<Vec<Value>, VertexAdapterError> {
    let tools = value
        .as_array()
        .ok_or(VertexAdapterError::InvalidToolsArray)?;
    let mut declarations = Vec::with_capacity(tools.len());
    for tool in tools {
        let object = tool
            .as_object()
            .ok_or(VertexAdapterError::InvalidToolEntry)?;
        let (function, schema_key) =
            if object.get("type").and_then(Value::as_str) == Some("function") {
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or(VertexAdapterError::InvalidToolFunction)?;
                (function, "parameters")
            } else if object.contains_key("name") {
                (object, "input_schema")
            } else {
                continue;
            };
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or(VertexAdapterError::MissingToolName)?;
        let mut declaration = Map::new();
        declaration.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = function.get("description").filter(|d| d.is_string()) {
            declaration.insert("description".to_string(), description.clone());
        }
        match function.get(schema_key) {
            Some(schema) => {
                declaration.insert("parametersJsonSchema".to_string(), schema.clone());
            }
            None if schema_key == "input_schema" => {
                return Err(VertexAdapterError::MissingToolInputSchema);
            }
            None => {}
        }
        declarations.push(Value::Object(declaration));
    }
    Ok(declarations)
}
