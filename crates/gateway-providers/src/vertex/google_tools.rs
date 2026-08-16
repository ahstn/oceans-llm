use super::*;

pub(super) fn map_google_anthropic_tool_use_part(
    object: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest("tool_use content must include `id`".to_string())
    })?;
    let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest("tool_use content must include `name`".to_string())
    })?;
    let input = object
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    Ok(json!({
        "functionCall": {
            "id": id,
            "name": name,
            "args": input
        }
    }))
}

pub(super) fn map_google_anthropic_tool_result_part(
    object: &Map<String, Value>,
    known_tool_names: &BTreeMap<String, String>,
) -> Result<Value, ProviderError> {
    let tool_call_id = object
        .get("tool_use_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include `tool_use_id`".to_string(),
            )
        })?;
    let tool_name = known_tool_names.get(tool_call_id).ok_or_else(|| {
        ProviderError::InvalidRequest(format!(
            "tool_result references unknown `tool_use_id` `{tool_call_id}`"
        ))
    })?;
    let content = object.get("content").ok_or_else(|| {
        ProviderError::InvalidRequest("tool_result content must include `content`".to_string())
    })?;

    map_google_function_response_part(tool_call_id, tool_name, content)
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
                        .and_then(Value::as_object)
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

pub(super) fn map_google_assistant_parts(
    message: &CoreChatMessage,
) -> Result<Vec<Value>, ProviderError> {
    let mut parts = match &message.content {
        Value::Null => Vec::new(),
        Value::String(text) if text.is_empty() => Vec::new(),
        other => map_google_parts(other, None)?,
    };

    if let Some(tool_calls) = message.extra.get("tool_calls") {
        let calls = tool_calls.as_array().ok_or_else(|| {
            ProviderError::InvalidRequest("assistant tool_calls must be an array".to_string())
        })?;
        for call in calls {
            let object = call.as_object().ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must be objects".to_string(),
                )
            })?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderError::InvalidRequest(
                    "only function tool_calls are supported for google vertex mapping".to_string(),
                ));
            }
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "assistant function tool_calls must include `function`".to_string(),
                    )
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "assistant function tool_calls must include function.name".to_string(),
                    )
                })?;
            let args = parse_openai_tool_arguments(function)?;
            let mut function_call_part = json!({
                "functionCall": {
                    "name": name,
                    "args": args
                }
            });
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                function_call_part["functionCall"]["id"] = json!(id);
            }
            if let Some(signature) = object
                .get("thought_signature")
                .or_else(|| object.get("thoughtSignature"))
            {
                function_call_part["thoughtSignature"] = signature.clone();
            }
            parts.push(function_call_part);
        }
    }

    if parts.is_empty() {
        parts.push(json!({ "text": "" }));
    }

    Ok(parts)
}

pub(super) fn map_google_tool_result_part(
    message: &CoreChatMessage,
    known_tool_names: &BTreeMap<String, String>,
) -> Result<Value, ProviderError> {
    let tool_call_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest("tool messages must include `tool_call_id`".to_string())
        })?;
    let tool_name = known_tool_names.get(tool_call_id).ok_or_else(|| {
        ProviderError::InvalidRequest(format!(
            "tool message references unknown `tool_call_id` `{tool_call_id}`"
        ))
    })?;

    map_google_function_response_part(tool_call_id, tool_name, &message.content)
}

fn map_google_function_response_part(
    tool_call_id: &str,
    tool_name: &str,
    content: &Value,
) -> Result<Value, ProviderError> {
    let raw_text = message_content_as_text(content)?;
    let response_object = if let Ok(parsed_json) = serde_json::from_str::<Value>(&raw_text) {
        match parsed_json {
            Value::Object(map) => Value::Object(map),
            other => json!({ "output": other }),
        }
    } else {
        json!({ "output": raw_text })
    };

    Ok(json!({
        "functionResponse": {
            "id": tool_call_id,
            "name": tool_name,
            "response": response_object
        }
    }))
}
pub(super) fn convert_openai_tools_for_google(
    body: &mut Map<String, Value>,
) -> Result<(), ProviderError> {
    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    if let Some(parallel_tool_calls) = body.remove("parallel_tool_calls") {
        match parallel_tool_calls {
            Value::Bool(true) => {}
            Value::Bool(false) if has_tools => {
                return Err(ProviderError::InvalidRequest(
                    "`parallel_tool_calls: false` is not supported for google vertex chat"
                        .to_string(),
                ));
            }
            Value::Bool(false) => {}
            _ => {
                return Err(ProviderError::InvalidRequest(
                    "`parallel_tool_calls` must be a boolean".to_string(),
                ));
            }
        }
    }
    if let Some(tool_choice) = body.remove("tool_choice") {
        let is_none = match &tool_choice {
            Value::String(choice) => choice == "none",
            _ => false,
        };
        let function_calling_config = match tool_choice {
            Value::String(choice) if choice == "none" => {
                body.remove("tools");
                Some(json!({ "mode": "NONE" }))
            }
            Value::String(choice) if choice == "auto" => Some(json!({ "mode": "AUTO" })),
            Value::String(choice) if choice == "required" || choice == "any" => {
                Some(json!({ "mode": "ANY" }))
            }
            Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("auto") => {
                Some(json!({ "mode": "AUTO" }))
            }
            Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("any") => {
                Some(json!({ "mode": "ANY" }))
            }
            Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("tool") => {
                let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool tool_choice must include `name`".to_string(),
                    )
                })?;
                Some(json!({
                    "mode": "ANY",
                    "allowedFunctionNames": [name]
                }))
            }
            Value::Object(object)
                if object.get("type").and_then(Value::as_str) == Some("function") =>
            {
                let name = object
                    .get("function")
                    .and_then(Value::as_object)
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include function.name".to_string(),
                        )
                    })?;
                Some(json!({
                    "mode": "ANY",
                    "allowedFunctionNames": [name]
                }))
            }
            Value::Null => None,
            _ => {
                return Err(ProviderError::InvalidRequest(
                    "unsupported tool_choice for google vertex mapping".to_string(),
                ));
            }
        };

        if let Some(config) = function_calling_config {
            let mut tool_config = body
                .remove("toolConfig")
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let mut function_calling_config = tool_config
                .remove("functionCallingConfig")
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            if let Value::Object(config) = config {
                function_calling_config.extend(config);
            }
            tool_config.insert(
                "functionCallingConfig".to_string(),
                Value::Object(function_calling_config),
            );
            body.insert("toolConfig".to_string(), Value::Object(tool_config));
            if is_none {
                return Ok(());
            }
        }
    }

    if let Some(tools) = body.remove("tools") {
        let declarations = convert_openai_function_declarations(&tools)?;
        if !declarations.is_empty() {
            let vertex_tools = json!([{
                "functionDeclarations": declarations
            }]);
            body.insert("tools".to_string(), vertex_tools);
        }
    }

    Ok(())
}

fn convert_openai_function_declarations(value: &Value) -> Result<Vec<Value>, ProviderError> {
    let Some(tools) = value.as_array() else {
        return Err(ProviderError::InvalidRequest(
            "tools must be an array for google vertex mapping".to_string(),
        ));
    };
    let mut declarations = Vec::new();
    for tool in tools {
        let Some(object) = tool.as_object() else {
            return Err(ProviderError::InvalidRequest(
                "tools entries must be objects for google vertex mapping".to_string(),
            ));
        };
        let (function, anthropic_shape) =
            if object.get("type").and_then(Value::as_str) == Some("function") {
                (
                    object
                        .get("function")
                        .and_then(Value::as_object)
                        .ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "function tools must include an object `function`".to_string(),
                            )
                        })?,
                    false,
                )
            } else if object.get("name").is_some() {
                (object, true)
            } else {
                continue;
            };
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidRequest("function tools must include a name".to_string())
            })?;
        let mut declaration = Map::new();
        declaration.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = function.get("description").and_then(Value::as_str) {
            declaration.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        if anthropic_shape {
            let input_schema = function.get("input_schema").ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "anthropic function tools must include `input_schema`".to_string(),
                )
            })?;
            declaration.insert("parameters".to_string(), input_schema.clone());
        } else if let Some(parameters) = function.get("parameters") {
            declaration.insert("parameters".to_string(), parameters.clone());
        }
        declarations.push(Value::Object(declaration));
    }
    Ok(declarations)
}
