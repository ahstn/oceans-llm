use std::collections::BTreeMap;

use gateway_core::{
    CoreChatRequest, CoreContentPartType, ProviderError, ProviderRequestContext, ReasoningEffort,
};
use serde_json::{Map, Value, json};

use super::{
    error::VertexAdapterError,
    gemini::{GeminiModel, ThinkingControl},
    google_tools::{
        FunctionIdPolicy, convert_openai_tools_for_google, map_google_anthropic_tool_result_part,
        map_google_anthropic_tool_use_part, map_google_assistant_parts,
        map_google_tool_result_part, record_google_tool_names,
    },
};
use crate::media::{infer_media_type_from_path, is_valid_media_type};

pub(super) fn map_google_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    model_id: &str,
    stream: bool,
) -> Result<Value, ProviderError> {
    let model = GeminiModel::parse(model_id);
    let function_ids = match model {
        Some(model) if model.supports_function_ids() => FunctionIdPolicy::Include,
        _ => FunctionIdPolicy::Omit,
    };

    let mut body = Map::new();
    map_google_contents(request, function_ids, &mut body)?;

    let mut generation_config = Map::new();
    let mut thinking = ThinkingRequest::default();
    let mut response_format = None;
    let mut caller_generation_configs = Vec::new();
    for (key, value) in &request.extra {
        match key.as_str() {
            // OpenAI-only fields with no Vertex equivalent; the official compat layer ignores them.
            "model" | "messages" | "stream" | "stream_options" | "user" | "store" | "metadata"
            | "service_tier" | "logit_bias" | "prompt_cache_key" | "safety_identifier"
            | "modalities" | "audio" | "prediction" | "verbosity" | "web_search_options"
            | "functions" | "function_call" => {}
            "temperature" => insert_config(&mut generation_config, "temperature", value),
            "top_p" => insert_config(&mut generation_config, "topP", value),
            "top_k" => insert_config(&mut generation_config, "topK", value),
            "presence_penalty" => insert_config(&mut generation_config, "presencePenalty", value),
            "frequency_penalty" => {
                insert_config(&mut generation_config, "frequencyPenalty", value);
            }
            "seed" => insert_config(&mut generation_config, "seed", value),
            "n" => insert_config(&mut generation_config, "candidateCount", value),
            "logprobs" => insert_config(&mut generation_config, "responseLogprobs", value),
            "top_logprobs" => insert_config(&mut generation_config, "logprobs", value),
            "max_tokens" | "max_completion_tokens" => {
                match generation_config.get("maxOutputTokens") {
                    Some(existing) if existing != value => {
                        return Err(VertexAdapterError::ConflictingMaxTokens.into());
                    }
                    Some(_) => {}
                    None => insert_config(&mut generation_config, "maxOutputTokens", value),
                }
            }
            "stop" => {
                let sequences = match value {
                    Value::String(sequence) => json!([sequence]),
                    other => other.clone(),
                };
                generation_config.insert("stopSequences".to_string(), sequences);
            }
            "reasoning_effort" => thinking.effort_field = Some(value),
            "reasoning" => thinking.reasoning_object = Some(value),
            "response_format" => response_format = Some(value),
            "generationConfig" | "generation_config" => caller_generation_configs.push(value),
            _ => {
                body.insert(key.clone(), value.clone());
            }
        }
    }

    // Caller-supplied native config wins over the mapped OpenAI sampling keys. Both aliases
    // merge in; when both are present the later one wins on overlapping keys.
    for caller in caller_generation_configs {
        let caller = caller
            .as_object()
            .ok_or(VertexAdapterError::InvalidGenerationConfig)?;
        merge_object_overrides(&mut generation_config, caller);
    }
    apply_thinking_config(&mut generation_config, thinking, model, model_id)?;
    if let Some(response_format) = response_format {
        apply_response_format(&mut generation_config, response_format)?;
    }
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    merge_object_overrides(&mut body, &context.extra_body);
    body.remove("stream");
    convert_openai_tools_for_google(&mut body)?;
    reject_google_streamed_function_call_arguments(&body)?;
    validate_google_stream_candidate_count(&body, stream)?;
    Ok(Value::Object(body))
}

fn insert_config(generation_config: &mut Map<String, Value>, key: &str, value: &Value) {
    generation_config.insert(key.to_string(), value.clone());
}

fn map_google_contents(
    request: &CoreChatRequest,
    function_ids: FunctionIdPolicy,
    body: &mut Map<String, Value>,
) -> Result<(), ProviderError> {
    let mut contents = Vec::new();
    let mut system_lines = Vec::new();
    let mut known_tool_names = BTreeMap::new();
    let mut pending_tool_parts = Vec::new();

    for message in &request.messages {
        if message.role == "tool" {
            let part = map_google_tool_result_part(message, &known_tool_names, function_ids)?;
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
                let mut parts =
                    map_google_parts(&message.content, Some(&known_tool_names), function_ids)?;
                if parts.is_empty() {
                    // Vertex rejects a content entry with no parts.
                    parts.push(json!({ "text": "" }));
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            "assistant" => {
                let parts = map_google_assistant_parts(message, function_ids)?;
                record_google_tool_names(message, &mut known_tool_names);
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            other => {
                return Err(VertexAdapterError::UnsupportedMessageRole(other.to_string()).into());
            }
        }
    }

    if !pending_tool_parts.is_empty() {
        contents.push(json!({ "role": "user", "parts": pending_tool_parts }));
    }
    if contents.is_empty() {
        return Err(VertexAdapterError::EmptyMessages.into());
    }
    body.insert("contents".to_string(), Value::Array(contents));

    if !system_lines.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({ "parts": [{ "text": system_lines.join("\n\n") }] }),
        );
    }
    Ok(())
}

#[derive(Default)]
struct ThinkingRequest<'a> {
    effort_field: Option<&'a Value>,
    reasoning_object: Option<&'a Value>,
}

impl ThinkingRequest<'_> {
    /// Resolves `reasoning_effort` and `reasoning.effort` into one categorical effort.
    /// `Some(None)` means the caller asked for thinking to be disabled (`"none"`).
    fn effort(&self) -> Result<Option<Option<ReasoningEffort>>, VertexAdapterError> {
        let from_object = match self.reasoning_object {
            None | Some(Value::Null) => None,
            Some(Value::Object(reasoning)) => reasoning.get("effort").filter(|v| !v.is_null()),
            Some(_) => return Err(VertexAdapterError::InvalidReasoning),
        };
        let from_field = self.effort_field.filter(|value| !value.is_null());
        let raw = match (from_field, from_object) {
            (Some(field), Some(object)) if field != object => {
                return Err(VertexAdapterError::ConflictingReasoningEffort);
            }
            (Some(value), _) | (None, Some(value)) => value,
            (None, None) => return Ok(None),
        };
        let raw = raw
            .as_str()
            .ok_or(VertexAdapterError::InvalidReasoningEffortType)?;
        if raw.eq_ignore_ascii_case("none") || raw.eq_ignore_ascii_case("off") {
            return Ok(Some(None));
        }
        ReasoningEffort::from_db(raw)
            .map(|effort| Some(Some(effort)))
            .ok_or_else(|| VertexAdapterError::UnsupportedReasoningEffort(raw.to_string()))
    }
}

fn apply_thinking_config(
    generation_config: &mut Map<String, Value>,
    thinking: ThinkingRequest<'_>,
    model: Option<GeminiModel>,
    model_id: &str,
) -> Result<(), VertexAdapterError> {
    let Some(effort) = thinking.effort()? else {
        return Ok(());
    };
    let model = model
        .filter(|model| model.supports_thinking())
        .ok_or_else(|| VertexAdapterError::ThinkingNotSupported {
            model: model_id.to_string(),
        })?;
    if generation_config.contains_key("thinkingConfig") {
        return Err(VertexAdapterError::ConflictingThinkingConfig);
    }
    let control = match effort {
        Some(effort) => model.thinking_control(effort),
        None => model.disabled_thinking_control(),
    }
    .expect("supports_thinking was checked above");
    let mut thinking_config = Map::new();
    thinking_config.insert("includeThoughts".to_string(), Value::Bool(effort.is_some()));
    match control {
        ThinkingControl::Level(level) => {
            thinking_config.insert(
                "thinkingLevel".to_string(),
                Value::String(level.to_string()),
            );
        }
        ThinkingControl::Budget(budget) => {
            thinking_config.insert("thinkingBudget".to_string(), Value::Number(budget.into()));
        }
    }
    generation_config.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
    Ok(())
}

fn apply_response_format(
    generation_config: &mut Map<String, Value>,
    response_format: &Value,
) -> Result<(), VertexAdapterError> {
    let format = response_format
        .as_object()
        .ok_or(VertexAdapterError::InvalidResponseFormat)?;
    let kind = format.get("type").and_then(Value::as_str).unwrap_or("text");
    if kind == "text" {
        return Ok(());
    }
    if ["responseMimeType", "responseSchema", "responseJsonSchema"]
        .iter()
        .any(|key| generation_config.contains_key(*key))
    {
        return Err(VertexAdapterError::ConflictingResponseFormat);
    }
    match kind {
        "json_object" => {}
        "json_schema" => {
            let schema = format
                .get("json_schema")
                .and_then(|json_schema| json_schema.get("schema"))
                .filter(|schema| schema.is_object())
                .ok_or(VertexAdapterError::MissingResponseSchema)?;
            generation_config.insert("responseJsonSchema".to_string(), schema.clone());
        }
        other => {
            return Err(VertexAdapterError::UnsupportedResponseFormat(
                other.to_string(),
            ));
        }
    }
    generation_config.insert(
        "responseMimeType".to_string(),
        Value::String("application/json".to_string()),
    );
    Ok(())
}

pub(super) fn message_content_as_text(content: &Value) -> Result<String, VertexAdapterError> {
    match content {
        Value::String(value) => Ok(value.clone()),
        Value::Array(items) => {
            let mut text = String::new();
            for item in items {
                let (kind, object) = content_entry_kind(item)?;
                if kind != "text" && kind != "input_text" {
                    return Err(VertexAdapterError::UnsupportedContentType(kind.to_string()));
                }
                let line = object
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(VertexAdapterError::MissingContentText)?;
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(line);
            }
            Ok(text)
        }
        _ => Err(VertexAdapterError::InvalidMessageContent),
    }
}

fn content_entry_kind(item: &Value) -> Result<(&str, &Map<String, Value>), VertexAdapterError> {
    let object = item
        .as_object()
        .ok_or(VertexAdapterError::InvalidContentEntry)?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingContentType)?;
    Ok((kind, object))
}

pub(super) fn map_google_parts(
    content: &Value,
    known_tool_names: Option<&BTreeMap<String, String>>,
    function_ids: FunctionIdPolicy,
) -> Result<Vec<Value>, VertexAdapterError> {
    match content {
        Value::String(text) => Ok(vec![json!({ "text": text })]),
        Value::Array(items) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                let (kind, object) = content_entry_kind(item)?;
                let part = match kind {
                    "text" | "input_text" => {
                        let text = object
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or(VertexAdapterError::MissingContentText)?;
                        json!({ "text": text })
                    }
                    "tool_use" => map_google_anthropic_tool_use_part(object, function_ids)?,
                    "tool_result" => {
                        let known_tool_names = known_tool_names
                            .ok_or(VertexAdapterError::ToolResultOutsideUserMessage)?;
                        map_google_anthropic_tool_result_part(
                            object,
                            known_tool_names,
                            function_ids,
                        )?
                    }
                    other => match CoreContentPartType::parse(other) {
                        Some(CoreContentPartType::ImageUrl | CoreContentPartType::InputImage) => {
                            map_google_media_part(object, "image_url", MediaModality::Image)?
                        }
                        Some(CoreContentPartType::VideoUrl | CoreContentPartType::InputVideo) => {
                            let field = if object.contains_key("input_video") {
                                "input_video"
                            } else {
                                "video_url"
                            };
                            map_google_media_part(object, field, MediaModality::Video)?
                        }
                        Some(
                            CoreContentPartType::File
                            | CoreContentPartType::InputFile
                            | CoreContentPartType::Document,
                        ) => {
                            let field = ["file", "input_file", "document"]
                                .into_iter()
                                .find(|field| object.contains_key(*field))
                                .unwrap_or("file");
                            map_google_media_part(object, field, MediaModality::File)?
                        }
                        _ => {
                            return Err(VertexAdapterError::UnsupportedContentType(
                                other.to_string(),
                            ));
                        }
                    },
                };
                parts.push(part);
            }
            Ok(parts)
        }
        _ => Err(VertexAdapterError::InvalidMessageContent),
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
) -> Result<Value, VertexAdapterError> {
    let media = content
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| VertexAdapterError::MissingMediaObject {
            field: field.to_string(),
        })?;
    let uri = media.get("url").and_then(Value::as_str).ok_or_else(|| {
        VertexAdapterError::MissingMediaUrl {
            field: field.to_string(),
        }
    })?;
    let parsed_uri = validate_google_media_uri(uri)?;

    let mime_type = explicit_media_mime_type(media, field)?
        .or_else(|| infer_media_type_from_path(parsed_uri.path()))
        .ok_or_else(|| VertexAdapterError::UnknownMediaMimeType {
            field: field.to_string(),
        })?;
    if let Some(expected_prefix) = modality.mime_prefix()
        && !mime_type.to_ascii_lowercase().starts_with(expected_prefix)
    {
        return Err(VertexAdapterError::MediaModalityMismatch {
            modality: modality.name(),
            expected_prefix,
            mime_type: mime_type.to_string(),
        });
    }

    Ok(json!({ "fileData": { "fileUri": uri, "mimeType": mime_type } }))
}

fn validate_google_media_uri(uri: &str) -> Result<url::Url, VertexAdapterError> {
    let parsed = url::Url::parse(uri)?;
    if !matches!(parsed.scheme(), "gs" | "https") {
        return Err(VertexAdapterError::UnsupportedMediaScheme(
            parsed.scheme().to_string(),
        ));
    }
    if parsed.host_str().is_none() {
        return Err(VertexAdapterError::MissingMediaHost);
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(VertexAdapterError::MediaUriCredentials);
    }
    Ok(parsed)
}

fn explicit_media_mime_type<'a>(
    media: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, VertexAdapterError> {
    const MIME_FIELDS: [&str; 3] = ["mime_type", "media_type", "mediaType"];
    let mut selected = None;
    for key in MIME_FIELDS {
        let Some(value) = media.get(key) else {
            continue;
        };
        let value = value
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| VertexAdapterError::InvalidMimeField {
                field: field.to_string(),
                key,
            })?;
        if !is_valid_media_type(value) {
            return Err(VertexAdapterError::InvalidMimeType {
                field: field.to_string(),
                key,
            });
        }
        if selected.is_some_and(|selected| selected != value) {
            return Err(VertexAdapterError::ConflictingMimeFields {
                field: field.to_string(),
            });
        }
        selected = Some(value);
    }
    Ok(selected)
}

fn reject_google_streamed_function_call_arguments(
    body: &Map<String, Value>,
) -> Result<(), VertexAdapterError> {
    let enabled = body
        .get("toolConfig")
        .and_then(|tool_config| tool_config.get("functionCallingConfig"))
        .and_then(|config| config.get("streamFunctionCallArguments"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if enabled {
        return Err(VertexAdapterError::StreamedFunctionCallArguments);
    }
    Ok(())
}

fn validate_google_stream_candidate_count(
    body: &Map<String, Value>,
    stream: bool,
) -> Result<(), VertexAdapterError> {
    if !stream {
        return Ok(());
    }
    let candidate_count = body
        .get("generationConfig")
        .and_then(|config| config.get("candidateCount"))
        .and_then(Value::as_i64);
    if candidate_count.is_some_and(|count| count > 1) {
        return Err(VertexAdapterError::StreamCandidateCount);
    }
    Ok(())
}

pub(super) fn merge_object_overrides(
    base: &mut Map<String, Value>,
    overrides: &Map<String, Value>,
) {
    for (key, value) in overrides {
        match (base.get_mut(key), value) {
            (Some(Value::Object(base_object)), Value::Object(override_object)) => {
                merge_object_overrides(base_object, override_object);
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
