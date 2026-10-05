use serde_json::json;

use crate::redaction::{RequestLogPayloadCaptureMode, RequestLogPayloadPolicy};

use super::super::{
    StreamFailureSummary, StreamResponseCollector, UsageSummary, usage_summary_from_value,
};

#[test]
fn collector_reassembles_split_frames_and_keeps_latest_usage() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk("data: {\"usage\":{\"prompt_tokens\":1".as_bytes());
    collector.observe_chunk(
        ",\"completion_tokens\":2,\"total_tokens\":3}}\n\ndata:{\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":5,\"total_tokens\":9}}\n\n"
            .as_bytes(),
    );
    collector.finish();
    assert_eq!(
        collector.usage(),
        Some(&json!({
            "prompt_tokens": 4,
            "completion_tokens": 5,
            "total_tokens": 9
        }))
    );
}

#[test]
fn collector_merges_anthropic_usage_observed_before_stream_failure() {
    let mut collector = StreamResponseCollector::default();
    collector.observe_chunk(
        br#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":30,"cache_read_input_tokens":20,"cache_creation_input_tokens":10}}}

event: message_delta
data: {"type":"message_delta","delta":{},"usage":{"output_tokens":7}}

event: error
data: {"type":"error","error":{"code":"upstream_failed"}}

"#,
    );
    collector.finish();

    assert_eq!(
        collector.usage(),
        Some(&json!({
            "input_tokens": 30,
            "cache_read_input_tokens": 20,
            "cache_creation_input_tokens": 10,
            "output_tokens": 7
        }))
    );
    assert_eq!(
        collector.failure(),
        Some(&StreamFailureSummary {
            status_code: 502,
            error_code: "upstream_failed".to_string(),
        })
    );
    assert_eq!(
        usage_summary_from_value(collector.usage()),
        UsageSummary {
            prompt_tokens: Some(30),
            completion_tokens: Some(7),
            total_tokens: Some(37),
        }
    );
}

#[test]
fn collector_reads_nested_responses_failure() {
    let mut collector = StreamResponseCollector::default();

    let observation = collector.observe_chunk(
        br#"event: response.failed
data: {"type":"response.failed","response":{"status":"failed","error":{"code":"server_error","message":"boom"}}}

"#,
    );

    assert!(observation.has_terminal_event);
    assert!(observation.ends_stream);
    assert_eq!(
        collector.failure(),
        Some(&StreamFailureSummary {
            status_code: 502,
            error_code: "server_error".to_string(),
        })
    );
}

#[test]
fn collector_defaults_responses_failure_without_error_details() {
    let mut collector = StreamResponseCollector::default();

    let observation = collector.observe_chunk(
        br#"data: {"type":"response.failed","response":{"status":"failed"}}

"#,
    );

    assert!(observation.ends_stream);
    assert_eq!(
        collector.failure(),
        Some(&StreamFailureSummary {
            status_code: 502,
            error_code: "stream_error".to_string(),
        })
    );
}

#[test]
fn collector_waits_for_done_after_responses_completion() {
    let mut collector = StreamResponseCollector::default();

    let completed = collector.observe_chunk(
        br#"data: {"type":"response.completed","response":{"status":"completed"}}

"#,
    );
    assert!(completed.has_terminal_event);
    assert!(!completed.ends_stream);

    let done = collector.observe_chunk(b"data: [DONE]\n\n");
    assert!(done.ends_stream);
}

#[test]
fn collector_retains_response_id_before_terminal_delivery_with_capture_disabled() {
    let policy =
        RequestLogPayloadPolicy::new(RequestLogPayloadCaptureMode::Disabled, 0, 0, 0, Vec::new());
    let mut collector = StreamResponseCollector::with_payload_policy(policy);
    let first = collector.observe_chunk(
        b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_",
    );
    assert!(!first.has_terminal_event);
    assert_eq!(collector.response_id(), None);

    collector.observe_chunk(b"created\"}}\n\n");
    assert_eq!(collector.response_id(), Some("resp_created"));

    let terminal = collector.observe_chunk(
        b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_completed\"}}\n\n",
    );
    assert!(terminal.has_terminal_event);
    assert_eq!(collector.response_id(), Some("resp_completed"));
    assert_eq!(collector.analysis_payload(), None);
    collector.finish();
    assert_eq!(collector.response_id(), Some("resp_completed"));
}

#[test]
fn collector_recognizes_root_response_id_from_sse_event_name() {
    let mut collector = StreamResponseCollector::default();
    collector.observe_chunk(b"event: response.created\ndata: {\"id\":\"resp_root\"}\n\n");
    assert_eq!(collector.response_id(), Some("resp_root"));
}

#[test]
fn collector_retains_id_from_terminal_only_incomplete_response() {
    let mut collector = StreamResponseCollector::default();
    let terminal = collector.observe_chunk(
        b"data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_incomplete\",\"status\":\"incomplete\"}}\n\n",
    );
    assert!(terminal.has_terminal_event);
    assert_eq!(collector.failure(), None);
    assert_eq!(collector.response_id(), Some("resp_incomplete"));
}

#[test]
fn collector_ignores_other_event_ids() {
    let mut collector = StreamResponseCollector::default();
    collector.observe_chunk(
        b"data: {\"id\":\"chatcmpl_a\",\"choices\":[]}\n\ndata: {\"type\":\"response.output_item.added\",\"id\":\"item_a\"}\n\n",
    );
    assert_eq!(collector.response_id(), None);
}

#[test]
fn collector_bounds_response_identifiers_without_truncation() {
    let mut collector = StreamResponseCollector::default();
    for id in [json!(null), json!(42), json!(""), json!("a".repeat(257))] {
        let event = json!({"type": "response.completed", "response": {"id": id}});
        collector.observe_chunk(format!("data: {event}\n\n").as_bytes());
        assert_eq!(collector.response_id(), None);
    }
    let id = "a".repeat(256);
    let event = json!({"type": "response.completed", "response": {"id": id}});
    collector.observe_chunk(format!("data: {event}\n\n").as_bytes());
    assert_eq!(collector.response_id(), Some(id.as_str()));
}

#[test]
fn request_summary_uses_provider_totals_without_cache_accounting() {
    assert_eq!(
        usage_summary_from_value(Some(&json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 40}
        }))),
        UsageSummary {
            prompt_tokens: Some(100),
            completion_tokens: Some(20),
            total_tokens: Some(120),
        }
    );
}

#[test]
fn collector_ignores_synthetic_anthropic_zero_usage_fallback() {
    let mut collector = StreamResponseCollector::default();

    let observation = collector.observe_chunk(
        br#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":0,"output_tokens":0}}

event: message_stop
data: {"type":"message_stop"}

"#,
    );
    assert!(observation.ends_stream);
    collector.finish();

    assert_eq!(collector.usage(), None);
}

#[test]
fn collector_reassembles_split_utf8_and_error_frames() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk(b"data: {\"delta\":\"");
    collector.observe_chunk(&[0xF0, 0x9F]);
    collector.observe_chunk(&[
        0x99, 0x82, b'"', b'}', b'\n', b'\n', b'd', b'a', b't', b'a', b':', b'{', b'"', b'e', b'r',
        b'r', b'o', b'r', b'"', b':', b'{', b'"', b'c', b'o', b'd', b'e', b'"', b':', b'"', b'u',
        b'p', b's', b't', b'r', b'e', b'a', b'm', b'_', b'b', b'a', b'd', b'"', b'}', b'}',
    ]);
    collector.observe_chunk(b"\n\n");
    collector.finish();

    assert_eq!(
        collector.failure(),
        Some(&StreamFailureSummary {
            status_code: 502,
            error_code: "upstream_bad".to_string(),
        })
    );
}

#[test]
fn collector_accepts_data_prefix_without_space() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk(b"data:{\"value\":1}\n\n");
    collector.finish();

    let (payload, truncated) = collector.into_payload(None);
    assert!(!truncated);
    assert_eq!(payload["events"][0]["value"], 1);
}

#[test]
fn collector_reports_first_output_usage_and_terminal_events() {
    let mut collector = StreamResponseCollector::default();

    let role = collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}

"#,
    );
    assert!(!role.has_output);

    let output = collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"content":"hello"},"finish_reason":null}],"usage":{"prompt_tokens":2}}

"#,
    );
    assert!(output.has_output);
    assert!(output.has_usage);
    assert!(!output.has_terminal_event);

    let finish_reason = collector.observe_chunk(
        br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

"#,
    );
    assert!(finish_reason.has_terminal_event);
    assert!(!finish_reason.ends_stream);

    let done = collector.observe_chunk(b"data: [DONE]\n\n");
    assert!(done.has_terminal_event);
    assert!(done.ends_stream);
}

#[test]
fn collector_reports_responses_and_anthropic_output_deltas() {
    let mut collector = StreamResponseCollector::default();

    let responses = collector.observe_chunk(
        br#"data: {"type":"response.output_text.delta","delta":"hello"}

"#,
    );
    assert!(responses.has_output);

    let anthropic = collector.observe_chunk(
        br#"event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"world"}}

"#,
    );
    assert!(anthropic.has_output);
}

#[test]
fn collector_ignores_empty_anthropic_delta_metadata() {
    let mut collector = StreamResponseCollector::default();

    let empty_thinking = collector.observe_chunk(
        br#"event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":""}}

"#,
    );
    assert!(!empty_thinking.has_output);

    let unknown_metadata = collector.observe_chunk(
        br#"event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"metadata_delta","sequence":1}}

"#,
    );
    assert!(!unknown_metadata.has_output);

    let thinking = collector.observe_chunk(
        br#"event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"reason"}}

"#,
    );
    assert!(thinking.has_output);

    let tool_input = collector.observe_chunk(
        br#"event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{}"}}

"#,
    );
    assert!(tool_input.has_output);
}

#[test]
fn stream_collector_counts_invoked_tools_from_sse_events() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","type":"function"}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","type":"function"}]}}]}

data: {"output":[{"id":"call_2","type":"function_call"}]}

"#,
    );
    collector.finish();

    assert_eq!(collector.invoked_tool_count(), 2);
}

#[test]
fn stream_collector_ignores_chat_tool_call_delta_fragments_without_ids() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function"}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"London\"}"}}]}}]}

"#,
    );
    collector.finish();

    assert_eq!(collector.invoked_tool_count(), 1);
}

#[test]
fn stream_collector_counts_distinct_tool_names_from_opening_deltas() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"search","arguments":""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_2","type":"function","function":{"name":"search","arguments":""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":2,"id":"call_3","type":"function","function":{"name":"fetch","arguments":""}}]}}]}

"#,
    );
    collector.finish();

    assert_eq!(collector.invoked_tool_count(), 3);
    assert_eq!(collector.invoked_distinct_tool_count(), 2);
}

#[test]
fn stream_collector_counts_anthropic_messages_tool_use_starts() {
    let mut collector = StreamResponseCollector::default();

    collector.observe_chunk(
        br#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}

"#,
    );
    collector.finish();

    assert_eq!(collector.invoked_tool_count(), 1);
}
