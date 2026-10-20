use super::*;
use crate::anthropic::{
    AnthropicAdapterError, ClaudeThinkingOff, ClaudeThinkingPolicy, claude_thinking_off,
    claude_thinking_policy,
    thinking::{resolve_thinking_off, validate_adaptive_effort},
};

pub(super) fn apply_anthropic_thinking_compatibility(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let reasoning_effort = extract_anthropic_reasoning_effort(extra)?;
    let native_effort = extract_existing_anthropic_output_effort(body)?;
    let has_native_effort = native_effort.is_some();
    let effort = merge_optional_efforts(reasoning_effort, native_effort, upstream_model)?;
    let budget_tokens = extract_anthropic_reasoning_budget_tokens(extra);
    let policy = claude_thinking_policy(upstream_model);

    validate_caller_thinking_for_policy(body, policy, upstream_model)?;

    let caller_thinking_type = body
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str);
    if policy == ClaudeThinkingPolicy::AdaptiveOnly
        && matches!(caller_thinking_type, Some("disabled" | "between_tools"))
    {
        if budget_tokens.is_some() {
            return Err(ProviderError::InvalidRequest(format!(
                "`reasoning.budget_tokens` is not supported for `{upstream_model}`; use adaptive thinking with `reasoning_effort` or `output_config.effort`"
            )));
        }
        return apply_anthropic_thinking_off(body, effort, upstream_model);
    }

    if let Some(effort) = effort {
        match policy {
            ClaudeThinkingPolicy::AdaptiveOnly
            | ClaudeThinkingPolicy::AdaptivePreferred
            | ClaudeThinkingPolicy::MythosPreview => {
                if policy == ClaudeThinkingPolicy::AdaptiveOnly {
                    validate_adaptive_effort(&effort, upstream_model)?;
                }
                ensure_anthropic_adaptive_thinking(body, upstream_model)?;
                merge_anthropic_output_effort(body, effort, upstream_model)?;
            }
            ClaudeThinkingPolicy::ManualWithEffort => {
                if let Some(budget_tokens) =
                    budget_tokens.or_else(|| existing_manual_thinking_budget(body))
                {
                    ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
                }
                merge_anthropic_output_effort(body, effort, upstream_model)?;
                ensure_anthropic_beta(body, "effort-2025-11-24")?;
            }
            ClaudeThinkingPolicy::ManualOnly => {
                if has_native_effort {
                    return Err(ProviderError::InvalidRequest(format!(
                        "`output_config.effort` is not supported for `{upstream_model}`"
                    )));
                }
                let budget_tokens = budget_tokens
                    .or_else(|| existing_manual_thinking_budget(body))
                    .ok_or_else(|| {
                    ProviderError::InvalidRequest(format!(
                        "`reasoning_effort` requires an explicit manual thinking budget for `{upstream_model}` because this Claude model does not support adaptive thinking"
                    ))
                })?;
                ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
            }
        }
    } else if let Some(budget_tokens) = budget_tokens {
        match policy {
            ClaudeThinkingPolicy::AdaptiveOnly => {
                return Err(ProviderError::InvalidRequest(format!(
                    "`reasoning.budget_tokens` is not supported for `{upstream_model}`; use adaptive thinking with `reasoning_effort` or `output_config.effort`"
                )));
            }
            ClaudeThinkingPolicy::AdaptivePreferred
            | ClaudeThinkingPolicy::ManualWithEffort
            | ClaudeThinkingPolicy::ManualOnly
            | ClaudeThinkingPolicy::MythosPreview => {
                ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
            }
        }
    }

    Ok(())
}

/// Keeps a caller's request to turn thinking off on an adaptive-only model, in the form that
/// model accepts, using the rules shared with the Anthropic adapter.
fn apply_anthropic_thinking_off(
    body: &mut Map<String, Value>,
    effort: Option<Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let thinking = body
        .get("thinking")
        .and_then(Value::as_object)
        .ok_or(AnthropicAdapterError::InvalidThinkingObject)?;
    let thinking = resolve_thinking_off(thinking, effort.as_ref(), upstream_model)?;
    body.insert("thinking".to_string(), thinking);
    if let Some(effort) = effort {
        merge_anthropic_output_effort(body, effort, upstream_model)?;
    }
    Ok(())
}

pub(super) fn extract_anthropic_reasoning_effort(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<Value>, ProviderError> {
    let reasoning_effort = extra
        .remove("reasoning_effort")
        .filter(|value| !value.is_null());
    let reasoning = extra.remove("reasoning");

    match (reasoning_effort, reasoning) {
        (Some(effort), None) => Ok(Some(effort)),
        (None, Some(Value::Object(mut reasoning))) => {
            if let Some(budget_tokens) = reasoning.remove("budget_tokens") {
                extra.insert("reasoning_budget_tokens".to_string(), budget_tokens);
            }
            Ok(reasoning.remove("effort").filter(|value| !value.is_null()))
        }
        (Some(effort), Some(Value::Object(mut reasoning))) => {
            if let Some(reasoning_effort) =
                reasoning.remove("effort").filter(|value| !value.is_null())
                && reasoning_effort != effort
            {
                return Err(ProviderError::InvalidRequest(
                    "`reasoning_effort` conflicts with `reasoning.effort` for Anthropic Claude mapping"
                        .to_string(),
                ));
            }
            if let Some(budget_tokens) = reasoning.remove("budget_tokens") {
                extra.insert("reasoning_budget_tokens".to_string(), budget_tokens);
            }
            Ok(Some(effort))
        }
        (None, Some(Value::Null)) => Ok(None),
        (Some(effort), Some(Value::Null)) => Ok(Some(effort)),
        (_, Some(_)) => Err(ProviderError::InvalidRequest(
            "`reasoning` must be an object for Anthropic Claude mapping".to_string(),
        )),
        (None, None) => Ok(None),
    }
}

pub(super) fn extract_existing_anthropic_output_effort(
    body: &mut Map<String, Value>,
) -> Result<Option<Value>, ProviderError> {
    let (effort, remove_output_config) = {
        let Some(output_config) = body.get_mut("output_config") else {
            return Ok(None);
        };
        let output_config = output_config.as_object_mut().ok_or_else(|| {
            ProviderError::InvalidRequest(
                "`output_config` must be an object for Anthropic Claude mapping".to_string(),
            )
        })?;

        let effort = output_config.get("effort").cloned();
        if effort.as_ref().is_some_and(Value::is_null) {
            output_config.remove("effort");
            (None, output_config.is_empty())
        } else {
            (effort, false)
        }
    };
    if remove_output_config {
        body.remove("output_config");
    }

    Ok(effort)
}

pub(super) fn merge_optional_efforts(
    reasoning_effort: Option<Value>,
    native_effort: Option<Value>,
    upstream_model: &str,
) -> Result<Option<Value>, ProviderError> {
    match (reasoning_effort, native_effort) {
        (Some(reasoning_effort), Some(native_effort)) if reasoning_effort != native_effort => {
            Err(ProviderError::InvalidRequest(format!(
                "`reasoning_effort` conflicts with `output_config.effort` for `{upstream_model}`"
            )))
        }
        (Some(reasoning_effort), _) => Ok(Some(reasoning_effort)),
        (None, Some(native_effort)) => Ok(Some(native_effort)),
        (None, None) => Ok(None),
    }
}

/// A null budget counts as absent, as on the Anthropic adapter.
pub(super) fn extract_anthropic_reasoning_budget_tokens(
    extra: &mut BTreeMap<String, Value>,
) -> Option<Value> {
    for key in ["thinking_budget_tokens", "reasoning_budget_tokens"] {
        if let Some(value) = extra.remove(key)
            && !value.is_null()
        {
            return Some(value);
        }
    }
    None
}

pub(super) fn validate_caller_thinking_for_policy(
    body: &Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let Some(thinking) = body.get("thinking") else {
        return Ok(());
    };
    let thinking = thinking.as_object().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "`thinking` must be an object for Anthropic Messages".to_string(),
        )
    })?;
    let thinking_type = thinking.get("type").and_then(Value::as_str);
    if thinking_type == Some("between_tools")
        && claude_thinking_off(upstream_model) != ClaudeThinkingOff::BetweenTools
    {
        return Err(AnthropicAdapterError::BetweenToolsNotSupported {
            model: upstream_model.to_string(),
        }
        .into());
    }

    match policy {
        ClaudeThinkingPolicy::AdaptiveOnly => {
            if thinking_type == Some("enabled") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: enabled` with manual `budget_tokens` is not supported for `{upstream_model}`; use `thinking.type: adaptive` and `output_config.effort`"
                )));
            }
            if thinking
                .get("budget_tokens")
                .is_some_and(|value| !value.is_null())
            {
                return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                    model: upstream_model.to_string(),
                }
                .into());
            }
        }
        ClaudeThinkingPolicy::ManualOnly => {
            if thinking_type == Some("adaptive") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: adaptive` is not supported for `{upstream_model}`; use `thinking.type: enabled` with `budget_tokens`"
                )));
            }
        }
        ClaudeThinkingPolicy::ManualWithEffort => {
            if thinking_type == Some("adaptive") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: adaptive` is not supported for `{upstream_model}`; use `thinking.type: enabled` with `budget_tokens`"
                )));
            }
        }
        ClaudeThinkingPolicy::MythosPreview => {
            if thinking_type == Some("disabled") {
                return Err(ProviderError::InvalidRequest(
                    "`thinking.type: disabled` is not supported for Claude Mythos Preview"
                        .to_string(),
                ));
            }
        }
        ClaudeThinkingPolicy::AdaptivePreferred => {}
    }

    if thinking_type == Some("enabled")
        && thinking
            .get("budget_tokens")
            .is_none_or(|value| value.is_null())
    {
        return Err(ProviderError::InvalidRequest(format!(
            "`thinking.type: enabled` for `{upstream_model}` must include `budget_tokens`"
        )));
    }

    Ok(())
}

pub(super) fn ensure_anthropic_adaptive_thinking(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    match body.get("thinking") {
        None => {
            body.insert("thinking".to_string(), json!({ "type": "adaptive" }));
            Ok(())
        }
        Some(Value::Object(object))
            if object.get("type").and_then(Value::as_str) == Some("adaptive") =>
        {
            Ok(())
        }
        Some(_) => Err(ProviderError::InvalidRequest(format!(
            "`reasoning_effort` requires `thinking.type: adaptive` for `{upstream_model}` and conflicts with caller-supplied `thinking`"
        ))),
    }
}

pub(super) fn ensure_anthropic_manual_thinking(
    body: &mut Map<String, Value>,
    budget_tokens: Value,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    match body.get("thinking") {
        None => {
            body.insert(
                "thinking".to_string(),
                json!({ "type": "enabled", "budget_tokens": budget_tokens }),
            );
            Ok(())
        }
        Some(Value::Object(object))
            if object.get("type").and_then(Value::as_str) == Some("enabled") =>
        {
            match object.get("budget_tokens") {
                Some(existing) if existing == &budget_tokens => Ok(()),
                Some(_) => Err(ProviderError::InvalidRequest(format!(
                    "manual Anthropic thinking budget for `{upstream_model}` conflicts with caller-supplied `thinking.budget_tokens`"
                ))),
                None => Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: enabled` for `{upstream_model}` must include `budget_tokens`"
                ))),
            }
        }
        Some(_) => Err(ProviderError::InvalidRequest(format!(
            "manual Anthropic thinking budget for `{upstream_model}` conflicts with caller-supplied `thinking`"
        ))),
    }
}

pub(super) fn existing_manual_thinking_budget(body: &Map<String, Value>) -> Option<Value> {
    let thinking = body.get("thinking")?.as_object()?;
    if thinking.get("type").and_then(Value::as_str) == Some("enabled") {
        thinking.get("budget_tokens").cloned()
    } else {
        None
    }
}

pub(super) fn merge_anthropic_output_effort(
    body: &mut Map<String, Value>,
    effort: Value,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    match body.get_mut("output_config") {
        None => {
            body.insert("output_config".to_string(), json!({ "effort": effort }));
            Ok(())
        }
        Some(Value::Object(output_config)) => match output_config.get("effort") {
            Some(existing) if existing != &effort => Err(ProviderError::InvalidRequest(format!(
                "`reasoning_effort` conflicts with `output_config.effort` for `{upstream_model}`"
            ))),
            Some(_) => Ok(()),
            None => {
                output_config.insert("effort".to_string(), effort);
                Ok(())
            }
        },
        Some(_) => Err(ProviderError::InvalidRequest(
            "`output_config` must be an object for Anthropic Claude mapping".to_string(),
        )),
    }
}

pub(super) fn validate_anthropic_sampling_fields(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if claude_thinking_policy(upstream_model) != ClaudeThinkingPolicy::AdaptiveOnly {
        return Ok(());
    }

    for field in ["temperature", "top_p", "top_k"] {
        let Some(value) = body.get(field) else {
            continue;
        };
        if value.is_null() || is_default_anthropic_sampling_value(field, value) {
            body.remove(field);
            continue;
        }
        return Err(ProviderError::InvalidRequest(format!(
            "`{field}` is not supported with non-default values for `{upstream_model}`; omit the field for adaptive-only Claude models"
        )));
    }

    Ok(())
}

pub(super) fn is_default_anthropic_sampling_value(field: &str, value: &Value) -> bool {
    match field {
        "temperature" | "top_p" => value
            .as_f64()
            .is_some_and(|number| (number - 1.0).abs() < f64::EPSILON),
        "top_k" => false,
        _ => false,
    }
}

pub(super) fn apply_converse_anthropic_thinking_compatibility(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if !is_anthropic_claude_model(upstream_model) {
        return Ok(());
    }

    let effort = extract_anthropic_reasoning_effort(extra)?;
    let budget_tokens = extract_anthropic_reasoning_budget_tokens(extra);
    let policy = claude_thinking_policy(upstream_model);
    move_converse_thinking_effort(body, upstream_model)?;

    let caller_thinking_type = body
        .get("additionalModelRequestFields")
        .and_then(|additional| additional.get("thinking"))
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str);
    if policy == ClaudeThinkingPolicy::AdaptiveOnly
        && matches!(caller_thinking_type, Some("disabled" | "between_tools"))
    {
        if budget_tokens.is_some() {
            return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                model: upstream_model.to_string(),
            }
            .into());
        }
        let additional = ensure_additional_model_request_fields(body)?;
        return apply_converse_thinking_off(additional, effort, upstream_model);
    }

    // A native `output_config.effort` turns on adaptive thinking, as on the Messages paths.
    let effort = effort.or_else(|| {
        matches!(
            policy,
            ClaudeThinkingPolicy::AdaptiveOnly
                | ClaudeThinkingPolicy::AdaptivePreferred
                | ClaudeThinkingPolicy::MythosPreview
        )
        .then(|| {
            body.get("additionalModelRequestFields")
                .and_then(Value::as_object)
                .and_then(converse_output_effort)
                .cloned()
        })
        .flatten()
    });

    if effort.is_none() && budget_tokens.is_none() {
        validate_converse_caller_thinking_for_policy(body, policy, upstream_model)?;
        return Ok(());
    }

    let additional = ensure_additional_model_request_fields(body)?;
    validate_converse_caller_thinking_for_policy_object(additional, policy, upstream_model)?;

    if let Some(effort) = effort {
        match policy {
            ClaudeThinkingPolicy::AdaptiveOnly
            | ClaudeThinkingPolicy::AdaptivePreferred
            | ClaudeThinkingPolicy::MythosPreview => {
                if policy == ClaudeThinkingPolicy::AdaptiveOnly {
                    validate_adaptive_effort(&effort, upstream_model)?;
                }
                merge_converse_thinking_field(
                    additional,
                    "type",
                    json!("adaptive"),
                    upstream_model,
                )?;
                merge_converse_output_effort(additional, effort, upstream_model)?;
            }
            ClaudeThinkingPolicy::ManualWithEffort => {
                let budget_tokens = budget_tokens
                    .or_else(|| existing_converse_manual_thinking_budget(additional))
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(format!(
                            "`reasoning_effort` requires an explicit manual thinking budget for `{upstream_model}` because this Claude model requires manual thinking when Bedrock effort is used"
                        ))
                    })?;
                merge_converse_thinking_field(
                    additional,
                    "type",
                    json!("enabled"),
                    upstream_model,
                )?;
                merge_converse_thinking_field(
                    additional,
                    "budget_tokens",
                    budget_tokens,
                    upstream_model,
                )?;
                merge_converse_output_effort(additional, effort, upstream_model)?;
            }
            ClaudeThinkingPolicy::ManualOnly => {
                let budget_tokens = budget_tokens
                    .or_else(|| existing_converse_manual_thinking_budget(additional))
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(format!(
                            "`reasoning_effort` requires an explicit manual thinking budget for `{upstream_model}` because this Claude model does not support adaptive thinking or Bedrock effort"
                        ))
                    })?;
                merge_converse_thinking_field(
                    additional,
                    "type",
                    json!("enabled"),
                    upstream_model,
                )?;
                merge_converse_thinking_field(
                    additional,
                    "budget_tokens",
                    budget_tokens,
                    upstream_model,
                )?;
            }
        }
    } else if let Some(budget_tokens) = budget_tokens {
        match policy {
            ClaudeThinkingPolicy::AdaptiveOnly => {
                return Err(ProviderError::InvalidRequest(format!(
                    "`reasoning.budget_tokens` is not supported for `{upstream_model}`; use adaptive thinking with `reasoning_effort`"
                )));
            }
            ClaudeThinkingPolicy::AdaptivePreferred
            | ClaudeThinkingPolicy::ManualWithEffort
            | ClaudeThinkingPolicy::ManualOnly
            | ClaudeThinkingPolicy::MythosPreview => {
                merge_converse_thinking_field(
                    additional,
                    "type",
                    json!("enabled"),
                    upstream_model,
                )?;
                merge_converse_thinking_field(
                    additional,
                    "budget_tokens",
                    budget_tokens,
                    upstream_model,
                )?;
            }
        }
    }

    Ok(())
}

/// Applies the shared thinking-off rules to `additionalModelRequestFields.thinking`. The thinking
/// object is replaced by the resolved type-only form, so caller fields such as `display` are
/// dropped as on Messages routes; effort stays in `output_config`.
fn apply_converse_thinking_off(
    additional: &mut Map<String, Value>,
    effort: Option<Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if let Some(effort) = effort {
        merge_converse_output_effort(additional, effort, upstream_model)?;
    }
    let effort = converse_output_effort(additional).cloned();
    let thinking = additional
        .get("thinking")
        .and_then(Value::as_object)
        .ok_or(AnthropicAdapterError::InvalidThinkingObject)?;
    let resolved = resolve_thinking_off(thinking, effort.as_ref(), upstream_model)?;
    additional.insert("thinking".to_string(), resolved);
    Ok(())
}

/// Bedrock Converse takes Claude effort in `additionalModelRequestFields.output_config.effort`
/// and rejects it inside `thinking`, so a caller's `thinking.effort` is moved there.
fn move_converse_thinking_effort(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let Some(additional) = body
        .get_mut("additionalModelRequestFields")
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    let Some(effort) = additional
        .get_mut("thinking")
        .and_then(Value::as_object_mut)
        .and_then(|thinking| thinking.remove("effort"))
        .filter(|effort| !effort.is_null())
    else {
        return Ok(());
    };
    merge_converse_output_effort(additional, effort, upstream_model)
}

fn merge_converse_output_effort(
    additional: &mut Map<String, Value>,
    effort: Value,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let Value::Object(output_config) = additional
        .entry("output_config".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
    else {
        return Err(ProviderError::InvalidRequest(
            "`additionalModelRequestFields.output_config` must be an object for aws_bedrock Converse"
                .to_string(),
        ));
    };
    match output_config.get("effort").filter(|value| !value.is_null()) {
        Some(existing) if existing != &effort => Err(ProviderError::InvalidRequest(format!(
            "`reasoning_effort` conflicts with `additionalModelRequestFields.output_config.effort` for `{upstream_model}`"
        ))),
        _ => {
            output_config.insert("effort".to_string(), effort);
            Ok(())
        }
    }
}

fn converse_output_effort(additional: &Map<String, Value>) -> Option<&Value> {
    additional
        .get("output_config")?
        .get("effort")
        .filter(|value| !value.is_null())
}

pub(super) fn ensure_additional_model_request_fields(
    body: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, ProviderError> {
    if !body.contains_key("additionalModelRequestFields") {
        body.insert(
            "additionalModelRequestFields".to_string(),
            Value::Object(Map::new()),
        );
    }
    body.get_mut("additionalModelRequestFields")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "`additionalModelRequestFields` must be an object for aws_bedrock Converse"
                    .to_string(),
            )
        })
}

pub(super) fn validate_converse_caller_thinking_for_policy(
    body: &Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let Some(additional) = body
        .get("additionalModelRequestFields")
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    validate_converse_caller_thinking_for_policy_object(additional, policy, upstream_model)
}

pub(super) fn validate_converse_caller_thinking_for_policy_object(
    additional: &Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if policy == ClaudeThinkingPolicy::AdaptiveOnly
        && let Some(effort) = converse_output_effort(additional)
    {
        validate_adaptive_effort(effort, upstream_model)?;
    }
    let Some(thinking) = additional.get("thinking") else {
        return Ok(());
    };
    let thinking = thinking.as_object().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "`additionalModelRequestFields.thinking` must be an object for aws_bedrock Converse"
                .to_string(),
        )
    })?;
    let thinking_type = thinking.get("type").and_then(Value::as_str);
    if thinking_type == Some("between_tools")
        && claude_thinking_off(upstream_model) != ClaudeThinkingOff::BetweenTools
    {
        return Err(AnthropicAdapterError::BetweenToolsNotSupported {
            model: upstream_model.to_string(),
        }
        .into());
    }

    match policy {
        ClaudeThinkingPolicy::AdaptiveOnly => {
            if thinking_type == Some("enabled") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`additionalModelRequestFields.thinking.type: enabled` is not supported for `{upstream_model}`; use adaptive thinking"
                )));
            }
            if thinking
                .get("budget_tokens")
                .is_some_and(|value| !value.is_null())
            {
                return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                    model: upstream_model.to_string(),
                }
                .into());
            }
        }
        ClaudeThinkingPolicy::ManualOnly | ClaudeThinkingPolicy::ManualWithEffort => {
            if thinking_type == Some("adaptive") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`additionalModelRequestFields.thinking.type: adaptive` is not supported for `{upstream_model}`; use manual `budget_tokens`"
                )));
            }
        }
        ClaudeThinkingPolicy::MythosPreview => {
            if thinking_type == Some("disabled") {
                return Err(ProviderError::InvalidRequest(
                    "`additionalModelRequestFields.thinking.type: disabled` is not supported for Claude Mythos Preview"
                        .to_string(),
                ));
            }
        }
        ClaudeThinkingPolicy::AdaptivePreferred => {}
    }
    if thinking_type == Some("enabled")
        && thinking
            .get("budget_tokens")
            .is_none_or(|value| value.is_null())
    {
        return Err(ProviderError::InvalidRequest(format!(
            "`additionalModelRequestFields.thinking.type: enabled` for `{upstream_model}` must include `budget_tokens`"
        )));
    }
    Ok(())
}

pub(super) fn merge_converse_thinking_field(
    additional: &mut Map<String, Value>,
    field: &str,
    value: Value,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if !additional.contains_key("thinking") {
        additional.insert("thinking".to_string(), Value::Object(Map::new()));
    }
    let thinking = additional
        .get_mut("thinking")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "`additionalModelRequestFields.thinking` must be an object for aws_bedrock Converse"
                    .to_string(),
            )
        })?;

    match thinking.get(field) {
        Some(existing) if existing != &value => Err(ProviderError::InvalidRequest(format!(
            "`reasoning_effort` conflicts with `additionalModelRequestFields.thinking.{field}` for `{upstream_model}`"
        ))),
        Some(_) => Ok(()),
        None => {
            thinking.insert(field.to_string(), value);
            Ok(())
        }
    }
}

pub(super) fn existing_converse_manual_thinking_budget(
    additional: &Map<String, Value>,
) -> Option<Value> {
    let thinking = additional.get("thinking")?.as_object()?;
    if thinking.get("type").and_then(Value::as_str) == Some("enabled") {
        thinking.get("budget_tokens").cloned()
    } else {
        None
    }
}

pub(super) fn validate_converse_anthropic_sampling_fields(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if !is_anthropic_claude_model(upstream_model)
        || claude_thinking_policy(upstream_model) != ClaudeThinkingPolicy::AdaptiveOnly
    {
        return Ok(());
    }

    if let Some(value) = extra.remove("top_k")
        && !value.is_null()
    {
        return Err(ProviderError::InvalidRequest(format!(
            "`top_k` is not supported for `{upstream_model}`; omit the field for adaptive-only Claude models"
        )));
    }

    for field in ["temperature", "top_p", "top_k"] {
        let Some(value) = body.get(field) else {
            continue;
        };
        if value.is_null() || is_default_anthropic_sampling_value(field, value) {
            body.remove(field);
            continue;
        }
        return Err(ProviderError::InvalidRequest(format!(
            "`{field}` is not supported with non-default values for `{upstream_model}`; omit the field for adaptive-only Claude models"
        )));
    }

    let Some(inference_config) = body
        .get_mut("inferenceConfig")
        .and_then(Value::as_object_mut)
    else {
        validate_converse_additional_top_k(body, upstream_model)?;
        return Ok(());
    };
    for (field, bedrock_field) in [("temperature", "temperature"), ("top_p", "topP")] {
        let Some(value) = inference_config.get(bedrock_field) else {
            continue;
        };
        if value.is_null() || is_default_anthropic_sampling_value(field, value) {
            inference_config.remove(bedrock_field);
            continue;
        }
        return Err(ProviderError::InvalidRequest(format!(
            "`{field}` is not supported with non-default values for `{upstream_model}`; omit the field for adaptive-only Claude models"
        )));
    }
    if inference_config.is_empty() {
        body.remove("inferenceConfig");
    }
    validate_converse_additional_top_k(body, upstream_model)
}

pub(super) fn validate_converse_additional_top_k(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let remove_additional = if let Some(additional) = body
        .get_mut("additionalModelRequestFields")
        .and_then(Value::as_object_mut)
    {
        for field in ["top_k", "topK"] {
            let Some(value) = additional.get(field) else {
                continue;
            };
            if value.is_null() {
                additional.remove(field);
                continue;
            }
            return Err(ProviderError::InvalidRequest(format!(
                "`{field}` is not supported for `{upstream_model}`; omit the field for adaptive-only Claude models"
            )));
        }
        additional.is_empty()
    } else {
        false
    };
    if remove_additional {
        body.remove("additionalModelRequestFields");
    }
    Ok(())
}

pub(super) fn ensure_anthropic_beta(
    body: &mut Map<String, Value>,
    beta: &str,
) -> Result<(), ProviderError> {
    match body.get_mut("anthropic_beta") {
        None => {
            body.insert(
                "anthropic_beta".to_string(),
                Value::Array(vec![Value::String(beta.to_string())]),
            );
            Ok(())
        }
        Some(Value::Array(values)) => {
            if !values.iter().any(|value| value.as_str() == Some(beta)) {
                values.push(Value::String(beta.to_string()));
            }
            Ok(())
        }
        Some(_) => Err(ProviderError::InvalidRequest(
            "`anthropic_beta` must be an array for Anthropic Claude mapping".to_string(),
        )),
    }
}
