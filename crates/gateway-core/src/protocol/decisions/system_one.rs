//! System One Decisions wire format: one `state` evaluated against a map of
//! named `noul`, `choice`, and `score` questions. TypeSafe owns this shape.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    DecisionAnswer, DecisionChoiceOption, DecisionChoiceValue, DecisionOutcome, DecisionQuestion,
    DecisionQuestionKind, DecisionScoreLevel, DecisionsInput, DecisionsRequest, DecisionsResponse,
    DecisionsUsage, NoulCriteria, check_probability, value_to_text,
};

/// System One request body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemOneDecisionsRequest {
    pub model: String,
    pub state: Value,
    #[serde(default)]
    pub questions: BTreeMap<String, SystemOneDecisionQuestion>,
    /// OpenAI-only field. Typed so it never rides along in `extra`: it reaches
    /// OpenAI providers and is rejected before any System One upstream call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SystemOneDecisionQuestion {
    Noul {
        instructions: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: Value,
        criteria: BTreeMap<String, Option<Value>>,
    },
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SystemOneDecisionAnswer {
    Noul {
        noul: f64,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    Choice {
        choice: String,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    Score {
        score: f64,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legend: Option<BTreeMap<String, String>>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    /// The model declined to answer. Only produced when translating an
    /// OpenAI Decisions refusal for a System One caller.
    Refusal {
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
}

impl SystemOneDecisionAnswer {
    const fn type_name(&self) -> &'static str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
            Self::Refusal { .. } => "refusal",
        }
    }
}

/// System One response body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemOneDecisionsResponse {
    pub model: String,
    pub answers: BTreeMap<String, SystemOneDecisionAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SystemOneDecisionsUsage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// System One token usage. OpenAI-sourced responses also carry
/// `input_tokens_details`, `output_tokens_details`, and `total_tokens` in
/// `extra` so accounting can see cached and reasoning tokens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SystemOneDecisionsUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// System One request to core. Lossless: [`core_decisions_request_to_system_one`]
/// reproduces the original request.
#[must_use]
pub fn system_one_decisions_request_to_core(
    request: &SystemOneDecisionsRequest,
) -> DecisionsRequest {
    DecisionsRequest {
        model: request.model.clone(),
        input: DecisionsInput::State(request.state.clone()),
        questions: request
            .questions
            .iter()
            .map(|(key, question)| system_one_question_to_core(key, question))
            .collect(),
        safety_identifier: request.safety_identifier.clone(),
        extra: request.extra.clone(),
    }
}

fn system_one_question_to_core(
    key: &str,
    question: &SystemOneDecisionQuestion,
) -> DecisionQuestion {
    let (instructions, kind) = match question {
        SystemOneDecisionQuestion::Noul {
            instructions,
            criteria,
        } => (
            instructions,
            DecisionQuestionKind::Predicate {
                criteria: criteria.clone(),
            },
        ),
        SystemOneDecisionQuestion::Choice {
            instructions,
            criteria,
        } => (
            instructions,
            DecisionQuestionKind::Choice {
                options: criteria
                    .iter()
                    .map(|(value, description)| DecisionChoiceOption {
                        value: DecisionChoiceValue::Text(value.clone()),
                        description: description.clone(),
                    })
                    .collect(),
            },
        ),
        SystemOneDecisionQuestion::Score {
            instructions,
            criteria,
        } => (
            instructions,
            DecisionQuestionKind::Score {
                levels: criteria
                    .iter()
                    .map(|label| DecisionScoreLevel {
                        label: label.clone(),
                        description: None,
                    })
                    .collect(),
            },
        ),
    };
    DecisionQuestion {
        key: key.to_string(),
        name: Some(key.to_string()),
        instructions: instructions.clone(),
        kind,
    }
}

/// Core request to System One. Fails for inputs System One cannot carry:
/// images, `safety_identifier`, and boolean choices whose `"true"`/`"false"`
/// key collides with a string choice.
pub fn core_decisions_request_to_system_one(
    request: &DecisionsRequest,
) -> Result<SystemOneDecisionsRequest, String> {
    if request.safety_identifier.is_some() {
        return Err(
            "`safety_identifier` is only supported by OpenAI Decisions providers".to_string(),
        );
    }
    let state = request.input.to_state()?;
    let mut questions = BTreeMap::new();
    for question in &request.questions {
        questions.insert(question.key.clone(), core_question_to_system_one(question)?);
    }
    Ok(SystemOneDecisionsRequest {
        model: request.model.clone(),
        state,
        questions,
        safety_identifier: None,
        extra: request.extra.clone(),
    })
}

fn core_question_to_system_one(
    question: &DecisionQuestion,
) -> Result<SystemOneDecisionQuestion, String> {
    let instructions = question.instructions.clone();
    Ok(match &question.kind {
        DecisionQuestionKind::Predicate { criteria } => SystemOneDecisionQuestion::Noul {
            instructions,
            criteria: criteria.clone(),
        },
        DecisionQuestionKind::Choice { options } => {
            let mut criteria = BTreeMap::new();
            for option in options {
                let key = option.value.system_one_key();
                if criteria
                    .insert(key.clone(), option.description.clone())
                    .is_some()
                {
                    return Err(format!(
                        "decisions question `{}` has choices that collide as `{key}` in the System One format",
                        question.key
                    ));
                }
            }
            SystemOneDecisionQuestion::Choice {
                instructions,
                criteria,
            }
        }
        DecisionQuestionKind::Score { levels } => SystemOneDecisionQuestion::Score {
            instructions,
            criteria: levels
                .iter()
                .map(|level| match &level.description {
                    Some(description) => Value::String(format!(
                        "{}: {}",
                        value_to_text(&level.label),
                        value_to_text(description)
                    )),
                    None => level.label.clone(),
                })
                .collect(),
        },
    })
}

/// Parse a System One response and reject shape drift: every requested
/// question needs a matching, in-bounds answer.
pub fn system_one_decisions_response_to_core(
    body: &Value,
    request: &DecisionsRequest,
) -> Result<DecisionsResponse, String> {
    let response = SystemOneDecisionsResponse::deserialize(body)
        .map_err(|error| format!("invalid decisions response: {error}"))?;

    let mut answers = Vec::with_capacity(request.questions.len());
    for question in &request.questions {
        let key = &question.key;
        let Some(answer) = response.answers.get(key) else {
            return Err(format!("decisions response is missing answer `{key}`"));
        };
        answers.push(DecisionOutcome {
            key: key.clone(),
            answer: system_one_answer_to_core(question, answer)?,
        });
    }

    Ok(DecisionsResponse {
        model: response.model,
        answers,
        usage: response.usage.as_ref().map(system_one_usage_to_core),
    })
}

fn system_one_answer_to_core(
    question: &DecisionQuestion,
    answer: &SystemOneDecisionAnswer,
) -> Result<DecisionAnswer, String> {
    let key = &question.key;
    match (&question.kind, answer) {
        (_, SystemOneDecisionAnswer::Refusal { .. }) => Ok(DecisionAnswer::Refusal),
        (DecisionQuestionKind::Predicate { .. }, SystemOneDecisionAnswer::Noul { noul, .. }) => {
            check_probability(key, "noul probability", *noul)?;
            Ok(DecisionAnswer::Predicate { probability: *noul })
        }
        (
            DecisionQuestionKind::Choice { options },
            SystemOneDecisionAnswer::Choice {
                choice,
                probabilities,
                confidence,
                ..
            },
        ) => {
            let find = |label: &str| {
                options
                    .iter()
                    .find(|option| option.value.system_one_key() == label)
                    .map(|option| option.value.clone())
            };
            let Some(selected) = find(choice) else {
                return Err(format!(
                    "decisions answer `{key}` selected unknown choice `{choice}`"
                ));
            };
            let mut typed = Vec::with_capacity(probabilities.len());
            for (label, probability) in probabilities {
                check_probability(key, &format!("probability for `{label}`"), *probability)?;
                let Some(value) = find(label) else {
                    return Err(format!(
                        "decisions answer `{key}` reports probability for unknown choice `{label}`"
                    ));
                };
                typed.push((value, *probability));
            }
            if let Some(confidence) = confidence {
                check_probability(key, "confidence", *confidence)?;
            }
            Ok(DecisionAnswer::Choice {
                choice: selected,
                confidence: *confidence,
                probabilities: typed,
            })
        }
        (
            DecisionQuestionKind::Score { levels },
            SystemOneDecisionAnswer::Score {
                score,
                probabilities,
                confidence,
                ..
            },
        ) => {
            let max = levels.len().saturating_sub(1);
            if !(0.0..=max as f64).contains(score) {
                return Err(format!(
                    "decisions answer `{key}` has score {score}, expected 0..={max}"
                ));
            }
            let mut indexed = Vec::with_capacity(probabilities.len());
            for (label, probability) in probabilities {
                check_probability(key, &format!("probability for `{label}`"), *probability)?;
                // System One keys score probabilities by level index.
                let Some(index) = label.parse::<usize>().ok().filter(|index| *index <= max) else {
                    return Err(format!(
                        "decisions answer `{key}` reports probability for unknown score level `{label}`, expected 0..={max}"
                    ));
                };
                indexed.push((index, *probability));
            }
            indexed.sort_by_key(|(index, _)| *index);
            if let Some(confidence) = confidence {
                check_probability(key, "confidence", *confidence)?;
            }
            Ok(DecisionAnswer::Score {
                score: *score,
                confidence: *confidence,
                probabilities: indexed,
            })
        }
        (kind, answer) => Err(format!(
            "decisions answer `{key}` has type `{}`, expected `{}`",
            answer.type_name(),
            system_one_type_name(kind)
        )),
    }
}

const fn system_one_type_name(kind: &DecisionQuestionKind) -> &'static str {
    match kind {
        DecisionQuestionKind::Predicate { .. } => "noul",
        DecisionQuestionKind::Choice { .. } => "choice",
        DecisionQuestionKind::Score { .. } => "score",
    }
}

fn system_one_usage_to_core(usage: &SystemOneDecisionsUsage) -> DecisionsUsage {
    let detail = |object: &str, field: &str| {
        usage
            .extra
            .get(object)
            .and_then(|details| details.get(field))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    DecisionsUsage {
        input_tokens: usage.input_tokens.unwrap_or(0),
        output_tokens: usage.output_tokens.unwrap_or(0),
        cached_tokens: detail("input_tokens_details", "cached_tokens"),
        cache_write_tokens: detail("input_tokens_details", "cache_write_tokens"),
        reasoning_tokens: detail("output_tokens_details", "reasoning_tokens"),
        total_tokens: usage.extra.get("total_tokens").and_then(Value::as_i64),
    }
}

/// Render a core response for a System One caller. Usage keeps the System
/// One `input_tokens`/`output_tokens` fields and adds the token detail
/// objects and `total_tokens`.
#[must_use]
pub fn core_decisions_response_to_system_one(
    response: &DecisionsResponse,
    request: &DecisionsRequest,
) -> Value {
    let questions = request.questions_by_key();
    let answers = response
        .answers
        .iter()
        .filter_map(|outcome| {
            questions.get(outcome.key.as_str())?;
            Some((
                outcome.key.clone(),
                core_answer_to_system_one(&outcome.answer),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut body = json!({
        "model": response.model,
        "answers": answers,
    });
    if let Some(usage) = response.usage {
        body["usage"] = json!({
            "input_tokens": usage.input_tokens,
            "input_tokens_details": {
                "cached_tokens": usage.cached_tokens,
                "cache_write_tokens": usage.cache_write_tokens,
            },
            "output_tokens": usage.output_tokens,
            "output_tokens_details": {
                "reasoning_tokens": usage.reasoning_tokens,
            },
            "total_tokens": usage.total(),
        });
    }
    body
}

fn core_answer_to_system_one(answer: &DecisionAnswer) -> Value {
    match answer {
        DecisionAnswer::Predicate { probability } => json!({"type": "noul", "noul": probability}),
        DecisionAnswer::Choice {
            choice,
            confidence,
            probabilities,
        } => {
            let mut value = json!({
                "type": "choice",
                "choice": choice.system_one_key(),
                "probabilities": probabilities
                    .iter()
                    .map(|(value, probability)| (value.system_one_key(), *probability))
                    .collect::<BTreeMap<_, _>>(),
            });
            if let Some(confidence) = confidence {
                value["confidence"] = json!(confidence);
            }
            value
        }
        DecisionAnswer::Score {
            score,
            confidence,
            probabilities,
        } => {
            let mut value = json!({
                "type": "score",
                "score": score,
                "probabilities": probabilities
                    .iter()
                    .map(|(index, probability)| (index.to_string(), *probability))
                    .collect::<BTreeMap<_, _>>(),
            });
            if let Some(confidence) = confidence {
                value["confidence"] = json!(confidence);
            }
            value
        }
        DecisionAnswer::Refusal => json!({"type": "refusal"}),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn system_one_request() -> SystemOneDecisionsRequest {
        serde_json::from_value(json!({
            "model": "jev",
            "state": "Help! My payouts have been failing for 3 days.",
            "questions": {
                "is_urgent": {"type": "noul", "instructions": "Does this convey urgency?"},
                "department": {
                    "type": "choice",
                    "instructions": "Which team?",
                    "criteria": {"billing": "Payments", "technical": null}
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated?",
                    "criteria": ["Calm", "Frustrated"]
                }
            }
        }))
        .expect("valid decisions request")
    }

    fn valid_response() -> Value {
        json!({
            "model": "jev",
            "answers": {
                "is_urgent": {"type": "noul", "noul": 0.9},
                "department": {
                    "type": "choice",
                    "choice": "billing",
                    "probabilities": {"billing": 0.9, "technical": 0.1},
                    "confidence": 0.9
                },
                "frustration": {
                    "type": "score",
                    "score": 0.8,
                    "probabilities": {"0": 0.2, "1": 0.8},
                    "confidence": 0.7
                }
            },
            "usage": {"input_tokens": 12, "output_tokens": 3}
        })
    }

    #[test]
    fn request_round_trips_between_system_one_and_core() {
        let wire = system_one_request();
        let core = system_one_decisions_request_to_core(&wire);
        assert_eq!(core.model, "jev");
        assert_eq!(core.questions.len(), 3);
        assert_eq!(core.validation_error(), None);

        let translated_back = core_decisions_request_to_system_one(&core).expect("translates");
        assert_eq!(translated_back, wire);
    }

    #[test]
    fn system_one_safety_identifier_is_typed_not_forwarded_as_extra() {
        let mut body = serde_json::to_value(system_one_request()).expect("request");
        body["safety_identifier"] = json!("user-1");
        let wire: SystemOneDecisionsRequest = serde_json::from_value(body).expect("request");
        assert!(wire.extra.is_empty());

        let core = system_one_decisions_request_to_core(&wire);
        assert_eq!(core.safety_identifier.as_deref(), Some("user-1"));
        let error = core_decisions_request_to_system_one(&core).expect_err("rejected");
        assert!(error.contains("safety_identifier"), "{error}");
    }

    #[test]
    fn rejects_inputs_system_one_cannot_carry() {
        let mut core = system_one_decisions_request_to_core(&system_one_request());
        core.safety_identifier = Some("user-1".to_string());
        assert!(
            core_decisions_request_to_system_one(&core)
                .expect_err("safety identifier")
                .contains("safety_identifier")
        );

        let mut core = system_one_decisions_request_to_core(&system_one_request());
        core.questions[0].kind = DecisionQuestionKind::Choice {
            options: vec![
                DecisionChoiceOption {
                    value: DecisionChoiceValue::Bool(true),
                    description: None,
                },
                DecisionChoiceOption {
                    value: DecisionChoiceValue::Text("true".to_string()),
                    description: None,
                },
            ],
        };
        assert!(core_decisions_request_to_system_one(&core).is_err());
    }

    #[test]
    fn validates_answers_against_requested_questions() {
        let request = system_one_decisions_request_to_core(&system_one_request());
        let valid = valid_response();
        let parsed =
            system_one_decisions_response_to_core(&valid, &request).expect("valid response");
        assert_eq!(parsed.answers.len(), 3);
        assert_eq!(
            parsed.usage,
            Some(DecisionsUsage {
                input_tokens: 12,
                output_tokens: 3,
                ..DecisionsUsage::default()
            })
        );
        assert!(system_one_decisions_response_to_core(&json!({}), &request).is_err());

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
        invalid_choice["answers"]["department"]["choice"] = json!("sales");
        let mut invalid_score = valid.clone();
        invalid_score["answers"]["frustration"]["score"] = json!(2.0);
        let mut invalid_choice_probability = valid.clone();
        invalid_choice_probability["answers"]["department"]["probabilities"]["billing"] =
            json!(-0.1);
        let mut unknown_choice_probability = valid.clone();
        unknown_choice_probability["answers"]["department"]["probabilities"]["sales"] = json!(0.1);
        let mut invalid_score_probability = valid.clone();
        invalid_score_probability["answers"]["frustration"]["probabilities"]["1"] = json!(1.1);
        let mut non_index_score_probability = valid.clone();
        non_index_score_probability["answers"]["frustration"]["probabilities"]["bogus"] =
            json!(0.1);
        let mut out_of_range_score_probability = valid.clone();
        out_of_range_score_probability["answers"]["frustration"]["probabilities"]["2"] = json!(0.1);
        let mut invalid_choice_confidence = valid.clone();
        invalid_choice_confidence["answers"]["department"]["confidence"] = json!(2.0);
        let mut invalid_score_confidence = valid.clone();
        invalid_score_confidence["answers"]["frustration"]["confidence"] = json!(-0.1);
        let mut missing_answer = valid.clone();
        missing_answer["answers"]
            .as_object_mut()
            .expect("answers")
            .remove("frustration");

        for invalid in [
            wrong_type,
            missing_payload,
            invalid_probability,
            invalid_choice,
            invalid_score,
            invalid_choice_probability,
            unknown_choice_probability,
            invalid_score_probability,
            non_index_score_probability,
            out_of_range_score_probability,
            invalid_choice_confidence,
            invalid_score_confidence,
            missing_answer,
        ] {
            assert!(system_one_decisions_response_to_core(&invalid, &request).is_err());
        }
    }

    #[test]
    fn renders_core_response_for_system_one_callers() {
        let request = system_one_decisions_request_to_core(&system_one_request());
        let parsed = system_one_decisions_response_to_core(&valid_response(), &request)
            .expect("valid response");
        let rendered = core_decisions_response_to_system_one(
            &DecisionsResponse {
                usage: Some(DecisionsUsage {
                    input_tokens: 100,
                    output_tokens: 4,
                    cached_tokens: 80,
                    cache_write_tokens: 0,
                    reasoning_tokens: 2,
                    total_tokens: Some(104),
                }),
                ..parsed
            },
            &request,
        );

        assert_eq!(
            rendered["answers"]["is_urgent"],
            json!({"type": "noul", "noul": 0.9})
        );
        assert_eq!(rendered["answers"]["department"]["choice"], "billing");
        assert_eq!(
            rendered["answers"]["frustration"]["probabilities"],
            json!({"0": 0.2, "1": 0.8})
        );
        assert_eq!(rendered["usage"]["input_tokens"], 100);
        assert_eq!(rendered["usage"]["output_tokens"], 4);
        assert_eq!(
            rendered["usage"]["input_tokens_details"]["cached_tokens"],
            80
        );
        assert_eq!(
            rendered["usage"]["output_tokens_details"]["reasoning_tokens"],
            2
        );
        assert_eq!(rendered["usage"]["total_tokens"], 104);

        // The rendered body satisfies the System One contract it came from.
        system_one_decisions_response_to_core(&rendered, &request).expect("round trips");
    }
}
