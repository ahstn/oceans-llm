use serde_json::{Map, Value, json};

use super::error::VertexAdapterError;

pub(super) fn reject_google_strict_tools(tools: Option<&Value>) -> Result<(), VertexAdapterError> {
    if tools.and_then(Value::as_array).is_some_and(|tools| {
        tools.iter().any(|tool| {
            tool.get("function").unwrap_or(tool).get("strict") == Some(&Value::Bool(true))
        })
    }) {
        return Err(VertexAdapterError::UnsupportedStrictControl);
    }
    Ok(())
}

pub(super) fn convert_openai_tools_for_google(
    body: &mut Map<String, Value>,
) -> Result<(), VertexAdapterError> {
    // Validate before `tool_choice: none` can discard declarations or overwrite native controls.
    reject_google_strict_tools(body.get("tools"))?;
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
            Some("none") => mode("NONE"),
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
                return Err(VertexAdapterError::InvalidToolEntry);
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
