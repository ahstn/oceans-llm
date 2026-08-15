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

pub(super) fn extract_converse_request_controls(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
    stream: bool,
) -> Result<(), ProviderError> {
    if let Some(value) = take_aliased_converse_field(extra, "requestMetadata", "request_metadata")?
    {
        validate_request_metadata(&value)?;
        body.insert("requestMetadata".to_string(), value);
    }
    if let Some(value) =
        take_aliased_converse_field(extra, "performanceConfig", "performance_config")?
    {
        validate_performance_config(&value)?;
        body.insert("performanceConfig".to_string(), value);
    }
    if let Some(value) = take_aliased_converse_field(extra, "guardrailConfig", "guardrail_config")?
    {
        validate_guardrail_config(&value, stream)?;
        body.insert("guardrailConfig".to_string(), value);
    }
    if let Some(value) = take_aliased_converse_field(
        extra,
        "additionalModelResponseFieldPaths",
        "additional_model_response_field_paths",
    )? {
        validate_additional_model_response_field_paths(&value)?;
        body.insert("additionalModelResponseFieldPaths".to_string(), value);
    }
    Ok(())
}

pub(super) fn validate_converse_request_controls(
    body: &Map<String, Value>,
    stream: bool,
) -> Result<(), ProviderError> {
    if let Some(value) = body.get("requestMetadata") {
        validate_request_metadata(value)?;
    }
    if let Some(value) = body.get("performanceConfig") {
        validate_performance_config(value)?;
    }
    if let Some(value) = body.get("guardrailConfig") {
        validate_guardrail_config(value, stream)?;
    }
    if let Some(value) = body.get("additionalModelResponseFieldPaths") {
        validate_additional_model_response_field_paths(value)?;
    }
    Ok(())
}

fn take_aliased_converse_field(
    extra: &mut BTreeMap<String, Value>,
    canonical: &str,
    alias: &str,
) -> Result<Option<Value>, ProviderError> {
    match (extra.remove(canonical), extra.remove(alias)) {
        (Some(canonical_value), Some(alias_value)) if canonical_value != alias_value => Err(
            invalid_converse_field(canonical, &format!("conflicts with `{alias}`")),
        ),
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn validate_request_metadata(value: &Value) -> Result<(), ProviderError> {
    let metadata = expect_converse_object(value, "requestMetadata")?;
    if metadata.len() > 16 {
        return Err(invalid_converse_field(
            "requestMetadata",
            "must contain at most 16 entries",
        ));
    }
    for (key, value) in metadata {
        if key.is_empty()
            || key.chars().count() > 256
            || !key.chars().all(is_request_metadata_character)
        {
            return Err(invalid_converse_field(
                "requestMetadata",
                "keys must be 1-256 characters using letters, numbers, whitespace, or :_@$#=/+,-.",
            ));
        }
        let value = value
            .as_str()
            .ok_or_else(|| invalid_converse_field("requestMetadata", "values must be strings"))?;
        if value.chars().count() > 256 || !value.chars().all(is_request_metadata_character) {
            return Err(invalid_converse_field(
                "requestMetadata",
                "values must be at most 256 characters using letters, numbers, whitespace, or :_@$#=/+,-.",
            ));
        }
    }
    Ok(())
}

fn is_request_metadata_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || character.is_ascii_whitespace()
        || ":_@$#=/+,-.".contains(character)
}

fn validate_performance_config(value: &Value) -> Result<(), ProviderError> {
    let config = expect_converse_object(value, "performanceConfig")?;
    reject_unknown_object_fields(config, "performanceConfig", &["latency"])?;
    if let Some(latency) = config.get("latency") {
        let latency = latency.as_str().ok_or_else(|| {
            invalid_converse_field("performanceConfig.latency", "must be a string")
        })?;
        if !matches!(latency, "standard" | "optimized") {
            return Err(invalid_converse_field(
                "performanceConfig.latency",
                "must be `standard` or `optimized`",
            ));
        }
    }
    Ok(())
}

fn validate_guardrail_config(value: &Value, stream: bool) -> Result<(), ProviderError> {
    let config = expect_converse_object(value, "guardrailConfig")?;
    let allowed_fields = if stream {
        &[
            "guardrailIdentifier",
            "guardrailVersion",
            "trace",
            "streamProcessingMode",
        ][..]
    } else {
        &["guardrailIdentifier", "guardrailVersion", "trace"][..]
    };
    reject_unknown_object_fields(config, "guardrailConfig", allowed_fields)?;

    if let Some(identifier) = config.get("guardrailIdentifier") {
        let identifier = identifier.as_str().ok_or_else(|| {
            invalid_converse_field("guardrailConfig.guardrailIdentifier", "must be a string")
        })?;
        if identifier.chars().count() > 2048 || !is_valid_guardrail_identifier(identifier) {
            return Err(invalid_converse_field(
                "guardrailConfig.guardrailIdentifier",
                "must be a lowercase alphanumeric ID or Bedrock guardrail ARN",
            ));
        }
    }
    if let Some(version) = config.get("guardrailVersion") {
        let version = version.as_str().ok_or_else(|| {
            invalid_converse_field("guardrailConfig.guardrailVersion", "must be a string")
        })?;
        let numbered_version = !version.is_empty()
            && version.len() <= 8
            && version.as_bytes()[0].is_ascii_digit()
            && version.as_bytes()[0] != b'0'
            && version.bytes().all(|byte| byte.is_ascii_digit());
        if !matches!(version, "" | "DRAFT") && !numbered_version {
            return Err(invalid_converse_field(
                "guardrailConfig.guardrailVersion",
                "must be `DRAFT`, empty, or a 1-8 digit positive version",
            ));
        }
    }
    validate_optional_string_enum(
        config,
        "trace",
        "guardrailConfig.trace",
        &["enabled", "disabled", "enabled_full"],
    )?;
    validate_optional_string_enum(
        config,
        "streamProcessingMode",
        "guardrailConfig.streamProcessingMode",
        &["sync", "async"],
    )
}

fn is_valid_guardrail_identifier(identifier: &str) -> bool {
    if identifier.is_empty()
        || identifier
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return true;
    }
    let mut parts = identifier.splitn(6, ':');
    let (
        Some("arn"),
        Some(partition),
        Some("bedrock"),
        Some(region),
        Some(account),
        Some(resource),
    ) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    )
    else {
        return false;
    };
    (partition == "aws"
        || partition
            .strip_prefix("aws-")
            .is_some_and(|suffix| !suffix.is_empty()))
        && !region.is_empty()
        && region.len() <= 20
        && region
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && account.len() == 12
        && account.bytes().all(|byte| byte.is_ascii_digit())
        && resource.strip_prefix("guardrail/").is_some_and(|id| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn validate_additional_model_response_field_paths(value: &Value) -> Result<(), ProviderError> {
    let paths = value.as_array().ok_or_else(|| {
        invalid_converse_field(
            "additionalModelResponseFieldPaths",
            "must be an array of JSON Pointer strings",
        )
    })?;
    if paths.len() > 10 {
        return Err(invalid_converse_field(
            "additionalModelResponseFieldPaths",
            "must contain at most 10 paths",
        ));
    }
    for path in paths {
        let path = path.as_str().ok_or_else(|| {
            invalid_converse_field(
                "additionalModelResponseFieldPaths",
                "entries must be strings",
            )
        })?;
        if path.is_empty() || path.chars().count() > 256 || !is_valid_json_pointer(path) {
            return Err(invalid_converse_field(
                "additionalModelResponseFieldPaths",
                "entries must be non-empty RFC 6901 JSON Pointers up to 256 characters",
            ));
        }
    }
    Ok(())
}

fn is_valid_json_pointer(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            index += 1;
            if index == bytes.len() || !matches!(bytes[index], b'0' | b'1') {
                return false;
            }
        }
        index += 1;
    }
    true
}

fn validate_optional_string_enum(
    object: &Map<String, Value>,
    field: &str,
    path: &str,
    allowed: &[&str],
) -> Result<(), ProviderError> {
    let Some(value) = object.get(field) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .ok_or_else(|| invalid_converse_field(path, "must be a string"))?;
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(invalid_converse_field(
            path,
            &format!("must be one of {}", allowed.join(", ")),
        ))
    }
}

fn expect_converse_object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a Map<String, Value>, ProviderError> {
    value
        .as_object()
        .ok_or_else(|| invalid_converse_field(field, "must be an object"))
}

fn reject_unknown_object_fields(
    object: &Map<String, Value>,
    field: &str,
    allowed: &[&str],
) -> Result<(), ProviderError> {
    if let Some(unknown) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid_converse_field(
            field,
            &format!("contains unsupported field `{unknown}`"),
        ));
    }
    Ok(())
}

fn invalid_converse_field(field: &str, message: &str) -> ProviderError {
    ProviderError::InvalidRequest(format!("aws_bedrock Converse `{field}` {message}"))
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
        "unsupported request field(s) for Anthropic Messages mapping: {unsupported_fields}. Use route `extra_body` for raw provider-specific overrides"
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
            "`{field}` is not supported by the Anthropic Messages mapping"
        )));
    }

    Ok(())
}
