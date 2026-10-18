use agent_session_analysis::tool_calls::ToolCallTally;
use serde_json::Value;

/// Tool calls in one response, as request logs persist them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct InvokedToolCounts {
    pub(super) calls: i64,
    pub(super) distinct_tools: i64,
    pub(super) mcp_calls: i64,
}

impl InvokedToolCounts {
    pub(super) fn from_response_body(value: &Value) -> Self {
        let mut tally = ToolCallTally::default();
        tally.observe(value);
        Self::from(&tally)
    }
}

impl From<&ToolCallTally> for InvokedToolCounts {
    fn from(tally: &ToolCallTally) -> Self {
        Self {
            calls: tally.count(),
            distinct_tools: tally.distinct_tool_count(),
            mcp_calls: tally.mcp_count(),
        }
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
    InvokedToolCounts::from_response_body(value).calls
}

#[must_use]
pub fn invoked_distinct_tool_count_from_response_body(value: &Value) -> i64 {
    InvokedToolCounts::from_response_body(value).distinct_tools
}
