use super::*;

mod content;
mod inference;
mod thinking;
mod tools;

use content::*;
use inference::*;
use thinking::*;
use tools::*;
const OPENAI_HOSTED_IMAGE_GENERATION_TOOL_TYPE: &str = "image_generation";

pub(super) fn map_chat_request_to_converse(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    let mut system = Vec::new();
    let mut messages = Vec::new();

    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                system.push(json!({ "text": message_content_as_text(&message.content)? }));
            }
            "user" => {
                messages.push(json!({
                    "role": "user",
                    "content": map_bedrock_content_blocks(&message.content)?
                }));
            }
            "assistant" => {
                let mut content = map_bedrock_content_blocks(&message.content)?;
                content.extend(map_assistant_tool_uses(message)?);
                messages.push(json!({
                    "role": "assistant",
                    "content": content
                }));
            }
            "tool" => {
                messages.push(json!({
                    "role": "user",
                    "content": [map_tool_result(message)?]
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for aws_bedrock Converse mapping"
                )));
            }
        }
    }

    if messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Converse requires at least one user, assistant, or tool message"
                .to_string(),
        ));
    }

    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }
    body.insert("messages".to_string(), Value::Array(messages));

    let mut passthrough = request.extra.clone();
    passthrough.remove("model");
    passthrough.remove("messages");
    passthrough.remove("stream");

    let inference_config = extract_inference_config(&mut passthrough)?;
    if !inference_config.is_empty() {
        body.insert(
            "inferenceConfig".to_string(),
            Value::Object(inference_config),
        );
    }

    if let Some(tool_config) = extract_tool_config(&mut passthrough)? {
        body.insert("toolConfig".to_string(), tool_config);
    }

    if let Some(additional) = passthrough.remove("additionalModelRequestFields") {
        body.insert("additionalModelRequestFields".to_string(), additional);
    }
    if let Some(additional) = passthrough.remove("additional_model_request_fields") {
        body.insert("additionalModelRequestFields".to_string(), additional);
    }
    merge_object_overrides(&mut body, &context.extra_body);
    apply_converse_anthropic_thinking_compatibility(
        &mut body,
        &mut passthrough,
        &context.upstream_model,
    )?;
    validate_converse_anthropic_sampling_fields(
        &mut body,
        &mut passthrough,
        &context.upstream_model,
    )?;

    reject_openai_only_fields(&passthrough)?;
    reject_unknown_converse_fields(&passthrough)?;
    Ok(Value::Object(body))
}

pub(super) fn map_openai_chat_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let mut request = request.clone();
    request.stream = stream;
    let wire_request = core_chat_request_to_openai(&request);
    let mut body = serde_json::to_value(wire_request)
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "model".to_string(),
            Value::String(context.upstream_model.clone()),
        );
        for (key, value) in &context.extra_body {
            object.insert(key.clone(), value.clone());
        }
    }
    Ok(body)
}

pub(super) fn map_openai_responses_request(
    request: &CoreResponsesRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let mut request = request.clone();
    request.stream = stream;
    let wire_request = core_responses_request_to_openai(&request);
    let mut body = serde_json::to_value(wire_request)
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "model".to_string(),
            Value::String(context.upstream_model.clone()),
        );
        for (key, value) in &context.extra_body {
            object.insert(key.clone(), value.clone());
        }
        enforce_bedrock_responses_hosted_tool_compatibility(object, context)?;
    }
    Ok(body)
}

fn enforce_bedrock_responses_hosted_tool_compatibility(
    object: &mut Map<String, Value>,
    context: &ProviderRequestContext,
) -> Result<(), ProviderError> {
    let tools_contain_image_generation = object.get("tools").is_some_and(|tools| {
        tools_contain_tool_type(tools, OPENAI_HOSTED_IMAGE_GENERATION_TOOL_TYPE)
    });
    let tool_choice_requires_image_generation =
        object.get("tool_choice").is_some_and(|tool_choice| {
            tool_choice_requires_tool_type(
                tool_choice,
                object.get("tools"),
                OPENAI_HOSTED_IMAGE_GENERATION_TOOL_TYPE,
            )
        });
    let tools_require_image_generation = object.get("tools").is_some_and(|tools| {
        tools_require_tool_type(tools, OPENAI_HOSTED_IMAGE_GENERATION_TOOL_TYPE)
    });

    if tool_choice_requires_image_generation || tools_require_image_generation {
        tracing::warn!(
            request_id = %context.request_id,
            provider_key = %context.provider_key,
            model_key = %context.model_key,
            upstream_model = %context.upstream_model,
            reason = "unsupported_hosted_tool_required",
            "aws_bedrock route does not support requested OpenAI hosted Responses tool"
        );
        return Err(ProviderError::InvalidRequest(format!(
            "Oceans aws_bedrock provider `{}` route to upstream model `{}` does not support the OpenAI hosted image_generation Responses tool; choose an image-generation-capable route or remove the explicit image_generation tool choice",
            context.provider_key, context.upstream_model
        )));
    }

    if !tools_contain_image_generation {
        return Ok(());
    }

    let removed_tool_count = object
        .get_mut("tools")
        .map(|tools| strip_tool_type_from_tools(tools, OPENAI_HOSTED_IMAGE_GENERATION_TOOL_TYPE))
        .unwrap_or_default();
    if object
        .get("tools")
        .is_some_and(|tools| matches!(tools, Value::Array(items) if items.is_empty()))
    {
        object.remove("tools");
        if object
            .get("tool_choice")
            .is_some_and(tool_choice_is_none_or_auto)
        {
            object.remove("tool_choice");
        }
    }

    if removed_tool_count > 0 {
        tracing::info!(
            request_id = %context.request_id,
            provider_key = %context.provider_key,
            model_key = %context.model_key,
            upstream_model = %context.upstream_model,
            removed_tool_count,
            reason = "unsupported_hosted_tool_stripped",
            "stripped unsupported OpenAI hosted Responses tool for aws_bedrock"
        );
    }

    Ok(())
}

fn tools_contain_tool_type(tools: &Value, tool_type: &str) -> bool {
    match tools {
        Value::Array(items) => items.iter().any(|tool| tool_is_type(tool, tool_type)),
        tool => tool_is_type(tool, tool_type),
    }
}

fn tools_require_tool_type(tools: &Value, tool_type: &str) -> bool {
    match tools {
        Value::Array(items) => items.iter().any(|tool| tool_requires_type(tool, tool_type)),
        tool => tool_requires_type(tool, tool_type),
    }
}

fn strip_tool_type_from_tools(tools: &mut Value, tool_type: &str) -> usize {
    let Value::Array(items) = tools else {
        return 0;
    };
    let original_len = items.len();
    items.retain(|tool| !tool_is_type(tool, tool_type));
    original_len - items.len()
}

fn tool_choice_requires_tool_type(
    tool_choice: &Value,
    tools: Option<&Value>,
    tool_type: &str,
) -> bool {
    if tool_choice_selects_type(tool_choice, tool_type) {
        return true;
    }

    tool_choice_is_required(tool_choice) && tools_have_only_tool_type(tools, tool_type)
}

fn tool_requires_type(tool: &Value, tool_type: &str) -> bool {
    tool_is_type(tool, tool_type)
        && tool
            .as_object()
            .and_then(|object| object.get("action"))
            .and_then(Value::as_str)
            .is_some_and(|action| action != "auto")
}

fn tool_choice_selects_type(tool_choice: &Value, tool_type: &str) -> bool {
    match tool_choice {
        Value::String(value) => value == tool_type,
        Value::Object(object) => {
            object.get("type").and_then(Value::as_str) == Some(tool_type)
                || object.get("name").and_then(Value::as_str) == Some(tool_type)
        }
        _ => false,
    }
}

fn tool_choice_is_required(tool_choice: &Value) -> bool {
    match tool_choice {
        Value::String(value) => value == "required",
        Value::Object(object) => {
            object.get("type").and_then(Value::as_str) == Some("required")
                || object.get("mode").and_then(Value::as_str) == Some("required")
        }
        _ => false,
    }
}

fn tools_have_only_tool_type(tools: Option<&Value>, tool_type: &str) -> bool {
    let Some(Value::Array(items)) = tools else {
        return false;
    };
    !items.is_empty() && items.iter().all(|tool| tool_is_type(tool, tool_type))
}

fn tool_is_type(tool: &Value, tool_type: &str) -> bool {
    tool.as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        == Some(tool_type)
}

pub(super) fn is_anthropic_claude_model(upstream_model: &str) -> bool {
    upstream_model
        .to_ascii_lowercase()
        .contains("anthropic.claude")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnthropicMessagesTarget {
    RuntimeInvoke,
    MantleMessages,
}

pub(super) fn map_chat_request_to_anthropic_messages(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    target: AnthropicMessagesTarget,
) -> Result<Value, ProviderError> {
    if request.stream && target == AnthropicMessagesTarget::RuntimeInvoke {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages streaming is gated until native InvokeModelWithResponseStream mapping lands"
                .to_string(),
        ));
    }

    let mut body = Map::new();
    match target {
        AnthropicMessagesTarget::RuntimeInvoke => {
            body.insert(
                "anthropic_version".to_string(),
                Value::String("bedrock-2023-05-31".to_string()),
            );
        }
        AnthropicMessagesTarget::MantleMessages => {
            body.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
            if request.stream {
                body.insert("stream".to_string(), Value::Bool(true));
            }
        }
    }

    let mut system = Vec::new();
    let mut messages = Vec::new();

    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                let text = message_content_as_text(&message.content)?;
                if !text.is_empty() {
                    system.push(text);
                }
            }
            "user" => {
                messages.push(json!({
                    "role": "user",
                    "content": map_anthropic_content_blocks(&message.content)?
                }));
            }
            "assistant" => {
                let mut content = map_anthropic_content_blocks(&message.content)?;
                content.extend(map_anthropic_assistant_tool_uses(message)?);
                messages.push(json!({
                    "role": "assistant",
                    "content": content
                }));
            }
            "tool" => {
                messages.push(json!({
                    "role": "user",
                    "content": [map_anthropic_tool_result(message)?]
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for aws_bedrock Anthropic Claude Messages mapping"
                )));
            }
        }
    }

    if messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages requires at least one user, assistant, or tool message"
                .to_string(),
        ));
    }

    if !system.is_empty() {
        body.insert("system".to_string(), Value::String(system.join("\n")));
    }
    body.insert("messages".to_string(), Value::Array(messages));

    let mut passthrough = request.extra.clone();
    passthrough.remove("model");
    passthrough.remove("messages");
    passthrough.remove("stream");

    extract_anthropic_inference_fields(&mut body, &mut passthrough)?;
    if let Some(tools) = extract_anthropic_tools(&mut passthrough)? {
        body.extend(tools);
    }
    extract_anthropic_passthrough_fields(&mut body, &mut passthrough);
    merge_object_overrides(&mut body, &context.extra_body);
    apply_anthropic_thinking_compatibility(&mut body, &mut passthrough, &context.upstream_model)?;
    reject_openai_only_fields(&passthrough)?;
    reject_unknown_anthropic_messages_fields(&passthrough)?;
    validate_anthropic_sampling_fields(&mut body, &context.upstream_model)?;

    if !body.contains_key("max_tokens") {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages requires `max_tokens` or `max_completion_tokens`"
                .to_string(),
        ));
    }

    Ok(Value::Object(body))
}

pub(super) fn merge_object_overrides(
    base: &mut Map<String, Value>,
    overrides: &Map<String, Value>,
) {
    for (key, value) in overrides {
        match (base.get_mut(key), value) {
            (Some(base_value), Value::Object(override_object)) => {
                if let Some(base_object) = base_value.as_object_mut() {
                    merge_object_overrides(base_object, override_object);
                } else {
                    *base_value = Value::Object(override_object.clone());
                }
            }
            (Some(base_value), override_value) => {
                *base_value = override_value.clone();
            }
            (None, override_value) => {
                base.insert(key.clone(), override_value.clone());
            }
        }
    }
}
