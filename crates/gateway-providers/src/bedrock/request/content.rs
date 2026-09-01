use super::*;
use crate::media::infer_media_type_from_path;

pub(super) fn message_content_as_text(content: &Value) -> Result<String, ProviderError> {
    match content {
        Value::Null => Ok(String::new()),
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
                match kind {
                    "text" | "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        lines.push(text.to_string());
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for aws_bedrock instruction text"
                        )));
                    }
                }
            }
            Ok(lines.join("\n"))
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

pub(super) fn map_bedrock_message_content_blocks(
    content: &Value,
    role: &str,
) -> Result<Vec<Value>, ProviderError> {
    let blocks = map_bedrock_content_blocks(content)?;
    let has_document = blocks.iter().any(|block| block.get("document").is_some());
    let has_image = blocks.iter().any(|block| block.get("image").is_some());
    if role != "user" && (has_document || has_image) {
        let content_type = if has_document { "document" } else { "image" };
        return Err(ProviderError::InvalidRequest(format!(
            "Bedrock {content_type} content is only supported in user messages"
        )));
    }
    if has_document && !blocks.iter().any(|block| block.get("text").is_some()) {
        return Err(ProviderError::InvalidRequest(
            "Bedrock user messages containing documents must also contain text".to_string(),
        ));
    }
    Ok(blocks)
}

fn map_bedrock_content_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({ "text": text })]),
        Value::Array(items) => {
            let mut blocks = Vec::new();
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
                    "tool_result" => {
                        blocks.push(map_tool_result_content_block(object)?);
                    }
                    _ => blocks.push(map_bedrock_message_content_block(object)?),
                }
            }
            Ok(blocks)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

pub(super) fn map_bedrock_message_content_block(
    object: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest("content entries must include `type`".to_string())
    })?;
    match kind {
        "text" | "input_text" => {
            let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "text content entries must include a string `text`".to_string(),
                )
            })?;
            Ok(json!({ "text": text }))
        }
        "image" | "image_url" | "input_image" => map_bedrock_image_block(object),
        "document" | "file" | "input_file" => map_bedrock_file_block(object),
        other => Err(ProviderError::InvalidRequest(format!(
            "unsupported content type `{other}` for aws_bedrock Converse mapping"
        ))),
    }
}

fn map_bedrock_file_block(object: &Map<String, Value>) -> Result<Value, ProviderError> {
    let file = object
        .get("file")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let source = file
        .get("source")
        .and_then(Value::as_object)
        .or_else(|| object.get("source").and_then(Value::as_object));
    let filename = string_field(file, &["filename", "name"])
        .or_else(|| string_field(object, &["filename", "name"]));
    let explicit_media_type = string_field(file, &["media_type", "mime_type", "mediaType"])
        .or_else(|| string_field(object, &["media_type", "mime_type", "mediaType"]))
        .or_else(|| source.and_then(|source| string_field(source, &["media_type", "mime_type"])));
    let encoded = string_field(file, &["file_data"])
        .or_else(|| file.get("data").and_then(encoded_data))
        .or_else(|| source.and_then(|source| string_field(source, &["data", "bytes"])))
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "Bedrock file content must include base64 `file_data`, `data`, or `source.data`"
                    .to_string(),
            )
        })?;
    let (data_url_media_type, bytes) = parse_base64_data_url(encoded)
        .map_or((None, encoded), |(media_type, data)| {
            (Some(media_type), data)
        });
    let media_type = explicit_media_type
        .or(data_url_media_type)
        .or_else(|| filename.and_then(infer_media_type_from_path))
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "Bedrock file content must include a supported media type or filename extension"
                    .to_string(),
            )
        })?;

    if media_type.starts_with("image/") {
        return map_bedrock_base64_image(media_type, bytes);
    }

    let format = bedrock_document_format(media_type).ok_or_else(|| {
        ProviderError::InvalidRequest(format!(
            "unsupported document media type `{media_type}` for aws_bedrock Converse"
        ))
    })?;
    Ok(json!({
        "document": {
            "format": format,
            "name": sanitize_bedrock_document_name(filename, format),
            "source": {"bytes": bytes}
        }
    }))
}

fn string_field<'a>(object: &'a Map<String, Value>, fields: &[&str]) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|field| object.get(*field).and_then(Value::as_str))
}

fn encoded_data(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| {
        value
            .as_object()
            .and_then(|data| data.get("data"))
            .and_then(Value::as_str)
    })
}

fn parse_base64_data_url(value: &str) -> Option<(&str, &str)> {
    value
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
}

fn bedrock_document_format(media_type: &str) -> Option<&'static str> {
    match media_type {
        "application/pdf" => Some("pdf"),
        "text/csv" | "application/csv" => Some("csv"),
        "application/msword" => Some("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some("docx"),
        "application/vnd.ms-excel" => Some("xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
        "text/html" => Some("html"),
        "text/plain" => Some("txt"),
        "text/markdown" | "text/x-markdown" => Some("md"),
        _ => None,
    }
}

fn sanitize_bedrock_document_name(filename: Option<&str>, format: &str) -> String {
    let filename = filename
        .and_then(|filename| {
            filename
                .rsplit(|character| ['/', '\\'].contains(&character))
                .next()
        })
        .unwrap_or("document");
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    let mut name = String::with_capacity(stem.len().min(200));
    let mut previous_was_space = true;
    for character in stem.chars() {
        if name.len() >= 200 {
            break;
        }
        let allowed =
            character.is_ascii_alphanumeric() || matches!(character, '-' | '(' | ')' | '[' | ']');
        if allowed {
            name.push(character);
            previous_was_space = false;
        } else if !previous_was_space {
            name.push(' ');
            previous_was_space = true;
        }
    }
    let name = name.trim();
    if name.is_empty() {
        format!("document {format}")
    } else {
        name.to_string()
    }
}

pub(super) fn map_bedrock_image_block(object: &Map<String, Value>) -> Result<Value, ProviderError> {
    let image_url = object
        .get("image_url")
        .or_else(|| object.get("source"))
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "image content entries must include `image_url` or `source`".to_string(),
            )
        })?;

    match image_url {
        Value::Object(image_object) => {
            if image_object.get("type").and_then(Value::as_str) == Some("base64") {
                return map_bedrock_base64_image_source(image_object);
            }
            if let Some(source) = image_object.get("source").and_then(Value::as_object)
                && source.get("type").and_then(Value::as_str) == Some("base64")
            {
                return map_bedrock_base64_image_source(source);
            }

            let url = image_object
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest("image_url.url must be a string".to_string())
                })?;
            map_bedrock_data_url_image(url, image_object)
        }
        Value::String(url) => map_bedrock_data_url_image(url, object),
        _ => Err(ProviderError::InvalidRequest(
            "image_url must be a string or object for aws_bedrock Converse".to_string(),
        )),
    }
}

pub(super) fn map_bedrock_base64_image_source(
    source: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let media_type = source
        .get("media_type")
        .or_else(|| source.get("mime_type"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "base64 image sources for aws_bedrock Converse must include `media_type`"
                    .to_string(),
            )
        })?;
    let data = source.get("data").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest(
            "base64 image sources for aws_bedrock Converse must include string `data`".to_string(),
        )
    })?;
    map_bedrock_base64_image(media_type, data)
}

pub(super) fn map_bedrock_data_url_image(
    url: &str,
    metadata: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let Some((media_type, data)) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
    else {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Converse only supports base64 image data URLs; remote image URLs are not supported"
                .to_string(),
        ));
    };
    let media_type = metadata
        .get("mime_type")
        .and_then(Value::as_str)
        .unwrap_or(media_type);
    map_bedrock_base64_image(media_type, data)
}

pub(super) fn map_bedrock_base64_image(
    media_type: &str,
    data: &str,
) -> Result<Value, ProviderError> {
    let format = match media_type {
        "image/jpeg" => "jpeg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        other => {
            return Err(ProviderError::InvalidRequest(format!(
                "unsupported image media type `{other}` for aws_bedrock Converse"
            )));
        }
    };

    Ok(json!({
        "image": {
            "format": format,
            "source": {
                "bytes": data
            }
        }
    }))
}

pub(super) fn map_anthropic_content_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({ "type": "text", "text": text })]),
        Value::Array(items) => {
            let mut blocks = Vec::new();
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
                    "text" => {
                        object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        blocks.push(item.clone());
                    }
                    "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        blocks.push(json!({ "type": "text", "text": text }));
                    }
                    "image" | "image_url" | "input_image" => {
                        blocks.push(map_anthropic_image_block(object)?);
                    }
                    _ => blocks.push(item.clone()),
                }
            }
            Ok(blocks)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

pub(super) fn map_anthropic_image_block(
    object: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let image_url = object
        .get("image_url")
        .or_else(|| object.get("source"))
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "image content entries must include `image_url` or `source`".to_string(),
            )
        })?;

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
                .ok_or_else(|| {
                    ProviderError::InvalidRequest("image_url.url must be a string".to_string())
                })?;
            map_anthropic_data_url_image(url, image_object)
        }
        Value::String(url) => map_anthropic_data_url_image(url, object),
        _ => Err(ProviderError::InvalidRequest(
            "image_url must be a string or object for Anthropic Messages".to_string(),
        )),
    }
}

fn validate_anthropic_base64_image_source(
    source: &Map<String, Value>,
) -> Result<(), ProviderError> {
    let media_type = source
        .get("media_type")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "base64 image sources for Anthropic Messages must include `media_type`".to_string(),
            )
        })?;
    if !matches!(
        media_type,
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    ) {
        return Err(ProviderError::InvalidRequest(format!(
            "unsupported image media type `{media_type}` for Anthropic Messages"
        )));
    }
    if source.get("data").and_then(Value::as_str).is_none() {
        return Err(ProviderError::InvalidRequest(
            "base64 image sources for Anthropic Messages must include string `data`".to_string(),
        ));
    }

    Ok(())
}

pub(super) fn map_anthropic_data_url_image(
    url: &str,
    metadata: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let Some((media_type, data)) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
    else {
        return Err(ProviderError::InvalidRequest(
            "Anthropic Messages only supports base64 image data URLs; remote image URLs are not supported"
                .to_string(),
        ));
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
        other => Err(ProviderError::InvalidRequest(format!(
            "unsupported image media type `{other}` for Anthropic Messages"
        ))),
    }
}
