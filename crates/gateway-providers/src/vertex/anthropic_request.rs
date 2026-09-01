use super::*;

pub(super) fn map_anthropic_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let mut body: Map<String, Value> = request
        .extra
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    body.remove("model");
    body.remove("messages");
    body.remove("stream");
    body.remove("context_management");

    let mut messages = Vec::new();
    let mut instructions = Vec::new();
    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                instructions.push(message_content_as_text(&message.content)?);
            }
            "user" | "assistant" => {
                let content = map_anthropic_message_content(message)?;
                messages.push(json!({
                    "role": message.role,
                    "content": content
                }));
            }
            "tool" => {
                messages.push(json!({
                    "role": "user",
                    "content": [map_openai_tool_result(message)?]
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for anthropic vertex mapping"
                )));
            }
        }
    }

    if messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "anthropic vertex request requires at least one user/assistant message".to_string(),
        ));
    }

    if !body.contains_key("max_tokens") {
        body.insert("max_tokens".to_string(), Value::Number(1024.into()));
    }
    if !instructions.is_empty()
        && !body.contains_key("system")
        && !context.extra_body.contains_key("system")
    {
        body.insert(
            "system".to_string(),
            Value::String(instructions.join("\n\n")),
        );
    }

    merge_object_overrides(&mut body, &context.extra_body);
    body.remove("model");
    body.insert(
        "anthropic_version".to_string(),
        Value::String("vertex-2023-10-16".to_string()),
    );
    body.insert("messages".to_string(), Value::Array(messages));
    body.insert("stream".to_string(), Value::Bool(stream));
    convert_openai_tools_for_anthropic(&mut body)?;
    apply_vertex_anthropic_thinking_compatibility(&mut body, &context.upstream_model)?;
    validate_vertex_anthropic_sampling_fields(&mut body, &context.upstream_model)?;
    Ok(Value::Object(body))
}
fn map_anthropic_message_content(message: &CoreChatMessage) -> Result<Value, ProviderError> {
    let mut content = match map_anthropic_content(&message.content)? {
        Value::String(text) if text.is_empty() => Vec::new(),
        Value::String(text) => vec![json!({"type":"text","text":text})],
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let has_native_tool_use = content
        .iter()
        .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"));
    if !has_native_tool_use {
        content.extend(map_openai_assistant_tool_uses(message)?);
    }

    if content.is_empty() {
        Ok(Value::String(String::new()))
    } else if content.len() == 1 && is_plain_anthropic_text_block(&content[0]) {
        Ok(content[0]
            .get("text")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())))
    } else {
        Ok(Value::Array(content))
    }
}

fn map_anthropic_content(content: &Value) -> Result<Value, ProviderError> {
    match content {
        Value::Null => Ok(Value::String(String::new())),
        Value::String(value) => Ok(Value::String(value.clone())),
        Value::Array(items) => {
            let mut mapped = Vec::new();
            for item in items {
                let object = item.as_object().ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must be objects".to_string(),
                    )
                })?;
                let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must include `type`".to_string(),
                    )
                })?;
                match kind {
                    "text" | "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        if kind == "text" {
                            mapped.push(Value::Object(object.clone()));
                        } else {
                            mapped.push(json!({"type":"text","text":text}));
                        }
                    }
                    "thinking" | "redacted_thinking" | "tool_use" | "tool_result" => {
                        mapped.push(Value::Object(object.clone()));
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for anthropic vertex mapping"
                        )));
                    }
                }
            }
            Ok(Value::Array(mapped))
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn is_plain_anthropic_text_block(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == 2
        && object.get("type").and_then(Value::as_str) == Some("text")
        && object.get("text").and_then(Value::as_str).is_some()
}

fn map_openai_assistant_tool_uses(message: &CoreChatMessage) -> Result<Vec<Value>, ProviderError> {
    let Some(tool_calls) = message.extra.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let calls = tool_calls.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("assistant tool_calls must be an array".to_string())
    })?;
    let mut mapped = Vec::new();
    for call in calls {
        let object = call.as_object().ok_or_else(|| {
            ProviderError::InvalidRequest(
                "assistant tool_calls entries must be objects".to_string(),
            )
        })?;
        if object.get("type").and_then(Value::as_str) != Some("function") {
            return Err(ProviderError::InvalidRequest(
                "only function tool_calls are supported for anthropic vertex mapping".to_string(),
            ));
        }
        let id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
            ProviderError::InvalidRequest(
                "assistant tool_calls entries must include `id`".to_string(),
            )
        })?;
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
        let input = parse_openai_tool_arguments(function)?;
        mapped.push(json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input
        }));
    }
    Ok(mapped)
}

pub(super) fn parse_openai_tool_arguments(
    function: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let Some(arguments) = function.get("arguments") else {
        return Ok(Value::Object(Map::new()));
    };
    let arguments = arguments.as_str().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "assistant function tool_calls arguments must be a JSON string".to_string(),
        )
    })?;
    serde_json::from_str::<Value>(arguments).map_err(|error| {
        ProviderError::InvalidRequest(format!(
            "assistant function tool_calls arguments must contain valid JSON: {error}"
        ))
    })
}

fn map_openai_tool_result(message: &CoreChatMessage) -> Result<Value, ProviderError> {
    let tool_use_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest("tool messages must include `tool_call_id`".to_string())
        })?;
    Ok(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": message_content_as_text(&message.content)?
    }))
}

fn convert_openai_tools_for_anthropic(body: &mut Map<String, Value>) -> Result<(), ProviderError> {
    if let Some(tools) = body.get_mut("tools") {
        convert_openai_function_tools(tools)?;
    }
    if let Some(tool_choice) = body.get_mut("tool_choice") {
        convert_openai_tool_choice(tool_choice)?;
    }
    Ok(())
}

fn convert_openai_function_tools(value: &mut Value) -> Result<(), ProviderError> {
    let Some(tools) = value.as_array_mut() else {
        return Err(ProviderError::InvalidRequest(
            "tools must be an array for anthropic vertex mapping".to_string(),
        ));
    };
    for tool in tools {
        let Some(object) = tool.as_object_mut() else {
            return Err(ProviderError::InvalidRequest(
                "tools entries must be objects for anthropic vertex mapping".to_string(),
            ));
        };
        if object.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let function = object.remove("function").ok_or_else(|| {
            ProviderError::InvalidRequest("function tools must include `function`".to_string())
        })?;
        let function = function.as_object().ok_or_else(|| {
            ProviderError::InvalidRequest(
                "function tools must include an object `function`".to_string(),
            )
        })?;
        let name = function.get("name").cloned().ok_or_else(|| {
            ProviderError::InvalidRequest("function tools must include function.name".to_string())
        })?;
        let input_schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({"type":"object","properties":{}}));
        object.remove("type");
        object.insert("name".to_string(), name);
        if let Some(description) = function.get("description").cloned() {
            object.insert("description".to_string(), description);
        }
        object.insert("input_schema".to_string(), input_schema);
    }
    Ok(())
}

fn convert_openai_tool_choice(value: &mut Value) -> Result<(), ProviderError> {
    match value {
        Value::String(choice) if choice == "required" => {
            *value = json!({"type":"any"});
        }
        Value::String(choice) if choice == "auto" => {
            *value = json!({"type":choice});
        }
        Value::String(choice) if choice == "none" => {
            *value = json!({"type":choice});
        }
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            let name = object
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "function tool_choice must include function.name".to_string(),
                    )
                })?
                .to_string();
            *value = json!({"type":"tool","name":name});
        }
        Value::Object(_) => {}
        Value::Null => {}
        _ => {
            return Err(ProviderError::InvalidRequest(
                "unsupported tool_choice for anthropic vertex mapping".to_string(),
            ));
        }
    }
    Ok(())
}
