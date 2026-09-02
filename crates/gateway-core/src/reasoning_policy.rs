use serde_json::Value;

use crate::{CoreChatRequest, CoreResponsesRequest, GatewayError, ReasoningEffort};

/// Rejects categorical reasoning effort settings that exceed the model policy.
///
/// The value may be either one effort value or a request/provider JSON object. Request objects
/// are inspected at the categorical effort paths accepted by the gateway. A missing model policy,
/// a JSON `null` effort, and objects without any effort fields leave provider behavior unchanged.
pub fn enforce_reasoning_effort_value(
    value: &Value,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), GatewayError> {
    let Some(max_reasoning_effort) = max_reasoning_effort else {
        return Ok(());
    };

    match value {
        Value::Null => Ok(()),
        Value::Object(object) => enforce_reasoning_effort_fields(object, max_reasoning_effort),
        _ => enforce_effort_value(value, max_reasoning_effort),
    }
}

fn enforce_effort_value(
    value: &Value,
    max_reasoning_effort: ReasoningEffort,
) -> Result<(), GatewayError> {
    if value.is_null() {
        return Ok(());
    }

    let requested = value
        .as_str()
        .map(str::to_ascii_lowercase)
        .as_deref()
        .and_then(ReasoningEffort::from_db)
        .ok_or_else(|| {
            GatewayError::InvalidRequest(format!(
                "reasoning effort must be one of `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`; received {value}"
            ))
        })?;

    if requested > max_reasoning_effort {
        return Err(GatewayError::InvalidRequest(format!(
            "reasoning effort `{}` exceeds the model maximum `{}`",
            requested.as_str(),
            max_reasoning_effort.as_str()
        )));
    }

    Ok(())
}

fn enforce_reasoning_effort_fields(
    object: &serde_json::Map<String, Value>,
    max_reasoning_effort: ReasoningEffort,
) -> Result<(), GatewayError> {
    enforce_optional_effort(object.get("reasoning_effort"), Some(max_reasoning_effort))?;
    for field in ["reasoning", "output_config", "thinking"] {
        enforce_nested_effort(object.get(field), "effort", Some(max_reasoning_effort))?;
    }

    for field in [
        "additionalModelRequestFields",
        "additional_model_request_fields",
    ] {
        enforce_provider_request_fields(object.get(field), max_reasoning_effort)?;
    }
    enforce_google_thinking_config(
        object.get("generationConfig"),
        "thinkingConfig",
        "thinkingLevel",
        max_reasoning_effort,
    )?;
    enforce_google_thinking_config(
        object.get("generation_config"),
        "thinking_config",
        "thinking_level",
        max_reasoning_effort,
    )?;

    for field in ["chat_template_kwargs", "chat_template_args"] {
        enforce_nested_effort(
            object.get(field),
            "reasoning_effort",
            Some(max_reasoning_effort),
        )?;
    }

    for field in ["messages", "input"] {
        if let Some(items) = object.get(field).and_then(Value::as_array) {
            for item in items {
                if let Some(item) = item.as_object() {
                    enforce_reasoning_effort_fields(item, max_reasoning_effort)?;
                }
            }
        }
    }

    Ok(())
}

fn enforce_provider_request_fields(
    value: Option<&Value>,
    max_reasoning_effort: ReasoningEffort,
) -> Result<(), GatewayError> {
    for field in ["reasoning", "output_config", "thinking"] {
        let nested = value
            .and_then(Value::as_object)
            .and_then(|object| object.get(field));
        enforce_nested_effort(nested, "effort", Some(max_reasoning_effort))?;
    }
    Ok(())
}

fn enforce_google_thinking_config(
    generation_config: Option<&Value>,
    thinking_config_field: &str,
    thinking_level_field: &str,
    max_reasoning_effort: ReasoningEffort,
) -> Result<(), GatewayError> {
    let thinking_config = generation_config
        .and_then(Value::as_object)
        .and_then(|object| object.get(thinking_config_field));
    enforce_nested_effort(
        thinking_config,
        thinking_level_field,
        Some(max_reasoning_effort),
    )
}

/// Enforces every categorical reasoning effort accepted by Chat Completions and Messages.
pub fn enforce_chat_reasoning_effort(
    request: &CoreChatRequest,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), GatewayError> {
    let Some(max_reasoning_effort) = max_reasoning_effort else {
        return Ok(());
    };

    enforce_optional_effort(
        request.extra.get("reasoning_effort"),
        Some(max_reasoning_effort),
    )?;
    for field in ["reasoning", "output_config", "thinking"] {
        enforce_nested_effort(
            request.extra.get(field),
            "effort",
            Some(max_reasoning_effort),
        )?;
    }
    for field in [
        "additionalModelRequestFields",
        "additional_model_request_fields",
    ] {
        enforce_provider_request_fields(request.extra.get(field), max_reasoning_effort)?;
    }
    enforce_google_thinking_config(
        request.extra.get("generationConfig"),
        "thinkingConfig",
        "thinkingLevel",
        max_reasoning_effort,
    )?;
    enforce_google_thinking_config(
        request.extra.get("generation_config"),
        "thinking_config",
        "thinking_level",
        max_reasoning_effort,
    )?;
    for field in ["chat_template_kwargs", "chat_template_args"] {
        enforce_nested_effort(
            request.extra.get(field),
            "reasoning_effort",
            Some(max_reasoning_effort),
        )?;
    }
    for message in &request.messages {
        for field in ["reasoning", "output_config", "thinking"] {
            enforce_nested_effort(
                message.extra.get(field),
                "effort",
                Some(max_reasoning_effort),
            )?;
        }
    }
    Ok(())
}

/// Enforces every categorical reasoning effort accepted by the Responses API.
pub fn enforce_responses_reasoning_effort(
    request: &CoreResponsesRequest,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), GatewayError> {
    let Some(max_reasoning_effort) = max_reasoning_effort else {
        return Ok(());
    };

    enforce_nested_effort(
        request.reasoning.as_ref(),
        "effort",
        Some(max_reasoning_effort),
    )?;
    enforce_optional_effort(
        request.extra.get("reasoning_effort"),
        Some(max_reasoning_effort),
    )?;
    for field in ["output_config", "thinking"] {
        enforce_nested_effort(
            request.extra.get(field),
            "effort",
            Some(max_reasoning_effort),
        )?;
    }
    for field in [
        "additionalModelRequestFields",
        "additional_model_request_fields",
    ] {
        enforce_provider_request_fields(request.extra.get(field), max_reasoning_effort)?;
    }
    if let Some(items) = request.input.as_array() {
        for item in items {
            if let Some(item) = item.as_object() {
                enforce_reasoning_effort_fields(item, max_reasoning_effort)?;
            }
        }
    }
    Ok(())
}

fn enforce_optional_effort(
    value: Option<&Value>,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), GatewayError> {
    value.map_or(Ok(()), |value| {
        let Some(max_reasoning_effort) = max_reasoning_effort else {
            return Ok(());
        };
        enforce_effort_value(value, max_reasoning_effort)
    })
}

fn enforce_nested_effort(
    value: Option<&Value>,
    field: &str,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), GatewayError> {
    enforce_optional_effort(
        value
            .and_then(Value::as_object)
            .and_then(|object| object.get(field)),
        max_reasoning_effort,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{
        enforce_chat_reasoning_effort, enforce_reasoning_effort_value,
        enforce_responses_reasoning_effort,
    };
    use crate::{
        CoreChatMessage, CoreChatRequest, CoreResponsesRequest, GatewayError, ReasoningEffort,
    };

    #[test]
    fn effort_value_accepts_known_values_at_or_below_the_ceiling() {
        for effort in ["minimal", "low", "medium"] {
            enforce_reasoning_effort_value(&json!(effort), Some(ReasoningEffort::Medium))
                .expect("effort at or below the ceiling should pass");
        }
    }

    #[test]
    fn effort_value_rejects_values_above_the_ceiling() {
        let error = enforce_reasoning_effort_value(&json!("high"), Some(ReasoningEffort::Medium))
            .expect_err("effort above the ceiling should fail");

        assert!(matches!(&error, GatewayError::InvalidRequest(_)));
        assert!(
            error
                .to_string()
                .contains("exceeds the model maximum `medium`")
        );
    }

    #[test]
    fn effort_value_rejects_unknown_and_malformed_values() {
        for value in [json!("extreme"), json!(42)] {
            assert!(matches!(
                enforce_reasoning_effort_value(&value, Some(ReasoningEffort::Max)),
                Err(GatewayError::InvalidRequest(_))
            ));
        }
        assert!(matches!(
            enforce_reasoning_effort_value(
                &json!({"reasoning_effort": {"level": "high"}}),
                Some(ReasoningEffort::Max)
            ),
            Err(GatewayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn omitted_policy_and_null_effort_pass() {
        enforce_reasoning_effort_value(&json!({"future": true}), None)
            .expect("an omitted policy should not inspect the request value");
        enforce_reasoning_effort_value(&Value::Null, Some(ReasoningEffort::Minimal))
            .expect("null should leave provider defaults unchanged");
        enforce_reasoning_effort_value(
            &json!({"reasoning": {"budget_tokens": 1_000_000}}),
            Some(ReasoningEffort::Minimal),
        )
        .expect("categorical policy should not cap numeric reasoning budgets");
    }

    #[test]
    fn chat_extracts_compatibility_and_native_effort_fields() {
        for (field, value) in [
            ("reasoning_effort", json!("high")),
            (
                "reasoning",
                json!({"effort": "high", "budget_tokens": 4096}),
            ),
            ("output_config", json!({"effort": "high"})),
        ] {
            let request = CoreChatRequest {
                model: "reasoning".to_string(),
                messages: Vec::new(),
                stream: false,
                extra: BTreeMap::from([(field.to_string(), value)]),
            };

            assert!(matches!(
                enforce_chat_reasoning_effort(&request, Some(ReasoningEffort::Medium)),
                Err(GatewayError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn chat_extracts_per_message_native_output_effort() {
        let request = CoreChatRequest {
            model: "reasoning".to_string(),
            messages: vec![CoreChatMessage {
                role: "system".to_string(),
                content: json!("Think carefully"),
                name: None,
                extra: BTreeMap::from([("output_config".to_string(), json!({"effort": "xhigh"}))]),
            }],
            stream: false,
            extra: BTreeMap::new(),
        };

        assert!(matches!(
            enforce_chat_reasoning_effort(&request, Some(ReasoningEffort::High)),
            Err(GatewayError::InvalidRequest(_))
        ));

        let provider_body = json!({
            "messages": [{
                "role": "system",
                "content": "Think carefully",
                "output_config": {"effort": "xhigh"}
            }]
        });
        assert!(matches!(
            enforce_reasoning_effort_value(&provider_body, Some(ReasoningEffort::High)),
            Err(GatewayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn provider_native_effort_paths_cannot_bypass_the_ceiling() {
        for body in [
            json!({"thinking": {"effort": "xhigh"}}),
            json!({"additionalModelRequestFields": {"thinking": {"effort": "max"}}}),
            json!({"additional_model_request_fields": {"output_config": {"effort": "xhigh"}}}),
            json!({"generationConfig": {"thinkingConfig": {"thinkingLevel": "XHIGH"}}}),
            json!({"generation_config": {"thinking_config": {"thinking_level": "MAX"}}}),
            json!({"chat_template_kwargs": {"reasoning_effort": "xhigh"}}),
            json!({"input": [{"type": "message", "output_config": {"effort": "max"}}]}),
        ] {
            assert!(matches!(
                enforce_reasoning_effort_value(&body, Some(ReasoningEffort::High)),
                Err(GatewayError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn responses_extracts_reasoning_effort_and_rejects_malformed_values() {
        let request = CoreResponsesRequest {
            model: "reasoning".to_string(),
            input: json!("hello"),
            stream: false,
            instructions: None,
            tools: None,
            tool_choice: None,
            reasoning: Some(json!({"effort": 3})),
            text: None,
            extra: BTreeMap::new(),
        };

        assert!(matches!(
            enforce_responses_reasoning_effort(&request, Some(ReasoningEffort::Max)),
            Err(GatewayError::InvalidRequest(_))
        ));
    }
}
