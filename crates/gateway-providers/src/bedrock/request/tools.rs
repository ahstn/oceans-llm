use super::*;

pub(super) fn map_assistant_tool_uses(
    message: &gateway_core::CoreChatMessage,
) -> Result<Vec<Value>, ProviderError> {
    let Some(tool_calls) = message.extra.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let calls = tool_calls.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("assistant tool_calls must be an array".to_string())
    })?;

    calls
        .iter()
        .map(|call| {
            let object = call.as_object().ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must be objects".to_string(),
                )
            })?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderError::InvalidRequest(
                    "only function tool_calls are supported for aws_bedrock Converse".to_string(),
                ));
            }
            let tool_use_id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
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
            let input = match function.get("arguments") {
                Some(Value::String(arguments)) => {
                    serde_json::from_str(arguments).map_err(|error| {
                        ProviderError::InvalidRequest(format!(
                            "assistant function tool_call arguments must be JSON: {error}"
                        ))
                    })?
                }
                Some(value) => value.clone(),
                None => Value::Object(Map::new()),
            };

            Ok(json!({
                "toolUse": {
                    "toolUseId": tool_use_id,
                    "name": name,
                    "input": input
                }
            }))
        })
        .collect()
}

pub(super) fn map_anthropic_assistant_tool_uses(
    message: &gateway_core::CoreChatMessage,
) -> Result<Vec<Value>, ProviderError> {
    let Some(tool_calls) = message.extra.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let calls = tool_calls.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("assistant tool_calls must be an array".to_string())
    })?;

    calls
        .iter()
        .map(|call| {
            let object = call.as_object().ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must be objects".to_string(),
                )
            })?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderError::InvalidRequest(
                    "only function tool_calls are supported for aws_bedrock Anthropic Claude Messages"
                        .to_string(),
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
            let input = match function.get("arguments") {
                Some(Value::String(arguments)) => {
                    serde_json::from_str(arguments).map_err(|error| {
                        ProviderError::InvalidRequest(format!(
                            "assistant function tool_call arguments must be JSON: {error}"
                        ))
                    })?
                }
                Some(value) => value.clone(),
                None => Value::Object(Map::new()),
            };

            Ok(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }))
        })
        .collect()
}

pub(super) fn map_tool_result(
    message: &gateway_core::CoreChatMessage,
) -> Result<Value, ProviderError> {
    let tool_call_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest("tool messages must include `tool_call_id`".to_string())
        })?;
    let content = match &message.content {
        Value::String(text) => vec![json!({ "text": text })],
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let object = item.as_object().ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool message content array entries must be objects".to_string(),
                    )
                })?;
                let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool message content entries must include string `text`".to_string(),
                    )
                })?;
                Ok(json!({ "text": text }))
            })
            .collect::<Result<Vec<_>, ProviderError>>()?,
        _ => {
            return Err(ProviderError::InvalidRequest(
                "tool message content must be a string or text content array".to_string(),
            ));
        }
    };

    Ok(json!({
        "toolResult": {
            "toolUseId": tool_call_id,
            "content": content
        }
    }))
}

pub(super) fn map_anthropic_tool_result(
    message: &gateway_core::CoreChatMessage,
) -> Result<Value, ProviderError> {
    let tool_use_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest("tool messages must include `tool_call_id`".to_string())
        })?;
    let content = match &message.content {
        Value::String(text) => Value::String(text.clone()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| {
                    let object = item.as_object().ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "tool message content array entries must be objects".to_string(),
                        )
                    })?;
                    let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "tool message content entries must include string `text`".to_string(),
                        )
                    })?;
                    Ok(json!({ "type": "text", "text": text }))
                })
                .collect::<Result<Vec<_>, ProviderError>>()?,
        ),
        _ => {
            return Err(ProviderError::InvalidRequest(
                "tool message content must be a string or text content array".to_string(),
            ));
        }
    };

    Ok(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content
    }))
}

pub(super) fn map_tool_result_content_block(
    object: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let tool_use_id = object
        .get("tool_use_id")
        .or_else(|| object.get("toolUseId"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include tool_use_id".to_string(),
            )
        })?;
    let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest("tool_result content must include string `text`".to_string())
    })?;

    Ok(json!({
        "toolResult": {
            "toolUseId": tool_use_id,
            "content": [{ "text": text }]
        }
    }))
}

pub(super) fn map_anthropic_tool_result_content_block(
    object: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let tool_use_id = object
        .get("tool_use_id")
        .or_else(|| object.get("toolUseId"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include tool_use_id".to_string(),
            )
        })?;
    let content = object
        .get("content")
        .cloned()
        .or_else(|| {
            object
                .get("text")
                .and_then(Value::as_str)
                .map(|text| Value::String(text.to_string()))
        })
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include `content` or string `text`".to_string(),
            )
        })?;

    Ok(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content
    }))
}

pub(super) fn extract_tool_config(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<Value>, ProviderError> {
    let Some(tools) = extra.remove("tools") else {
        if let Some(tool_choice) = extra.remove("tool_choice")
            && !tool_choice_is_none_or_auto(&tool_choice)
        {
            return Err(ProviderError::InvalidRequest(
                "tool_choice requires non-empty tools for aws_bedrock Converse".to_string(),
            ));
        }
        return Ok(None);
    };

    let tools_array = tools.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("tools must be an array for aws_bedrock Converse".to_string())
    })?;
    if tools_array.is_empty() {
        return Ok(None);
    }

    let tool_choice = extra.remove("tool_choice");
    if tool_choice.as_ref().is_some_and(tool_choice_is_none) {
        return Ok(None);
    }

    let mut bedrock_tools = Vec::new();
    for tool in tools_array {
        let object = tool.as_object().ok_or_else(|| {
            ProviderError::InvalidRequest("tool entries must be objects".to_string())
        })?;
        if object.get("type").and_then(Value::as_str) != Some("function") {
            return Err(ProviderError::InvalidRequest(
                "only OpenAI function tools are supported for aws_bedrock Converse".to_string(),
            ));
        }
        let function = object
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProviderError::InvalidRequest("function tools must include `function`".to_string())
            })?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "function tools must include function.name".to_string(),
                )
            })?;
        let schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        let mut spec = Map::new();
        spec.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = function
            .get("description")
            .and_then(Value::as_str)
            .filter(|description| !description.trim().is_empty())
        {
            spec.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        spec.insert("inputSchema".to_string(), json!({ "json": schema }));
        if let Some(strict) = function.get("strict").and_then(Value::as_bool) {
            spec.insert("strict".to_string(), Value::Bool(strict));
        }
        bedrock_tools.push(json!({ "toolSpec": spec }));
    }

    let mut tool_config = Map::new();
    tool_config.insert("tools".to_string(), Value::Array(bedrock_tools));
    if let Some(tool_choice) = tool_choice
        && let Some(mapped) = map_tool_choice(&tool_choice)?
    {
        tool_config.insert("toolChoice".to_string(), mapped);
    }

    Ok(Some(Value::Object(tool_config)))
}

pub(super) fn extract_anthropic_tools(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<Map<String, Value>>, ProviderError> {
    let tool_choice = extra.remove("tool_choice");
    let Some(tools) = extra.remove("tools") else {
        if let Some(tool_choice) = tool_choice
            && !tool_choice_is_none_or_auto(&tool_choice)
        {
            return Err(ProviderError::InvalidRequest(
                "tool_choice requires non-empty tools for aws_bedrock Anthropic Claude Messages"
                    .to_string(),
            ));
        }
        return Ok(None);
    };

    if tool_choice.as_ref().is_some_and(tool_choice_is_none) {
        return Ok(None);
    }

    let tools_array = tools.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "tools must be an array for aws_bedrock Anthropic Claude Messages".to_string(),
        )
    })?;
    if tools_array.is_empty() {
        return Ok(None);
    }

    let mut anthropic_tools = Vec::new();
    for tool in tools_array {
        let object = tool.as_object().ok_or_else(|| {
            ProviderError::InvalidRequest("tool entries must be objects".to_string())
        })?;
        if object.get("type").and_then(Value::as_str) != Some("function") {
            return Err(ProviderError::InvalidRequest(
                "only OpenAI function tools are supported for aws_bedrock Anthropic Claude Messages"
                    .to_string(),
            ));
        }
        let function = object
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProviderError::InvalidRequest("function tools must include `function`".to_string())
            })?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "function tools must include function.name".to_string(),
                )
            })?;
        let schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        let mut spec = Map::new();
        spec.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = function
            .get("description")
            .and_then(Value::as_str)
            .filter(|description| !description.trim().is_empty())
        {
            spec.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        spec.insert("input_schema".to_string(), schema);
        anthropic_tools.push(Value::Object(spec));
    }

    let mut mapped = Map::new();
    mapped.insert("tools".to_string(), Value::Array(anthropic_tools));
    if let Some(tool_choice) = tool_choice
        && let Some(choice) = map_anthropic_tool_choice(&tool_choice)?
    {
        mapped.insert("tool_choice".to_string(), choice);
    }
    Ok(Some(mapped))
}

pub(super) fn tool_choice_is_none_or_auto(value: &Value) -> bool {
    matches!(value.as_str(), Some("none" | "auto"))
        || value
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "none" | "auto"))
}

pub(super) fn tool_choice_is_none(value: &Value) -> bool {
    value.as_str() == Some("none")
        || value
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            == Some("none")
}

pub(super) fn map_tool_choice(value: &Value) -> Result<Option<Value>, ProviderError> {
    match value {
        Value::String(choice) => match choice.as_str() {
            "auto" => Ok(Some(json!({ "auto": {} }))),
            "required" => Ok(Some(json!({ "any": {} }))),
            "none" => Ok(None),
            other => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice `{other}` for aws_bedrock Converse"
            ))),
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => Ok(Some(json!({ "auto": {} }))),
            Some("required") => Ok(Some(json!({ "any": {} }))),
            Some("none") => Ok(None),
            Some("function") => {
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include `function`".to_string(),
                        )
                    })?;
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include function.name".to_string(),
                        )
                    })?;
                Ok(Some(json!({ "tool": { "name": name } })))
            }
            Some(other) => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice type `{other}` for aws_bedrock Converse"
            ))),
            None => Err(ProviderError::InvalidRequest(
                "tool_choice object must include `type`".to_string(),
            )),
        },
        Value::Null => Ok(None),
        _ => Err(ProviderError::InvalidRequest(
            "tool_choice must be a string or object for aws_bedrock Converse".to_string(),
        )),
    }
}

pub(super) fn map_anthropic_tool_choice(value: &Value) -> Result<Option<Value>, ProviderError> {
    match value {
        Value::String(choice) => match choice.as_str() {
            "auto" => Ok(Some(json!({ "type": "auto" }))),
            "required" => Ok(Some(json!({ "type": "any" }))),
            "none" => Ok(None),
            other => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice `{other}` for aws_bedrock Anthropic Claude Messages"
            ))),
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => Ok(Some(json!({ "type": "auto" }))),
            Some("required") => Ok(Some(json!({ "type": "any" }))),
            Some("none") => Ok(None),
            Some("function") => {
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include `function`".to_string(),
                        )
                    })?;
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include function.name".to_string(),
                        )
                    })?;
                Ok(Some(json!({ "type": "tool", "name": name })))
            }
            Some("tool") => {
                let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool tool_choice must include `name`".to_string(),
                    )
                })?;
                Ok(Some(json!({ "type": "tool", "name": name })))
            }
            Some(other) => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice type `{other}` for aws_bedrock Anthropic Claude Messages"
            ))),
            None => Err(ProviderError::InvalidRequest(
                "tool_choice object must include `type`".to_string(),
            )),
        },
        Value::Null => Ok(None),
        _ => Err(ProviderError::InvalidRequest(
            "tool_choice must be a string or object for aws_bedrock Anthropic Claude Messages"
                .to_string(),
        )),
    }
}
