//! Native Bedrock Messages retain their schema while applying platform versions and betas.
use super::*;

pub(super) fn map_native_anthropic_messages(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    target: AnthropicMessagesTarget,
    betas: Vec<String>,
) -> Result<Value, ProviderError> {
    if !["max_tokens", "max_completion_tokens"].iter().any(|field| {
        request
            .extra
            .get(*field)
            .or_else(|| context.extra_body.get(*field))
            .is_some_and(|value| !value.is_null())
    }) {
        return Err(ProviderError::InvalidRequest(
            "Anthropic Messages requires `max_tokens` or `max_completion_tokens`".into(),
        ));
    }
    let runtime = target == AnthropicMessagesTarget::RuntimeInvoke;
    let options = crate::anthropic::AnthropicRequestOptions {
        include_model: !runtime,
        anthropic_version_body: runtime.then_some("bedrock-2023-05-31"),
        ..Default::default()
    };
    let manual_effort = crate::anthropic::claude_thinking_policy(&context.upstream_model)
        == crate::anthropic::ClaudeThinkingPolicy::ManualWithEffort;
    let mut aliases = BTreeMap::new();
    let mut body = if manual_effort {
        let mut native_request = request.clone();
        let mut native_context = context.clone();
        for field in [
            "reasoning",
            "reasoning_effort",
            "thinking_budget_tokens",
            "reasoning_budget_tokens",
        ] {
            if let Some(value) = native_context
                .extra_body
                .remove(field)
                .or_else(|| native_request.extra.remove(field))
            {
                aliases.insert(field.to_string(), value);
            }
            native_request.extra.remove(field);
        }
        crate::anthropic::map_anthropic_request(
            &native_request,
            &native_context,
            request.stream,
            &options,
        )?
    } else {
        crate::anthropic::map_anthropic_request(request, context, request.stream, &options)?
    };
    let object = body.as_object_mut().expect("Messages body");
    object.remove("anthropic_beta");
    crate::anthropic::beta::put_body_betas(object, betas);
    if manual_effort {
        apply_messages_thinking(object, &mut aliases, &context.upstream_model)?;
    }
    if runtime {
        object.remove("stream");
    }
    Ok(body)
}
