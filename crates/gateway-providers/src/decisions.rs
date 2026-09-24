//! Shared transport helpers for the Decisions API family.
//!
//! OpenRouter (alpha) and native TypeSafe share the System One wire shape: one
//! `state` evaluated against named typed questions. Adapters own URL routing
//! and auth; request shaping and alpha shape checks live here so both
//! transports stay identical.

use std::collections::BTreeMap;

use gateway_core::{
    CoreDecisionQuestion, CoreDecisionsRequest, DecisionAnswer, DecisionsResponse, ProviderError,
    ProviderRequestContext,
};
use serde::Deserialize;
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

/// Reject alpha shape drift loudly: the response must satisfy the public
/// contract, and every requested question needs a matching valid answer.
pub(crate) fn validate_decisions_response(
    value: &Value,
    request: &CoreDecisionsRequest,
) -> Result<(), ProviderError> {
    let response = DecisionsResponse::deserialize(value).map_err(|error| {
        ProviderError::Transport(format!("invalid decisions response: {error}"))
    })?;

    for (id, question) in &request.questions {
        let Some(answer) = response.answers.get(id) else {
            return Err(ProviderError::Transport(format!(
                "decisions response is missing answer `{id}`"
            )));
        };
        let expected = question.answer_type();
        let actual = match answer {
            DecisionAnswer::Noul { .. } => "noul",
            DecisionAnswer::Choice { .. } => "choice",
            DecisionAnswer::Score { .. } => "score",
        };
        if !matches!(
            (question, answer),
            (
                CoreDecisionQuestion::Noul { .. },
                DecisionAnswer::Noul { .. }
            ) | (
                CoreDecisionQuestion::Choice { .. },
                DecisionAnswer::Choice { .. }
            ) | (
                CoreDecisionQuestion::Score { .. },
                DecisionAnswer::Score { .. }
            )
        ) {
            return Err(ProviderError::Transport(format!(
                "decisions answer `{id}` has type `{actual}`, expected `{expected}`"
            )));
        }

        match (question, answer) {
            (CoreDecisionQuestion::Noul { .. }, DecisionAnswer::Noul { noul, .. })
                if !(0.0..=1.0).contains(noul) =>
            {
                return Err(ProviderError::Transport(format!(
                    "decisions answer `{id}` has noul probability {noul}, expected 0..=1"
                )));
            }
            (
                CoreDecisionQuestion::Choice { criteria, .. },
                DecisionAnswer::Choice { choice, .. },
            ) if !criteria.contains_key(choice) => {
                return Err(ProviderError::Transport(format!(
                    "decisions answer `{id}` selected unknown choice `{choice}`"
                )));
            }
            (
                CoreDecisionQuestion::Choice { criteria, .. },
                DecisionAnswer::Choice {
                    probabilities,
                    confidence,
                    ..
                },
            ) => {
                validate_probability_metadata(id, probabilities, *confidence)?;
                if let Some(choice) = probabilities
                    .keys()
                    .find(|choice| !criteria.contains_key(*choice))
                {
                    return Err(ProviderError::Transport(format!(
                        "decisions answer `{id}` reports probability for unknown choice `{choice}`"
                    )));
                }
            }
            (
                CoreDecisionQuestion::Score { criteria, .. },
                DecisionAnswer::Score {
                    score,
                    probabilities,
                    confidence,
                    ..
                },
            ) => {
                if !(0.0..=criteria.len().saturating_sub(1) as f64).contains(score) {
                    return Err(ProviderError::Transport(format!(
                        "decisions answer `{id}` has score {score}, expected 0..={}",
                        criteria.len().saturating_sub(1)
                    )));
                }
                validate_probability_metadata(id, probabilities, *confidence)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_probability_metadata(
    id: &str,
    probabilities: &BTreeMap<String, f64>,
    confidence: Option<f64>,
) -> Result<(), ProviderError> {
    if let Some((label, probability)) = probabilities
        .iter()
        .find(|(_, probability)| !(0.0..=1.0).contains(*probability))
    {
        return Err(ProviderError::Transport(format!(
            "decisions answer `{id}` has probability {probability} for `{label}`, expected 0..=1"
        )));
    }
    if let Some(confidence) = confidence
        && !(0.0..=1.0).contains(&confidence)
    {
        return Err(ProviderError::Transport(format!(
            "decisions answer `{id}` has confidence {confidence}, expected 0..=1"
        )));
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
                (
                    "frustration".to_string(),
                    CoreDecisionQuestion::Score {
                        instructions: json!("How frustrated?"),
                        criteria: vec![json!("Calm"), json!("Angry")],
                    },
                ),
            ]),
            extra: BTreeMap::new(),
        };
        let valid = json!({
            "model": "jev",
            "answers": {
                "is_urgent": {"type": "noul", "noul": 0.9},
                "department": {
                    "type": "choice",
                    "choice": "billing",
                    "probabilities": {"billing": 1.0},
                    "confidence": 0.9
                },
                "frustration": {
                    "type": "score",
                    "score": 0.8,
                    "probabilities": {"0": 0.2, "1": 0.8},
                    "confidence": 0.7
                }
            }
        });
        validate_decisions_response(&valid, &request).expect("valid");
        assert!(validate_decisions_response(&json!({}), &request).is_err());

        let mut wrong_type = valid.clone();
        wrong_type["answers"]["is_urgent"] = json!({"type": "choice", "choice": "billing"});
        let mut missing_payload = valid.clone();
        missing_payload["answers"]["department"]
            .as_object_mut()
            .expect("choice answer")
            .remove("choice");
        let mut invalid_probability = valid.clone();
        invalid_probability["answers"]["is_urgent"]["noul"] = json!(1.1);
        let mut invalid_choice = valid.clone();
        invalid_choice["answers"]["department"]["choice"] = json!("technical");
        let mut invalid_score = valid.clone();
        invalid_score["answers"]["frustration"]["score"] = json!(2.0);
        let mut invalid_choice_probability = valid.clone();
        invalid_choice_probability["answers"]["department"]["probabilities"]["billing"] =
            json!(-0.1);
        let mut unknown_choice_probability = valid.clone();
        unknown_choice_probability["answers"]["department"]["probabilities"]["technical"] =
            json!(0.1);
        let mut invalid_score_probability = valid.clone();
        invalid_score_probability["answers"]["frustration"]["probabilities"]["1"] = json!(1.1);
        let mut invalid_choice_confidence = valid.clone();
        invalid_choice_confidence["answers"]["department"]["confidence"] = json!(2.0);
        let mut invalid_score_confidence = valid.clone();
        invalid_score_confidence["answers"]["frustration"]["confidence"] = json!(-0.1);

        for invalid in [
            wrong_type,
            missing_payload,
            invalid_probability,
            invalid_choice,
            invalid_score,
            invalid_choice_probability,
            unknown_choice_probability,
            invalid_score_probability,
            invalid_choice_confidence,
            invalid_score_confidence,
        ] {
            assert!(validate_decisions_response(&invalid, &request).is_err());
        }
    }
}
