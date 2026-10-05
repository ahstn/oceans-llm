use std::{collections::BTreeMap, sync::Arc};

use gateway_core::RequestTags;
use serde_json::json;

use super::super::{
    RequestLogging, invoked_distinct_tool_count_from_response_body,
    invoked_tool_count_from_response_body, shallow_tool_count_from_request_body,
};
use super::{InMemoryRepo, sample_request};

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

#[test]
fn invoked_distinct_tool_count_counts_tool_names_not_calls() {
    let chat = json!({
        "choices": [{
            "message": {
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "search"}},
                    {"id": "call_2", "type": "function", "function": {"name": "search"}},
                    {"id": "call_3", "type": "function", "function": {"name": "fetch"}}
                ]
            }
        }]
    });
    assert_eq!(invoked_tool_count_from_response_body(&chat), 3);
    assert_eq!(invoked_distinct_tool_count_from_response_body(&chat), 2);

    let responses = json!({
        "output": [
            {"call_id": "call_1", "type": "function_call", "name": "search"},
            {"call_id": "call_2", "type": "function_call", "name": "search"}
        ]
    });
    assert_eq!(invoked_tool_count_from_response_body(&responses), 2);
    assert_eq!(
        invoked_distinct_tool_count_from_response_body(&responses),
        1
    );

    let messages = json!({
        "content": [
            {"type": "text", "text": "checking"},
            {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}},
            {"type": "tool_use", "id": "toolu_2", "name": "lookup", "input": {}}
        ]
    });
    assert_eq!(invoked_tool_count_from_response_body(&messages), 2);
    assert_eq!(invoked_distinct_tool_count_from_response_body(&messages), 1);
}

#[test]
fn begin_request_records_request_body_tool_count_separately() {
    let repo = Arc::new(InMemoryRepo {
        users: Default::default(),
        logs: Default::default(),
        payloads: Default::default(),
        attempts: Default::default(),
    });
    let logging = RequestLogging::new(repo);
    let mut request = sample_request(false);
    request.extra.insert(
        "tools".to_string(),
        json!([{"type": "function"}, {"type": "function"}]),
    );

    let context = logging.begin_chat_request(
        "req_tools",
        "fast",
        "fast",
        &request,
        &BTreeMap::new(),
        RequestTags::default(),
    );

    assert_eq!(context.tool_cardinality.request_tool_count, Some(2));
    assert_eq!(context.tool_cardinality.exposed_tool_count, Some(2));
}
