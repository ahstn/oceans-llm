use std::{collections::BTreeMap, fmt, str::FromStr, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::selectors::McpCall;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    #[default]
    Audit,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardPhase {
    Prompt,
    ModelResponse,
    GeneratedToolCall,
    McpCall,
    McpResult,
    HarnessPreTool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureDisposition {
    #[default]
    FailOpen,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction {
    Allow,
    Audit,
    Deny,
    Transformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedService {
    AmazonBedrock,
    GoogleModelArmor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReasonCode(String);

impl ReasonCode {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidReasonCode> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 96
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
        {
            return Err(InvalidReasonCode(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ReasonCode {
    type Err = InvalidReasonCode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid guardrail reason code `{0}`")]
pub struct InvalidReasonCode(String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvaluationPayload {
    Text {
        text: String,
    },
    TextSegments {
        segments: Vec<String>,
    },
    ShellCommand {
        command: String,
    },
    ToolCall {
        name: String,
        arguments: Value,
    },
    McpCall {
        call: McpCall,
    },
    McpResult {
        server: String,
        tool: String,
        result: Value,
    },
}

impl EvaluationPayload {
    pub fn content_hash(&self) -> String {
        let encoded =
            serde_json::to_vec(self).expect("guardrail payload serialization cannot fail");
        format!("sha256:{:x}", Sha256::digest(encoded))
    }

    pub fn byte_len(&self) -> usize {
        match self {
            Self::Text { text } => text.len(),
            Self::TextSegments { segments } => segments.iter().map(String::len).sum(),
            Self::ShellCommand { command } => command.len(),
            Self::ToolCall { name, arguments } => name.len() + arguments.to_string().len(),
            Self::McpCall { call } => {
                call.server.len() + call.tool.len() + call.arguments.to_string().len()
            }
            Self::McpResult {
                server,
                tool,
                result,
            } => server.len() + tool.len() + result.to_string().len(),
        }
    }

    pub fn text_content(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::ShellCommand { command } => Some(command),
            _ => None,
        }
    }

    pub fn inspection_text(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::TextSegments { segments } => {
                serde_json::to_string(segments).expect("text segments serialize as JSON")
            }
            Self::ShellCommand { command } => command.clone(),
            Self::ToolCall { arguments, .. } => arguments.to_string(),
            Self::McpCall { call } => call.arguments.to_string(),
            Self::McpResult { result, .. } => result.to_string(),
        }
    }

    pub fn replace_text(&mut self, replacement: String) -> bool {
        match self {
            Self::Text { text } => {
                *text = replacement;
                true
            }
            Self::TextSegments { segments } => serde_json::from_str::<Vec<String>>(&replacement)
                .map(|values| {
                    if values.len() != segments.len() {
                        return false;
                    }
                    *segments = values;
                    true
                })
                .unwrap_or(false),
            Self::ShellCommand { command } => {
                *command = replacement;
                true
            }
            Self::ToolCall { arguments, .. } => serde_json::from_str(&replacement)
                .map(|value| {
                    if !same_json_shape(arguments, &value) {
                        return false;
                    }
                    *arguments = value;
                    true
                })
                .unwrap_or(false),
            Self::McpCall { call } => serde_json::from_str(&replacement)
                .map(|value| {
                    if !same_json_shape(&call.arguments, &value) {
                        return false;
                    }
                    call.arguments = value;
                    true
                })
                .unwrap_or(false),
            Self::McpResult { result, .. } => serde_json::from_str(&replacement)
                .map(|value| {
                    if !same_json_shape(result, &value) {
                        return false;
                    }
                    *result = value;
                    true
                })
                .unwrap_or(false),
        }
    }
}

fn same_json_shape(original: &Value, replacement: &Value) -> bool {
    match (original, replacement) {
        (Value::Object(original), Value::Object(replacement)) => {
            original.len() == replacement.len()
                && original.iter().all(|(key, value)| {
                    replacement
                        .get(key)
                        .is_some_and(|replacement| same_json_shape(value, replacement))
                })
        }
        (Value::Array(original), Value::Array(replacement)) => {
            original.len() == replacement.len()
                && original
                    .iter()
                    .zip(replacement)
                    .all(|(original, replacement)| same_json_shape(original, replacement))
        }
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => true,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationInput {
    pub phase: GuardPhase,
    pub payload: EvaluationPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub associated_prompt: Option<String>,
}

impl EvaluationInput {
    pub fn new(phase: GuardPhase, payload: EvaluationPayload) -> Self {
        Self {
            phase,
            payload,
            associated_prompt: None,
        }
    }

    pub fn with_associated_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.associated_prompt = Some(prompt.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", content = "key", rename_all = "snake_case")]
pub enum EffectiveScope {
    Global,
    ModelRoute(String),
    McpServer(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedRule {
    pub pack_id: String,
    pub rule_id: String,
    pub matched_field: String,
    pub reason_code: ReasonCode,
    pub description: String,
    pub safer_action: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentTransformation {
    pub content: String,
    pub content_hash: String,
}

impl ContentTransformation {
    pub fn new(content: String) -> Self {
        let content_hash = format!("sha256:{:x}", Sha256::digest(content.as_bytes()));
        Self {
            content,
            content_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecisionId(Uuid);

impl DecisionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for DecisionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DecisionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedDecisionMetadata {
    pub assessment_count: u32,
    pub matched_filters: Vec<String>,
    pub usage_units: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub decision_id: DecisionId,
    pub phase: GuardPhase,
    pub scope: EffectiveScope,
    pub evaluator: String,
    pub managed_service: Option<ManagedService>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_metadata: Option<ManagedDecisionMetadata>,
    pub action: DecisionAction,
    pub reason_code: ReasonCode,
    pub matched_rule: Option<MatchedRule>,
    pub latency_micros: u64,
    pub failure_disposition: Option<FailureDisposition>,
    pub transformed: bool,
    pub content_hash: String,
}

impl DecisionRecord {
    pub(crate) fn latency(duration: Duration) -> u64 {
        duration.as_micros().try_into().unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailEvaluation {
    pub action: DecisionAction,
    pub decisions: Vec<DecisionRecord>,
    pub output: EvaluationPayload,
}

impl GuardrailEvaluation {
    pub fn denied(&self) -> bool {
        self.action == DecisionAction::Deny
    }
}
