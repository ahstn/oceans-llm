//! Decisions API family: one format-neutral representation shared by the
//! System One wire format (`state` plus named questions) and the OpenAI
//! Decisions wire format (`input` plus ordered questions).
//!
//! Callers send either format. Providers receive whichever format they speak,
//! translated from the core request, and responses are rendered back in the
//! caller's format. Translation failures that a provider cannot represent
//! (image inputs for System One, single-option choices for OpenAI) surface as
//! invalid-request errors before any upstream call.

pub mod openai;
pub mod system_one;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::core::RequestRequirements;

/// Maximum number of options in a Decisions `choice` question.
pub const DECISIONS_CHOICE_MAX_OPTIONS: usize = 255;
/// Minimum number of levels in a Decisions `score` question.
pub const DECISIONS_SCORE_MIN_LEVELS: usize = 2;
/// Maximum number of levels in a Decisions `score` question.
pub const DECISIONS_SCORE_MAX_LEVELS: usize = 10;

/// Wire format a caller or provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionsWireFormat {
    /// `state` plus a map of named `noul`/`choice`/`score` questions.
    SystemOne,
    /// `input` plus an ordered list of `predicate`/`choice`/`score` questions.
    OpenAi,
}

impl DecisionsWireFormat {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemOne => "System One",
            Self::OpenAi => "OpenAI",
        }
    }

    /// Parse and validate an upstream response body against the request it
    /// answers.
    pub fn parse_response(
        self,
        body: &Value,
        request: &DecisionsRequest,
    ) -> Result<DecisionsResponse, String> {
        match self {
            Self::SystemOne => system_one::system_one_decisions_response_to_core(body, request),
            Self::OpenAi => openai::openai_decisions_response_to_core(body, request),
        }
    }

    /// Render a core response in this wire format.
    #[must_use]
    pub fn render_response(
        self,
        response: &DecisionsResponse,
        request: &DecisionsRequest,
    ) -> Value {
        match self {
            Self::SystemOne => system_one::core_decisions_response_to_system_one(response, request),
            Self::OpenAi => openai::core_decisions_response_to_openai(response, request),
        }
    }
}

/// A validated provider response: the raw upstream body in the provider's
/// wire format plus its parsed core form.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderDecisionsResponse {
    pub format: DecisionsWireFormat,
    pub body: Value,
    pub parsed: DecisionsResponse,
}

/// Canonical Decisions request. Questions keep caller order; each carries a
/// stable `key` (the System One map key, or the OpenAI name or a positional
/// fallback) used to match answers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionsRequest {
    pub model: String,
    pub input: DecisionsInput,
    pub questions: Vec<DecisionQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

impl DecisionsRequest {
    #[must_use]
    pub const fn requirements(&self) -> RequestRequirements {
        let _ = self;
        RequestRequirements {
            chat_completions: false,
            responses: false,
            stream: false,
            embeddings: false,
            decisions: true,
            tools: false,
            vision: false,
            json_schema: false,
            developer_role: false,
        }
    }

    /// Gateway-edge validation shared by both wire formats.
    #[must_use]
    pub fn validation_error(&self) -> Option<String> {
        if self.model.trim().is_empty() {
            return Some("decisions request `model` cannot be empty".to_string());
        }
        if self.questions.is_empty() {
            return Some("decisions request `questions` cannot be empty".to_string());
        }
        let mut keys = BTreeSet::new();
        for question in &self.questions {
            if !keys.insert(question.key.as_str()) {
                return Some(format!(
                    "decisions question `{}` is defined more than once",
                    question.key
                ));
            }
            if let Some(message) = question.validation_error() {
                return Some(message);
            }
        }
        None
    }

    pub(crate) fn question(&self, key: &str) -> Option<&DecisionQuestion> {
        self.questions.iter().find(|question| question.key == key)
    }
}

/// What the questions are asked about.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionsInput {
    /// System One `state`: any JSON value.
    State(Value),
    /// OpenAI `input` as a bare string.
    Text(String),
    /// OpenAI `input` as user messages with text and image parts.
    Messages(Vec<DecisionsInputMessage>),
}

impl DecisionsInput {
    /// System One `state` for this input. Text parts are joined with blank
    /// lines; image parts cannot be represented.
    pub fn to_state(&self) -> Result<Value, String> {
        match self {
            Self::State(state) => Ok(state.clone()),
            Self::Text(text) => Ok(Value::String(text.clone())),
            Self::Messages(messages) => {
                if self.image_count() > 0 {
                    return Err(
                        "image inputs are not supported by System One Decisions providers"
                            .to_string(),
                    );
                }
                let text = messages
                    .iter()
                    .flat_map(DecisionsInputMessage::texts)
                    .collect::<Vec<_>>()
                    .join("\n\n");
                Ok(Value::String(text))
            }
        }
    }

    /// OpenAI `input` for this input. A string `state` passes through as
    /// text; any other `state` is sent as compact JSON text.
    #[must_use]
    pub fn to_openai_input(&self) -> openai::OpenAiDecisionsInput {
        match self {
            Self::State(Value::String(text)) | Self::Text(text) => {
                openai::OpenAiDecisionsInput::Text(text.clone())
            }
            Self::State(state) => openai::OpenAiDecisionsInput::Text(state.to_string()),
            Self::Messages(messages) => openai::OpenAiDecisionsInput::Messages(messages.clone()),
        }
    }

    #[must_use]
    pub fn image_count(&self) -> usize {
        match self {
            Self::Messages(messages) => messages
                .iter()
                .map(|message| match &message.content {
                    DecisionsMessageContent::Text(_) => 0,
                    DecisionsMessageContent::Parts(parts) => parts
                        .iter()
                        .filter(|part| matches!(part, DecisionsInputPart::InputImage { .. }))
                        .count(),
                })
                .sum(),
            Self::State(_) | Self::Text(_) => 0,
        }
    }
}

/// One OpenAI Decisions input message. Only `role: "user"` is accepted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionsInputMessage {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub role: String,
    pub content: DecisionsMessageContent,
}

impl DecisionsInputMessage {
    fn texts(&self) -> Vec<String> {
        match &self.content {
            DecisionsMessageContent::Text(text) => vec![text.clone()],
            DecisionsMessageContent::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    DecisionsInputPart::InputText { text } => Some(text.clone()),
                    DecisionsInputPart::InputImage { .. } => None,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DecisionsMessageContent {
    Text(String),
    Parts(Vec<DecisionsInputPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionsInputPart {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

/// One typed question. `instructions` stays a free-form value so System One
/// string, object, and array shapes round-trip unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionQuestion {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub instructions: Value,
    pub kind: DecisionQuestionKind,
}

impl DecisionQuestion {
    fn validation_error(&self) -> Option<String> {
        let key = &self.key;
        match &self.kind {
            DecisionQuestionKind::Predicate { .. } => None,
            DecisionQuestionKind::Choice { options } => {
                if options.is_empty() {
                    return Some(format!(
                        "decisions question `{key}` choice criteria cannot be empty"
                    ));
                }
                if options.len() > DECISIONS_CHOICE_MAX_OPTIONS {
                    return Some(format!(
                        "decisions question `{key}` has {} choice options, maximum is {DECISIONS_CHOICE_MAX_OPTIONS}",
                        options.len()
                    ));
                }
                let mut seen = Vec::with_capacity(options.len());
                for option in options {
                    if seen.contains(&&option.value) {
                        return Some(format!(
                            "decisions question `{key}` repeats choice {}",
                            option.value.to_value()
                        ));
                    }
                    seen.push(&option.value);
                }
                None
            }
            DecisionQuestionKind::Score { levels } => {
                if levels.len() < DECISIONS_SCORE_MIN_LEVELS
                    || levels.len() > DECISIONS_SCORE_MAX_LEVELS
                {
                    return Some(format!(
                        "decisions question `{key}` has {} score levels, expected {DECISIONS_SCORE_MIN_LEVELS}..={DECISIONS_SCORE_MAX_LEVELS}",
                        levels.len()
                    ));
                }
                None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionQuestionKind {
    /// Probability that a statement is true. System One calls this `noul`.
    Predicate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        options: Vec<DecisionChoiceOption>,
    },
    Score {
        levels: Vec<DecisionScoreLevel>,
    },
}

/// Optional yes/no rubric descriptions for a `noul` question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct NoulCriteria {
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub yes: Option<Value>,
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub no: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionChoiceOption {
    pub value: DecisionChoiceValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
}

/// Choice values are typed: the string `"true"` and the boolean `true` are
/// distinct options.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DecisionChoiceValue {
    Bool(bool),
    Text(String),
}

impl DecisionChoiceValue {
    /// System One criteria are keyed by string, so booleans become
    /// `"true"`/`"false"`.
    #[must_use]
    pub fn system_one_key(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Text(value) => value.clone(),
        }
    }

    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Bool(value) => Value::Bool(*value),
            Self::Text(value) => Value::String(value.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionScoreLevel {
    pub label: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
}

/// Canonical Decisions response: answers in request question order.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionsResponse {
    pub model: String,
    pub answers: Vec<DecisionOutcome>,
    pub usage: Option<DecisionsUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionOutcome {
    pub key: String,
    pub answer: DecisionAnswer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecisionAnswer {
    Predicate {
        probability: f64,
    },
    Choice {
        choice: DecisionChoiceValue,
        confidence: Option<f64>,
        probabilities: Vec<(DecisionChoiceValue, f64)>,
    },
    Score {
        score: f64,
        confidence: Option<f64>,
        /// Probability per zero-based level index.
        probabilities: Vec<(usize, f64)>,
    },
    /// The model declined to answer this question.
    Refusal,
}

/// Token usage. Cached and cache-write tokens are included in
/// `input_tokens`; reasoning tokens are included in `output_tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecisionsUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: Option<i64>,
}

impl DecisionsUsage {
    #[must_use]
    pub fn total(&self) -> i64 {
        self.total_tokens
            .unwrap_or_else(|| self.input_tokens.saturating_add(self.output_tokens))
    }
}

/// Free-form System One text (instructions, descriptions, labels) as a plain
/// string for formats that only accept strings.
pub(crate) fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn check_probability(key: &str, label: &str, probability: f64) -> Result<(), String> {
    if (0.0..=1.0).contains(&probability) {
        Ok(())
    } else {
        Err(format!(
            "decisions answer `{key}` has {label} {probability}, expected 0..=1"
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn question(key: &str, kind: DecisionQuestionKind) -> DecisionQuestion {
        DecisionQuestion {
            key: key.to_string(),
            name: Some(key.to_string()),
            instructions: json!("instructions"),
            kind,
        }
    }

    fn request(questions: Vec<DecisionQuestion>) -> DecisionsRequest {
        DecisionsRequest {
            model: "jev".to_string(),
            input: DecisionsInput::State(json!("state")),
            questions,
            safety_identifier: None,
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn decisions_request_requires_decisions_capability() {
        let request = request(vec![question(
            "is_urgent",
            DecisionQuestionKind::Predicate { criteria: None },
        )]);

        let requirements = request.requirements();
        assert!(requirements.decisions);
        assert!(!requirements.chat_completions);
        assert!(!requirements.responses);
        assert!(!requirements.stream);
        assert!(!requirements.embeddings);
        assert_eq!(requirements.required_capability_names(), vec!["decisions"]);
        assert_eq!(request.validation_error(), None);
    }

    #[test]
    fn decisions_request_rejects_empty_questions_and_bad_shapes() {
        assert!(request(Vec::new()).validation_error().is_some());

        let empty_choice = request(vec![question(
            "department",
            DecisionQuestionKind::Choice {
                options: Vec::new(),
            },
        )]);
        assert!(empty_choice.validation_error().is_some());

        let short_score = request(vec![question(
            "frustration",
            DecisionQuestionKind::Score {
                levels: vec![DecisionScoreLevel {
                    label: json!("Calm"),
                    description: None,
                }],
            },
        )]);
        assert!(short_score.validation_error().is_some());

        let oversized_choice = request(vec![question(
            "department",
            DecisionQuestionKind::Choice {
                options: (0..=DECISIONS_CHOICE_MAX_OPTIONS)
                    .map(|index| DecisionChoiceOption {
                        value: DecisionChoiceValue::Text(format!("option-{index}")),
                        description: None,
                    })
                    .collect(),
            },
        )]);
        assert!(oversized_choice.validation_error().is_some());

        let duplicate_keys = request(vec![
            question("a", DecisionQuestionKind::Predicate { criteria: None }),
            question("a", DecisionQuestionKind::Predicate { criteria: None }),
        ]);
        assert!(duplicate_keys.validation_error().is_some());
    }

    #[test]
    fn typed_choice_values_are_distinct() {
        let typed = request(vec![question(
            "flag",
            DecisionQuestionKind::Choice {
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
            },
        )]);
        assert_eq!(typed.validation_error(), None);

        let repeated = request(vec![question(
            "flag",
            DecisionQuestionKind::Choice {
                options: vec![
                    DecisionChoiceOption {
                        value: DecisionChoiceValue::Bool(true),
                        description: None,
                    },
                    DecisionChoiceOption {
                        value: DecisionChoiceValue::Bool(true),
                        description: None,
                    },
                ],
            },
        )]);
        assert!(repeated.validation_error().is_some());
    }

    #[test]
    fn input_converts_between_state_and_openai_input() {
        let messages = DecisionsInput::Messages(vec![DecisionsInputMessage {
            kind: Some("message".to_string()),
            role: "user".to_string(),
            content: DecisionsMessageContent::Parts(vec![
                DecisionsInputPart::InputText {
                    text: "first".to_string(),
                },
                DecisionsInputPart::InputText {
                    text: "second".to_string(),
                },
            ]),
        }]);
        assert_eq!(messages.to_state(), Ok(json!("first\n\nsecond")));

        let image = DecisionsInput::Messages(vec![DecisionsInputMessage {
            kind: None,
            role: "user".to_string(),
            content: DecisionsMessageContent::Parts(vec![DecisionsInputPart::InputImage {
                image_url: "https://example.com/a.png".to_string(),
                detail: None,
            }]),
        }]);
        assert_eq!(image.image_count(), 1);
        assert!(image.to_state().is_err());

        assert_eq!(
            DecisionsInput::State(json!("plain")).to_openai_input(),
            openai::OpenAiDecisionsInput::Text("plain".to_string())
        );
        assert_eq!(
            DecisionsInput::State(json!({"ticket": 1})).to_openai_input(),
            openai::OpenAiDecisionsInput::Text("{\"ticket\":1}".to_string())
        );
    }
}
