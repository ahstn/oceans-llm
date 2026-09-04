use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::GatewayError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiErrorEnvelope {
    pub error: OpenAiErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: Option<String>,
    pub param: Option<String>,
}

impl OpenAiErrorEnvelope {
    #[must_use]
    pub fn from_gateway_error(error: &GatewayError) -> Self {
        Self {
            error: OpenAiErrorBody {
                message: error.to_string(),
                error_type: error.error_type().to_string(),
                code: Some(error.error_code().to_string()),
                param: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsListResponse {
    pub object: String,
    pub data: Vec<ModelCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCard {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionsRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

// The core and OpenAI message contracts are identical, including extension fields.
pub use super::core::ChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Value,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    #[serde(default)]
    pub output: Vec<ResponseOutputItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponseUsage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseOutputItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsesStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::ChatMessage;
    use crate::error::GatewayError;
    use serde_json::{Value, json};

    use super::OpenAiErrorEnvelope;

    #[test]
    fn assistant_tool_history_accepts_omitted_content_and_preserves_extensions() {
        for tool in [
            json!({"tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{}"}}]}),
            json!({"function_call":{"name":"lookup","arguments":"{}"}}),
        ] {
            let mut message = tool;
            message["role"] = json!("assistant");
            message["reasoning_details"] = json!([{"type":"reasoning.encrypted","data":"opaque"}]);
            let decoded: ChatMessage =
                serde_json::from_value(message.clone()).expect("valid tool history");
            assert_eq!(decoded.content, Value::Null);
            let wire = serde_json::to_value(decoded).unwrap();
            assert!(wire.get("name").is_none());
            assert_eq!(wire["reasoning_details"], message["reasoning_details"]);
            for key in ["tool_calls", "function_call"] {
                assert_eq!(wire.get(key), message.get(key));
            }
            let core: crate::CoreChatMessage =
                serde_json::from_value(message).expect("core accepts the same contract");
            assert_eq!(core.content, Value::Null);
        }
    }

    #[test]
    fn missing_content_requires_an_assistant_tool_call() {
        for message in [
            json!({"role":"user"}),
            json!({"role":"system"}),
            json!({"role":"tool","tool_call_id":"call_1"}),
            json!({"role":"assistant"}),
            json!({"role":"assistant","tool_calls":[]}),
        ] {
            assert!(serde_json::from_value::<ChatMessage>(message).is_err());
        }
    }

    #[test]
    fn chat_content_forms_names_and_unknown_fields_round_trip() {
        for content in [
            Value::Null,
            json!("hello"),
            json!([{"type":"text","text":"hello"}]),
        ] {
            let message = json!({"role":"assistant","content":content,"name":"helper","custom":{"flag":true}});
            let decoded: ChatMessage = serde_json::from_value(message.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), message);
        }
        let unnamed: ChatMessage =
            serde_json::from_value(json!({"role":"user","content":"hello"})).unwrap();
        assert!(serde_json::to_value(unnamed).unwrap().get("name").is_none());
    }

    #[test]
    fn serializes_openai_error_envelope() {
        let envelope = OpenAiErrorEnvelope::from_gateway_error(&GatewayError::NotImplemented(
            "chat completions execution is deferred".to_string(),
        ));

        let serialized = serde_json::to_value(envelope).expect("must serialize");
        assert_eq!(serialized["error"]["type"], "not_implemented_error");
        assert_eq!(serialized["error"]["code"], "not_implemented");
    }
}
