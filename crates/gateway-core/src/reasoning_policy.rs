use std::fmt;

use serde_json::{Map, Value};

use crate::{CoreChatRequest, CoreResponsesRequest, GatewayError, ReasoningEffort};

/// Keys from a request object down to one categorical effort value.
type EffortPath = &'static [&'static str];

const EFFORT_OBJECT_PATHS: &[EffortPath] = &[
    &["reasoning", "effort"],
    &["output_config", "effort"],
    &["thinking", "effort"],
];

const REQUEST_PATHS: &[EffortPath] = &[
    &["reasoning_effort"],
    &["additionalModelRequestFields", "reasoning", "effort"],
    &["additionalModelRequestFields", "output_config", "effort"],
    &["additionalModelRequestFields", "thinking", "effort"],
    &["additional_model_request_fields", "reasoning", "effort"],
    &["additional_model_request_fields", "output_config", "effort"],
    &["additional_model_request_fields", "thinking", "effort"],
];

const CHAT_REQUEST_PATHS: &[EffortPath] = &[
    &["generationConfig", "thinkingConfig", "thinkingLevel"],
    &["generation_config", "thinking_config", "thinking_level"],
    // Gemini's OpenAI-compatible endpoint reads thinking settings from `google`, sent either
    // at the top level or inside a literal `extra_body`.
    &["google", "thinking_config", "thinking_level"],
    &["extra_body", "google", "thinking_config", "thinking_level"],
    &["chat_template_kwargs", "reasoning_effort"],
    &["chat_template_args", "reasoning_effort"],
];

/// A message can change the effort from its position onward.
const CONFIGURATION_UPDATE_PATHS: &[EffortPath] = &[
    &["configuration_update", "reasoning", "effort"],
    &["configuration_update", "output_config", "effort"],
    &["configuration_update", "thinking", "effort"],
];

/// A reasoning effort that the model policy refused. It holds no prompt content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningEffortRejection {
    /// Where the refused value sits in the checked request, for example
    /// `messages[2].configuration_update.reasoning.effort`. Empty for a bare effort value.
    pub field_path: String,
    /// The effort above the ceiling, or `None` when the value is not a known effort.
    pub requested: Option<ReasoningEffort>,
    pub ceiling: ReasoningEffort,
    /// The refused value as JSON, for the caller-facing message only.
    received: String,
}

impl ReasoningEffortRejection {
    fn nested_in(mut self, parent: impl fmt::Display) -> Self {
        self.field_path = if self.field_path.is_empty() {
            parent.to_string()
        } else {
            format!("{parent}.{}", self.field_path)
        };
        self
    }
}

impl fmt::Display for ReasoningEffortRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.requested {
            Some(requested) => write!(
                f,
                "reasoning effort `{}` exceeds the model maximum `{}`",
                requested.as_str(),
                self.ceiling.as_str()
            )?,
            None => write!(
                f,
                "reasoning effort must be one of `none`, `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`; received {}",
                self.received
            )?,
        }
        if self.field_path.is_empty() {
            return Ok(());
        }
        write!(f, " at `{}`", self.field_path)
    }
}

impl std::error::Error for ReasoningEffortRejection {}

impl From<ReasoningEffortRejection> for GatewayError {
    fn from(rejection: ReasoningEffortRejection) -> Self {
        Self::InvalidRequest(rejection.to_string())
    }
}

/// Rejects categorical reasoning effort settings that exceed the model policy.
///
/// The value may be either one effort value or a request/provider JSON object. Request objects
/// are inspected at the categorical effort paths accepted by the gateway. A missing model policy,
/// a JSON `null` effort, and objects without any effort fields leave provider behavior unchanged.
pub fn enforce_reasoning_effort_value(
    value: &Value,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), ReasoningEffortRejection> {
    let Some(ceiling) = max_reasoning_effort else {
        return Ok(());
    };

    match value {
        Value::Object(object) => enforce_object_efforts(object, ceiling),
        _ => enforce_effort_value(value, ceiling),
    }
}

/// Rejects categorical reasoning effort settings in a borrowed request/provider object.
pub fn enforce_reasoning_effort_map(
    object: &Map<String, Value>,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), ReasoningEffortRejection> {
    max_reasoning_effort.map_or(Ok(()), |ceiling| enforce_object_efforts(object, ceiling))
}

/// Enforces every categorical reasoning effort accepted by Chat Completions and Messages.
pub fn enforce_chat_reasoning_effort(
    request: &CoreChatRequest,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), ReasoningEffortRejection> {
    let Some(ceiling) = max_reasoning_effort else {
        return Ok(());
    };

    enforce_effort_paths(
        |key| request.extra.get(key),
        &[EFFORT_OBJECT_PATHS, REQUEST_PATHS, CHAT_REQUEST_PATHS],
        ceiling,
    )?;
    for (index, message) in request.messages.iter().enumerate() {
        enforce_effort_paths(
            |key| message.extra.get(key),
            &[EFFORT_OBJECT_PATHS, CONFIGURATION_UPDATE_PATHS],
            ceiling,
        )
        .map_err(|rejection| rejection.nested_in(format_args!("messages[{index}]")))?;
    }
    Ok(())
}

/// Enforces every categorical reasoning effort accepted by the Responses API.
pub fn enforce_responses_reasoning_effort(
    request: &CoreResponsesRequest,
    max_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), ReasoningEffortRejection> {
    let Some(ceiling) = max_reasoning_effort else {
        return Ok(());
    };

    enforce_effort_paths(
        |key| match key {
            "reasoning" => request.reasoning.as_ref(),
            _ => request.extra.get(key),
        },
        &[EFFORT_OBJECT_PATHS, REQUEST_PATHS],
        ceiling,
    )?;
    enforce_item_efforts(Some(&request.input), "input", ceiling)
}

/// Checks a request/provider object at every effort path, including its nested conversation.
fn enforce_object_efforts(
    object: &Map<String, Value>,
    ceiling: ReasoningEffort,
) -> Result<(), ReasoningEffortRejection> {
    enforce_effort_paths(
        |key| object.get(key),
        &[
            EFFORT_OBJECT_PATHS,
            REQUEST_PATHS,
            CHAT_REQUEST_PATHS,
            CONFIGURATION_UPDATE_PATHS,
        ],
        ceiling,
    )?;
    for field in ["messages", "input"] {
        enforce_item_efforts(object.get(field), field, ceiling)?;
    }
    Ok(())
}

fn enforce_item_efforts(
    items: Option<&Value>,
    field: &str,
    ceiling: ReasoningEffort,
) -> Result<(), ReasoningEffortRejection> {
    let Some(items) = items.and_then(Value::as_array) else {
        return Ok(());
    };
    for (index, item) in items.iter().enumerate() {
        if let Some(item) = item.as_object() {
            enforce_object_efforts(item, ceiling)
                .map_err(|rejection| rejection.nested_in(format_args!("{field}[{index}]")))?;
        }
    }
    Ok(())
}

/// Checks the value at each path. `lookup` reads a top-level field of the checked object.
fn enforce_effort_paths<'a>(
    lookup: impl Fn(&str) -> Option<&'a Value>,
    path_groups: &[&[EffortPath]],
    ceiling: ReasoningEffort,
) -> Result<(), ReasoningEffortRejection> {
    for path in path_groups.iter().copied().flatten() {
        let Some((field, nested)) = path.split_first() else {
            continue;
        };
        let value = lookup(field)
            .and_then(|value| nested.iter().try_fold(value, |value, key| value.get(*key)));
        if let Some(value) = value {
            enforce_effort_value(value, ceiling)
                .map_err(|rejection| rejection.nested_in(path.join(".")))?;
        }
    }
    Ok(())
}

fn enforce_effort_value(
    value: &Value,
    ceiling: ReasoningEffort,
) -> Result<(), ReasoningEffortRejection> {
    if value.is_null() {
        return Ok(());
    }
    let reject = |requested| ReasoningEffortRejection {
        field_path: String::new(),
        requested,
        ceiling,
        received: value.to_string(),
    };

    let Some(raw_effort) = value.as_str() else {
        return Err(reject(None));
    };
    let requested = if raw_effort.eq_ignore_ascii_case("off") {
        Some(ReasoningEffort::None)
    } else {
        ReasoningEffort::ALL
            .into_iter()
            .find(|effort| effort.as_str().eq_ignore_ascii_case(raw_effort))
    };
    let Some(requested) = requested else {
        return Err(reject(None));
    };
    if requested > ceiling {
        return Err(reject(Some(requested)));
    }
    Ok(())
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
        for effort in ["none", "off", "minimal", "low", "medium", "MEDIUM"] {
            enforce_reasoning_effort_value(&json!(effort), Some(ReasoningEffort::Medium))
                .expect("effort at or below the ceiling should pass");
        }
    }

    #[test]
    fn effort_value_rejects_values_above_the_ceiling() {
        let error = enforce_reasoning_effort_value(&json!("high"), Some(ReasoningEffort::Medium))
            .expect_err("effort above the ceiling should fail");

        assert_eq!(error.requested, Some(ReasoningEffort::High));
        assert!(
            error
                .to_string()
                .contains("exceeds the model maximum `medium`")
        );
        assert!(matches!(
            GatewayError::from(error),
            GatewayError::InvalidRequest(_)
        ));
    }

    #[test]
    fn none_ceiling_allows_only_disabled_reasoning() {
        for effort in ["none", "off", "NONE", "Off"] {
            enforce_reasoning_effort_value(&json!(effort), Some(ReasoningEffort::None))
                .expect("disabled reasoning should pass a `none` ceiling");
        }
        for (effort, requested) in [
            ("minimal", ReasoningEffort::Minimal),
            ("low", ReasoningEffort::Low),
            ("max", ReasoningEffort::Max),
        ] {
            let error = enforce_reasoning_effort_value(&json!(effort), Some(ReasoningEffort::None))
                .expect_err("any reasoning should exceed a `none` ceiling");
            assert_eq!(error.requested, Some(requested));
            assert_eq!(
                error.to_string(),
                format!("reasoning effort `{effort}` exceeds the model maximum `none`")
            );
        }
    }

    #[test]
    fn gemini_openai_compat_thinking_level_is_held_to_the_ceiling() {
        let thinking =
            |level: &str| json!({"google": {"thinking_config": {"thinking_level": level}}});
        for (field, value) in [
            ("google", thinking("HIGH")["google"].clone()),
            ("extra_body", thinking("high")),
            ("extra_body", thinking("XHigh")),
        ] {
            let request = CoreChatRequest {
                model: "gemini".to_string(),
                messages: Vec::new(),
                stream: false,
                extra: BTreeMap::from([(field.to_string(), value)]),
            };
            let rejection = enforce_chat_reasoning_effort(&request, Some(ReasoningEffort::Medium))
                .expect_err("a Gemini thinking level above the ceiling should fail");
            assert!(
                rejection
                    .field_path
                    .ends_with("google.thinking_config.thinking_level"),
                "unexpected path {}",
                rejection.field_path
            );
        }

        for level in ["MINIMAL", "low", "Medium"] {
            enforce_reasoning_effort_value(
                &json!({"extra_body": thinking(level)}),
                Some(ReasoningEffort::Medium),
            )
            .expect("a Gemini thinking level at or below the ceiling should pass");
        }
        assert!(
            enforce_reasoning_effort_value(&thinking("EXTREME"), Some(ReasoningEffort::Max))
                .is_err()
        );
    }

    #[test]
    fn effort_value_rejects_unknown_and_malformed_values() {
        for value in [json!("extreme"), json!(42)] {
            assert!(enforce_reasoning_effort_value(&value, Some(ReasoningEffort::Max)).is_err());
        }
        assert!(
            enforce_reasoning_effort_value(
                &json!({"reasoning_effort": {"level": "high"}}),
                Some(ReasoningEffort::Max)
            )
            .is_err()
        );
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

            assert!(
                enforce_chat_reasoning_effort(&request, Some(ReasoningEffort::Medium)).is_err()
            );
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

        assert!(enforce_chat_reasoning_effort(&request, Some(ReasoningEffort::High)).is_err());

        let provider_body = json!({
            "messages": [{
                "role": "system",
                "content": "Think carefully",
                "output_config": {"effort": "xhigh"}
            }]
        });
        assert!(
            enforce_reasoning_effort_value(&provider_body, Some(ReasoningEffort::High)).is_err()
        );
    }

    fn chat_with_configuration_updates(updates: &[Value]) -> CoreChatRequest {
        CoreChatRequest {
            model: "reasoning".to_string(),
            messages: updates
                .iter()
                .map(|update| CoreChatMessage {
                    role: "system".to_string(),
                    content: json!(""),
                    name: None,
                    extra: BTreeMap::from([("configuration_update".to_string(), update.clone())]),
                })
                .collect(),
            stream: false,
            extra: BTreeMap::from([("reasoning".to_string(), json!({"effort": "high"}))]),
        }
    }

    #[test]
    fn mid_conversation_effort_updates_are_held_to_the_ceiling() {
        let update = |effort: Value| json!({"reasoning": {"effort": effort}});

        let allowed = [
            update(json!("low")),
            update(json!("medium")),
            update(json!("high")),
            update(Value::Null),
            json!({"reasoning": {}}),
            json!({}),
            Value::Null,
        ];
        enforce_chat_reasoning_effort(
            &chat_with_configuration_updates(&allowed),
            Some(ReasoningEffort::High),
        )
        .expect("updates at or below the ceiling, null and missing fields should pass");

        for rejected in [json!("xhigh"), json!("max"), json!("extreme"), json!(7)] {
            // A valid update first proves that every message is checked, not only the first.
            let updates = [update(json!("low")), update(rejected)];
            let request = chat_with_configuration_updates(&updates);
            let rejection = enforce_chat_reasoning_effort(&request, Some(ReasoningEffort::High))
                .expect_err("an update above the ceiling or malformed should fail");
            assert_eq!(
                rejection.field_path,
                "messages[1].configuration_update.reasoning.effort"
            );

            enforce_chat_reasoning_effort(&request, None)
                .expect("a model without a ceiling should not inspect updates");

            let batch_body = serde_json::to_value(&request).expect("request should serialize");
            assert_eq!(
                enforce_reasoning_effort_value(&batch_body, Some(ReasoningEffort::High)),
                Err(rejection)
            );
        }

        let rejection = enforce_chat_reasoning_effort(
            &chat_with_configuration_updates(&[update(json!("xhigh"))]),
            Some(ReasoningEffort::High),
        )
        .expect_err("xhigh should exceed a high ceiling");
        assert_eq!(rejection.requested, Some(ReasoningEffort::XHigh));
        assert_eq!(
            rejection.to_string(),
            "reasoning effort `xhigh` exceeds the model maximum `high` at `messages[0].configuration_update.reasoning.effort`"
        );
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
            assert!(enforce_reasoning_effort_value(&body, Some(ReasoningEffort::High)).is_err());
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

        assert!(enforce_responses_reasoning_effort(&request, Some(ReasoningEffort::Max)).is_err());
    }

    #[test]
    fn responses_input_effort_updates_are_held_to_the_ceiling() {
        let request = |effort: &str| CoreResponsesRequest {
            model: "reasoning".to_string(),
            input: json!([
                {"role": "user", "content": "Plan the change."},
                {"type": "configuration_update", "reasoning": {"effort": "low"}},
                {"role": "system", "content": "", "configuration_update": {"reasoning": {"effort": effort}}},
            ]),
            stream: false,
            instructions: None,
            tools: None,
            tool_choice: None,
            reasoning: Some(json!({"effort": "high"})),
            text: None,
            extra: BTreeMap::new(),
        };

        enforce_responses_reasoning_effort(&request("high"), Some(ReasoningEffort::High))
            .expect("updates at or below the ceiling should pass");

        let rejection =
            enforce_responses_reasoning_effort(&request("max"), Some(ReasoningEffort::High))
                .expect_err("an input update above the ceiling should fail");
        assert_eq!(
            rejection.field_path,
            "input[2].configuration_update.reasoning.effort"
        );
    }
}
