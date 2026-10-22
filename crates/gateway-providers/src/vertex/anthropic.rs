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
/// Betas are collected in an internal body array, then moved into one `anthropic-beta`
/// header by the HTTP transport. Vertex reads only the first occurrence of that header.
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
    let needs_effort_beta = body
        .get("output_config")
        .and_then(|config| config.get("effort"))
        .is_some_and(|effort| !effort.is_null())
        && claude_thinking_policy(&context.upstream_model)
            == ClaudeThinkingPolicy::ManualWithEffort;
    let betas = crate::anthropic::beta::merged_request_betas(
        body.get("anthropic_beta"),
        context,
        Some(default_headers),
        if needs_effort_beta {
            &[EFFORT_BETA]
        } else {
            &[]
        },
    )?;
    body.remove("anthropic_beta");
    crate::anthropic::beta::put_body_betas(body, betas);
    Ok(())
}
