use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestRequirements {
    pub chat_completions: bool,
    pub responses: bool,
    pub stream: bool,
    pub embeddings: bool,
    pub tools: bool,
    pub vision: bool,
    pub json_schema: bool,
    pub developer_role: bool,
}

impl RequestRequirements {
    #[must_use]
    pub fn required_capability_names(self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.chat_completions {
            names.push("chat_completions");
        }
        if self.responses {
            names.push("responses");
        }
        if self.stream {
            names.push("stream");
        }
        if self.embeddings {
            names.push("embeddings");
        }
        if self.tools {
            names.push("tools");
        }
        if self.vision {
            names.push("vision");
        }
        if self.json_schema {
            names.push("json_schema");
        }
        if self.developer_role {
            names.push("developer_role");
        }
        names
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ChatRequest {
    #[must_use]
    pub fn requirements(&self) -> RequestRequirements {
        RequestRequirements {
            chat_completions: true,
            responses: false,
            stream: self.stream,
            embeddings: false,
            tools: self
                .extra
                .get("tools")
                .is_some_and(value_is_present_for_capability),
            vision: self
                .messages
                .iter()
                .any(|message| message_has_vision_input(&message.content)),
            json_schema: self
                .extra
                .get("response_format")
                .is_some_and(response_format_requires_json_schema),
            developer_role: self
                .messages
                .iter()
                .any(|message| message.role.eq_ignore_ascii_case("developer")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl EmbeddingsRequest {
    #[must_use]
    pub const fn requirements(&self) -> RequestRequirements {
        let _ = self;
        RequestRequirements {
            chat_completions: false,
            responses: false,
            stream: false,
            embeddings: true,
            tools: false,
            vision: false,
            json_schema: false,
            developer_role: false,
        }
    }
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

impl ResponsesRequest {
    #[must_use]
    pub fn requirements(&self) -> RequestRequirements {
        RequestRequirements {
            chat_completions: false,
            responses: true,
            stream: self.stream,
            embeddings: false,
            tools: self
                .tools
                .as_ref()
                .is_some_and(value_is_present_for_capability),
            vision: response_input_has_vision(&self.input),
            json_schema: self
                .text
                .as_ref()
                .is_some_and(responses_text_requires_json_schema),
            developer_role: response_input_has_developer_role(&self.input),
        }
    }
}

fn value_is_present_for_capability(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
        _ => true,
    }
}

fn responses_text_requires_json_schema(value: &Value) -> bool {
    let Some(format) = value.as_object().and_then(|object| object.get("format")) else {
        return false;
    };
    response_format_requires_json_schema(format)
}

fn response_input_has_vision(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(response_input_item_has_vision),
        Value::Object(_) => response_input_item_has_vision(value),
        _ => false,
    }
}

fn response_input_item_has_vision(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    matches!(
        object.get("type").and_then(Value::as_str),
        Some("input_image" | "input_file")
    ) || object
        .get("content")
        .is_some_and(response_input_content_has_vision)
}

fn response_input_content_has_vision(value: &Value) -> bool {
    match value {
        Value::Array(parts) => parts.iter().any(response_input_item_has_vision),
        Value::Object(_) => response_input_item_has_vision(value),
        _ => false,
    }
}

fn response_input_has_developer_role(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(response_input_item_has_developer_role),
        Value::Object(_) => response_input_item_has_developer_role(value),
        _ => false,
    }
}

fn response_input_item_has_developer_role(value: &Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get("role"))
        .and_then(Value::as_str)
        .is_some_and(|role| role.eq_ignore_ascii_case("developer"))
}

fn response_format_requires_json_schema(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "json_schema")
}

fn message_has_vision_input(content: &Value) -> bool {
    match content {
        Value::Array(parts) => parts.iter().any(content_part_has_vision_input),
        Value::Object(_) => content_part_has_vision_input(content),
        _ => false,
    }
}

fn content_part_has_vision_input(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    matches!(
        object.get("type").and_then(Value::as_str),
        Some("image_url" | "input_image")
    ) || object.contains_key("image_url")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{ChatMessage, ChatRequest, EmbeddingsRequest};

    #[test]
    fn chat_request_requirements_reflect_request_shape() {
        let mut extra = BTreeMap::new();
        extra.insert("tools".to_string(), json!([{"type":"function"}]));
        extra.insert(
            "response_format".to_string(),
            json!({"type":"json_schema","json_schema": {"name":"answer"}}),
        );

        let request = ChatRequest {
            model: "fast".to_string(),
            messages: vec![
                ChatMessage {
                    role: "developer".to_string(),
                    content: Value::String("be concise".to_string()),
                    name: None,
                    extra: BTreeMap::new(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: json!([
                        {"type":"text","text":"Describe this"},
                        {"type":"image_url","image_url":{"url":"https://example.test/cat.png"}}
                    ]),
                    name: None,
                    extra: BTreeMap::new(),
                },
            ],
            stream: true,
            extra,
        };

        let requirements = request.requirements();
        assert!(requirements.chat_completions);
        assert!(requirements.stream);
        assert!(requirements.tools);
        assert!(requirements.vision);
        assert!(requirements.json_schema);
        assert!(requirements.developer_role);
        assert!(!requirements.embeddings);
        assert!(!requirements.responses);
    }

    #[test]
    fn embeddings_request_requires_embeddings_capability() {
        let request = EmbeddingsRequest {
            model: "embed-fast".to_string(),
            input: json!(["hello"]),
            extra: BTreeMap::new(),
        };

        let requirements = request.requirements();
        assert!(requirements.embeddings);
        assert!(!requirements.chat_completions);
        assert!(!requirements.responses);
        assert!(!requirements.stream);
    }

    #[test]
    fn responses_request_requirements_reflect_request_shape() {
        let request = super::ResponsesRequest {
            model: "reasoning".to_string(),
            input: json!([
                {"type":"message","role":"developer","content":"be concise"},
                {"type":"message","role":"user","content":[
                    {"type":"input_text","text":"Describe this"},
                    {"type":"input_image","image_url":"https://example.test/cat.png"}
                ]}
            ]),
            stream: true,
            instructions: None,
            tools: Some(json!([{"type":"function","name":"lookup"}])),
            tool_choice: None,
            reasoning: Some(json!({"effort":"medium"})),
            text: Some(json!({"format":{"type":"json_schema","name":"answer","schema":{}}})),
            extra: BTreeMap::new(),
        };

        let requirements = request.requirements();
        assert!(requirements.responses);
        assert!(requirements.stream);
        assert!(requirements.tools);
        assert!(requirements.vision);
        assert!(requirements.json_schema);
        assert!(requirements.developer_role);
        assert!(!requirements.chat_completions);
        assert!(!requirements.embeddings);
    }
}
