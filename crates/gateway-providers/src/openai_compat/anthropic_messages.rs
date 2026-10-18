//! Rewrites Anthropic Messages request shapes into Chat Completions shapes.
//!
//! `/v1/messages` requests reach the core chat boundary with their Anthropic
//! tool definitions and content blocks intact. OpenAI-compatible upstreams need
//! those rewritten before `/chat/completions`: tool definitions and
//! `tool_choice`, plus `tool_use`, `tool_result`, `thinking`, and `image`
//! blocks in the message history. OpenAI-shaped requests pass through
//! unchanged because none of the rewritten shapes exist in Chat Completions.

use serde_json::{Map, Value, json};

pub(super) fn normalize_anthropic_messages_for_openai(body: &mut Map<String, Value>) {
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            normalize_tool(tool);
        }
    }

    if let Some(tool_choice) = body.get_mut("tool_choice")
        && let Some(parallel_tool_calls) = normalize_tool_choice(tool_choice)
    {
        body.entry("parallel_tool_calls")
            .or_insert(Value::Bool(parallel_tool_calls));
    }

    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        *messages = std::mem::take(messages)
            .into_iter()
            .flat_map(normalize_message)
            .collect();
    }
}

fn normalize_tool(tool: &mut Value) {
    let Some(object) = tool.as_object_mut() else {
        return;
    };
    if object.contains_key("type") || !object.contains_key("input_schema") {
        return;
    }
    let Some(name) = object.get("name").cloned() else {
        return;
    };
    let mut function = Map::from_iter([
        ("name".to_string(), name),
        (
            "parameters".to_string(),
            object
                .get("input_schema")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object","properties":{}})),
        ),
    ]);
    if let Some(description) = object.get("description").cloned() {
        function.insert("description".to_string(), description);
    }
    *tool = json!({
        "type": "function",
        "function": Value::Object(function)
    });
}

/// Rewrites an Anthropic `tool_choice` object and returns the
/// `parallel_tool_calls` value implied by `disable_parallel_tool_use`.
fn normalize_tool_choice(tool_choice: &mut Value) -> Option<bool> {
    let object = tool_choice.as_object()?;
    let parallel_tool_calls = object
        .get("disable_parallel_tool_use")
        .and_then(Value::as_bool)
        .map(|disabled| !disabled);
    match object.get("type").and_then(Value::as_str) {
        Some("auto") => *tool_choice = Value::String("auto".to_string()),
        Some("any") => *tool_choice = Value::String("required".to_string()),
        Some("none") => *tool_choice = Value::String("none".to_string()),
        Some("tool") => {
            if let Some(name) = object.get("name").and_then(Value::as_str) {
                *tool_choice = json!({"type":"function","function":{"name":name}});
            }
        }
        _ => return None,
    }
    parallel_tool_calls
}

fn normalize_message(message: Value) -> Vec<Value> {
    let Value::Object(mut object) = message else {
        return vec![message];
    };
    let needs_rewrite = object
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|blocks| blocks.iter().any(is_anthropic_only_block));
    if !needs_rewrite {
        return vec![Value::Object(object)];
    }
    let Some(Value::Array(blocks)) = object.remove("content") else {
        return vec![Value::Object(object)];
    };

    match object.get("role").and_then(Value::as_str) {
        Some("assistant") => vec![assistant_message(object, blocks)],
        Some("user") => user_messages(object, blocks),
        _ => {
            object.insert("content".to_string(), Value::Array(blocks));
            vec![Value::Object(object)]
        }
    }
}

fn is_anthropic_only_block(block: &Value) -> bool {
    matches!(
        block_type(block),
        Some("tool_use" | "tool_result" | "thinking" | "redacted_thinking" | "image")
    )
}

fn block_type(block: &Value) -> Option<&str> {
    block.get("type").and_then(Value::as_str)
}

/// Assistant `tool_use` blocks become `tool_calls`. Thinking blocks are
/// dropped because Chat Completions has no request field that carries them
/// back, and their signatures are only meaningful to Anthropic upstreams.
fn assistant_message(mut object: Map<String, Value>, blocks: Vec<Value>) -> Value {
    let mut tool_calls = match object.remove("tool_calls") {
        Some(Value::Array(existing)) => existing,
        _ => Vec::new(),
    };
    let mut content = Vec::new();
    for block in blocks {
        match block_type(&block) {
            Some("tool_use") => tool_calls.push(tool_call(&block)),
            Some("thinking" | "redacted_thinking") => {}
            _ => content.push(block),
        }
    }

    object.insert("content".to_string(), assistant_content(content));
    if !tool_calls.is_empty() {
        object.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    Value::Object(object)
}

fn tool_call(block: &Value) -> Value {
    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
    json!({
        "id": block.get("id").cloned().unwrap_or(Value::Null),
        "type": "function",
        "function": {
            "name": block.get("name").cloned().unwrap_or(Value::Null),
            "arguments": input.to_string(),
        }
    })
}

/// Collapses text-only assistant content to a string, which every
/// OpenAI-compatible server accepts, and uses `null` when only tool calls
/// remain.
fn assistant_content(blocks: Vec<Value>) -> Value {
    if blocks.is_empty() {
        return Value::Null;
    }
    let texts = blocks
        .iter()
        .map(|block| {
            (block_type(block) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Option<Vec<_>>>();
    match texts {
        Some(texts) => Value::String(texts.concat()),
        None => Value::Array(blocks),
    }
}

/// Each `tool_result` block becomes a `tool` message ahead of the user's
/// remaining content, which keeps every tool response adjacent to the
/// assistant message that requested it. Images returned by a tool cannot live
/// in a `tool` message, so they move into the trailing user message.
fn user_messages(object: Map<String, Value>, blocks: Vec<Value>) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut remaining = Vec::new();
    for block in blocks {
        match block_type(&block) {
            Some("tool_result") => {
                let (message, images) = tool_message(&block);
                messages.push(message);
                remaining.extend(images);
            }
            Some("image") => remaining.push(image_part(block)),
            _ => remaining.push(block),
        }
    }

    if !remaining.is_empty() {
        let mut user = object;
        user.insert("content".to_string(), Value::Array(remaining));
        messages.push(Value::Object(user));
    }
    messages
}

fn tool_message(block: &Value) -> (Value, Vec<Value>) {
    let mut images = Vec::new();
    let mut text = match block.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let mut texts = Vec::new();
            for part in parts {
                match block_type(part) {
                    Some("text") => texts.push(
                        part.get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    Some("image") => images.push(image_part(part.clone())),
                    _ => texts.push(part.to_string()),
                }
            }
            texts.join("\n")
        }
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };
    if block.get("is_error").and_then(Value::as_bool) == Some(true) {
        text = if text.is_empty() {
            "Error".to_string()
        } else {
            format!("Error: {text}")
        };
    }

    let message = json!({
        "role": "tool",
        "tool_call_id": block.get("tool_use_id").cloned().unwrap_or(Value::Null),
        "content": text,
    });
    (message, images)
}

/// Converts an Anthropic `image` block with a base64 or URL source into an
/// OpenAI `image_url` part. Other blocks are returned unchanged.
fn image_part(block: Value) -> Value {
    let Some(source) = block.get("source").and_then(Value::as_object) else {
        return block;
    };
    let url = match source.get("type").and_then(Value::as_str) {
        Some("base64") => {
            let (Some(media_type), Some(data)) = (
                source.get("media_type").and_then(Value::as_str),
                source.get("data").and_then(Value::as_str),
            ) else {
                return block;
            };
            format!("data:{media_type};base64,{data}")
        }
        Some("url") => match source.get("url").and_then(Value::as_str) {
            Some(url) => url.to_string(),
            None => return block,
        },
        _ => return block,
    };
    json!({"type": "image_url", "image_url": {"url": url}})
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::normalize_anthropic_messages_for_openai;

    fn normalize(body: Value) -> Value {
        let Value::Object(mut object) = body else {
            panic!("body must be an object");
        };
        normalize_anthropic_messages_for_openai(&mut object);
        Value::Object(object)
    }

    fn messages(messages: Value) -> Value {
        normalize(json!({ "messages": messages }))["messages"].clone()
    }

    #[test]
    fn translates_parallel_tool_use_and_tool_result_turns() {
        let translated = messages(json!([
            {"role": "user", "content": "weather in Paris and Rome?"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "two cities", "signature": "sig"},
                {"type": "text", "text": "Checking both."},
                {"type": "tool_use", "id": "toolu_1", "name": "weather", "input": {"city": "Paris"}},
                {"type": "tool_use", "id": "toolu_2", "name": "weather", "input": {"city": "Rome"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "18C"},
                {"type": "tool_result", "tool_use_id": "toolu_2", "content": "24C"}
            ]}
        ]));

        assert_eq!(
            translated,
            json!([
                {"role": "user", "content": "weather in Paris and Rome?"},
                {"role": "assistant", "content": "Checking both.", "tool_calls": [
                    {"id": "toolu_1", "type": "function", "function": {
                        "name": "weather", "arguments": "{\"city\":\"Paris\"}"
                    }},
                    {"id": "toolu_2", "type": "function", "function": {
                        "name": "weather", "arguments": "{\"city\":\"Rome\"}"
                    }}
                ]},
                {"role": "tool", "tool_call_id": "toolu_1", "content": "18C"},
                {"role": "tool", "tool_call_id": "toolu_2", "content": "24C"}
            ])
        );
    }

    #[test]
    fn tool_only_assistant_turn_uses_null_content_and_empty_input() {
        let translated = messages(json!([
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "now", "input": {}}
            ]}
        ]));

        assert_eq!(translated[0]["content"], Value::Null);
        assert_eq!(
            translated[0]["tool_calls"][0]["function"]["arguments"],
            "{}"
        );
    }

    #[test]
    fn places_tool_results_before_remaining_user_text() {
        let translated = messages(json!([
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "42"},
                {"type": "text", "text": "Now summarize."}
            ]}
        ]));

        assert_eq!(
            translated,
            json!([
                {"role": "tool", "tool_call_id": "toolu_1", "content": "42"},
                {"role": "user", "content": [{"type": "text", "text": "Now summarize."}]}
            ])
        );
    }

    #[test]
    fn flattens_block_array_tool_results_and_moves_images_to_user_turn() {
        let translated = messages(json!([
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": [
                    {"type": "text", "text": "line one"},
                    {"type": "text", "text": "line two"},
                    {"type": "image", "source": {
                        "type": "base64", "media_type": "image/png", "data": "iVBOR"
                    }}
                ]}
            ]}
        ]));

        assert_eq!(
            translated,
            json!([
                {"role": "tool", "tool_call_id": "toolu_1", "content": "line one\nline two"},
                {"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBOR"}}
                ]}
            ])
        );
    }

    #[test]
    fn marks_error_tool_results() {
        let translated = messages(json!([
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "is_error": true, "content": "not found"},
                {"type": "tool_result", "tool_use_id": "toolu_2", "is_error": true}
            ]}
        ]));

        assert_eq!(translated[0]["content"], "Error: not found");
        assert_eq!(translated[1]["content"], "Error");
    }

    #[test]
    fn converts_user_image_blocks_to_image_url_parts() {
        let translated = messages(json!([
            {"role": "user", "content": [
                {"type": "text", "text": "what is this?"},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/a.png"}}
            ]}
        ]));

        assert_eq!(
            translated[0]["content"][1],
            json!({"type": "image_url", "image_url": {"url": "https://example.com/a.png"}})
        );
    }

    #[test]
    fn leaves_openai_shaped_messages_unchanged() {
        let original = json!([
            {"role": "user", "content": [{"type": "text", "text": "hi"}]},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        ]);

        assert_eq!(messages(original.clone()), original);
    }

    #[test]
    fn translates_tool_choice_variants() {
        let cases = [
            (json!({"type": "auto"}), json!("auto"), None),
            (json!({"type": "any"}), json!("required"), None),
            (json!({"type": "none"}), json!("none"), None),
            (
                json!({"type": "tool", "name": "lookup", "disable_parallel_tool_use": true}),
                json!({"type": "function", "function": {"name": "lookup"}}),
                Some(json!(false)),
            ),
        ];
        for (input, expected, parallel_tool_calls) in cases {
            let body = normalize(json!({ "tool_choice": input }));
            assert_eq!(body["tool_choice"], expected);
            assert_eq!(
                body.get("parallel_tool_calls").cloned(),
                parallel_tool_calls
            );
        }

        let openai_choice = normalize(json!({ "tool_choice": "required" }));
        assert_eq!(
            openai_choice,
            Value::Object(Map::from_iter([(
                "tool_choice".to_string(),
                json!("required")
            )]))
        );
    }
}
