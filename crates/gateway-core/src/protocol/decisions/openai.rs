//! OpenAI Decisions wire format: `input` (text or user messages with text and
//! image parts) evaluated against an ordered list of `predicate`, `choice`,
//! and `score` questions. Answers come back in question order.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    DecisionAnswer, DecisionChoiceOption, DecisionChoiceValue, DecisionOutcome, DecisionQuestion,
    DecisionQuestionKind, DecisionScoreLevel, DecisionsInput, DecisionsInputMessage,
    DecisionsRequest, DecisionsResponse, DecisionsUsage, check_probability,
    check_probability_count, value_to_text,
};

/// Maximum number of questions in one OpenAI Decisions request.
pub const OPENAI_DECISIONS_MAX_QUESTIONS: usize = 200;
/// Minimum number of options in an OpenAI Decisions `choice` question.
pub const OPENAI_DECISIONS_CHOICE_MIN_OPTIONS: usize = 2;
/// Maximum number of images across all input messages.
pub const OPENAI_DECISIONS_MAX_IMAGES: usize = 128;

/// OpenAI Decisions request body (`POST /v1/decisions`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiDecisionsRequest {
    pub model: String,
    pub input: OpenAiDecisionsInput,
    pub questions: Vec<OpenAiDecisionQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum OpenAiDecisionsInput {
    Text(String),
    Messages(Vec<DecisionsInputMessage>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OpenAiDecisionQuestion {
    Predicate {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Choice {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        choices: Vec<OpenAiDecisionChoice>,
    },
    Score {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        levels: Vec<OpenAiDecisionScoreLevel>,
    },
}

impl OpenAiDecisionQuestion {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Predicate { name, .. } | Self::Choice { name, .. } | Self::Score { name, .. } => {
                name.as_deref()
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiDecisionChoice {
    pub value: DecisionChoiceValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiDecisionScoreLevel {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// OpenAI Decisions response body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiDecisionsResponse {
    pub answers: Vec<OpenAiDecisionAnswer>,
    pub model: String,
    pub usage: OpenAiDecisionsUsage,
}

/// One answer. `name` echoes the question's name, or `null` when unnamed;
/// the field is required, so an upstream that omits it fails as shape drift.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OpenAiDecisionAnswer {
    Predicate {
        #[serde(deserialize_with = "required_nullable")]
        name: Option<String>,
        probability: f64,
    },
    Choice {
        #[serde(deserialize_with = "required_nullable")]
        name: Option<String>,
        choice: DecisionChoiceValue,
        confidence: f64,
        probabilities: Vec<OpenAiChoiceProbability>,
    },
    Score {
        #[serde(deserialize_with = "required_nullable")]
        name: Option<String>,
        score: f64,
        confidence: f64,
        probabilities: Vec<OpenAiScoreProbability>,
    },
    Refusal {
        #[serde(deserialize_with = "required_nullable")]
        name: Option<String>,
    },
}

/// `Option` fields are implicitly optional in serde; `deserialize_with`
/// makes the key required while still accepting `null`.
fn required_nullable<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

impl OpenAiDecisionAnswer {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Predicate { name, .. }
            | Self::Choice { name, .. }
            | Self::Score { name, .. }
            | Self::Refusal { name } => name.as_deref(),
        }
    }

    const fn type_name(&self) -> &'static str {
        match self {
            Self::Predicate { .. } => "predicate",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
            Self::Refusal { .. } => "refusal",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiChoiceProbability {
    pub value: DecisionChoiceValue,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiScoreProbability {
    pub value: usize,
    pub label: String,
    pub probability: f64,
}

/// OpenAI Decisions usage. Every field is required on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OpenAiDecisionsUsage {
    pub input_tokens: i64,
    pub input_tokens_details: OpenAiDecisionsInputTokensDetails,
    pub output_tokens: i64,
    pub output_tokens_details: OpenAiDecisionsOutputTokensDetails,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OpenAiDecisionsInputTokensDetails {
    pub cached_tokens: i64,
    pub cache_write_tokens: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OpenAiDecisionsOutputTokensDetails {
    pub reasoning_tokens: i64,
}

/// OpenAI request to core, with the OpenAI edge checks: 1–200 questions,
/// 2+ unique typed choices, user-only messages, and at most 128 images.
/// Answers correlate by position, so names need not be unique: a question
/// keeps its name as its key unless the name is missing or already taken,
/// in which case it gets a positional `question_<index>` key. The original
/// name is kept for rendering either way.
pub fn openai_decisions_request_to_core(
    request: &OpenAiDecisionsRequest,
) -> Result<DecisionsRequest, String> {
    if request.questions.len() > OPENAI_DECISIONS_MAX_QUESTIONS {
        return Err(format!(
            "decisions request has {} questions, maximum is {OPENAI_DECISIONS_MAX_QUESTIONS}",
            request.questions.len()
        ));
    }
    let input = openai_input_to_core(&request.input)?;

    let names = request
        .questions
        .iter()
        .filter_map(OpenAiDecisionQuestion::name)
        .collect::<BTreeSet<_>>();
    let mut keys = BTreeSet::new();
    let mut questions = Vec::with_capacity(request.questions.len());
    for (index, question) in request.questions.iter().enumerate() {
        let key = match question.name() {
            Some(name) if !keys.contains(name) => name.to_string(),
            _ => positional_key(index, |key| names.contains(key) || keys.contains(key)),
        };
        keys.insert(key.clone());
        questions.push(openai_question_to_core(key, question)?);
    }

    Ok(DecisionsRequest {
        model: request.model.clone(),
        input,
        questions,
        safety_identifier: request.safety_identifier.clone(),
        extra: request.extra.clone(),
    })
}

fn positional_key(index: usize, taken: impl Fn(&str) -> bool) -> String {
    let base = format!("question_{index}");
    let mut key = base.clone();
    let mut suffix = 1;
    while taken(&key) {
        key = format!("{base}_{suffix}");
        suffix += 1;
    }
    key
}

fn openai_input_to_core(input: &OpenAiDecisionsInput) -> Result<DecisionsInput, String> {
    let input = match input {
        OpenAiDecisionsInput::Text(text) => DecisionsInput::Text(text.clone()),
        OpenAiDecisionsInput::Messages(messages) => {
            if let Some(message) = messages.iter().find(|message| message.role != "user") {
                return Err(format!(
                    "decisions input messages must have role `user`, got `{}`",
                    message.role
                ));
            }
            if let Some(kind) = messages
                .iter()
                .filter_map(|message| message.kind.as_deref())
                .find(|kind| *kind != "message")
            {
                return Err(format!(
                    "decisions input items must have type `message`, got `{kind}`"
                ));
            }
            DecisionsInput::Messages(messages.clone())
        }
    };
    let images = input.image_count();
    if images > OPENAI_DECISIONS_MAX_IMAGES {
        return Err(format!(
            "decisions input has {images} images, maximum is {OPENAI_DECISIONS_MAX_IMAGES}"
        ));
    }
    Ok(input)
}

fn openai_question_to_core(
    key: String,
    question: &OpenAiDecisionQuestion,
) -> Result<DecisionQuestion, String> {
    let (instructions, name, kind) = match question {
        OpenAiDecisionQuestion::Predicate { instructions, name } => (
            instructions,
            name,
            DecisionQuestionKind::Predicate { criteria: None },
        ),
        OpenAiDecisionQuestion::Choice {
            instructions,
            name,
            choices,
        } => {
            if choices.len() < OPENAI_DECISIONS_CHOICE_MIN_OPTIONS {
                return Err(format!(
                    "decisions question `{key}` has {} choices, expected at least {OPENAI_DECISIONS_CHOICE_MIN_OPTIONS}",
                    choices.len()
                ));
            }
            (
                instructions,
                name,
                DecisionQuestionKind::Choice {
                    options: choices
                        .iter()
                        .map(|choice| DecisionChoiceOption {
                            value: choice.value.clone(),
                            description: choice.description.clone().map(Value::String),
                        })
                        .collect(),
                },
            )
        }
        OpenAiDecisionQuestion::Score {
            instructions,
            name,
            levels,
        } => (
            instructions,
            name,
            DecisionQuestionKind::Score {
                levels: levels
                    .iter()
                    .map(|level| DecisionScoreLevel {
                        label: Value::String(level.label.clone()),
                        description: level.description.clone().map(Value::String),
                    })
                    .collect(),
            },
        ),
    };
    Ok(DecisionQuestion {
        key,
        name: name.clone(),
        instructions: Value::String(instructions.clone()),
        kind,
    })
}

/// Core request to OpenAI. System One `noul` criteria fold into the
/// instructions, non-string text is sent as compact JSON, and requests
/// OpenAI cannot accept (more than 200 questions, single-option choices)
/// fail before the upstream call.
pub fn core_decisions_request_to_openai(
    request: &DecisionsRequest,
) -> Result<OpenAiDecisionsRequest, String> {
    if request.questions.len() > OPENAI_DECISIONS_MAX_QUESTIONS {
        return Err(format!(
            "decisions request has {} questions, but OpenAI Decisions providers accept at most {OPENAI_DECISIONS_MAX_QUESTIONS}",
            request.questions.len()
        ));
    }
    let questions = request
        .questions
        .iter()
        .map(core_question_to_openai)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OpenAiDecisionsRequest {
        model: request.model.clone(),
        input: request.input.to_openai_input(),
        questions,
        safety_identifier: request.safety_identifier.clone(),
        extra: request.extra.clone(),
    })
}

fn core_question_to_openai(question: &DecisionQuestion) -> Result<OpenAiDecisionQuestion, String> {
    let mut instructions = value_to_text(&question.instructions);
    let name = question.name.clone();
    Ok(match &question.kind {
        DecisionQuestionKind::Predicate { criteria } => {
            if let Some(criteria) = criteria {
                if let Some(yes) = &criteria.yes {
                    instructions.push_str(&format!("\n\nTrue when: {}", value_to_text(yes)));
                }
                if let Some(no) = &criteria.no {
                    instructions.push_str(&format!("\n\nFalse when: {}", value_to_text(no)));
                }
            }
            OpenAiDecisionQuestion::Predicate { instructions, name }
        }
        DecisionQuestionKind::Choice { options } => {
            if options.len() < OPENAI_DECISIONS_CHOICE_MIN_OPTIONS {
                return Err(format!(
                    "decisions question `{}` has {} choice, but OpenAI Decisions providers require at least {OPENAI_DECISIONS_CHOICE_MIN_OPTIONS}",
                    question.key,
                    options.len()
                ));
            }
            OpenAiDecisionQuestion::Choice {
                instructions,
                name,
                choices: options
                    .iter()
                    .map(|option| OpenAiDecisionChoice {
                        value: option.value.clone(),
                        description: option.description.as_ref().map(value_to_text),
                    })
                    .collect(),
            }
        }
        DecisionQuestionKind::Score { levels } => OpenAiDecisionQuestion::Score {
            instructions,
            name,
            levels: levels
                .iter()
                .map(|level| OpenAiDecisionScoreLevel {
                    label: value_to_text(&level.label),
                    description: level.description.as_ref().map(value_to_text),
                })
                .collect(),
        },
    })
}

/// Parse an OpenAI response and reject shape drift: one answer per question
/// in order, matching names and types (or a refusal), known choices, and
/// in-bounds scores and probabilities. Scores are zero-based level indexes.
pub fn openai_decisions_response_to_core(
    body: &Value,
    request: &DecisionsRequest,
) -> Result<DecisionsResponse, String> {
    let response = OpenAiDecisionsResponse::deserialize(body)
        .map_err(|error| format!("invalid decisions response: {error}"))?;
    if response.answers.len() != request.questions.len() {
        return Err(format!(
            "decisions response has {} answers for {} questions",
            response.answers.len(),
            request.questions.len()
        ));
    }

    let answers = request
        .questions
        .iter()
        .zip(&response.answers)
        .map(|(question, answer)| {
            if answer.name() != question.name.as_deref() {
                return Err(format!(
                    "decisions answer for `{}` has name {:?}, expected {:?}",
                    question.key,
                    answer.name(),
                    question.name
                ));
            }
            Ok(DecisionOutcome {
                key: question.key.clone(),
                answer: openai_answer_to_core(question, answer)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let usage = response.usage;
    Ok(DecisionsResponse {
        model: response.model,
        answers,
        usage: Some(DecisionsUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_tokens: usage.input_tokens_details.cached_tokens,
            cache_write_tokens: usage.input_tokens_details.cache_write_tokens,
            reasoning_tokens: usage.output_tokens_details.reasoning_tokens,
            total_tokens: Some(usage.total_tokens),
        }),
    })
}

fn openai_answer_to_core(
    question: &DecisionQuestion,
    answer: &OpenAiDecisionAnswer,
) -> Result<DecisionAnswer, String> {
    let key = &question.key;
    match (&question.kind, answer) {
        (_, OpenAiDecisionAnswer::Refusal { .. }) => Ok(DecisionAnswer::Refusal),
        (
            DecisionQuestionKind::Predicate { .. },
            OpenAiDecisionAnswer::Predicate { probability, .. },
        ) => {
            check_probability(key, "probability", *probability)?;
            Ok(DecisionAnswer::Predicate {
                probability: *probability,
            })
        }
        (
            DecisionQuestionKind::Choice { options },
            OpenAiDecisionAnswer::Choice {
                choice,
                confidence,
                probabilities,
                ..
            },
        ) => {
            let known =
                |value: &DecisionChoiceValue| options.iter().any(|option| &option.value == value);
            if !known(choice) {
                return Err(format!(
                    "decisions answer `{key}` selected unknown choice {}",
                    choice.to_value()
                ));
            }
            check_probability(key, "confidence", *confidence)?;
            check_probability_count(key, probabilities.len(), options.len())?;
            let mut typed = Vec::with_capacity(probabilities.len());
            for entry in probabilities {
                if !known(&entry.value) {
                    return Err(format!(
                        "decisions answer `{key}` reports probability for unknown choice {}",
                        entry.value.to_value()
                    ));
                }
                check_probability(key, "choice probability", entry.probability)?;
                typed.push((entry.value.clone(), entry.probability));
            }
            Ok(DecisionAnswer::Choice {
                choice: choice.clone(),
                confidence: Some(*confidence),
                probabilities: typed,
            })
        }
        (
            DecisionQuestionKind::Score { levels },
            OpenAiDecisionAnswer::Score {
                score,
                confidence,
                probabilities,
                ..
            },
        ) => {
            let max = levels.len().saturating_sub(1);
            if !(0.0..=max as f64).contains(score) {
                return Err(format!(
                    "decisions answer `{key}` has score {score}, expected 0..={max}"
                ));
            }
            check_probability(key, "confidence", *confidence)?;
            check_probability_count(key, probabilities.len(), levels.len())?;
            let mut indexed = Vec::with_capacity(probabilities.len());
            for entry in probabilities {
                if entry.value > max {
                    return Err(format!(
                        "decisions answer `{key}` reports probability for level {}, expected 0..={max}",
                        entry.value
                    ));
                }
                check_probability(key, "level probability", entry.probability)?;
                indexed.push((entry.value, entry.probability));
            }
            Ok(DecisionAnswer::Score {
                score: *score,
                confidence: Some(*confidence),
                probabilities: indexed,
            })
        }
        (kind, answer) => Err(format!(
            "decisions answer `{key}` has type `{}`, expected `{}`",
            answer.type_name(),
            openai_type_name(kind)
        )),
    }
}

const fn openai_type_name(kind: &DecisionQuestionKind) -> &'static str {
    match kind {
        DecisionQuestionKind::Predicate { .. } => "predicate",
        DecisionQuestionKind::Choice { .. } => "choice",
        DecisionQuestionKind::Score { .. } => "score",
    }
}

/// Render a core response for an OpenAI caller. Missing confidence falls back
/// to the selected option's probability; missing usage reports zero tokens so
/// every required usage field is present.
#[must_use]
pub fn core_decisions_response_to_openai(
    response: &DecisionsResponse,
    request: &DecisionsRequest,
) -> Value {
    let questions = request.questions_by_key();
    let answers = response
        .answers
        .iter()
        .filter_map(|outcome| {
            let question = questions.get(outcome.key.as_str())?;
            Some(core_answer_to_openai(question, &outcome.answer))
        })
        .collect();
    let usage = response.usage.unwrap_or_default();
    let body = OpenAiDecisionsResponse {
        answers,
        model: response.model.clone(),
        usage: OpenAiDecisionsUsage {
            input_tokens: usage.input_tokens,
            input_tokens_details: OpenAiDecisionsInputTokensDetails {
                cached_tokens: usage.cached_tokens,
                cache_write_tokens: usage.cache_write_tokens,
            },
            output_tokens: usage.output_tokens,
            output_tokens_details: OpenAiDecisionsOutputTokensDetails {
                reasoning_tokens: usage.reasoning_tokens,
            },
            total_tokens: usage.total(),
        },
    };
    serde_json::to_value(body).unwrap_or(Value::Null)
}

fn core_answer_to_openai(
    question: &DecisionQuestion,
    answer: &DecisionAnswer,
) -> OpenAiDecisionAnswer {
    let name = question.name.clone();
    match answer {
        DecisionAnswer::Predicate { probability } => OpenAiDecisionAnswer::Predicate {
            name,
            probability: *probability,
        },
        DecisionAnswer::Choice {
            choice,
            confidence,
            probabilities,
        } => {
            let selected = probabilities
                .iter()
                .find(|(value, _)| value == choice)
                .map(|(_, probability)| *probability);
            OpenAiDecisionAnswer::Choice {
                name,
                choice: choice.clone(),
                confidence: confidence.or(selected).unwrap_or(1.0),
                probabilities: probabilities
                    .iter()
                    .map(|(value, probability)| OpenAiChoiceProbability {
                        value: value.clone(),
                        probability: *probability,
                    })
                    .collect(),
            }
        }
        DecisionAnswer::Score {
            score,
            confidence,
            probabilities,
        } => {
            let levels = match &question.kind {
                DecisionQuestionKind::Score { levels } => levels.as_slice(),
                _ => &[],
            };
            let label = |index: usize| {
                levels
                    .get(index)
                    .map(|level| value_to_text(&level.label))
                    .unwrap_or_default()
            };
            // Round half up to the nearest level for the fallback confidence.
            let nearest = score.round() as usize;
            let selected = probabilities
                .iter()
                .find(|(index, _)| *index == nearest)
                .map(|(_, probability)| *probability);
            OpenAiDecisionAnswer::Score {
                name,
                score: *score,
                confidence: confidence.or(selected).unwrap_or(1.0),
                probabilities: probabilities
                    .iter()
                    .map(|(index, probability)| OpenAiScoreProbability {
                        value: *index,
                        label: label(*index),
                        probability: *probability,
                    })
                    .collect(),
            }
        }
        DecisionAnswer::Refusal => OpenAiDecisionAnswer::Refusal { name },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::system_one::{
        SystemOneDecisionsRequest, core_decisions_request_to_system_one,
        core_decisions_response_to_system_one, system_one_decisions_request_to_core,
        system_one_decisions_response_to_core,
    };
    use super::*;

    fn openai_request() -> OpenAiDecisionsRequest {
        serde_json::from_value(json!({
            "model": "gpt-6-luna",
            "input": "Help! My payouts have been failing for 3 days.",
            "questions": [
                {"type": "predicate", "name": "is_urgent", "instructions": "Does this convey urgency?"},
                {
                    "type": "choice",
                    "name": "department",
                    "instructions": "Which team?",
                    "choices": [
                        {"value": "billing", "description": "Payments"},
                        {"value": "technical"},
                        {"value": true}
                    ]
                },
                {
                    "type": "score",
                    "instructions": "How frustrated?",
                    "levels": [
                        {"label": "Calm"},
                        {"label": "Frustrated", "description": "Raised voice"}
                    ]
                }
            ],
            "safety_identifier": "user-123"
        }))
        .expect("valid OpenAI decisions request")
    }

    fn openai_response() -> Value {
        json!({
            "answers": [
                {"type": "predicate", "name": "is_urgent", "probability": 0.92},
                {
                    "type": "choice",
                    "name": "department",
                    "choice": "billing",
                    "confidence": 0.8,
                    "probabilities": [
                        {"value": "billing", "probability": 0.8},
                        {"value": "technical", "probability": 0.15},
                        {"value": true, "probability": 0.05}
                    ]
                },
                {
                    "type": "score",
                    "name": null,
                    "score": 1.0,
                    "confidence": 0.7,
                    "probabilities": [
                        {"value": 0, "label": "Calm", "probability": 0.3},
                        {"value": 1, "label": "Frustrated", "probability": 0.7}
                    ]
                }
            ],
            "model": "gpt-6-luna",
            "usage": {
                "input_tokens": 120,
                "input_tokens_details": {"cached_tokens": 100, "cache_write_tokens": 0},
                "output_tokens": 6,
                "output_tokens_details": {"reasoning_tokens": 4},
                "total_tokens": 126
            }
        })
    }

    #[test]
    fn request_round_trips_between_openai_and_core() {
        let wire = openai_request();
        let core = openai_decisions_request_to_core(&wire).expect("translates");
        assert_eq!(core.validation_error(), None);
        assert_eq!(core.questions[2].key, "question_2");
        assert_eq!(core.questions[2].name, None);
        assert_eq!(core.safety_identifier.as_deref(), Some("user-123"));

        let translated_back = core_decisions_request_to_openai(&core).expect("translates");
        assert_eq!(translated_back, wire);
        assert_eq!(
            serde_json::to_value(&translated_back).expect("serializes")["questions"][1]["choices"]
                [2],
            json!({"value": true})
        );
    }

    #[test]
    fn openai_request_translates_to_system_one() {
        let mut wire = openai_request();
        wire.safety_identifier = None;
        let core = openai_decisions_request_to_core(&wire).expect("translates");
        let system_one = core_decisions_request_to_system_one(&core).expect("translates");
        let body = serde_json::to_value(&system_one).expect("serializes");

        assert_eq!(
            body["state"],
            "Help! My payouts have been failing for 3 days."
        );
        assert_eq!(body["questions"]["is_urgent"]["type"], "noul");
        assert_eq!(
            body["questions"]["department"]["criteria"],
            json!({"billing": "Payments", "technical": null, "true": null})
        );
        assert_eq!(
            body["questions"]["question_2"]["criteria"],
            json!(["Calm", "Frustrated: Raised voice"])
        );
    }

    #[test]
    fn system_one_request_translates_to_openai() {
        let system_one: SystemOneDecisionsRequest = serde_json::from_value(json!({
            "model": "jev",
            "state": {"ticket": "payouts failing"},
            "questions": {
                "is_urgent": {
                    "type": "noul",
                    "instructions": "Does this convey urgency?",
                    "criteria": {"true": "Customer is blocked", "false": "Routine"}
                },
                "department": {
                    "type": "choice",
                    "instructions": {"prompt": "Which team?"},
                    "criteria": {"billing": "Payments", "technical": null}
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated?",
                    "criteria": ["Calm", "Frustrated"]
                }
            }
        }))
        .expect("valid System One request");
        let core = system_one_decisions_request_to_core(&system_one);
        let openai =
            serde_json::to_value(core_decisions_request_to_openai(&core).expect("translates"))
                .expect("serializes");

        assert_eq!(openai["input"], "{\"ticket\":\"payouts failing\"}");
        // System One questions are keyed by name, so OpenAI receives them sorted by name.
        assert_eq!(openai["questions"][0]["name"], "department");
        assert_eq!(
            openai["questions"][0]["instructions"],
            "{\"prompt\":\"Which team?\"}"
        );
        assert_eq!(
            openai["questions"][0]["choices"],
            json!([{"value": "billing", "description": "Payments"}, {"value": "technical"}])
        );
        assert_eq!(openai["questions"][2]["type"], "predicate");
        assert_eq!(
            openai["questions"][2]["instructions"],
            "Does this convey urgency?\n\nTrue when: Customer is blocked\n\nFalse when: Routine"
        );
        assert_eq!(
            openai["questions"][1]["levels"],
            json!([{"label": "Calm"}, {"label": "Frustrated"}])
        );
    }

    #[test]
    fn duplicate_question_names_get_positional_keys_and_keep_their_names() {
        let mut duplicate = openai_request();
        if let OpenAiDecisionQuestion::Score { name, .. } = &mut duplicate.questions[2] {
            *name = Some("is_urgent".to_string());
        }
        let core = openai_decisions_request_to_core(&duplicate).expect("duplicates are valid");
        assert_eq!(core.validation_error(), None);
        assert_eq!(core.questions[0].key, "is_urgent");
        assert_eq!(core.questions[2].key, "question_2");
        assert_eq!(core.questions[2].name.as_deref(), Some("is_urgent"));

        let forwarded = core_decisions_request_to_openai(&core).expect("translates");
        assert_eq!(forwarded.questions[2].name(), Some("is_urgent"));
    }

    #[test]
    fn rejects_requests_openai_cannot_accept() {
        let single_choice: SystemOneDecisionsRequest = serde_json::from_value(json!({
            "model": "jev",
            "state": "state",
            "questions": {
                "department": {"type": "choice", "instructions": "Which team?", "criteria": {"billing": null}}
            }
        }))
        .expect("valid System One request");
        let core = system_one_decisions_request_to_core(&single_choice);
        assert!(core_decisions_request_to_openai(&core).is_err());

        let mut one_choice = openai_request();
        if let OpenAiDecisionQuestion::Choice { choices, .. } = &mut one_choice.questions[1] {
            choices.truncate(1);
        }
        assert!(openai_decisions_request_to_core(&one_choice).is_err());

        let mut too_many = openai_request();
        too_many.questions = (0..=OPENAI_DECISIONS_MAX_QUESTIONS)
            .map(|_| OpenAiDecisionQuestion::Predicate {
                instructions: "q".to_string(),
                name: None,
            })
            .collect();
        assert!(openai_decisions_request_to_core(&too_many).is_err());

        let assistant: OpenAiDecisionsRequest = serde_json::from_value(json!({
            "model": "gpt-6-luna",
            "input": [{"role": "assistant", "content": "hi"}],
            "questions": [{"type": "predicate", "instructions": "q"}]
        }))
        .expect("parses");
        assert!(openai_decisions_request_to_core(&assistant).is_err());
    }

    #[test]
    fn accepts_an_empty_message_list() {
        let empty: OpenAiDecisionsRequest = serde_json::from_value(json!({
            "model": "gpt-6-luna",
            "input": [],
            "questions": [{"type": "predicate", "instructions": "q"}]
        }))
        .expect("parses");
        let core = openai_decisions_request_to_core(&empty).expect("translates");
        assert!(core.validation_error().is_none());
        assert_eq!(
            core_decisions_request_to_openai(&core)
                .expect("round trips")
                .input,
            OpenAiDecisionsInput::Messages(vec![])
        );
    }

    #[test]
    fn positional_keys_avoid_given_names() {
        let request: OpenAiDecisionsRequest = serde_json::from_value(json!({
            "model": "gpt-6-luna",
            "input": "x",
            "questions": [
                {"type": "predicate", "instructions": "a"},
                {"type": "predicate", "name": "question_0", "instructions": "b"}
            ]
        }))
        .expect("parses");
        let core = openai_decisions_request_to_core(&request).expect("translates");
        assert_eq!(core.questions[0].key, "question_0_1");
        assert_eq!(core.validation_error(), None);
    }

    #[test]
    fn image_inputs_parse_and_stay_out_of_system_one() {
        let request: OpenAiDecisionsRequest = serde_json::from_value(json!({
            "model": "gpt-6-luna",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Is this a receipt?"},
                    {"type": "input_image", "image_url": "data:image/png;base64,AAAA", "detail": "low"}
                ]
            }],
            "questions": [{"type": "predicate", "instructions": "Receipt?"}]
        }))
        .expect("parses");
        let core = openai_decisions_request_to_core(&request).expect("translates");
        assert_eq!(core.input.image_count(), 1);
        assert_eq!(
            core_decisions_request_to_openai(&core).expect("translates"),
            request
        );
        assert!(
            core_decisions_request_to_system_one(&core)
                .expect_err("images")
                .contains("image")
        );
    }

    #[test]
    fn openai_response_parses_and_renders_for_both_callers() {
        let request = openai_decisions_request_to_core(&openai_request()).expect("translates");
        let parsed = openai_decisions_response_to_core(&openai_response(), &request)
            .expect("valid response");
        assert_eq!(
            parsed.usage,
            Some(DecisionsUsage {
                input_tokens: 120,
                output_tokens: 6,
                cached_tokens: 100,
                cache_write_tokens: 0,
                reasoning_tokens: 4,
                total_tokens: Some(126),
            })
        );

        // Same-format rendering reproduces the upstream body.
        assert_eq!(
            core_decisions_response_to_openai(&parsed, &request),
            openai_response()
        );

        let system_one = core_decisions_response_to_system_one(&parsed, &request);
        assert_eq!(
            system_one["answers"]["is_urgent"],
            json!({"type": "noul", "noul": 0.92})
        );
        assert_eq!(
            system_one["answers"]["department"]["probabilities"],
            json!({"billing": 0.8, "technical": 0.15, "true": 0.05})
        );
        assert_eq!(system_one["answers"]["question_2"]["score"], 1.0);
        assert_eq!(system_one["usage"]["input_tokens"], 120);
        assert_eq!(
            system_one["usage"]["input_tokens_details"]["cached_tokens"],
            100
        );
        assert_eq!(
            system_one["usage"]["output_tokens_details"]["reasoning_tokens"],
            4
        );
    }

    #[test]
    fn system_one_response_renders_for_openai_callers() {
        let mut wire = openai_request();
        wire.safety_identifier = None;
        let request = openai_decisions_request_to_core(&wire).expect("translates");
        let upstream = json!({
            "model": "jev",
            "answers": {
                "is_urgent": {"type": "noul", "noul": 0.9},
                "department": {
                    "type": "choice",
                    "choice": "true",
                    "probabilities": {"billing": 0.2, "true": 0.8}
                },
                "question_2": {
                    "type": "score",
                    "score": 0.8,
                    "probabilities": {"0": 0.2, "1": 0.8},
                    "confidence": 0.6
                }
            },
            "usage": {"input_tokens": 50, "output_tokens": 2}
        });
        let parsed =
            system_one_decisions_response_to_core(&upstream, &request).expect("valid response");
        let rendered = core_decisions_response_to_openai(&parsed, &request);

        assert_eq!(
            rendered["answers"][0],
            json!({"type": "predicate", "name": "is_urgent", "probability": 0.9})
        );
        assert_eq!(rendered["answers"][1]["choice"], json!(true));
        assert_eq!(rendered["answers"][1]["confidence"], 0.8);
        assert_eq!(
            rendered["answers"][1]["probabilities"],
            json!([{"value": "billing", "probability": 0.2}, {"value": true, "probability": 0.8}])
        );
        assert_eq!(rendered["answers"][2]["name"], Value::Null);
        assert_eq!(
            rendered["answers"][2]["probabilities"],
            json!([
                {"value": 0, "label": "Calm", "probability": 0.2},
                {"value": 1, "label": "Frustrated", "probability": 0.8}
            ])
        );
        assert_eq!(
            rendered["usage"],
            json!({
                "input_tokens": 50,
                "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
                "output_tokens": 2,
                "output_tokens_details": {"reasoning_tokens": 0},
                "total_tokens": 52
            })
        );

        // The rendered body satisfies the OpenAI contract.
        openai_decisions_response_to_core(&rendered, &request).expect("round trips");
    }

    #[test]
    fn rejects_openai_response_drift() {
        let request = openai_decisions_request_to_core(&openai_request()).expect("translates");
        let valid = openai_response();
        openai_decisions_response_to_core(&valid, &request).expect("valid");

        let mut refusal = valid.clone();
        refusal["answers"][0] = json!({"type": "refusal", "name": "is_urgent"});
        let parsed = openai_decisions_response_to_core(&refusal, &request).expect("refusal");
        assert_eq!(parsed.answers[0].answer, DecisionAnswer::Refusal);

        let mut missing_answer = valid.clone();
        missing_answer["answers"]
            .as_array_mut()
            .expect("answers")
            .pop();
        let mut wrong_name = valid.clone();
        wrong_name["answers"][0]["name"] = json!("other");
        let mut wrong_type = valid.clone();
        wrong_type["answers"][0] = json!({"type": "score", "name": "is_urgent", "score": 0.0, "confidence": 1.0, "probabilities": []});
        let mut unknown_choice = valid.clone();
        unknown_choice["answers"][1]["choice"] = json!("sales");
        let mut string_for_bool = valid.clone();
        string_for_bool["answers"][1]["probabilities"][2]["value"] = json!("true");
        let mut out_of_range_score = valid.clone();
        out_of_range_score["answers"][2]["score"] = json!(2.0);
        let mut bad_probability = valid.clone();
        bad_probability["answers"][0]["probability"] = json!(1.5);
        let mut missing_usage = valid.clone();
        missing_usage["usage"]
            .as_object_mut()
            .expect("usage")
            .remove("total_tokens");

        // `name` is required but nullable: `null` parses, omission does not.
        let mut missing_name = valid.clone();
        missing_name["answers"][2]
            .as_object_mut()
            .expect("score answer")
            .remove("name");
        let mut extra_probabilities = valid.clone();
        extra_probabilities["answers"][2]["probabilities"]
            .as_array_mut()
            .expect("probabilities")
            .push(json!({"value": 0, "label": "Calm", "probability": 0.0}));

        for invalid in [
            missing_answer,
            wrong_name,
            wrong_type,
            unknown_choice,
            string_for_bool,
            out_of_range_score,
            bad_probability,
            missing_usage,
            missing_name,
            extra_probabilities,
        ] {
            assert!(openai_decisions_response_to_core(&invalid, &request).is_err());
        }
    }
}
