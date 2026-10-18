use serde_json::{Value, json};

use super::*;

fn calls(value: &Value) -> Vec<ToolCall<'_>> {
    distinct_named_tool_calls(value, usize::MAX).0
}

fn tally(values: &[Value]) -> (i64, i64) {
    let mut tally = ToolCallTally::default();
    values.iter().for_each(|value| tally.observe(value));
    (tally.count(), tally.distinct_tool_count())
}

fn client(id: &'static str, name: &'static str) -> ToolCall<'static> {
    ToolCall {
        id: Some(id),
        name: Some(name),
        kind: ToolCallKind::Client,
        mcp_server: None,
    }
}

#[test]
fn chat_completions_reads_function_custom_and_legacy_calls() {
    let response = json!({"choices": [{"message": {
        "tool_calls": [
            {"id": "call_1", "type": "function", "function": {"name": "search", "arguments": "{}"}},
            {"id": "call_2", "type": "custom", "custom": {"name": "apply_patch", "input": "..."}}
        ],
        "function_call": {"name": "lookup", "arguments": "{}"}
    }}]});

    assert_eq!(
        calls(&response),
        vec![
            client("call_1", "search"),
            client("call_2", "apply_patch"),
            ToolCall {
                id: None,
                ..client("", "lookup")
            },
        ]
    );
}

#[test]
fn chat_completions_stream_counts_each_call_once() {
    let events = [
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "id": "call_1", "type": "function", "function": {"name": "search", "arguments": ""}}
        ]}}]}),
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "function": {"arguments": "{\"q\":"}}
        ]}}]}),
        json!({"choices": [{"index": 0, "delta": {"function_call": {"name": "lookup", "arguments": ""}}}]}),
        json!({"choices": [{"index": 0, "delta": {"function_call": {"arguments": "{}"}}}]}),
    ];

    assert_eq!(tally(&events), (2, 2));
}

#[test]
fn responses_reads_every_call_item_type() {
    let response = json!({"output": [
        {"type": "reasoning", "id": "rs_1"},
        {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "search"},
        {"type": "custom_tool_call", "id": "ctc_1", "call_id": "call_2", "name": "apply_patch"},
        {"type": "mcp_list_tools", "id": "mcpl_1", "server_label": "github", "tools": []},
        {"type": "mcp_call", "id": "mcp_1", "server_label": "github", "name": "create_issue"},
        {"type": "web_search_call", "id": "ws_1", "status": "completed"},
        {"type": "function_call_output", "call_id": "call_1", "output": "{}"},
        {"type": "message", "id": "msg_1", "content": [{"type": "output_text", "text": "done"}]}
    ]});

    assert_eq!(
        calls(&response),
        vec![
            client("fc_1", "search"),
            client("ctc_1", "apply_patch"),
            ToolCall {
                id: Some("mcp_1"),
                name: Some("create_issue"),
                kind: ToolCallKind::ProviderMcp,
                mcp_server: Some("github"),
            },
            ToolCall {
                id: Some("ws_1"),
                name: Some("web_search"),
                kind: ToolCallKind::ProviderHosted,
                mcp_server: None,
            },
        ]
    );
}

#[test]
fn responses_stream_counts_a_call_seen_on_several_events_once() {
    let item =
        json!({"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "search"});
    let events = [
        json!({"type": "response.output_item.added", "output_index": 0, "item": item}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{}"}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": item}),
        json!({"type": "response.completed", "response": {"output": [item]}}),
    ];

    assert_eq!(tally(&events), (1, 1));
}

#[test]
fn messages_reads_client_server_and_mcp_tool_use_blocks() {
    let response = json!({"content": [
        {"type": "text", "text": "checking"},
        {"type": "tool_use", "id": "toolu_1", "name": "mcp__github__create_issue", "input": {}},
        {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {}},
        {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_1", "content": []},
        {"type": "mcp_tool_use", "id": "mcptoolu_1", "name": "search", "server_name": "linear", "input": {}},
        {"type": "mcp_tool_result", "tool_use_id": "mcptoolu_1", "content": []}
    ]});

    let calls = calls(&response);
    assert_eq!(
        calls,
        vec![
            ToolCall {
                mcp_server: Some("github"),
                ..client("toolu_1", "mcp__github__create_issue")
            },
            ToolCall {
                id: Some("srvtoolu_1"),
                name: Some("web_search"),
                kind: ToolCallKind::ProviderHosted,
                mcp_server: None,
            },
            ToolCall {
                id: Some("mcptoolu_1"),
                name: Some("search"),
                kind: ToolCallKind::ProviderMcp,
                mcp_server: Some("linear"),
            },
        ]
    );
    assert_eq!(
        calls.iter().map(ToolCall::is_mcp).collect::<Vec<_>>(),
        [true, false, true]
    );
}

#[test]
fn messages_stream_reads_content_block_start() {
    let events = json!({"stream": true, "events": [
        {"type": "message_start", "message": {"id": "msg_1", "content": []}},
        {"type": "content_block_start", "index": 0,
         "content_block": {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {}}},
        {"type": "content_block_delta", "index": 0,
         "delta": {"type": "input_json_delta", "partial_json": "{}"}},
        {"type": "content_block_stop", "index": 0}
    ]});

    assert_eq!(calls(&events), vec![client("toolu_1", "search")]);
}

#[test]
fn tool_shapes_inside_arguments_are_not_calls() {
    let response = json!({"content": [{
        "type": "tool_use", "id": "toolu_1", "name": "lookup",
        "input": {"output": [{"type": "function_call", "id": "fc_1", "name": "nested"}]}
    }]});

    assert_eq!(calls(&response), vec![client("toolu_1", "lookup")]);
}

#[test]
fn tool_server_key_reads_mcp_and_namespaced_names() {
    assert_eq!(
        tool_server_key("mcp__github__create_issue").as_deref(),
        Some("github")
    );
    assert_eq!(tool_server_key("github.search").as_deref(), Some("github"));
    assert_eq!(tool_server_key("fs/read").as_deref(), Some("fs"));
    assert_eq!(tool_server_key("mcp__"), None);
    assert_eq!(tool_server_key("read_file"), None);
}

#[test]
fn tally_counts_mcp_calls_once_across_stream_events() {
    let mcp_item = json!({"type": "mcp_call", "id": "mcp_1", "server_label": "github", "name": "create_issue"});
    let mut tally = ToolCallTally::default();
    for event in [
        json!({"type": "response.output_item.added", "item": mcp_item}),
        json!({"type": "response.output_item.added",
               "item": {"type": "function_call", "id": "fc_1", "name": "mcp__linear__search"}}),
        json!({"type": "response.output_item.added",
               "item": {"type": "function_call", "id": "fc_2", "name": "read_file"}}),
        json!({"type": "response.completed", "response": {"output": [mcp_item]}}),
    ] {
        tally.observe(&event);
    }

    assert_eq!(tally.count(), 3);
    assert_eq!(tally.mcp_count(), 2);
}

#[test]
fn responses_caller_run_built_ins_are_client_calls() {
    let response = json!({"output": [
        {"type": "apply_patch_call", "id": "apc_1", "call_id": "call_1", "operation": {}},
        {"type": "shell_call", "id": "sh_1", "call_id": "call_2"},
        {"type": "apply_patch_call_output", "call_id": "call_1"}
    ]});

    assert_eq!(
        calls(&response),
        vec![client("apc_1", "apply_patch"), client("sh_1", "shell")]
    );
}

#[test]
fn distinct_named_tool_calls_stops_at_the_limit() {
    let events = json!({"stream": true, "events": [
        {"type": "response.output_item.added",
         "item": {"type": "function_call", "id": "fc_1", "name": "search"}},
        {"type": "response.output_item.done",
         "item": {"type": "function_call", "id": "fc_1", "name": "search"}},
        {"type": "response.output_item.added",
         "item": {"type": "function_call", "id": "fc_2", "name": "read"}},
        {"type": "response.output_item.added",
         "item": {"type": "function_call", "id": "fc_3", "name": "write"}}
    ]});

    let (within, truncated) = distinct_named_tool_calls(&events, 3);
    assert_eq!(within.len(), 3);
    assert!(!truncated);

    let (capped, truncated) = distinct_named_tool_calls(&events, 2);
    assert_eq!(
        capped,
        vec![client("fc_1", "search"), client("fc_2", "read")]
    );
    assert!(truncated);
}
