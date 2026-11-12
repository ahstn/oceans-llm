use std::collections::{BTreeMap, btree_map::Entry};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use gateway_core::ProviderError;
use serde_json::{Map, Value, json};

#[derive(Debug, Default)]
pub(super) struct ConverseReplayStream {
    native_blocks: BTreeMap<u64, Value>,
    tool_inputs: BTreeMap<u64, String>,
    redacted_bytes: BTreeMap<u64, Vec<u8>>,
}

impl ConverseReplayStream {
    pub(super) fn record_start(
        &mut self,
        block_index: u64,
        start: Option<&Value>,
    ) -> Result<(), ProviderError> {
        if let Some(start) = start {
            validate_native_containers(start)?;
        }
        if let Some(start) = start.and_then(Value::as_object)
            && !start.is_empty()
        {
            self.native_blocks
                .insert(block_index, Value::Object(start.clone()));
            self.redacted_bytes.remove(&block_index);
        }
        Ok(())
    }

    pub(super) fn start_tool_input(&mut self, block_index: u64) {
        self.tool_inputs.insert(block_index, String::new());
    }

    pub(super) fn record_delta(
        &mut self,
        payload: &Value,
        delta: &Value,
    ) -> Result<(), ProviderError> {
        validate_native_containers(delta)?;
        let index = payload
            .get("contentBlockIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let block = self
            .native_blocks
            .entry(index)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| malformed_native_container("content block"))?;
        for (key, value) in delta.as_object().into_iter().flatten() {
            match key.as_str() {
                "text" => {
                    append_native_string_field(block, key, native_delta_string(value)?)?;
                }
                "toolUse" => {
                    let tool = native_object_field(block, key)?;
                    for (key, value) in value.as_object().into_iter().flatten() {
                        if key == "input" {
                            self.tool_inputs
                                .get_mut(&index)
                                .ok_or_else(|| {
                                    ProviderError::Transport(format!(
                                        "aws_bedrock tool delta references unstarted content block `{index}`"
                                    ))
                                })?
                                .push_str(native_delta_string(value)?);
                        } else {
                            tool.insert(key.clone(), value.clone());
                        }
                    }
                }
                "reasoningContent" => {
                    let reasoning = native_object_field(block, key)?;
                    for (key, value) in value.as_object().into_iter().flatten() {
                        match key.as_str() {
                            "text" | "signature" => {
                                let text = native_object_field(reasoning, "reasoningText")?;
                                append_native_string_field(text, key, native_delta_string(value)?)?;
                            }
                            "reasoningText" => {
                                let text = native_object_field(reasoning, key)?;
                                for (key, value) in value.as_object().into_iter().flatten() {
                                    if matches!(key.as_str(), "text" | "signature") {
                                        append_native_string_field(
                                            text,
                                            key,
                                            native_delta_string(value)?,
                                        )?;
                                    } else {
                                        text.insert(key.clone(), value.clone());
                                    }
                                }
                            }
                            "redactedContent" | "data" => match self.redacted_bytes.entry(index) {
                                Entry::Occupied(mut entry) => {
                                    append_redacted_bytes(
                                        entry.get_mut(),
                                        native_delta_string(value)?,
                                    )?;
                                }
                                Entry::Vacant(entry) => {
                                    let previous = reasoning
                                        .get("redactedContent")
                                        .or_else(|| reasoning.get("data"))
                                        .map(native_delta_string)
                                        .transpose()?
                                        .unwrap_or_default();
                                    let mut bytes = Vec::new();
                                    append_redacted_bytes(&mut bytes, previous)?;
                                    append_redacted_bytes(&mut bytes, native_delta_string(value)?)?;
                                    entry.insert(bytes);
                                }
                            },
                            _ => {
                                reasoning.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }
                _ => {
                    block.insert(key.clone(), value.clone());
                }
            }
        }
        Ok(())
    }

    pub(super) fn attach_terminal(
        &mut self,
        delta: &mut Value,
        finish_reason: &str,
        mut replayable: bool,
    ) -> Option<String> {
        let mut replay_error = None;
        let tool_inputs = std::mem::take(&mut self.tool_inputs);
        if replayable {
            for (index, input) in tool_inputs {
                match serde_json::from_str::<Value>(&input) {
                    Ok(input) => {
                        let tool = self
                            .native_blocks
                            .get_mut(&index)
                            .and_then(|block| block.get_mut("toolUse"))
                            .and_then(Value::as_object_mut);
                        if let Some(tool) = tool {
                            tool.insert("input".to_string(), input);
                        } else {
                            replayable = false;
                            replay_error = Some(format!(
                                "aws_bedrock streamed tool content block `{index}` must contain a toolUse object"
                            ));
                            break;
                        }
                    }
                    Err(error) => {
                        replayable = false;
                        if !matches!(finish_reason, "length" | "content_filter") {
                            replay_error =
                                Some(format!("invalid aws_bedrock streamed tool input: {error}"));
                        }
                        break;
                    }
                }
            }
        }
        let mut native_blocks = std::mem::take(&mut self.native_blocks);
        for (index, bytes) in std::mem::take(&mut self.redacted_bytes) {
            if let Some(reasoning) = native_blocks
                .get_mut(&index)
                .and_then(|block| block.get_mut("reasoningContent"))
                .and_then(Value::as_object_mut)
            {
                reasoning.insert(
                    "redactedContent".to_string(),
                    json!(BASE64_STANDARD.encode(bytes)),
                );
            }
        }
        let reasoning = native_blocks
            .values()
            .filter_map(|block| block.get("reasoningContent"))
            .filter_map(normalize_bedrock_reasoning_content)
            .collect::<Vec<_>>();
        let mut reasoning_metadata = json!({"source": "bedrock_converse_stream"});
        reasoning_metadata["blocks"] = Value::Array(reasoning);
        delta["provider_metadata"]["aws_bedrock"]["reasoning"] = reasoning_metadata;
        if replayable {
            // Ordered inline continuation requires one complete replay sequence.
            let mut native_content = json!({"source": "bedrock_converse"});
            native_content["blocks"] = Value::Array(native_blocks.into_values().collect());
            delta["provider_metadata"]["aws_bedrock"]["native_content"] = native_content;
        }
        replay_error
    }
}

fn append_redacted_bytes(bytes: &mut Vec<u8>, part: &str) -> Result<(), ProviderError> {
    let previous_len = bytes.len();
    BASE64_STANDARD.decode_vec(part, bytes).map_err(|error| {
        // decode_vec can resize and partially write before rejecting a fragment.
        bytes.truncate(previous_len);
        ProviderError::Transport(format!("invalid Bedrock redacted reasoning: {error}"))
    })
}

fn malformed_native_container(name: &str) -> ProviderError {
    ProviderError::Transport(format!("aws_bedrock native `{name}` must be an object"))
}

fn validate_native_containers(value: &Value) -> Result<(), ProviderError> {
    let object = value
        .as_object()
        .ok_or_else(|| malformed_native_container("content block"))?;
    if let Some(tool) = object.get("toolUse")
        && !tool.is_object()
    {
        return Err(malformed_native_container("toolUse"));
    }
    if let Some(reasoning) = object.get("reasoningContent") {
        let reasoning = reasoning
            .as_object()
            .ok_or_else(|| malformed_native_container("reasoningContent"))?;
        if let Some(text) = reasoning.get("reasoningText")
            && !text.is_object()
        {
            return Err(malformed_native_container("reasoningText"));
        }
    }
    Ok(())
}

fn native_object_field<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, ProviderError> {
    if !object.contains_key(key) {
        object.insert(key.to_string(), Value::Object(Map::new()));
    }
    object
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| malformed_native_container(key))
}

fn native_delta_string(value: &Value) -> Result<&str, ProviderError> {
    value.as_str().ok_or_else(|| {
        ProviderError::Transport("aws_bedrock native string delta must be a string".to_string())
    })
}

fn append_native_string_field(
    object: &mut Map<String, Value>,
    key: &str,
    part: &str,
) -> Result<(), ProviderError> {
    match object.get_mut(key) {
        Some(Value::String(text)) => text.push_str(part),
        Some(value @ Value::Null) => *value = Value::String(part.to_string()),
        Some(_) => {
            return Err(ProviderError::Transport(
                "aws_bedrock native string delta does not match its content block".to_string(),
            ));
        }
        None => {
            object.insert(key.to_string(), Value::String(part.to_string()));
        }
    }
    Ok(())
}

pub(super) fn extract_bedrock_reasoning_blocks(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| block.get("reasoningContent"))
        .filter_map(normalize_bedrock_reasoning_content)
        .collect()
}

pub(super) fn normalize_bedrock_reasoning_content(reasoning: &Value) -> Option<Value> {
    if let Some(reasoning_text) = reasoning.get("reasoningText").and_then(Value::as_object) {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("reasoning_text".to_string()),
        );
        normalized.insert(
            "text".to_string(),
            reasoning_text
                .get("text")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
        );
        if let Some(signature) = reasoning_text.get("signature").cloned() {
            normalized.insert("signature".to_string(), signature);
        }
        return Some(Value::Object(normalized));
    }

    if let Some(text) = reasoning.get("text").cloned() {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("reasoning_text".to_string()),
        );
        normalized.insert("text".to_string(), text);
        if let Some(signature) = reasoning.get("signature").cloned() {
            normalized.insert("signature".to_string(), signature);
        }
        return Some(Value::Object(normalized));
    }

    if let Some(signature) = reasoning.get("signature").cloned() {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("reasoning_signature".to_string()),
        );
        normalized.insert("signature".to_string(), signature);
        return Some(Value::Object(normalized));
    }

    if let Some(data) = reasoning
        .get("redactedContent")
        .or_else(|| reasoning.get("data"))
        .cloned()
    {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("redacted_reasoning".to_string()),
        );
        normalized.insert("data".to_string(), data);
        return Some(Value::Object(normalized));
    }

    if let Some(redacted) = reasoning.get("redactedReasoning") {
        let mut normalized = Map::new();
        normalized.insert(
            "type".to_string(),
            Value::String("redacted_reasoning".to_string()),
        );
        if let Some(data) = redacted.get("data").cloned() {
            normalized.insert("data".to_string(), data);
        }
        return Some(Value::Object(normalized));
    }

    None
}

pub(super) fn converse_content(
    message: &gateway_core::CoreChatMessage,
    public_blocks: &[Value],
) -> Result<Option<Vec<Value>>, ProviderError> {
    let Some(namespace) = message
        .extra
        .get("provider_metadata")
        .and_then(|metadata| metadata.get("aws_bedrock"))
    else {
        return Ok(None);
    };
    let Some(native) = namespace.get("native_content") else {
        return Ok(None);
    };
    if native.get("source").and_then(Value::as_str) == Some("anthropic_messages") {
        return Ok(None);
    }
    let mismatch = || {
        ProviderError::InvalidRequest(
        "aws_bedrock native_content projection mismatch with public assistant text, reasoning, or tool calls".to_string(),
    )
    };
    if native.get("source").and_then(Value::as_str) != Some("bedrock_converse") {
        return Err(mismatch());
    }
    let blocks = native
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(mismatch)?;
    if blocks.iter().any(|block| {
        !block.is_object()
            || block.get("text").is_some_and(|text| !text.is_string())
            || block.get("toolUse").is_some_and(|tool| {
                !tool["toolUseId"].is_string()
                    || !tool["name"].is_string()
                    || tool.get("input").is_none()
            })
    }) {
        return Err(mismatch());
    }
    if !text_bytes(blocks).eq(text_bytes(public_blocks))
        || !tool_projection(blocks).eq(tool_projection(public_blocks))
        || public_blocks.iter().any(is_other_visible_block)
            && !blocks
                .iter()
                .filter(|block| is_other_visible_block(block))
                .eq(public_blocks
                    .iter()
                    .filter(|block| is_other_visible_block(block)))
    {
        return Err(mismatch());
    }
    let reasoning = extract_bedrock_reasoning_blocks(blocks);
    if let Some(projection) = namespace
        .get("reasoning")
        .and_then(|value| value.get("blocks"))
        && projection.as_array().map(Vec::as_slice) != Some(reasoning.as_slice())
    {
        return Err(mismatch());
    }
    if let Some(public) = message.extra.get("reasoning_content")
        && !public
            .as_str()
            .is_some_and(|text| text.bytes().eq(text_bytes(&reasoning)))
    {
        return Err(mismatch());
    }
    Ok(Some(blocks.clone()))
}

fn tool_projection(blocks: &[Value]) -> impl Iterator<Item = (&Value, &Value, &Value)> {
    blocks
        .iter()
        .filter_map(|block| block.get("toolUse"))
        .map(|tool| (&tool["toolUseId"], &tool["name"], &tool["input"]))
}

fn text_bytes(blocks: &[Value]) -> impl Iterator<Item = u8> + '_ {
    blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .flat_map(str::bytes)
}

fn is_other_visible_block(block: &Value) -> bool {
    block.get("text").is_none()
        && block.get("toolUse").is_none()
        && block.get("reasoningContent").is_none()
}
