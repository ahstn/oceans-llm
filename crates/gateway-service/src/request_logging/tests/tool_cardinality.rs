use serde_json::json;

use super::super::{invoked_tool_count_from_response_body, shallow_tool_count_from_request_body};

#[test]
fn shallow_tool_count_reads_chat_and_responses_shapes() {
    assert_eq!(shallow_tool_count_from_request_body(&json!({})), Some(0));
    assert_eq!(
        shallow_tool_count_from_request_body(&json!({
            "tools": [{"type": "function"}]
        })),
        Some(1)
    );
    assert_eq!(
        shallow_tool_count_from_request_body(&json!({
            "request": {
                "tools": [
                    {"type": "function"},
                    {"type": "web_search_preview"}
                ]
            }
        })),
        Some(2)
    );
    assert_eq!(
        shallow_tool_count_from_request_body(&json!({ "tools": "malformed" })),
        Some(0)
    );
    assert_eq!(
        shallow_tool_count_from_request_body(&json!({
            "request": { "tools": {"not": "array"} }
        })),
        Some(0)
    );
}

#[test]
fn invoked_tool_count_reads_non_stream_chat_and_responses_artifacts() {
    assert_eq!(
        invoked_tool_count_from_response_body(&json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call_1", "type": "function"},
                        {"id": "call_2", "type": "function"}
                    ]
                }
            }]
        })),
        2
    );
    assert_eq!(
        invoked_tool_count_from_response_body(&json!({
            "output": [
                {"id": "call_1", "type": "function_call"},
                {"call_id": "call_2", "type": "function_call"}
            ]
        })),
        2
    );
    assert_eq!(
        invoked_tool_count_from_response_body(&json!({
            "choices": [{
                "message": {
                    "tool_calls": [
                        {"id": "call_1", "type": "function"},
                        {"id": "call_1", "type": "function"}
                    ]
                }
            }]
        })),
        1
    );
    assert_eq!(
        invoked_tool_count_from_response_body(&json!({
            "output": [{
                "type": "message",
                "tool_calls": [
                    {"id": "call_1", "type": "function"},
                    {"id": "call_2", "type": "function"}
                ]
            }]
        })),
        2
    );
}
