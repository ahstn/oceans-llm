use serde_json::{Map, Value, json};

use super::error::AnthropicAdapterError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeThinkingPolicy {
    AdaptiveOnly,
    AdaptivePreferred,
    ManualWithEffort,
    ManualOnly,
    MythosPreview,
}

#[must_use]
pub fn claude_thinking_policy(upstream_model: &str) -> ClaudeThinkingPolicy {
    let model = upstream_model.to_ascii_lowercase();
    if model.contains("claude-mythos-preview") {
        ClaudeThinkingPolicy::MythosPreview
    } else if is_adaptive_only_claude(&model) {
        ClaudeThinkingPolicy::AdaptiveOnly
    } else if model.contains("claude-opus-4-6") || model.contains("claude-sonnet-4-6") {
        ClaudeThinkingPolicy::AdaptivePreferred
    } else if model.contains("claude-opus-4-5") {
        ClaudeThinkingPolicy::ManualWithEffort
    } else {
        ClaudeThinkingPolicy::ManualOnly
    }
}

pub fn is_adaptive_only_claude(model: &str) -> bool {
    is_opus_4_7_or_later(model)
        || contains_exact_claude_model_marker(model, "claude-fable-5")
        || contains_exact_claude_model_marker(model, "claude-sonnet-5")
}

pub fn contains_exact_claude_model_marker(model: &str, marker: &str) -> bool {
    model.split(marker).skip(1).any(|rest| {
        rest.chars().next().is_none_or(|ch| {
            ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']' | '-')
        })
    })
}

fn is_opus_4_7_or_later(model: &str) -> bool {
    let Some(rest) = model.split("claude-opus-4-").nth(1) else {
        return false;
    };
    rest.split(|ch: char| !ch.is_ascii_digit())
        .next()
        .and_then(|minor| minor.parse::<u16>().ok())
        .is_some_and(|minor| minor >= 7)
}

const ADAPTIVE_EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

fn validate_adaptive_effort(effort: &Value, model: &str) -> Result<(), AnthropicAdapterError> {
    let effort_str = effort.as_str().unwrap_or_default();
    if !ADAPTIVE_EFFORT_LEVELS.contains(&effort_str) {
        return Err(AnthropicAdapterError::UnsupportedAdaptiveEffort {
            effort: effort.to_string(),
            model: model.to_string(),
        });
    }
    Ok(())
}

pub fn apply_anthropic_thinking_compatibility(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    let reasoning_effort = extract_anthropic_reasoning_effort(body)?;
    let native_effort = extract_existing_anthropic_output_effort(body)?;
    let has_native_effort = native_effort.is_some();
    let effort = merge_optional_efforts(reasoning_effort, native_effort, upstream_model)?;
    let budget_tokens = extract_anthropic_reasoning_budget_tokens(body)?;
    let policy = claude_thinking_policy(upstream_model);

    validate_caller_thinking_for_policy(body, policy, upstream_model)?;

    if policy == ClaudeThinkingPolicy::AdaptiveOnly {
        if budget_tokens.is_some() {
            return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                model: upstream_model.to_string(),
            });
        }
        let effort = effort.unwrap_or_else(|| Value::String("high".to_string()));
        validate_adaptive_effort(&effort, upstream_model)?;
        apply_policy_effort(
            body,
            policy,
            effort,
            None,
            has_native_effort,
            upstream_model,
        )?;
    } else if let Some(effort) = effort {
        apply_policy_effort(
            body,
            policy,
            effort,
            budget_tokens,
            has_native_effort,
            upstream_model,
        )?;
    } else if let Some(budget_tokens) = budget_tokens {
        apply_policy_budget(body, policy, budget_tokens, upstream_model)?;
    }
    Ok(())
}

fn apply_policy_effort(
    body: &mut Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    effort: Value,
    budget_tokens: Option<Value>,
    has_native_effort: bool,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    match policy {
        ClaudeThinkingPolicy::AdaptiveOnly
        | ClaudeThinkingPolicy::AdaptivePreferred
        | ClaudeThinkingPolicy::MythosPreview => {
            ensure_anthropic_adaptive_thinking(body, upstream_model)?;
            restore_anthropic_output_effort(body, effort);
        }
        ClaudeThinkingPolicy::ManualWithEffort => {
            let budget_tokens = budget_tokens
                .or_else(|| existing_manual_thinking_budget(body))
                .ok_or_else(|| AnthropicAdapterError::ManualBudgetRequiredForEffort {
                    model: upstream_model.to_string(),
                })?;
            ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
            restore_anthropic_output_effort(body, effort);
        }
        ClaudeThinkingPolicy::ManualOnly => {
            if has_native_effort {
                return Err(AnthropicAdapterError::EffortNotSupported {
                    model: upstream_model.to_string(),
                });
            }
            let budget_tokens = budget_tokens
                .or_else(|| existing_manual_thinking_budget(body))
                .ok_or_else(|| AnthropicAdapterError::ManualBudgetRequired {
                    model: upstream_model.to_string(),
                })?;
            ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
        }
    }
    Ok(())
}

fn apply_policy_budget(
    body: &mut Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    budget_tokens: Value,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    match policy {
        ClaudeThinkingPolicy::AdaptiveOnly => {
            Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                model: upstream_model.to_string(),
            })
        }
        ClaudeThinkingPolicy::AdaptivePreferred
        | ClaudeThinkingPolicy::ManualWithEffort
        | ClaudeThinkingPolicy::ManualOnly
        | ClaudeThinkingPolicy::MythosPreview => {
            ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)
        }
    }
}

pub fn restore_anthropic_output_effort(body: &mut Map<String, Value>, effort: Value) {
    body.entry("output_config".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("output_config is an object")
        .insert("effort".to_string(), effort);
}

fn extract_anthropic_reasoning_effort(
    body: &mut Map<String, Value>,
) -> Result<Option<Value>, AnthropicAdapterError> {
    let reasoning_effort = body.remove("reasoning_effort").filter(|val| !val.is_null());
    let reasoning = body.remove("reasoning");

    match (reasoning_effort, reasoning) {
        (Some(effort), None) => Ok(Some(effort)),
        (None, Some(Value::Object(mut reasoning))) => {
            if let Some(budget_tokens) = reasoning.remove("budget_tokens") {
                merge_reasoning_budget_tokens(body, budget_tokens)?;
            }
            Ok(reasoning.remove("effort").filter(|val| !val.is_null()))
        }
        (Some(effort), Some(Value::Object(mut reasoning))) => {
            if let Some(inner) = reasoning.remove("effort").filter(|val| !val.is_null())
                && inner != effort
            {
                return Err(AnthropicAdapterError::ConflictingReasoningEffort);
            }
            if let Some(budget_tokens) = reasoning.remove("budget_tokens") {
                merge_reasoning_budget_tokens(body, budget_tokens)?;
            }
            Ok(Some(effort))
        }
        (None, Some(Value::Null)) => Ok(None),
        (Some(effort), Some(Value::Null)) => Ok(Some(effort)),
        (_, Some(_)) => Err(AnthropicAdapterError::InvalidReasoningConfig),
        (None, None) => Ok(None),
    }
}

fn merge_reasoning_budget_tokens(
    body: &mut Map<String, Value>,
    budget_tokens: Value,
) -> Result<(), AnthropicAdapterError> {
    if let Some(existing) = body
        .get("reasoning_budget_tokens")
        .filter(|val| !val.is_null())
        && existing != &budget_tokens
    {
        return Err(AnthropicAdapterError::ConflictingReasoningBudgetTokens);
    }
    body.insert("reasoning_budget_tokens".to_string(), budget_tokens);
    Ok(())
}

fn extract_existing_anthropic_output_effort(
    body: &mut Map<String, Value>,
) -> Result<Option<Value>, AnthropicAdapterError> {
    let (effort, remove_output_config) = {
        let Some(output_config) = body.get_mut("output_config") else {
            return Ok(None);
        };
        let output_config = output_config
            .as_object_mut()
            .ok_or(AnthropicAdapterError::InvalidOutputConfig)?;

        let effort = output_config.remove("effort").filter(|val| !val.is_null());
        (effort, output_config.is_empty())
    };
    if remove_output_config {
        body.remove("output_config");
    }

    Ok(effort)
}

fn merge_optional_efforts(
    reasoning_effort: Option<Value>,
    native_effort: Option<Value>,
    upstream_model: &str,
) -> Result<Option<Value>, AnthropicAdapterError> {
    match (reasoning_effort, native_effort) {
        (Some(left), Some(right)) if left != right => {
            Err(AnthropicAdapterError::ConflictingEffort {
                model: upstream_model.to_string(),
            })
        }
        (Some(left), _) => Ok(Some(left)),
        (None, Some(right)) => Ok(Some(right)),
        (None, None) => Ok(None),
    }
}

fn extract_anthropic_reasoning_budget_tokens(
    body: &mut Map<String, Value>,
) -> Result<Option<Value>, AnthropicAdapterError> {
    let thinking_budget = body
        .remove("thinking_budget_tokens")
        .filter(|val| !val.is_null());
    let reasoning_budget = body
        .remove("reasoning_budget_tokens")
        .filter(|val| !val.is_null());

    match (thinking_budget, reasoning_budget) {
        (Some(left), Some(right)) if left != right => {
            Err(AnthropicAdapterError::ConflictingThinkingBudgetTokens)
        }
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn validate_caller_thinking_for_policy(
    body: &Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    let Some(thinking) = body.get("thinking") else {
        return Ok(());
    };
    let thinking = thinking
        .as_object()
        .ok_or(AnthropicAdapterError::InvalidThinkingObject)?;
    let thinking_type = thinking.get("type").and_then(Value::as_str);

    match policy {
        ClaudeThinkingPolicy::AdaptiveOnly => {
            if thinking_type == Some("enabled") {
                return Err(
                    AnthropicAdapterError::AdaptiveOnlyManualThinkingNotSupported {
                        model: upstream_model.to_string(),
                    },
                );
            }
            if thinking_type == Some("disabled") {
                return Err(AnthropicAdapterError::AdaptiveOnlyDisabledNotSupported {
                    model: upstream_model.to_string(),
                });
            }
            if thinking_type != Some("adaptive") {
                return Err(
                    AnthropicAdapterError::AdaptiveOnlyManualThinkingNotSupported {
                        model: upstream_model.to_string(),
                    },
                );
            }
            if thinking
                .get("budget_tokens")
                .is_some_and(|val| !val.is_null())
            {
                return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                    model: upstream_model.to_string(),
                });
            }
            for key in thinking.keys() {
                if key != "type" {
                    return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                        model: upstream_model.to_string(),
                    });
                }
            }
        }
        ClaudeThinkingPolicy::ManualOnly | ClaudeThinkingPolicy::ManualWithEffort => {
            if thinking_type == Some("adaptive") {
                return Err(AnthropicAdapterError::AdaptiveNotSupported {
                    model: upstream_model.to_string(),
                });
            }
        }
        ClaudeThinkingPolicy::MythosPreview => {
            if thinking_type == Some("disabled") {
                return Err(AnthropicAdapterError::MythosDisabledNotSupported);
            }
        }
        ClaudeThinkingPolicy::AdaptivePreferred => {}
    }

    if thinking_type == Some("enabled") && thinking.get("budget_tokens").is_none_or(Value::is_null)
    {
        return Err(AnthropicAdapterError::MissingBudgetTokens {
            model: upstream_model.to_string(),
        });
    }

    Ok(())
}

fn ensure_anthropic_adaptive_thinking(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    match body.get("thinking") {
        None => {
            body.insert("thinking".to_string(), json!({ "type": "adaptive" }));
            Ok(())
        }
        Some(Value::Object(object))
            if object.get("type").and_then(Value::as_str) == Some("adaptive") =>
        {
            if object
                .get("budget_tokens")
                .is_some_and(|val| !val.is_null())
            {
                return Err(AnthropicAdapterError::AdaptiveOnlyBudgetNotSupported {
                    model: upstream_model.to_string(),
                });
            }
            Ok(())
        }
        Some(_) => Err(AnthropicAdapterError::ConflictingAdaptiveThinking {
            model: upstream_model.to_string(),
        }),
    }
}

fn ensure_anthropic_manual_thinking(
    body: &mut Map<String, Value>,
    budget_tokens: Value,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
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
                Some(_) => Err(AnthropicAdapterError::ConflictingManualBudget {
                    model: upstream_model.to_string(),
                }),
                None => Err(AnthropicAdapterError::MissingBudgetTokens {
                    model: upstream_model.to_string(),
                }),
            }
        }
        Some(_) => Err(AnthropicAdapterError::ConflictingCallerThinking {
            model: upstream_model.to_string(),
        }),
    }
}

fn existing_manual_thinking_budget(body: &Map<String, Value>) -> Option<Value> {
    let thinking = body.get("thinking")?.as_object()?;
    if thinking.get("type").and_then(Value::as_str) == Some("enabled") {
        thinking.get("budget_tokens").cloned()
    } else {
        None
    }
}

pub fn validate_anthropic_sampling_fields(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
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
        return Err(AnthropicAdapterError::UnsupportedSamplingField {
            field,
            model: upstream_model.to_string(),
        });
    }

    Ok(())
}

fn is_default_anthropic_sampling_value(field: &str, value: &Value) -> bool {
    match field {
        "temperature" | "top_p" => value
            .as_f64()
            .is_some_and(|num| (num - 1.0).abs() < f64::EPSILON),
        "top_k" => false,
        _ => false,
    }
}

pub fn validate_anthropic_tool_choice(
    value: &Value,
    upstream_model: &str,
) -> Result<(), AnthropicAdapterError> {
    if !contains_exact_claude_model_marker(upstream_model, "claude-fable-5") {
        return Ok(());
    }

    match value {
        Value::String(choice) if choice == "required" => {
            Err(AnthropicAdapterError::ForcedToolChoiceRejected {
                model: upstream_model.to_string(),
            })
        }
        Value::Object(object) => {
            let choice_type = object.get("type").and_then(Value::as_str);
            if matches!(choice_type, Some("any" | "tool" | "function")) {
                Err(AnthropicAdapterError::ForcedToolChoiceRejected {
                    model: upstream_model.to_string(),
                })
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}
