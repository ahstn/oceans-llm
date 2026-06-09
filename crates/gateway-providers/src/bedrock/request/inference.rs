use super::*;

pub(super) fn extract_inference_config(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Map<String, Value>, ProviderError> {
    let mut config = Map::new();
    if let Some(value) = extra
        .remove("max_completion_tokens")
        .or_else(|| extra.remove("max_tokens"))
    {
        config.insert("maxTokens".to_string(), value);
    }
    if let Some(value) = extra.remove("temperature") {
        config.insert("temperature".to_string(), value);
    }
    if let Some(value) = extra.remove("top_p") {
        config.insert("topP".to_string(), value);
    }
    if let Some(value) = extra.remove("stop") {
        config.insert(
            "stopSequences".to_string(),
            normalize_stop_sequences(value)?,
        );
    }
    Ok(config)
}

pub(super) fn extract_anthropic_inference_fields(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
) -> Result<(), ProviderError> {
    if let Some(value) = extra
        .remove("max_completion_tokens")
        .or_else(|| extra.remove("max_tokens"))
    {
        body.insert("max_tokens".to_string(), value);
    }
    for field in ["temperature", "top_p", "top_k"] {
        if let Some(value) = extra.remove(field) {
            body.insert(field.to_string(), value);
        }
    }
    if let Some(value) = extra.remove("stop") {
        body.insert(
            "stop_sequences".to_string(),
            normalize_stop_sequences(value)?,
        );
    }
    if let Some(value) = extra.remove("stop_sequences") {
        body.insert(
            "stop_sequences".to_string(),
            normalize_stop_sequences(value)?,
        );
    }
    Ok(())
}

pub(super) fn normalize_stop_sequences(value: Value) -> Result<Value, ProviderError> {
    match value {
        Value::String(sequence) => Ok(Value::Array(vec![Value::String(sequence)])),
        Value::Array(values) if values.iter().all(Value::is_string) => Ok(Value::Array(values)),
        Value::Null => Ok(Value::Array(Vec::new())),
        _ => Err(ProviderError::InvalidRequest(
            "`stop` must be a string or array of strings for aws_bedrock chat".to_string(),
        )),
    }
}

pub(super) fn reject_unknown_converse_fields(
    extra: &BTreeMap<String, Value>,
) -> Result<(), ProviderError> {
    if extra.is_empty() {
        return Ok(());
    }
    let unsupported_fields = extra.keys().cloned().collect::<Vec<_>>().join(", ");
    Err(ProviderError::InvalidRequest(format!(
        "unsupported request field(s) for aws_bedrock Converse mapping: {unsupported_fields}. Use `additionalModelRequestFields` / `additional_model_request_fields` for model-specific Bedrock controls, or route `extra_body` to override raw Bedrock request fields"
    )))
}

pub(super) fn reject_unknown_anthropic_messages_fields(
    extra: &BTreeMap<String, Value>,
) -> Result<(), ProviderError> {
    if extra.is_empty() {
        return Ok(());
    }
    let unsupported_fields = extra.keys().cloned().collect::<Vec<_>>().join(", ");
    Err(ProviderError::InvalidRequest(format!(
        "unsupported request field(s) for aws_bedrock Anthropic Claude Messages mapping: {unsupported_fields}. Use route `extra_body` for raw provider-specific overrides"
    )))
}

pub(super) fn extract_anthropic_passthrough_fields(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
) {
    for field in [
        "anthropic_beta",
        "thinking",
        "output_config",
        "container",
        "context_management",
        "metadata",
    ] {
        if let Some(value) = extra.remove(field) {
            body.insert(field.to_string(), value);
        }
    }
}

pub(super) fn reject_openai_only_fields(
    extra: &BTreeMap<String, Value>,
) -> Result<(), ProviderError> {
    const UNSUPPORTED: &[&str] = &[
        "frequency_penalty",
        "presence_penalty",
        "logit_bias",
        "logprobs",
        "top_logprobs",
        "n",
        "response_format",
        "seed",
        "store",
        "metadata",
        "parallel_tool_calls",
        "user",
    ];

    if let Some(field) = UNSUPPORTED.iter().find(|field| extra.contains_key(**field)) {
        return Err(ProviderError::InvalidRequest(format!(
            "`{field}` is not supported for aws_bedrock chat in this slice"
        )));
    }

    Ok(())
}
