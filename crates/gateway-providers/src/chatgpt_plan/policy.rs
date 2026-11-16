use serde_json::{Value, json};

const UNSUPPORTED_FIELDS: &[&str] = &[
    "background",
    "conversation",
    "max_output_tokens",
    "max_tool_calls",
    "metadata",
    "moderation",
    "multi_agent",
    "prompt",
    "prompt_cache_retention",
    "safety_identifier",
    "temperature",
    "top_logprobs",
    "top_p",
    "truncation",
    "user",
    "previous_response_id",
];

/// Required SIWC wire changes apply even when optional compatibility stripping is off.
pub(super) fn apply_request_policy(body: &mut Value, strip_unsupported_fields: bool) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    object.insert("store".to_string(), Value::Bool(false));
    object.insert("stream".to_string(), Value::Bool(true));
    if let Some(input @ Value::String(_)) = object.get_mut("input") {
        let text = input.take();
        *input = json!([{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": text}]
        }]);
    }
    if let Some(Value::Array(input)) = object.get_mut("input") {
        for item in input {
            if let Some(item) = item.as_object_mut() {
                if item.get("role").and_then(Value::as_str) == Some("system") {
                    item.insert("role".to_string(), Value::String("developer".to_string()));
                }
                if strip_unsupported_fields
                    && item.get("type").and_then(Value::as_str) == Some("additional_tools")
                    && let Some(Value::Array(tools)) = item.get_mut("tools")
                {
                    retain_client_tools(tools, false);
                }
            }
        }
    }
    if !strip_unsupported_fields {
        return;
    }
    for field in UNSUPPORTED_FIELDS {
        object.remove(*field);
    }
    if let Some(Value::Array(tools)) = object.get_mut("tools") {
        retain_client_tools(tools, true);
        if tools.is_empty() {
            object.remove("tools");
            object.remove("tool_choice");
        }
    }
}

fn retain_client_tools(tools: &mut Vec<Value>, top_level: bool) {
    tools.retain(|tool| {
        let kind = tool.get("type").and_then(Value::as_str);
        if top_level && kind == Some("programmatic_tool_calling") {
            return false;
        }
        !matches!(
            kind,
            Some(
                "image_generation"
                    | "file_search"
                    | "code_interpreter"
                    | "computer_use"
                    | "computer_use_preview"
                    | "computer"
                    | "mcp"
                    | "tool_search"
            )
        )
    });
}
