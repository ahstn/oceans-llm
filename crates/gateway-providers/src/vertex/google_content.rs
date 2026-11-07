use std::collections::BTreeMap;

use gateway_core::{CoreChatMessage, CoreContentPartType};
use serde_json::{Map, Value, json};

use super::error::VertexAdapterError;
use crate::media::{infer_media_type_from_path, is_valid_media_type};

/// Whether `functionCall.id` / `functionResponse.id` are emitted. Vertex rejects them on models
/// older than Gemini 3.5, so the request mapper decides per model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FunctionIdPolicy {
    Include,
    Omit,
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
    // Chat Completions nests the URL (`image_url: { url }`); Responses-style parts carry the
    // URL string directly (`input_image.image_url: "https://..."`).
    let (media, uri) = match content.get(field) {
        Some(Value::Object(media)) => {
            let uri = media.get("url").and_then(Value::as_str).ok_or_else(|| {
                VertexAdapterError::MissingMediaUrl {
                    field: field.to_string(),
                }
            })?;
            (Some(media), uri)
        }
        Some(Value::String(uri)) => (None, uri.as_str()),
        _ => {
            return Err(VertexAdapterError::MissingMediaObject {
                field: field.to_string(),
            });
        }
    };
    let parsed_uri = validate_google_media_uri(uri)?;

    let mime_type = media
        .map(|media| explicit_media_mime_type(media, field))
        .transpose()?
        .flatten()
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

fn map_google_anthropic_tool_use_part(
    object: &Map<String, Value>,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolUseField("id"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolUseField("name"))?;
    let args = object
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    Ok(function_call_part(id, name, args, function_ids, None))
}

fn map_google_anthropic_tool_result_part(
    object: &Map<String, Value>,
    known_tool_names: &BTreeMap<String, String>,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let tool_call_id = object
        .get("tool_use_id")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolResultField("tool_use_id"))?;
    let tool_name = known_tool_names
        .get(tool_call_id)
        .ok_or_else(|| VertexAdapterError::UnknownToolCallId(tool_call_id.to_string()))?;
    let content = object
        .get("content")
        .ok_or(VertexAdapterError::MissingToolResultField("content"))?;
    map_google_function_response_part(tool_call_id, tool_name, content, function_ids)
}

pub(super) fn record_google_tool_names(
    message: &CoreChatMessage,
    known_tool_names: &mut BTreeMap<String, String>,
) {
    if let Some(tool_calls) = message.extra.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            if let Some(call) = call.as_object()
                && let (Some(id), Some(name)) = (
                    call.get("id").and_then(Value::as_str),
                    call.get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str),
                )
            {
                known_tool_names.insert(id.to_string(), name.to_string());
            }
        }
    }

    if let Value::Array(blocks) = &message.content {
        for block in blocks {
            if let Some(block) = block.as_object()
                && block.get("type").and_then(Value::as_str) == Some("tool_use")
                && let (Some(id), Some(name)) = (
                    block.get("id").and_then(Value::as_str),
                    block.get("name").and_then(Value::as_str),
                )
            {
                known_tool_names.insert(id.to_string(), name.to_string());
            }
        }
    }
}

pub(super) fn function_call_part(
    id: &str,
    name: &str,
    args: Value,
    function_ids: FunctionIdPolicy,
    signature: Option<&Value>,
) -> Value {
    let mut function_call = Map::new();
    if function_ids == FunctionIdPolicy::Include && !id.is_empty() {
        function_call.insert("id".to_string(), Value::String(id.to_string()));
    }
    function_call.insert("name".to_string(), Value::String(name.to_string()));
    function_call.insert("args".to_string(), args);

    let mut part = Map::new();
    part.insert("functionCall".to_string(), Value::Object(function_call));
    if let Some(signature) = signature {
        part.insert("thoughtSignature".to_string(), signature.clone());
    }
    Value::Object(part)
}

pub(super) fn parse_openai_tool_arguments(
    function: &Map<String, Value>,
) -> Result<Value, VertexAdapterError> {
    let Some(arguments) = function.get("arguments") else {
        return Ok(Value::Object(Map::new()));
    };
    let arguments = arguments
        .as_str()
        .ok_or(VertexAdapterError::InvalidToolCallArguments)?;
    if arguments.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(arguments).map_err(VertexAdapterError::MalformedToolCallArguments)
}

pub(super) fn map_google_tool_result_part(
    message: &CoreChatMessage,
    known_tool_names: &BTreeMap<String, String>,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let tool_call_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or(VertexAdapterError::MissingToolCallId)?;
    let tool_name = known_tool_names
        .get(tool_call_id)
        .ok_or_else(|| VertexAdapterError::UnknownToolCallId(tool_call_id.to_string()))?;
    map_google_function_response_part(tool_call_id, tool_name, &message.content, function_ids)
}

fn map_google_function_response_part(
    tool_call_id: &str,
    tool_name: &str,
    content: &Value,
    function_ids: FunctionIdPolicy,
) -> Result<Value, VertexAdapterError> {
    let raw_text = message_content_as_text(content)?;
    let response = match serde_json::from_str::<Value>(&raw_text) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(other) => json!({ "output": other }),
        Err(_) => json!({ "output": raw_text }),
    };

    let mut function_response = Map::new();
    if function_ids == FunctionIdPolicy::Include {
        function_response.insert("id".to_string(), Value::String(tool_call_id.to_string()));
    }
    function_response.insert("name".to_string(), Value::String(tool_name.to_string()));
    function_response.insert("response".to_string(), response);
    Ok(json!({ "functionResponse": function_response }))
}
