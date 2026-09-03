use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<AnthropicMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<Value>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub fn anthropic_message_from_openai_chat(value: &Value, model_key: &str) -> Value {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_oceans");
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let message = choice.and_then(|choice| choice.get("message"));

    let mut content = Vec::new();
    content.extend(
        anthropic_reasoning_blocks(
            message.and_then(|message| message.get("provider_metadata")),
            "anthropic_messages",
        )
        .into_iter()
        .filter(|block| {
            matches!(
                block.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            )
        })
        .cloned(),
    );

    if let Some(text) = message
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        content.push(json!({ "type": "text", "text": text }));
    }

    if let Some(tool_calls) = message
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
    {
        content.extend(tool_calls.iter().filter_map(anthropic_tool_use_from_openai));
    }

    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model_key,
        "content": content,
        "stop_reason": choice
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(Value::as_str)
            .map(openai_finish_reason_to_anthropic)
            .unwrap_or("end_turn"),
        "stop_sequence": null,
        "usage": anthropic_usage_from_openai(value.get("usage"))
    })
}

pub fn anthropic_reasoning_blocks<'a>(
    provider_metadata: Option<&'a Value>,
    source: &str,
) -> Vec<&'a Value> {
    const NAMESPACES: [&str; 5] = [
        "gcp_vertex",
        "aws_bedrock",
        "github_copilot",
        "anthropic_compat",
        "anthropic",
    ];

    let Some(provider_metadata) = provider_metadata else {
        return Vec::new();
    };

    NAMESPACES
        .iter()
        .filter_map(|namespace| provider_metadata.get(namespace))
        .filter_map(|metadata| metadata.get("reasoning"))
        .filter(|reasoning| reasoning.get("source").and_then(Value::as_str) == Some(source))
        .filter_map(|reasoning| reasoning.get("blocks").and_then(Value::as_array))
        .flatten()
        .collect()
}

fn anthropic_tool_use_from_openai(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    if object.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }
    let function = object.get("function")?.as_object()?;
    let name = function.get("name")?.as_str()?;
    let id = object.get("id")?.as_str()?;
    let input = function
        .get("arguments")
        .and_then(Value::as_str)
        .map(parse_openai_tool_arguments_for_anthropic)
        .unwrap_or_else(|| Value::Object(Map::new()));

    Some(json!({
        "type": "tool_use",
        "id": id,
        "name": name,
        "input": input
    }))
}

fn anthropic_usage_from_openai(value: Option<&Value>) -> Value {
    let prompt = value
        .and_then(|usage| usage.get("prompt_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = value
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    json!({
        "input_tokens": prompt,
        "output_tokens": completion
    })
}

fn parse_openai_tool_arguments_for_anthropic(arguments: &str) -> Value {
    serde_json::from_str::<Value>(arguments)
        .unwrap_or_else(|_| json!({"_raw_arguments": arguments}))
}

pub fn openai_finish_reason_to_anthropic(value: &str) -> &'static str {
    match value {
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        "content_filter" => "refusal",
        _ => "end_turn",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::anthropic_message_from_openai_chat;

    #[test]
    fn message_conversion_preserves_native_anthropic_thinking_blocks() {
        for namespace in [
            "gcp_vertex",
            "aws_bedrock",
            "github_copilot",
            "anthropic_compat",
            "anthropic",
        ] {
            let value = json!({
                "id": "chatcmpl_1",
                "choices": [{
                    "finish_reason": "stop",
                    "message": {
                        "role": "assistant",
                        "content": "visible",
                        "provider_metadata": {
                            namespace: {
                                "reasoning": {
                                    "source": "anthropic_messages",
                                    "blocks": [
                                        {"type": "thinking", "thinking": "hidden", "signature": "sig"},
                                        {"type": "redacted_thinking", "data": "encrypted"}
                                    ]
                                }
                            }
                        }
                    }
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 2}
            });

            let converted = anthropic_message_from_openai_chat(&value, "claude");

            assert_eq!(
                converted["content"],
                json!([
                    {"type": "thinking", "thinking": "hidden", "signature": "sig"},
                    {"type": "redacted_thinking", "data": "encrypted"},
                    {"type": "text", "text": "visible"}
                ]),
                "namespace {namespace}"
            );
        }
    }

    #[test]
    fn message_conversion_ignores_non_native_reasoning_blocks() {
        let value = json!({
            "choices": [{
                "message": {
                    "content": "visible",
                    "provider_metadata": {
                        "aws_bedrock": {
                            "reasoning": {
                                "source": "bedrock_converse",
                                "blocks": [{"type": "reasoning_text", "text": "hidden"}]
                            }
                        }
                    }
                }
            }]
        });

        let converted = anthropic_message_from_openai_chat(&value, "claude");

        assert_eq!(
            converted["content"],
            json!([{"type": "text", "text": "visible"}])
        );
    }

    #[test]
    fn message_conversion_preserves_malformed_tool_arguments_as_raw_input() {
        let value = json!({
            "id": "chatcmpl_1",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{\"city\":"
                        }
                    }]
                }
            }]
        });

        let converted = anthropic_message_from_openai_chat(&value, "claude");

        assert_eq!(
            converted["content"][0]["input"],
            json!({"_raw_arguments": "{\"city\":"})
        );
    }
}
