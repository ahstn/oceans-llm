use serde_json::{Map, Value, json};

use super::error::AnthropicAdapterError;

/// Translate after route overrides and tool conversion so native controls cannot
/// be overwritten by unconverted Chat fields.
pub(super) fn translate_chat_controls(
    body: &mut Map<String, Value>,
) -> Result<(), AnthropicAdapterError> {
    translate_stop(body)?;
    translate_parallel_tool_calls(body)?;
    if let Some(options) = body.remove("stream_options") {
        // Native Messages includes stream usage without this Chat transport hint.
        let supported = options.is_null()
            || options.as_object().is_some_and(|fields| {
                fields
                    .iter()
                    .all(|(key, value)| key == "include_usage" && value.is_boolean())
            });
        if !supported {
            return Err(AnthropicAdapterError::UnsupportedStreamOptions);
        }
    }
    if let Some(store) = body.remove("store")
        && !matches!(store, Value::Null | Value::Bool(false))
    {
        return Err(AnthropicAdapterError::UnsupportedStore);
    }
    Ok(())
}

fn translate_stop(body: &mut Map<String, Value>) -> Result<(), AnthropicAdapterError> {
    let sequences = match body.remove("stop") {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::String(text)) => Value::Array(vec![Value::String(text)]),
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => Value::Array(items),
        _ => return Err(AnthropicAdapterError::InvalidStop),
    };
    if let Some(native) = body.get("stop_sequences")
        && native != &sequences
    {
        return Err(AnthropicAdapterError::ConflictingStop);
    }
    body.insert("stop_sequences".into(), sequences);
    Ok(())
}

fn translate_parallel_tool_calls(
    body: &mut Map<String, Value>,
) -> Result<(), AnthropicAdapterError> {
    let parallel = match body.remove("parallel_tool_calls") {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Bool(parallel)) => parallel,
        _ => return Err(AnthropicAdapterError::InvalidParallelToolCalls),
    };
    // Without tools, or with tool use disabled, the constraint is vacuous.
    if body
        .get("tools")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
        || body
            .get("tool_choice")
            .and_then(|choice| choice.get("type"))
            .and_then(Value::as_str)
            == Some("none")
    {
        return Ok(());
    }
    let choice = body.entry("tool_choice").or_insert(Value::Null);
    if choice.is_null() {
        *choice = json!({"type": "auto"});
    }
    let choice = choice
        .as_object_mut()
        .ok_or(AnthropicAdapterError::UnsupportedToolChoice)?;
    if !matches!(
        choice.get("type").and_then(Value::as_str),
        Some("auto" | "any" | "tool")
    ) {
        return Err(AnthropicAdapterError::UnsupportedToolChoice);
    }
    let disabled = Value::Bool(!parallel);
    if let Some(native) = choice.get("disable_parallel_tool_use")
        && native != &disabled
    {
        return Err(AnthropicAdapterError::ConflictingParallelToolCalls);
    }
    choice.insert("disable_parallel_tool_use".into(), disabled);
    Ok(())
}
