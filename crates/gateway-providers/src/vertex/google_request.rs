use gateway_core::{CoreChatRequest, ProviderError, ProviderRequestContext, ReasoningEffort};
use serde_json::{Map, Value, json};

use super::{
    error::VertexAdapterError,
    gemini::{GeminiModel, ThinkingControl},
    google_content::FunctionIdPolicy,
    google_conversation::map_google_contents,
    google_tools::{convert_openai_tools_for_google, reject_google_strict_tools},
};

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

    reject_google_strict_tools(request.extra.get("tools"))?;
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
    if model.is_some_and(GeminiModel::deprecates_sampling) {
        enforce_deprecated_sampling(&mut body, model_id)?;
    }
    convert_openai_tools_for_google(&mut body)?;
    reject_google_streamed_function_call_arguments(&body)?;
    validate_google_stream_candidate_count(&body, stream)?;
    Ok(Value::Object(body))
}

/// Gemini 3.6+ ignores `temperature`/`topP`/`topK` and errors on `presencePenalty`,
/// `frequencyPenalty`, and `candidateCount`. Runs on the fully merged body so mapped OpenAI
/// keys, caller `generationConfig`, and route `extra_body` are all covered; the ignored keys are
/// dropped and the erroring keys are refused unless they carry the no-op default.
fn enforce_deprecated_sampling(
    body: &mut Map<String, Value>,
    model_id: &str,
) -> Result<(), VertexAdapterError> {
    let Some(config) = body
        .get_mut("generationConfig")
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    for key in ["temperature", "topP", "topK"] {
        config.remove(key);
    }
    for (key, noop) in [
        ("presencePenalty", 0.0),
        ("frequencyPenalty", 0.0),
        ("candidateCount", 1.0),
    ] {
        let Some(value) = config.get(key) else {
            continue;
        };
        if value.is_null() || value.as_f64() == Some(noop) {
            config.remove(key);
            continue;
        }
        return Err(VertexAdapterError::UnsupportedSamplingField {
            field: key.to_string(),
            model_id: model_id.to_string(),
        });
    }
    if config.is_empty() {
        body.remove("generationConfig");
    }
    Ok(())
}

fn insert_config(generation_config: &mut Map<String, Value>, key: &str, value: &Value) {
    generation_config.insert(key.to_string(), value.clone());
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
    if format
        .get("json_schema")
        .and_then(|schema| schema.get("strict"))
        == Some(&Value::Bool(true))
    {
        return Err(VertexAdapterError::UnsupportedStrictControl);
    }
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
