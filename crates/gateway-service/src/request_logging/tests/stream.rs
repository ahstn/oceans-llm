use serde_json::json;

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

    collector.observe_chunk(
        br#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":0,"output_tokens":0}}

event: message_stop
data: {"type":"message_stop"}

"#,
    );
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

    let terminal = collector.observe_chunk(
        br#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}

data: [DONE]

"#,
    );
    assert!(terminal.has_terminal_event);
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
