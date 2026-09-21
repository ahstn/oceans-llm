//! Shared transport helpers for the Decisions API family.
//!
//! OpenRouter (alpha) and native TypeSafe share the System One wire shape: one
//! `state` evaluated against named typed questions. Adapters own URL routing
//! and auth; request shaping and alpha shape checks live here so both
//! transports stay identical.

use std::collections::BTreeMap;

use gateway_core::{CoreDecisionsRequest, ProviderError, ProviderRequestContext};
use serde_json::Value;

/// Serialize a Decisions request, pin the upstream model, and merge route
/// `extra_body` overrides. OpenRouter applies its routing policy on top.
pub(crate) fn base_decisions_request_body(
    request: &CoreDecisionsRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut body = serde_json::to_value(request)
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    let Some(object) = body.as_object_mut() else {
        return Err(ProviderError::Transport(
            "decisions request must serialize to a JSON object".to_string(),
        ));
    };
    object.insert(
        "model".to_string(),
        Value::String(context.upstream_model.clone()),
    );
    for (key, value) in &context.extra_body {
        object.insert(key.clone(), value.clone());
    }
    Ok(body)
}

/// POST builder with default headers, route extra headers, and `x-request-id`.
/// Callers apply their own bearer auth before building.
pub(crate) fn decisions_request_builder(
    client: &reqwest::Client,
    url: String,
    body: &Value,
    default_headers: &BTreeMap<String, String>,
    context: &ProviderRequestContext,
) -> reqwest::RequestBuilder {
    let mut builder = client.post(url).json(body);
    for (name, value) in default_headers {
        builder = builder.header(name, value);
    }
    for (name, value) in &context.extra_headers {
        if let Some(value) = value.as_str() {
            builder = builder.header(name, value);
        }
    }
    builder.header("x-request-id", &context.request_id)
}

/// Trimmed base root plus its host. Rejects query parameters and fragments so
/// adapters can safely append their own Decisions path.
pub(crate) fn base_root_and_host(
    base_url: &str,
) -> Result<(String, Option<String>), ProviderError> {
    let trimmed = base_url.trim_end_matches('/');
    let parsed = url::Url::parse(trimmed)
        .map_err(|error| ProviderError::Transport(format!("invalid base_url: {error}")))?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ProviderError::InvalidRequest(
            "base_url cannot contain query parameters or fragments".to_string(),
        ));
    }
    Ok((trimmed.to_string(), parsed.host_str().map(str::to_string)))
}

/// Reject alpha shape drift loudly: every requested question needs an answer
/// whose `type` matches the question discriminant.
pub(crate) fn validate_decisions_response(
    value: &Value,
    request: &CoreDecisionsRequest,
) -> Result<(), ProviderError> {
    let Some(answers) = value.get("answers").and_then(Value::as_object) else {
        return Err(ProviderError::Transport(
            "decisions response is missing the `answers` object".to_string(),
        ));
    };
    for (id, question) in &request.questions {
        let Some(answer) = answers.get(id) else {
            return Err(ProviderError::Transport(format!(
                "decisions response is missing answer `{id}`"
            )));
        };
        let expected = question.answer_type();
        let actual = answer.get("type").and_then(Value::as_str).unwrap_or("");
        if actual != expected {
            return Err(ProviderError::Transport(format!(
                "decisions answer `{id}` has type `{actual}`, expected `{expected}`"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gateway_core::CoreDecisionQuestion;
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_answers_against_requested_questions() {
        let request = CoreDecisionsRequest {
            model: "jev".to_string(),
            state: json!("state"),
            questions: BTreeMap::from([
                (
                    "is_urgent".to_string(),
                    CoreDecisionQuestion::Noul {
                        instructions: json!("urgent?"),
                        criteria: None,
                    },
                ),
                (
                    "department".to_string(),
                    CoreDecisionQuestion::Choice {
                        instructions: json!("Which team?"),
                        criteria: BTreeMap::from([("billing".to_string(), None)]),
                    },
                ),
            ]),
            extra: BTreeMap::new(),
        };
        let valid = json!({
            "answers": {
                "is_urgent": {"type": "noul", "noul": 0.9},
                "department": {"type": "choice", "choice": "billing"}
            }
        });
        validate_decisions_response(&valid, &request).expect("valid");
        assert!(validate_decisions_response(&json!({}), &request).is_err());
        assert!(
            validate_decisions_response(
                &json!({"answers": {"is_urgent": {"type": "choice", "choice": "a"}}}),
                &request
            )
            .is_err()
        );
    }
}
