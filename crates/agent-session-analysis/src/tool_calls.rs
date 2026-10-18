//! Tool calls a model made, read from the response bodies and stream events the
//! gateway returns to callers. Request logs count them and passive session
//! analysis classifies them, so both read the same formats from here.
//!
//! Supported shapes, as one response body or one stream event each:
//! - Chat Completions: `choices[].message.tool_calls`, `choices[].delta.tool_calls`,
//!   and the legacy `function_call`, for both `function` and `custom` tools.
//! - Responses: every `*_call` output item (`function_call`, `custom_tool_call`,
//!   `mcp_call`, and hosted tools such as `web_search_call`) in `output`, in
//!   `response.output` on lifecycle events, and in `item` on output item events.
//! - Anthropic Messages: `tool_use`, `server_tool_use`, and `mcp_tool_use` blocks
//!   in `content` and on `content_block_start` events.

use std::collections::HashSet;

use serde_json::Value;

const MCP_TOOL_NAME_PREFIX: &str = "mcp__";
const CALL_ID_KEYS: &[&str] = &["id", "call_id", "tool_call_id"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallKind {
    /// A tool the caller defined and runs itself.
    Client,
    /// An MCP tool the provider calls for the caller (`mcp_call`, `mcp_tool_use`).
    ProviderMcp,
    /// A built-in tool the provider defines, such as web search or code execution.
    ProviderHosted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCall<'a> {
    pub id: Option<&'a str>,
    /// Hosted Responses tools carry no name, so it comes from the item type
    /// (`web_search_call` is `web_search`).
    pub name: Option<&'a str>,
    pub kind: ToolCallKind,
    /// The MCP server label the provider reports, or the `<server>` in a
    /// `mcp__<server>__<tool>` client tool name.
    pub mcp_server: Option<&'a str>,
}

impl ToolCall<'_> {
    #[must_use]
    pub fn is_mcp(&self) -> bool {
        self.mcp_server.is_some() || self.kind == ToolCallKind::ProviderMcp
    }
}

/// Appends the tool calls in one response body or one stream event.
///
/// Streamed Chat Completions deltas carry the call ID on the delta that opens a
/// call, so deltas without one are argument continuations and are skipped.
pub fn collect_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    collect_chat_tool_calls(value, calls);
    collect_responses_tool_calls(value, calls);
    collect_messages_tool_calls(value, calls);
}

/// Appends the tool calls in a logged response payload: a response body, or a
/// streamed response captured as `{ "events": [...] }`.
pub fn collect_logged_response_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    match value.get("events").and_then(Value::as_array) {
        Some(events) => events
            .iter()
            .for_each(|event| collect_tool_calls(event, calls)),
        None => collect_tool_calls(value, calls),
    }
}

/// Counts tool calls across a response, removing calls repeated by ID. A
/// streamed call appears on several events, such as `response.output_item.added`
/// and `response.completed`, under one ID.
#[derive(Debug, Clone, Default)]
pub struct ToolCallTally {
    all: CallIdentities,
    mcp: CallIdentities,
    names: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct CallIdentities {
    known_ids: HashSet<String>,
    anonymous: u64,
}

impl CallIdentities {
    fn insert(&mut self, id: Option<&str>) {
        match id {
            Some(id) => {
                self.known_ids.insert(id.to_string());
            }
            None => self.anonymous = self.anonymous.saturating_add(1),
        }
    }

    fn count(&self) -> i64 {
        i64::try_from(self.known_ids.len())
            .unwrap_or(i64::MAX)
            .saturating_add(i64::try_from(self.anonymous).unwrap_or(i64::MAX))
    }
}

impl ToolCallTally {
    pub fn observe(&mut self, value: &Value) {
        let mut calls = Vec::new();
        collect_tool_calls(value, &mut calls);
        for call in calls {
            if let Some(name) = call.name {
                self.names.insert(name.to_string());
            }
            self.all.insert(call.id);
            if call.is_mcp() {
                self.mcp.insert(call.id);
            }
        }
    }

    #[must_use]
    pub fn count(&self) -> i64 {
        self.all.count()
    }

    /// Calls to MCP tools, whether the provider made them or the client will.
    #[must_use]
    pub fn mcp_count(&self) -> i64 {
        self.mcp.count()
    }

    #[must_use]
    pub fn distinct_tool_count(&self) -> i64 {
        i64::try_from(self.names.len()).unwrap_or(i64::MAX)
    }
}

/// The server a tool name belongs to: `mcp__<server>__<tool>`, or the prefix
/// before the first `.` or `/` in namespaced names such as `github.search`.
#[must_use]
pub fn tool_server_key(name: &str) -> Option<String> {
    if name.starts_with(MCP_TOOL_NAME_PREFIX) {
        return mcp_server_from_tool_name(name).map(str::to_string);
    }
    ['.', '/']
        .into_iter()
        .find_map(|delimiter| name.split_once(delimiter).map(|(server, _)| server))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn mcp_server_from_tool_name(name: &str) -> Option<&str> {
    name.strip_prefix(MCP_TOOL_NAME_PREFIX)?
        .split_once("__")
        .map(|(server, _)| server)
        .filter(|server| !server.is_empty())
}

fn collect_chat_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for choice in choices {
        if let Some(message) = choice.get("message") {
            collect_chat_message_tool_calls(message, calls, false);
        }
        for delta in [
            choice.get("delta"),
            choice.get("chunk").and_then(|chunk| chunk.get("delta")),
        ]
        .into_iter()
        .flatten()
        {
            collect_chat_message_tool_calls(delta, calls, true);
        }
    }
}

fn collect_chat_message_tool_calls<'a>(
    message: &'a Value,
    calls: &mut Vec<ToolCall<'a>>,
    is_delta: bool,
) {
    for tool_call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = string_field(tool_call, CALL_ID_KEYS);
        if is_delta && id.is_none() {
            continue;
        }
        // `function` and `custom` tool calls nest their name under the tool type.
        let name = ["function", "custom"].into_iter().find_map(|key| {
            tool_call
                .get(key)
                .and_then(|tool| string_field(tool, &["name"]))
        });
        calls.push(client_call(id, name));
    }

    // The legacy `function_call` has no ID; it names the function once per message,
    // and once per stream on the delta that opens it.
    if let Some(function_call) = message.get("function_call") {
        let name = string_field(function_call, &["name"]);
        if !is_delta || name.is_some() {
            calls.push(client_call(None, name));
        }
    }
}

fn collect_responses_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    let outputs = [
        value.get("output"),
        value
            .get("response")
            .and_then(|response| response.get("output")),
    ];
    for item in outputs
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
    {
        collect_responses_item_tool_calls(item, calls);
    }
    for key in ["item", "output_item"] {
        if let Some(item) = value.get(key) {
            collect_responses_item_tool_calls(item, calls);
        }
    }
}

fn collect_responses_item_tool_calls<'a>(item: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    if let Some(tool_calls) = item.get("tool_calls").and_then(Value::as_array) {
        calls.extend(tool_calls.iter().map(|tool_call| {
            let name = tool_call
                .get("function")
                .and_then(|function| string_field(function, &["name"]))
                .or_else(|| string_field(tool_call, &["name"]));
            client_call(string_field(tool_call, CALL_ID_KEYS), name)
        }));
        return;
    }
    if let Some(function) = item.get("function").filter(|function| function.is_object()) {
        calls.push(client_call(
            string_field(item, CALL_ID_KEYS),
            string_field(function, &["name"]),
        ));
        return;
    }

    let Some(item_type) = string_field(item, &["type"]) else {
        return;
    };
    // Every tool call item type ends in `_call`; results end in `_call_output`.
    let Some(tool_type) = item_type.strip_suffix("_call") else {
        return;
    };
    let id = string_field(item, CALL_ID_KEYS);
    let name = string_field(item, &["name"]);
    calls.push(match item_type {
        "function_call" | "custom_tool_call" | "tool_call" => client_call(id, name),
        "mcp_call" => ToolCall {
            id,
            name,
            kind: ToolCallKind::ProviderMcp,
            mcp_server: string_field(item, &["server_label"]),
        },
        _ => ToolCall {
            id,
            name: name.or(Some(tool_type)),
            kind: ToolCallKind::ProviderHosted,
            mcp_server: None,
        },
    });
}

fn collect_messages_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    if string_field(value, &["type"]) == Some("content_block_start")
        && let Some(block) = value.get("content_block")
    {
        collect_messages_block_tool_call(block, calls);
    }
    for block in value
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        collect_messages_block_tool_call(block, calls);
    }
}

fn collect_messages_block_tool_call<'a>(block: &'a Value, calls: &mut Vec<ToolCall<'a>>) {
    let id = string_field(block, &["id"]);
    let name = string_field(block, &["name"]);
    match string_field(block, &["type"]) {
        Some("tool_use") => calls.push(client_call(id, name)),
        Some("mcp_tool_use") => calls.push(ToolCall {
            id,
            name,
            kind: ToolCallKind::ProviderMcp,
            mcp_server: string_field(block, &["server_name"]),
        }),
        Some("server_tool_use") => calls.push(ToolCall {
            id,
            name,
            kind: ToolCallKind::ProviderHosted,
            mcp_server: None,
        }),
        _ => {}
    }
}

fn client_call<'a>(id: Option<&'a str>, name: Option<&'a str>) -> ToolCall<'a> {
    ToolCall {
        id,
        name,
        kind: ToolCallKind::Client,
        mcp_server: name.and_then(mcp_server_from_tool_name),
    }
}

/// The first of `keys` holding a non-blank string, trimmed.
fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
