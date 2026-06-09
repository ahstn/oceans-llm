use super::*;

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

pub(super) fn map_bedrock_content_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
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
                    "text" | "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        blocks.push(json!({ "text": text }));
                    }
                    "tool_result" => {
                        blocks.push(map_tool_result_content_block(object)?);
                    }
                    "image" | "image_url" | "input_image" => {
                        blocks.push(map_bedrock_image_block(object)?);
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for aws_bedrock Converse mapping"
                        )));
                    }
                }
            }
            Ok(blocks)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
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
                    "text" | "input_text" => {
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
                    "tool_result" => {
                        blocks.push(map_anthropic_tool_result_content_block(object)?);
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for aws_bedrock Anthropic Claude Messages mapping"
                        )));
                    }
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
                return Ok(json!({ "type": "image", "source": image_object }));
            }
            if let Some(source) = image_object.get("source").and_then(Value::as_object)
                && source.get("type").and_then(Value::as_str) == Some("base64")
            {
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
            "image_url must be a string or object for aws_bedrock Anthropic Claude Messages"
                .to_string(),
        )),
    }
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
            "aws_bedrock Anthropic Claude Messages only supports base64 image data URLs; remote image URLs are not supported"
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
            "unsupported image media type `{other}` for aws_bedrock Anthropic Claude Messages"
        ))),
    }
}
