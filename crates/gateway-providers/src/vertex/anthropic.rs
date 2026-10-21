use std::collections::BTreeMap;

use gateway_core::{CoreChatRequest, ProviderError, ProviderRequestContext};
use serde_json::{Map, Value};

use crate::anthropic::{
    AnthropicRequestOptions, COUNT_TOKENS_FIELDS, ClaudeThinkingPolicy, claude_thinking_policy,
    map_anthropic_request, retain_count_tokens_fields,
};

/// Body field Vertex `rawPredict` requires instead of the `anthropic-version` header.
const VERTEX_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";

/// Beta the ManualWithEffort Claude policy (Opus 4.5) needs before `output_config.effort` works.
const EFFORT_BETA: &str = "effort-2025-11-24";

/// Maps a chat request to the Anthropic Messages body Vertex `rawPredict` accepts.
///
/// Vertex ignores the `anthropic-beta` HTTP header, so every beta the route or provider
/// configured as a header is moved into the `anthropic_beta` body array.
pub(super) fn map_vertex_anthropic_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
    default_headers: &BTreeMap<String, String>,
) -> Result<Value, ProviderError> {
    let options = AnthropicRequestOptions {
        include_model: false,
        anthropic_version_body: Some(VERTEX_ANTHROPIC_VERSION),
        default_max_tokens: Some(4096),
        default_headers: Some(default_headers),
    };
    let mut body = map_anthropic_request(request, context, stream, &options)?;
    if let Some(object) = body.as_object_mut() {
        apply_body_betas(object, context, default_headers)?;
    }
    Ok(body)
}

/// Maps a chat request to the body of the Vertex Claude `count-tokens:rawPredict` call.
///
/// The count endpoint is a shared model path, so the Claude model travels in the
/// body. Vertex rejects a versioned id there, so the `@version` suffix is dropped.
pub(super) fn map_vertex_anthropic_count_tokens_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    model_id: &str,
    default_headers: &BTreeMap<String, String>,
) -> Result<Value, ProviderError> {
    let mut body = map_vertex_anthropic_request(request, context, false, default_headers)?;
    if let Some(object) = body.as_object_mut() {
        let model = model_id
            .split_once('@')
            .map_or(model_id, |(model, _)| model);
        object.insert("model".to_string(), Value::String(model.to_string()));
    }
    let fields: Vec<&str> = COUNT_TOKENS_FIELDS
        .iter()
        .copied()
        .chain(["anthropic_version", "anthropic_beta"])
        .collect();
    Ok(retain_count_tokens_fields(body, &fields))
}

fn apply_body_betas(
    body: &mut Map<String, Value>,
    context: &ProviderRequestContext,
    default_headers: &BTreeMap<String, String>,
) -> Result<(), ProviderError> {
    let mut betas: Vec<String> = match body.remove("anthropic_beta") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "`anthropic_beta` must be an array of strings for Vertex Anthropic mapping"
                        .to_string(),
                )
            })?,
        Some(Value::String(value)) => split_betas(&value).map(str::to_string).collect(),
        Some(_) => {
            return Err(ProviderError::InvalidRequest(
                "`anthropic_beta` must be an array of strings for Vertex Anthropic mapping"
                    .to_string(),
            ));
        }
    };
    let header_betas = default_headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.as_str())
        .chain(
            context
                .extra_headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
                .filter_map(|(_, value)| value.as_str()),
        )
        .flat_map(split_betas);
    for beta in header_betas {
        push_unique(&mut betas, beta);
    }
    if body
        .get("output_config")
        .and_then(|config| config.get("effort"))
        .is_some_and(|effort| !effort.is_null())
        && claude_thinking_policy(&context.upstream_model) == ClaudeThinkingPolicy::ManualWithEffort
    {
        push_unique(&mut betas, EFFORT_BETA);
    }
    if !betas.is_empty() {
        body.insert(
            "anthropic_beta".to_string(),
            Value::Array(betas.into_iter().map(Value::String).collect()),
        );
    }
    Ok(())
}

fn split_betas(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|beta| !beta.is_empty())
}

fn push_unique(betas: &mut Vec<String>, beta: &str) {
    if !betas.iter().any(|existing| existing == beta) {
        betas.push(beta.to_string());
    }
}
