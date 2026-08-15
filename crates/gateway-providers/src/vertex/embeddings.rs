use super::*;

pub(super) fn validate_vertex_embedding_model(model_id: &str) -> Result<(), ProviderError> {
    if is_supported_vertex_text_embedding_model_id(model_id) {
        return Ok(());
    }

    Err(ProviderError::InvalidRequest(format!(
        "vertex embeddings route google/{model_id} is not a supported text embedding model; supported models are {}",
        VERTEX_TEXT_EMBEDDING_MODEL_IDS.join(", ")
    )))
}

fn uses_vertex_embed_content(model_id: &str) -> bool {
    VERTEX_EMBED_CONTENT_MODEL_IDS.contains(&model_id)
}

pub(super) fn vertex_embedding_method(model_id: &str) -> &'static str {
    if uses_vertex_embed_content(model_id) {
        "embedContent"
    } else {
        "predict"
    }
}

pub(super) fn map_google_embedding_request(
    request: &CoreEmbeddingsRequest,
    context: &ProviderRequestContext,
    model_id: &str,
) -> Result<GoogleEmbeddingRequestMapping, ProviderError> {
    let inputs = vertex_embedding_inputs(&request.input)?;
    let mut extra = request.extra.clone();
    extra.remove("model");
    extra.remove("input");
    extra.remove("user");

    validate_vertex_embedding_encoding_format(extra.remove("encoding_format"))?;
    let output_dimensionality =
        extract_vertex_embedding_output_dimensionality(&mut extra, model_id)?;

    if uses_vertex_embed_content(model_id) {
        return map_google_embed_content_request(inputs, extra, output_dimensionality, context);
    }

    let task_type = extract_vertex_embedding_task_type(&mut extra)?;
    let title = extract_optional_string_field(&mut extra, "title")?;
    if title.is_some() && task_type.as_deref() != Some("RETRIEVAL_DOCUMENT") {
        return Err(ProviderError::InvalidRequest(
            "vertex embeddings title is only supported with task_type RETRIEVAL_DOCUMENT"
                .to_string(),
        ));
    }
    let auto_truncate = extract_vertex_embedding_auto_truncate(&mut extra)?;
    if !extra.is_empty() {
        let unsupported = extra.keys().cloned().collect::<Vec<_>>().join(", ");
        return Err(ProviderError::InvalidRequest(format!(
            "unsupported vertex embeddings request field(s): {unsupported}"
        )));
    }

    let mut bodies = Vec::with_capacity(inputs.len());
    for input in inputs {
        let mut instance = Map::new();
        instance.insert("content".to_string(), Value::String(input));
        if let Some(task_type) = &task_type {
            instance.insert("task_type".to_string(), Value::String(task_type.clone()));
        }
        if let Some(title) = &title {
            instance.insert("title".to_string(), Value::String(title.clone()));
        }

        let mut body = Map::new();
        body.insert(
            "instances".to_string(),
            Value::Array(vec![Value::Object(instance)]),
        );

        let mut parameters = Map::new();
        if let Some(output_dimensionality) = output_dimensionality {
            parameters.insert(
                "outputDimensionality".to_string(),
                Value::Number(output_dimensionality.into()),
            );
        }
        if let Some(auto_truncate) = auto_truncate {
            parameters.insert("autoTruncate".to_string(), Value::Bool(auto_truncate));
        }
        if !parameters.is_empty() {
            body.insert("parameters".to_string(), Value::Object(parameters));
        }

        merge_object_overrides(&mut body, &context.extra_body);
        bodies.push(Value::Object(body));
    }

    Ok(GoogleEmbeddingRequestMapping { bodies })
}

fn map_google_embed_content_request(
    inputs: Vec<String>,
    mut extra: BTreeMap<String, Value>,
    output_dimensionality: Option<i64>,
    context: &ProviderRequestContext,
) -> Result<GoogleEmbeddingRequestMapping, ProviderError> {
    reject_vertex_embed_content_only_field(&mut extra, "task_type")?;
    reject_vertex_embed_content_only_field(&mut extra, "input_type")?;
    reject_vertex_embed_content_only_field(&mut extra, "title")?;
    reject_vertex_embed_content_only_field(&mut extra, "auto_truncate")?;
    reject_vertex_embed_content_only_field(&mut extra, "autoTruncate")?;

    if !extra.is_empty() {
        let unsupported = extra.keys().cloned().collect::<Vec<_>>().join(", ");
        return Err(ProviderError::InvalidRequest(format!(
            "unsupported vertex embeddings request field(s): {unsupported}"
        )));
    }

    let mut bodies = Vec::with_capacity(inputs.len());
    for input in inputs {
        let mut body = Map::new();
        body.insert(
            "content".to_string(),
            json!({
                "parts": [
                    { "text": input }
                ]
            }),
        );

        if let Some(output_dimensionality) = output_dimensionality {
            body.insert(
                "embedContentConfig".to_string(),
                json!({
                    "outputDimensionality": output_dimensionality
                }),
            );
        }

        merge_object_overrides(&mut body, &context.extra_body);
        bodies.push(Value::Object(body));
    }

    Ok(GoogleEmbeddingRequestMapping { bodies })
}

fn reject_vertex_embed_content_only_field(
    extra: &mut BTreeMap<String, Value>,
    field: &str,
) -> Result<(), ProviderError> {
    match extra.remove(field) {
        None | Some(Value::Null) => Ok(()),
        Some(_) => Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings google/gemini-embedding-2 does not support `{field}`; put task instructions in the input text"
        ))),
    }
}

fn vertex_embedding_inputs(input: &Value) -> Result<Vec<String>, ProviderError> {
    match input {
        Value::String(value) => {
            let value = validate_vertex_embedding_input_text(value)?;
            Ok(vec![value.to_string()])
        }
        Value::Array(values) => {
            if values.is_empty() {
                return Err(ProviderError::InvalidRequest(
                    "vertex embeddings input array must contain at least one string".to_string(),
                ));
            }

            values
                .iter()
                .map(|value| {
                    let Some(text) = value.as_str() else {
                        return Err(ProviderError::InvalidRequest(
                            "vertex embeddings input must be a string or array of strings; token arrays and multimodal inputs are not supported".to_string(),
                        ));
                    };
                    validate_vertex_embedding_input_text(text).map(str::to_string)
                })
                .collect()
        }
        _ => Err(ProviderError::InvalidRequest(
            "vertex embeddings input must be a string or array of strings".to_string(),
        )),
    }
}

fn validate_vertex_embedding_input_text(value: &str) -> Result<&str, ProviderError> {
    if value.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "vertex embeddings input strings must not be empty".to_string(),
        ));
    }
    Ok(value)
}

fn validate_vertex_embedding_encoding_format(value: Option<Value>) -> Result<(), ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(value)) if value == "float" => Ok(()),
        Some(Value::String(value)) => Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings encoding_format `{value}` is not supported; use `float`"
        ))),
        Some(_) => Err(ProviderError::InvalidRequest(
            "vertex embeddings encoding_format must be a string".to_string(),
        )),
    }
}

fn extract_vertex_embedding_output_dimensionality(
    extra: &mut BTreeMap<String, Value>,
    model_id: &str,
) -> Result<Option<i64>, ProviderError> {
    let mut selected: Option<(&'static str, i64)> = None;
    for field in [
        "dimensions",
        "output_dimensionality",
        "outputDimensionality",
    ] {
        let Some(value) = extra.remove(field) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let dimension = positive_i64_field(field, &value)?;
        if let Some((selected_field, selected_dimension)) = selected {
            if selected_dimension != dimension {
                return Err(ProviderError::InvalidRequest(format!(
                    "conflicting vertex embeddings dimensionality fields `{selected_field}` and `{field}`"
                )));
            }
        } else {
            selected = Some((field, dimension));
        }
    }

    let Some((field, dimension)) = selected else {
        return Ok(None);
    };
    let max_dimension = vertex_embedding_max_dimensions(model_id);
    if dimension > max_dimension {
        return Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings {field} must be <= {max_dimension} for google/{model_id}"
        )));
    }
    Ok(Some(dimension))
}

fn positive_i64_field(field: &str, value: &Value) -> Result<i64, ProviderError> {
    let Some(value) = value.as_i64() else {
        return Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings {field} must be a positive integer"
        )));
    };
    if value <= 0 {
        return Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings {field} must be a positive integer"
        )));
    }
    Ok(value)
}

fn vertex_embedding_max_dimensions(model_id: &str) -> i64 {
    match model_id {
        "gemini-embedding-001" | "gemini-embedding-2" => 3072,
        "text-embedding-005" | "text-multilingual-embedding-002" => 768,
        _ => unreachable!(
            "model_id is pre-validated by validate_vertex_embedding_model; \
             add a new arm here whenever VERTEX_TEXT_EMBEDDING_MODEL_IDS is extended"
        ),
    }
}

fn extract_vertex_embedding_task_type(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<String>, ProviderError> {
    let task_type = extra
        .remove("task_type")
        .map(|value| canonical_vertex_embedding_task_type("task_type", &value))
        .transpose()?
        .flatten();
    let input_type = extra
        .remove("input_type")
        .map(|value| canonical_vertex_embedding_task_type("input_type", &value))
        .transpose()?
        .flatten();

    match (task_type, input_type) {
        (Some(task_type), Some(input_type)) if task_type != input_type => {
            Err(ProviderError::InvalidRequest(
                "conflicting vertex embeddings task_type and input_type fields".to_string(),
            ))
        }
        (Some(task_type), _) | (_, Some(task_type)) => Ok(Some(task_type)),
        (None, None) => Ok(None),
    }
}

fn canonical_vertex_embedding_task_type(
    field: &str,
    value: &Value,
) -> Result<Option<String>, ProviderError> {
    if value.is_null() {
        return Ok(None);
    }
    let Some(value) = value.as_str() else {
        return Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings {field} must be a string"
        )));
    };
    let normalized = value.trim().replace(['-', ' '], "_").to_ascii_uppercase();
    let canonical = match normalized.as_str() {
        "QUERY" | "RETRIEVAL_QUERY" => "RETRIEVAL_QUERY",
        "DOCUMENT" | "RETRIEVAL_DOCUMENT" => "RETRIEVAL_DOCUMENT",
        "SEMANTIC_SIMILARITY" => "SEMANTIC_SIMILARITY",
        "CLASSIFICATION" => "CLASSIFICATION",
        "CLUSTERING" => "CLUSTERING",
        "QUESTION_ANSWERING" => "QUESTION_ANSWERING",
        "FACT_VERIFICATION" => "FACT_VERIFICATION",
        "CODE_RETRIEVAL_QUERY" => "CODE_RETRIEVAL_QUERY",
        _ => {
            return Err(ProviderError::InvalidRequest(format!(
                "unsupported vertex embeddings {field} `{value}`; supported task types are {}",
                VERTEX_EMBEDDING_TASK_TYPES.join(", ")
            )));
        }
    };
    Ok(Some(canonical.to_string()))
}

fn extract_optional_string_field(
    extra: &mut BTreeMap<String, Value>,
    field: &str,
) -> Result<Option<String>, ProviderError> {
    match extra.remove(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value)),
        Some(Value::String(_)) => Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings {field} must not be empty"
        ))),
        Some(_) => Err(ProviderError::InvalidRequest(format!(
            "vertex embeddings {field} must be a string"
        ))),
    }
}

fn extract_vertex_embedding_auto_truncate(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<bool>, ProviderError> {
    let snake = extra
        .remove("auto_truncate")
        .map(|value| optional_bool_field("auto_truncate", &value))
        .transpose()?
        .flatten();
    let camel = extra
        .remove("autoTruncate")
        .map(|value| optional_bool_field("autoTruncate", &value))
        .transpose()?
        .flatten();

    match (snake, camel) {
        (Some(snake), Some(camel)) if snake != camel => Err(ProviderError::InvalidRequest(
            "conflicting vertex embeddings auto_truncate and autoTruncate fields".to_string(),
        )),
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn optional_bool_field(field: &str, value: &Value) -> Result<Option<bool>, ProviderError> {
    if value.is_null() {
        return Ok(None);
    }
    value.as_bool().map(Some).ok_or_else(|| {
        ProviderError::InvalidRequest(format!("vertex embeddings {field} must be a boolean"))
    })
}

pub(super) fn extract_google_embedding_output(
    value: &Value,
    index: usize,
    model_id: &str,
) -> Result<GoogleEmbeddingOutput, ProviderError> {
    if uses_vertex_embed_content(model_id) {
        return extract_google_embed_content_output(value, index);
    }
    let prediction = value
        .get("predictions")
        .and_then(Value::as_array)
        .and_then(|predictions| predictions.first())
        .ok_or_else(|| {
            ProviderError::Transport(
                "invalid JSON from vertex embeddings: missing predictions[0]".to_string(),
            )
        })?;
    let embedding = prediction
        .get("embeddings")
        .and_then(|embeddings| embeddings.get("values"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProviderError::Transport(
                "invalid JSON from vertex embeddings: missing embeddings.values".to_string(),
            )
        })?;
    if embedding.iter().any(|value| value.as_f64().is_none()) {
        return Err(ProviderError::Transport(
            "invalid JSON from vertex embeddings: embedding values must be numbers".to_string(),
        ));
    }
    let token_count = prediction
        .get("embeddings")
        .and_then(|embeddings| embeddings.get("statistics"))
        .and_then(|statistics| statistics.get("token_count"))
        .and_then(Value::as_i64);

    Ok(GoogleEmbeddingOutput {
        index,
        embedding: Value::Array(embedding.clone()),
        token_count,
    })
}

fn extract_google_embed_content_output(
    value: &Value,
    index: usize,
) -> Result<GoogleEmbeddingOutput, ProviderError> {
    let embedding = value
        .get("embedding")
        .and_then(|embedding| embedding.get("values"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProviderError::Transport(
                "invalid JSON from vertex embeddings: missing embedding.values".to_string(),
            )
        })?;
    if embedding.iter().any(|value| value.as_f64().is_none()) {
        return Err(ProviderError::Transport(
            "invalid JSON from vertex embeddings: embedding values must be numbers".to_string(),
        ));
    }
    let token_count = value
        .get("usageMetadata")
        .and_then(|usage| usage.get("promptTokenCount"))
        .or_else(|| {
            value
                .get("usageMetadata")
                .and_then(|usage| usage.get("totalTokenCount"))
        })
        .and_then(Value::as_i64);

    Ok(GoogleEmbeddingOutput {
        index,
        embedding: Value::Array(embedding.clone()),
        token_count,
    })
}

pub(super) fn partial_google_embedding_failure(
    source: ProviderError,
    outputs: &[GoogleEmbeddingOutput],
    record_empty_usage: bool,
) -> ProviderError {
    if outputs.is_empty() && !record_empty_usage {
        return source;
    }

    ProviderError::PartialUsage {
        source: Box::new(source),
        provider_usage: google_embedding_usage_from_outputs(outputs).unwrap_or(None),
    }
}

fn google_embedding_usage_from_outputs(
    outputs: &[GoogleEmbeddingOutput],
) -> Result<Option<Value>, ProviderError> {
    let mut total_tokens: Option<i64> = Some(0);
    for output in outputs {
        if let (Some(current), Some(token_count)) = (total_tokens, output.token_count) {
            total_tokens = Some(current.checked_add(token_count).ok_or_else(|| {
                ProviderError::Transport(
                    "invalid JSON from vertex embeddings: token_count overflow".to_string(),
                )
            })?);
        } else {
            total_tokens = None;
        }
    }

    Ok(total_tokens.map(|total_tokens| {
        json!({
            "prompt_tokens": total_tokens,
            "total_tokens": total_tokens
        })
    }))
}

pub(super) fn normalize_google_embedding_outputs(
    outputs: Vec<GoogleEmbeddingOutput>,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut data = Vec::with_capacity(outputs.len());
    let usage = google_embedding_usage_from_outputs(&outputs)?;

    for output in outputs {
        data.push(json!({
            "object": "embedding",
            "index": output.index,
            "embedding": output.embedding
        }));
    }

    let mut response = Map::new();
    response.insert("object".to_string(), Value::String("list".to_string()));
    response.insert("data".to_string(), Value::Array(data));
    response.insert(
        "model".to_string(),
        Value::String(context.model_key.clone()),
    );
    if let Some(usage) = usage {
        response.insert("usage".to_string(), usage);
    }

    Ok(Value::Object(response))
}
