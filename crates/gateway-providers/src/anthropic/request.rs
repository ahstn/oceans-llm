use std::collections::BTreeMap;

use gateway_core::protocol::anthropic::anthropic_reasoning_blocks;
use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderError, ProviderRequestContext};
use serde_json::{Map, Value, json};

use super::error::AnthropicAdapterError;
use super::thinking::{
    apply_anthropic_thinking_compatibility, validate_anthropic_sampling_fields,
    validate_anthropic_tool_choice,
};

#[derive(Debug, Clone)]
pub struct AnthropicRequestOptions<'a> {
    pub include_model: bool,
    pub anthropic_version_body: Option<&'a str>,
    pub default_max_tokens: Option<i64>,
    pub default_headers: Option<&'a BTreeMap<String, String>>,
}

impl Default for AnthropicRequestOptions<'_> {
    fn default() -> Self {
        Self {
            include_model: true,
            anthropic_version_body: None,
            default_max_tokens: Some(4096),
            default_headers: None,
        }
    }
}

pub fn map_anthropic_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
    options: &AnthropicRequestOptions<'_>,
) -> Result<Value, ProviderError> {
    let mut body: Map<String, Value> = request
        .extra
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    body.remove("model");
    body.remove("messages");
    body.remove("stream");

    let (messages, instructions) = map_chat_messages(&request.messages)?;

    if messages.is_empty() {
        return Err(AnthropicAdapterError::EmptyMessages.into());
    }

    let max_completion_tokens = body.remove("max_completion_tokens");
    let max_tokens = body.get("max_tokens").cloned();
    match (max_completion_tokens, max_tokens) {
        (Some(completion), Some(tokens)) => {
            if completion != tokens {
                return Err(AnthropicAdapterError::ConflictingMaxTokens.into());
            }
        }
        (Some(completion), None) => {
            body.insert("max_tokens".to_string(), completion);
        }
        (None, Some(_)) => {}
        (None, None) => {
            let max_tokens = options.default_max_tokens.unwrap_or(4096);
            body.insert("max_tokens".to_string(), Value::Number(max_tokens.into()));
        }
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
    if !has_anthropic_beta(context, options, "context-management-2025-06-27") {
        body.remove("context_management");
    }

    if options.include_model {
        body.insert(
            "model".to_string(),
            Value::String(context.upstream_model.clone()),
        );
    } else {
        body.remove("model");
    }

    if let Some(version) = options.anthropic_version_body {
        body.insert(
            "anthropic_version".to_string(),
            Value::String(version.to_string()),
        );
    }

    body.insert("messages".to_string(), Value::Array(messages));
    body.insert("stream".to_string(), Value::Bool(stream));

    convert_openai_tools_for_anthropic(&mut body, &context.upstream_model)?;
    apply_anthropic_thinking_compatibility(&mut body, &context.upstream_model)?;
    validate_anthropic_sampling_fields(&mut body, &context.upstream_model)?;

    Ok(Value::Object(body))
}

fn map_chat_messages(
    messages: &[CoreChatMessage],
) -> Result<(Vec<Value>, Vec<String>), ProviderError> {
    let mut mapped_messages = Vec::with_capacity(messages.len());
    let mut instructions = Vec::new();

    for message in messages {
        match message.role.as_str() {
            "system" | "developer" => {
                instructions.push(message_content_as_text(&message.content)?);
            }
            "user" => {
                let content = map_anthropic_message_content(message, false)?;
                mapped_messages.push(json!({
                    "role": "user",
                    "content": content
                }));
            }
            "assistant" => {
                let content = map_anthropic_message_content(message, true)?;
                mapped_messages.push(json!({
                    "role": "assistant",
                    "content": content
                }));
            }
            "tool" => {
                mapped_messages.push(json!({
                    "role": "user",
                    "content": [map_openai_tool_result(message)?]
                }));
            }
            other => {
                return Err(AnthropicAdapterError::UnsupportedMessageRole {
                    role: other.to_string(),
                }
                .into());
            }
        }
    }

    Ok((mapped_messages, instructions))
}

fn map_anthropic_message_content(
    message: &CoreChatMessage,
    is_assistant: bool,
) -> Result<Value, ProviderError> {
    let mut content = match map_anthropic_content(&message.content)? {
        Value::String(text) if text.is_empty() => Vec::new(),
        Value::String(text) => vec![json!({"type": "text", "text": text})],
        Value::Array(items) => items,
        _ => Vec::new(),
    };

    if is_assistant {
        let has_existing_thinking = content.iter().any(|block| {
            matches!(
                block.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            )
        });

        if !has_existing_thinking
            && let Some(provider_metadata) = message.extra.get("provider_metadata")
        {
            let mut reasoning_blocks =
                anthropic_reasoning_blocks(Some(provider_metadata), "anthropic_messages");
            if reasoning_blocks.is_empty() {
                reasoning_blocks = anthropic_reasoning_blocks(
                    Some(provider_metadata),
                    "anthropic_messages_stream",
                );
            }
            let mut extracted = Vec::with_capacity(reasoning_blocks.len());
            for block in reasoning_blocks {
                if matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                ) {
                    extracted.push(block.clone());
                }
            }
            if !extracted.is_empty() {
                extracted.append(&mut content);
                content = extracted;
            }
        }

        for tool_use in map_openai_assistant_tool_uses(message)? {
            let id = tool_use.get("id").and_then(Value::as_str);
            let is_duplicate = content.iter().any(|block| {
                block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && block.get("id").and_then(Value::as_str) == id
            });
            if !is_duplicate {
                content.push(tool_use);
            }
        }
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

fn map_anthropic_content(content: &Value) -> Result<Value, AnthropicAdapterError> {
    match content {
        Value::Null => Ok(Value::String(String::new())),
        Value::String(val) => Ok(Value::String(val.clone())),
        Value::Array(items) => {
            let mut mapped = Vec::with_capacity(items.len());
            for item in items {
                let object = item
                    .as_object()
                    .ok_or(AnthropicAdapterError::InvalidContentEntry)?;
                let kind = object
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(AnthropicAdapterError::MissingContentType)?;
                match kind {
                    "text" | "input_text" => {
                        let text = object
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or(AnthropicAdapterError::MissingContentText)?;
                        if kind == "text" {
                            mapped.push(Value::Object(object.clone()));
                        } else {
                            mapped.push(json!({"type": "text", "text": text}));
                        }
                    }
                    "image" | "image_url" | "input_image" => {
                        let image_block = map_anthropic_image_content_block(object)?;
                        mapped.push(image_block);
                    }
                    _ => mapped.push(Value::Object(object.clone())),
                }
            }
            Ok(Value::Array(mapped))
        }
        _ => Err(AnthropicAdapterError::InvalidMessageContent),
    }
}

fn map_anthropic_image_content_block(
    object: &Map<String, Value>,
) -> Result<Value, AnthropicAdapterError> {
    if object.get("type").and_then(Value::as_str) == Some("image")
        && let Some(source) = object.get("source").and_then(Value::as_object)
    {
        validate_anthropic_base64_image_source(source)?;
        return Ok(Value::Object(object.clone()));
    }

    let image_url = object
        .get("image_url")
        .or_else(|| object.get("source"))
        .ok_or(AnthropicAdapterError::MissingImageSource)?;

    match image_url {
        Value::Object(image_object) => {
            if image_object.get("type").and_then(Value::as_str) == Some("base64") {
                validate_anthropic_base64_image_source(image_object)?;
                return Ok(json!({ "type": "image", "source": image_object }));
            }
            if let Some(source) = image_object.get("source").and_then(Value::as_object)
                && source.get("type").and_then(Value::as_str) == Some("base64")
            {
                validate_anthropic_base64_image_source(source)?;
                return Ok(json!({ "type": "image", "source": source }));
            }

            let url = image_object
                .get("url")
                .and_then(Value::as_str)
                .ok_or(AnthropicAdapterError::MissingImageUrlString)?;
            map_anthropic_data_url_image(url, image_object)
        }
        Value::String(url) => map_anthropic_data_url_image(url, object),
        _ => Err(AnthropicAdapterError::InvalidImageUrl),
    }
}

fn validate_anthropic_base64_image_source(
    source: &Map<String, Value>,
) -> Result<(), AnthropicAdapterError> {
    let media_type = source
        .get("media_type")
        .and_then(Value::as_str)
        .ok_or(AnthropicAdapterError::MissingImageMediaType)?;
    if !matches!(
        media_type,
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    ) {
        return Err(AnthropicAdapterError::UnsupportedImageMediaType {
            media_type: media_type.to_string(),
        });
    }
    if source.get("data").and_then(Value::as_str).is_none() {
        return Err(AnthropicAdapterError::MissingImageData);
    }
    Ok(())
}

fn map_anthropic_data_url_image(
    url: &str,
    metadata: &Map<String, Value>,
) -> Result<Value, AnthropicAdapterError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        return Err(AnthropicAdapterError::RemoteImageUrlNotSupported);
    }
    let Some((media_type, data)) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
    else {
        return Err(AnthropicAdapterError::RemoteImageUrlNotSupported);
    };
    let media_type = metadata
        .get("mime_type")
        .and_then(Value::as_str)
        .unwrap_or(media_type);

    match media_type {
        "image/jpeg" | "image/png" | "image/webp" | "image/gif" => Ok(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data
            }
        })),
        other => Err(AnthropicAdapterError::UnsupportedImageMediaType {
            media_type: other.to_string(),
        }),
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

fn map_openai_assistant_tool_uses(
    message: &CoreChatMessage,
) -> Result<Vec<Value>, AnthropicAdapterError> {
    let Some(tool_calls) = message.extra.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let calls = tool_calls
        .as_array()
        .ok_or(AnthropicAdapterError::InvalidToolCalls)?;
    let mut mapped = Vec::with_capacity(calls.len());
    for call in calls {
        let object = call
            .as_object()
            .ok_or(AnthropicAdapterError::InvalidToolCallEntry)?;
        if object.get("type").and_then(Value::as_str) != Some("function") {
            return Err(AnthropicAdapterError::UnsupportedToolCallType);
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .ok_or(AnthropicAdapterError::MissingToolCallId)?;
        let function = object
            .get("function")
            .and_then(Value::as_object)
            .ok_or(AnthropicAdapterError::MissingToolCallFunction)?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or(AnthropicAdapterError::MissingToolCallFunctionName)?;
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

pub fn parse_openai_tool_arguments(
    function: &Map<String, Value>,
) -> Result<Value, AnthropicAdapterError> {
    let Some(arguments) = function.get("arguments") else {
        return Ok(Value::Object(Map::new()));
    };
    let arguments =
        arguments
            .as_str()
            .ok_or_else(|| AnthropicAdapterError::InvalidToolArguments {
                reason: "arguments must be a string".to_string(),
            })?;
    let value = serde_json::from_str::<Value>(arguments).map_err(|err| {
        AnthropicAdapterError::InvalidToolArguments {
            reason: format!("arguments must be valid JSON: {err}"),
        }
    })?;
    if !value.is_object() {
        return Err(AnthropicAdapterError::InvalidToolArguments {
            reason: "arguments must decode to a JSON object".to_string(),
        });
    }
    Ok(value)
}

fn map_openai_tool_result(message: &CoreChatMessage) -> Result<Value, ProviderError> {
    let tool_use_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or(AnthropicAdapterError::MissingToolCallIdInToolMessage)?;
    Ok(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": message_content_as_text(&message.content)?
    }))
}

pub fn message_content_as_text(content: &Value) -> Result<String, AnthropicAdapterError> {
    match content {
        Value::Null => Ok(String::new()),
        Value::String(val) => Ok(val.clone()),
        Value::Array(items) => {
            let mut pieces = Vec::new();
            for item in items {
                let object = item
                    .as_object()
                    .ok_or(AnthropicAdapterError::InvalidContentEntry)?;
                let kind = object
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(AnthropicAdapterError::MissingContentType)?;
                if matches!(kind, "text" | "input_text") {
                    let text = object
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or(AnthropicAdapterError::MissingContentText)?;
                    pieces.push(text);
                }
            }
            Ok(pieces.join("\n\n"))
        }
        _ => Err(AnthropicAdapterError::InvalidMessageContent),
    }
}

pub fn convert_openai_tools_for_anthropic(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    if let Some(tools) = body.get_mut("tools") {
        convert_openai_function_tools(tools)?;
    }
    if let Some(tool_choice) = body.get_mut("tool_choice") {
        validate_anthropic_tool_choice(tool_choice, upstream_model)?;
        convert_openai_tool_choice(tool_choice)?;
    }
    Ok(())
}

fn convert_openai_function_tools(value: &mut Value) -> Result<(), AnthropicAdapterError> {
    let Some(tools) = value.as_array_mut() else {
        return Err(AnthropicAdapterError::InvalidToolsArray);
    };
    for tool in tools {
        let Some(object) = tool.as_object_mut() else {
            return Err(AnthropicAdapterError::InvalidToolEntry);
        };
        if object.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let function = object
            .remove("function")
            .ok_or(AnthropicAdapterError::MissingToolFunction)?;
        let function = function
            .as_object()
            .ok_or(AnthropicAdapterError::InvalidToolFunction)?;
        let name = function
            .get("name")
            .cloned()
            .ok_or(AnthropicAdapterError::MissingFunctionName)?;
        let input_schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
        object.remove("type");
        object.insert("name".to_string(), name);
        if let Some(desc) = function.get("description").cloned() {
            object.insert("description".to_string(), desc);
        }
        object.insert("input_schema".to_string(), input_schema);
    }
    Ok(())
}

fn convert_openai_tool_choice(value: &mut Value) -> Result<(), AnthropicAdapterError> {
    match value {
        Value::String(choice) if choice == "required" => {
            *value = json!({"type": "any"});
        }
        Value::String(choice) if choice == "auto" => {
            *value = json!({"type": choice});
        }
        Value::String(choice) if choice == "none" => {
            *value = json!({"type": choice});
        }
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            let name = object
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .ok_or(AnthropicAdapterError::MissingToolChoiceFunctionName)?
                .to_string();
            *value = json!({"type": "tool", "name": name});
        }
        Value::Object(_) | Value::Null => {}
        _ => {
            return Err(AnthropicAdapterError::UnsupportedToolChoice);
        }
    }
    Ok(())
}

fn has_anthropic_beta(
    context: &ProviderRequestContext,
    options: &AnthropicRequestOptions<'_>,
    beta: &str,
) -> bool {
    let in_extra = context.extra_headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("anthropic-beta")
            && value
                .as_str()
                .is_some_and(|betas| betas.split(',').any(|cand| cand.trim() == beta))
    });
    if in_extra {
        return true;
    }
    if let Some(default_headers) = options.default_headers {
        default_headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("anthropic-beta")
                && value.split(',').any(|cand| cand.trim() == beta)
        })
    } else {
        false
    }
}

fn merge_object_overrides(target: &mut Map<String, Value>, overrides: &Map<String, Value>) {
    for (key, value) in overrides {
        target.insert(key.clone(), value.clone());
    }
}
