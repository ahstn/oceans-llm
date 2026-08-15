use super::*;

pub(super) fn map_google_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    let mut contents = Vec::new();
    let mut system_lines = Vec::new();
    let mut known_tool_names = BTreeMap::new();

    let mut pending_tool_parts = Vec::new();

    for message in &request.messages {
        if message.role == "tool" {
            let part = map_google_tool_result_part(message, &known_tool_names)?;
            pending_tool_parts.push(part);
            continue;
        }

        if !pending_tool_parts.is_empty() {
            contents.push(json!({
                "role": "user",
                "parts": std::mem::take(&mut pending_tool_parts)
            }));
        }

        match message.role.as_str() {
            "system" | "developer" => {
                system_lines.push(message_content_as_text(&message.content)?);
            }
            "user" => {
                let parts = map_google_parts(&message.content, Some(&known_tool_names))?;
                contents.push(json!({
                    "role": "user",
                    "parts": parts
                }));
            }
            "assistant" => {
                let parts = map_google_assistant_parts(message)?;
                record_google_tool_names(message, &mut known_tool_names);
                contents.push(json!({
                    "role": "model",
                    "parts": parts
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for google vertex mapping"
                )));
            }
        }
    }

    if !pending_tool_parts.is_empty() {
        contents.push(json!({
            "role": "user",
            "parts": pending_tool_parts
        }));
    }

    if contents.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "google vertex request requires at least one user/assistant message".to_string(),
        ));
    }
    body.insert("contents".to_string(), Value::Array(contents));

    if !system_lines.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({
                "parts": [{"text": system_lines.join("\n\n")}]
            }),
        );
    }

    let mut passthrough = request.extra.clone();
    passthrough.remove("model");
    passthrough.remove("messages");
    passthrough.remove("stream");

    let generation_config = extract_google_generation_config(&mut passthrough);
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    for (key, value) in passthrough {
        body.insert(key, value);
    }

    if stream {
        body.remove("stream");
    }

    merge_object_overrides(&mut body, &context.extra_body);
    convert_openai_tools_for_google(&mut body)?;
    reject_google_streamed_function_call_arguments(&body)?;
    validate_google_stream_candidate_count(&body, stream)?;
    Ok(Value::Object(body))
}

pub(super) fn message_content_as_text(content: &Value) -> Result<String, ProviderError> {
    match content {
        Value::String(value) => Ok(value.clone()),
        Value::Array(items) => {
            let mut lines = Vec::new();
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
                if kind != "text" && kind != "input_text" {
                    return Err(ProviderError::InvalidRequest(format!(
                        "unsupported content type `{kind}` for instruction text"
                    )));
                }
                let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "text content entries must include a string `text`".to_string(),
                    )
                })?;
                lines.push(text.to_string());
            }
            Ok(lines.join("\n"))
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

pub(super) fn map_google_parts(
    content: &Value,
    known_tool_names: Option<&BTreeMap<String, String>>,
) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::String(text) => Ok(vec![json!({ "text": text })]),
        Value::Array(items) => {
            let mut parts = Vec::new();
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
                        parts.push(json!({ "text": text }));
                    }
                    "image_url" | "input_image" => {
                        parts.push(map_google_media_part(
                            object,
                            "image_url",
                            MediaModality::Image,
                        )?);
                    }
                    "video_url" => {
                        parts.push(map_google_media_part(
                            object,
                            "video_url",
                            MediaModality::Video,
                        )?);
                    }
                    "input_video" => {
                        let field = if object.contains_key("input_video") {
                            "input_video"
                        } else {
                            "video_url"
                        };
                        parts.push(map_google_media_part(object, field, MediaModality::Video)?);
                    }
                    "file" => {
                        parts.push(map_google_media_part(object, "file", MediaModality::File)?);
                    }
                    "tool_use" => {
                        parts.push(map_google_anthropic_tool_use_part(object)?);
                    }
                    "tool_result" => {
                        let known_tool_names = known_tool_names.ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "tool_result content is only valid in user messages".to_string(),
                            )
                        })?;
                        parts.push(map_google_anthropic_tool_result_part(
                            object,
                            known_tool_names,
                        )?);
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for google vertex mapping"
                        )));
                    }
                }
            }
            Ok(parts)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy)]
enum MediaModality {
    Image,
    Video,
    File,
}

impl MediaModality {
    const fn mime_prefix(self) -> Option<&'static str> {
        match self {
            Self::Image => Some("image/"),
            Self::Video => Some("video/"),
            Self::File => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::File => "file",
        }
    }
}

fn map_google_media_part(
    content: &Map<String, Value>,
    field: &str,
    modality: MediaModality,
) -> Result<Value, ProviderError> {
    let media = content
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(format!(
                "{field} content entries must include a `{field}` object"
            ))
        })?;
    let uri = media
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::InvalidRequest(format!("{field}.url must be a string")))?;
    let parsed_uri = validate_google_media_uri(uri)?;

    let mime_type = explicit_media_mime_type(media, field)?
        .or_else(|| guess_mime_type(parsed_uri.path()))
        .ok_or_else(|| {
            ProviderError::InvalidRequest(format!(
                "could not infer MIME type for {field} URI; set {field}.mime_type"
            ))
        })?;
    if let Some(expected_prefix) = modality.mime_prefix()
        && !mime_type.to_ascii_lowercase().starts_with(expected_prefix)
    {
        return Err(ProviderError::InvalidRequest(format!(
            "{} content requires a {expected_prefix} MIME type, got `{mime_type}`",
            modality.name()
        )));
    }

    Ok(json!({
        "fileData": {
            "fileUri": uri,
            "mimeType": mime_type
        }
    }))
}

fn validate_google_media_uri(uri: &str) -> Result<url::Url, ProviderError> {
    let parsed = url::Url::parse(uri).map_err(|error| {
        ProviderError::InvalidRequest(format!("invalid google vertex media URI: {error}"))
    })?;
    if !matches!(parsed.scheme(), "gs" | "https") {
        return Err(ProviderError::InvalidRequest(format!(
            "unsupported google vertex media URI scheme `{}`; expected gs:// or https://",
            parsed.scheme()
        )));
    }
    if parsed.host_str().is_none() {
        return Err(ProviderError::InvalidRequest(
            "google vertex media URI must include a host; expected gs:// or https://".to_string(),
        ));
    }

    Ok(parsed)
}

fn explicit_media_mime_type<'a>(
    media: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, ProviderError> {
    const MIME_FIELDS: [&str; 3] = ["mime_type", "media_type", "mediaType"];
    let mut selected = None;
    for key in MIME_FIELDS {
        let Some(value) = media.get(key) else {
            continue;
        };
        let value = value
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ProviderError::InvalidRequest(format!("{field}.{key} must be a non-empty string"))
            })?;
        if selected.is_some_and(|selected| selected != value) {
            return Err(ProviderError::InvalidRequest(format!(
                "{field} MIME type fields conflict"
            )));
        }
        selected = Some(value);
    }
    Ok(selected)
}
fn extract_google_generation_config(extra: &mut BTreeMap<String, Value>) -> Map<String, Value> {
    let mut generation_config = Map::new();

    if let Some(value) = extra.remove("temperature") {
        generation_config.insert("temperature".to_string(), value);
    }
    if let Some(value) = extra.remove("top_p") {
        generation_config.insert("topP".to_string(), value);
    }
    if let Some(value) = extra.remove("top_k") {
        generation_config.insert("topK".to_string(), value);
    }
    if let Some(value) = extra.remove("max_tokens") {
        generation_config.insert("maxOutputTokens".to_string(), value);
    }
    if let Some(value) = extra.remove("presence_penalty") {
        generation_config.insert("presencePenalty".to_string(), value);
    }
    if let Some(value) = extra.remove("frequency_penalty") {
        generation_config.insert("frequencyPenalty".to_string(), value);
    }
    if let Some(value) = extra.remove("seed") {
        generation_config.insert("seed".to_string(), value);
    }
    if let Some(value) = extra.remove("n") {
        generation_config.insert("candidateCount".to_string(), value);
    }
    if let Some(value) = extra.remove("stop") {
        let normalized = match value {
            Value::String(sequence) => Value::Array(vec![Value::String(sequence)]),
            Value::Array(values) => Value::Array(values),
            other => other,
        };
        generation_config.insert("stopSequences".to_string(), normalized);
    }

    generation_config
}

fn reject_google_streamed_function_call_arguments(
    body: &Map<String, Value>,
) -> Result<(), ProviderError> {
    let enabled = body
        .get("toolConfig")
        .and_then(Value::as_object)
        .and_then(|tool_config| tool_config.get("functionCallingConfig"))
        .and_then(Value::as_object)
        .and_then(|config| config.get("streamFunctionCallArguments"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if enabled {
        return Err(ProviderError::InvalidRequest(
            "`streamFunctionCallArguments` is not supported for google vertex chat until partial argument accumulation is implemented".to_string(),
        ));
    }

    Ok(())
}

fn validate_google_stream_candidate_count(
    body: &Map<String, Value>,
    stream: bool,
) -> Result<(), ProviderError> {
    if !stream {
        return Ok(());
    }

    let candidate_count = body
        .get("generationConfig")
        .and_then(Value::as_object)
        .and_then(|config| config.get("candidateCount"))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|count| u64::try_from(count).ok()))
        });

    if candidate_count.is_some_and(|count| count > 1) {
        return Err(ProviderError::InvalidRequest(
            "google vertex streaming supports only a single candidate in this slice; remove `n`/`candidateCount` or use non-streaming".to_string(),
        ));
    }

    Ok(())
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

fn guess_mime_type(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/mov",
        "mpeg" => "video/mpeg",
        "mpg" => "video/mpg",
        "avi" => "video/avi",
        "wmv" => "video/wmv",
        "mpegps" => "video/mpegps",
        "flv" => "video/flv",
        _ => return None,
    })
}
