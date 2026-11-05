use super::*;

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
