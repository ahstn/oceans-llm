use std::collections::HashSet;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolCallIdentity {
    Known(String),
    Anonymous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObservedToolCall {
    pub(super) identity: ToolCallIdentity,
    /// Present on the item that opens a call; streamed argument deltas carry no name.
    pub(super) name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ToolCallCounter {
    known_ids: HashSet<String>,
    anonymous: i64,
    names: HashSet<String>,
}

impl ToolCallCounter {
    pub(super) fn observe_value(&mut self, value: &Value) {
        for call in tool_call_identities_from_value(value) {
            if let Some(name) = call.name {
                self.names.insert(name);
            }
            match call.identity {
                ToolCallIdentity::Known(id) => {
                    self.known_ids.insert(id);
                }
                ToolCallIdentity::Anonymous => {
                    self.anonymous = self.anonymous.saturating_add(1);
                }
            }
        }
    }

    pub(super) fn count(&self) -> i64 {
        i64::try_from(self.known_ids.len())
            .unwrap_or(i64::MAX)
            .saturating_add(self.anonymous)
    }

    pub(super) fn distinct_tool_count(&self) -> i64 {
        i64::try_from(self.names.len()).unwrap_or(i64::MAX)
    }
}

pub(super) fn shallow_tool_count_from_request_body(value: &Value) -> Option<i64> {
    let tools = value.get("tools").or_else(|| {
        value
            .get("request")
            .and_then(|request| request.get("tools"))
    });

    match tools {
        None | Some(Value::Null) => Some(0),
        Some(Value::Array(items)) => Some(i64::try_from(items.len()).unwrap_or(i64::MAX)),
        Some(_) => Some(0),
    }
}

#[must_use]
pub fn invoked_tool_count_from_response_body(value: &Value) -> i64 {
    let mut counter = ToolCallCounter::default();
    counter.observe_value(value);
    counter.count()
}

#[must_use]
pub fn invoked_distinct_tool_count_from_response_body(value: &Value) -> i64 {
    let mut counter = ToolCallCounter::default();
    counter.observe_value(value);
    counter.distinct_tool_count()
}

pub(super) fn tool_call_identities_from_value(value: &Value) -> Vec<ObservedToolCall> {
    let mut identities = Vec::new();
    collect_chat_tool_call_identities(value, &mut identities);
    collect_responses_tool_call_identities(value, &mut identities);
    collect_anthropic_messages_tool_call_identities(value, &mut identities);
    identities
}

fn collect_chat_tool_call_identities(value: &Value, identities: &mut Vec<ObservedToolCall>) {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for choice in choices {
        if let Some(message) = choice.get("message")
            && let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
        {
            collect_tool_call_array_identities(tool_calls, identities, true);
        }
        for delta in [
            choice.get("delta"),
            choice.get("chunk").and_then(|chunk| chunk.get("delta")),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                collect_tool_call_array_identities(tool_calls, identities, false);
            }
        }
    }
}

fn collect_responses_tool_call_identities(value: &Value, identities: &mut Vec<ObservedToolCall>) {
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            collect_tool_call_item_identity(item, identities, true);
        }
    }

    for key in ["item", "output_item"] {
        if let Some(item) = value.get(key) {
            collect_tool_call_item_identity(item, identities, true);
        }
    }

    if let Some(delta) = value.get("delta") {
        collect_tool_call_item_identity(delta, identities, false);
    }
}

fn collect_anthropic_messages_tool_call_identities(
    value: &Value,
    identities: &mut Vec<ObservedToolCall>,
) {
    if value.get("type").and_then(Value::as_str) == Some("content_block_start")
        && let Some(content_block) = value.get("content_block")
        && content_block.get("type").and_then(Value::as_str) == Some("tool_use")
    {
        push_tool_call_identity(content_block, identities, true);
    }

    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for item in content {
            if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                push_tool_call_identity(item, identities, true);
            }
        }
    }
}

fn collect_tool_call_array_identities(
    items: &[Value],
    identities: &mut Vec<ObservedToolCall>,
    allow_anonymous: bool,
) {
    for item in items {
        push_tool_call_identity(item, identities, allow_anonymous);
    }
}

fn collect_tool_call_item_identity(
    item: &Value,
    identities: &mut Vec<ObservedToolCall>,
    allow_anonymous: bool,
) {
    let object = match item.as_object() {
        Some(object) => object,
        None => return,
    };

    if let Some(tool_calls) = object.get("tool_calls").and_then(Value::as_array) {
        collect_tool_call_array_identities(tool_calls, identities, allow_anonymous);
        return;
    }

    let item_type = object
        .get("type")
        .or_else(|| object.get("item_type"))
        .and_then(Value::as_str);
    let is_tool_call = item_type.is_some_and(|item_type| {
        item_type == "function_call" || item_type == "tool_call" || item_type.contains("tool_call")
    }) || object.contains_key("function");

    if !is_tool_call {
        return;
    }

    push_tool_call_identity(item, identities, allow_anonymous);
}

fn push_tool_call_identity(
    item: &Value,
    identities: &mut Vec<ObservedToolCall>,
    allow_anonymous: bool,
) {
    let Some(object) = item.as_object() else {
        return;
    };

    let id = object
        .get("id")
        .or_else(|| object.get("call_id"))
        .or_else(|| object.get("tool_call_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let identity = match id {
        Some(id) => ToolCallIdentity::Known(id.to_string()),
        None if allow_anonymous => ToolCallIdentity::Anonymous,
        None => return,
    };
    // Chat Completions nests the name under `function`; Responses and Messages put it inline.
    let name = object
        .get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    identities.push(ObservedToolCall { identity, name });
}
