use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestRequirements {
    pub chat_completions: bool,
    pub responses: bool,
    pub stream: bool,
    pub embeddings: bool,
    pub images: bool,
    pub decisions: bool,
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
        if self.images {
            names.push("images");
        }
        if self.decisions {
            names.push("decisions");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentPartType {
    ImageUrl,
    InputImage,
    VideoUrl,
    InputVideo,
    File,
    InputFile,
    Document,
}

impl ContentPartType {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "image_url" => Some(Self::ImageUrl),
            "input_image" => Some(Self::InputImage),
            "video_url" => Some(Self::VideoUrl),
            "input_video" => Some(Self::InputVideo),
            "file" => Some(Self::File),
            "input_file" => Some(Self::InputFile),
            "document" => Some(Self::Document),
            _ => None,
        }
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
            images: false,
            decisions: false,
            // Tool history needs a tool-capable route even when the caller omits `tools`,
            // because Chat Completions adapters replay it as `tool_calls` and `tool` messages.
            tools: self
                .extra
                .get("tools")
                .is_some_and(value_is_present_for_capability)
                || self.messages.iter().any(message_has_tool_history),
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
#[serde(try_from = "ChatMessageFields")]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct ChatMessageFields {
    role: String,
    // Preserve the difference between an absent field and an explicit JSON null.
    #[serde(default, deserialize_with = "deserialize_present_content")]
    content: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn deserialize_present_content<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Debug, thiserror::Error)]
#[error("message with role `{role}` requires `content` unless it is an assistant tool call")]
struct MissingChatContent {
    role: String,
}

impl TryFrom<ChatMessageFields> for ChatMessage {
    type Error = MissingChatContent;

    fn try_from(fields: ChatMessageFields) -> Result<Self, Self::Error> {
        let content = match fields.content {
            Some(content) => content,
            None if fields.role == "assistant"
                && (fields
                    .extra
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty())
                    || fields
                        .extra
                        .get("function_call")
                        .is_some_and(Value::is_object)) =>
            {
                Value::Null
            }
            None => return Err(MissingChatContent { role: fields.role }),
        };
        Ok(Self {
            role: fields.role,
            content,
            name: fields.name,
            extra: fields.extra,
        })
    }
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
            images: false,
            decisions: false,
            tools: false,
            vision: false,
            json_schema: false,
            developer_role: false,
        }
    }
}

/// An Images API generation request (`POST /v1/images`). Only `model` and
/// `prompt` are typed: the gateway routes on `model`, guardrails inspect
/// `prompt`, and every other field (`input_references`, `resolution`,
/// `aspect_ratio`, `quality`, `n`, ...) passes through unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImagesRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ImagesRequest {
    #[must_use]
    pub const fn requirements(&self) -> RequestRequirements {
        RequestRequirements {
            chat_completions: false,
            responses: false,
            stream: self.stream,
            embeddings: false,
            images: true,
            decisions: false,
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
            images: false,
            decisions: false,
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

    object
        .get("type")
        .and_then(Value::as_str)
        .and_then(ContentPartType::parse)
        .is_some_and(|content_type| {
            matches!(
                content_type,
                ContentPartType::InputImage
                    | ContentPartType::InputVideo
                    | ContentPartType::InputFile
            )
        })
        || object
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
    let kind = object.get("type").and_then(Value::as_str);

    // Anthropic Messages sends images as `image` blocks, including inside tool results.
    match kind {
        Some("image") => return true,
        Some("tool_result") => {
            return object.get("content").is_some_and(message_has_vision_input);
        }
        _ => {}
    }

    // `vision` is the gateway's capability gate for non-text inputs. Keep the Chat file
    // aliases accepted by provider adapters aligned with Responses `input_file` routing.
    kind.and_then(ContentPartType::parse).is_some() || object.contains_key("image_url")
}

/// Detects OpenAI (`tool` role, `tool_calls`) and Anthropic (`tool_use`, `tool_result`)
/// tool history.
fn message_has_tool_history(message: &ChatMessage) -> bool {
    message.role.eq_ignore_ascii_case("tool")
        || message
            .extra
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty())
        || message.content.as_array().is_some_and(|blocks| {
            blocks.iter().any(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("tool_use" | "tool_result")
                )
            })
        })
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
    fn anthropic_image_blocks_require_vision_capability() {
        let message = |content: Value| ChatRequest {
            model: "claude".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content,
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra: BTreeMap::new(),
        };
        let image = json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBOR"}});

        assert!(message(json!([image.clone()])).requirements().vision);
        assert!(
            message(json!([{"type":"tool_result","tool_use_id":"toolu_1","content":[image]}]))
                .requirements()
                .vision
        );
        assert!(
            !message(json!([{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]))
                .requirements()
                .vision
        );
    }

    #[test]
    fn tool_history_requires_tools_capability_without_tool_definitions() {
        let chat = |role: &str, content: Value, extra: BTreeMap<String, Value>| ChatRequest {
            model: "fast".to_string(),
            messages: vec![ChatMessage {
                role: role.to_string(),
                content,
                name: None,
                extra,
            }],
            stream: false,
            extra: BTreeMap::from([("tools".to_string(), json!([]))]),
        };

        let anthropic_use = chat(
            "assistant",
            json!([{"type":"tool_use","id":"toolu_1","name":"f","input":{}}]),
            BTreeMap::new(),
        );
        let anthropic_result = chat(
            "user",
            json!([{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]),
            BTreeMap::new(),
        );
        let openai_calls = chat(
            "assistant",
            Value::Null,
            BTreeMap::from([(
                "tool_calls".to_string(),
                json!([{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]),
            )]),
        );
        let openai_result = chat("tool", json!("ok"), BTreeMap::new());
        let plain = chat("user", json!("hello"), BTreeMap::new());

        for request in [anthropic_use, anthropic_result, openai_calls, openai_result] {
            assert!(request.requirements().tools, "{:?}", request.messages[0]);
        }
        assert!(!plain.requirements().tools);
    }

    #[test]
    fn chat_file_content_parts_require_vision_capability() {
        for content_type in ["file", "input_file", "document"] {
            let request = ChatRequest {
                model: "document-reader".to_string(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: json!([{
                        "type": content_type,
                        "file": {
                            "file_data": "data:application/pdf;base64,cGRm",
                            "filename": "document.pdf"
                        }
                    }]),
                    name: None,
                    extra: BTreeMap::new(),
                }],
                stream: false,
                extra: BTreeMap::new(),
            };

            assert!(
                request.requirements().vision,
                "{content_type} must require a vision-capable route"
            );
        }
    }

    #[test]
    fn chat_video_content_parts_require_vision_capability() {
        for content_type in ["video_url", "input_video"] {
            let request = ChatRequest {
                model: "video-reader".to_string(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: json!([{"type": content_type}]),
                    name: None,
                    extra: BTreeMap::new(),
                }],
                stream: false,
                extra: BTreeMap::new(),
            };

            assert!(
                request.requirements().vision,
                "{content_type} must require a vision-capable route"
            );
        }
    }

    #[test]
    fn responses_input_video_requires_vision_capability() {
        let request = super::ResponsesRequest {
            model: "video-reader".to_string(),
            input: json!([{"type":"input_video","video_url":"https://example.test/video.mp4"}]),
            stream: false,
            instructions: None,
            tools: None,
            tool_choice: None,
            reasoning: None,
            text: None,
            extra: BTreeMap::new(),
        };

        assert!(request.requirements().vision);
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
