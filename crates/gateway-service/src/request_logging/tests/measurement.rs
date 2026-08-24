use std::{collections::BTreeMap, sync::Arc, time::Instant};

use gateway_core::{RequestTags, ResponsesRequest};
use serde_json::{Value, json};

use crate::{
    payload_bounding::bound_request_payload,
    redaction::{MAX_INLINE_REQUEST_BYTES, RequestLogPayloadCaptureMode},
};

use super::super::RequestLogging;
use super::{InMemoryRepo, policy};

fn measured_responses_request(scenario: &str, bytes: usize) -> ResponsesRequest {
    let (input, tools) = match scenario {
        "long_message" => (
            json!([{"type": "message", "role": "user", "content": "x".repeat(bytes)}]),
            None,
        ),
        "many_messages" => {
            let per_message = bytes.div_ceil(128);
            (
                Value::Array(
                    (0..128)
                        .map(|index| {
                            json!({
                                "type": "message",
                                "role": "user",
                                "id": format!("message-{index}"),
                                "content": "x".repeat(per_message),
                            })
                        })
                        .collect(),
                ),
                None,
            )
        }
        "tool_heavy" => (
            json!([{"type": "message", "role": "user", "content": "measure"}]),
            Some(Value::Array(
                (0..8)
                    .map(|index| {
                        json!({
                            "type": "function",
                            "name": format!("tool_{index}"),
                            "description": "description ".repeat(bytes.div_ceil(96)),
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "input": {
                                        "type": "string",
                                        "description": "property ".repeat(bytes.div_ceil(128)),
                                        "examples": ["example ".repeat(bytes.div_ceil(128))]
                                    }
                                }
                            }
                        })
                    })
                    .collect(),
            )),
        ),
        "content_arrays" => (
            json!([{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "a".repeat(bytes / 2)},
                    {"type": "input_text", "text": "b".repeat(bytes / 2)}
                ]
            }]),
            None,
        ),
        "binary_blocks" => (
            json!([{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "data:image/png;base64,".to_string() + &"A".repeat(bytes)
                }]
            }]),
            None,
        ),
        "multibyte_utf8" => (
            json!([{
                "type": "message",
                "role": "user",
                "content": "🙂".repeat(bytes.div_ceil(4))
            }]),
            None,
        ),
        _ => unreachable!("known measurement scenario"),
    };
    ResponsesRequest {
        model: "measurement-model".to_string(),
        input,
        stream: false,
        instructions: None,
        tools,
        tool_choice: Some(json!("auto")),
        reasoning: Some(json!({"effort": "high"})),
        text: None,
        extra: BTreeMap::new(),
    }
}

#[test]
#[ignore = "manual request truncation measurement harness"]
fn measures_payload_helper_and_request_setup_matrix() {
    let repo = Arc::new(InMemoryRepo::default());
    let max_bytes = 128 * 1024;
    let mut stored_sizes = Vec::new();
    let scenarios = [
        "long_message",
        "many_messages",
        "tool_heavy",
        "content_arrays",
        "binary_blocks",
        "multibyte_utf8",
    ];
    for bytes in [8 * 1024, 64 * 1024, 256 * 1024, 1024 * 1024] {
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy(
                RequestLogPayloadCaptureMode::RedactedPayloads,
                max_bytes,
                64 * 1024,
                128,
            ),
        );
        for scenario in scenarios {
            let request = measured_responses_request(scenario, bytes);
            let wrapped = json!({
                "headers": {"session_id": "measurement-session"},
                "body": serde_json::to_value(&request).expect("serialize request"),
            });
            let helper_started = Instant::now();
            let (stored, truncated) = bound_request_payload(wrapped, max_bytes);
            let helper_elapsed = helper_started.elapsed();
            let stored_size = serde_json::to_vec(&stored).expect("serialize stored").len();
            assert!(stored_size <= max_bytes);
            assert!(stored_size <= MAX_INLINE_REQUEST_BYTES);
            stored_sizes.push(stored_size);

            let setup_started = Instant::now();
            let context = logging.begin_responses_request(
                "measurement-request",
                "measurement-model",
                "measurement-model",
                &request,
                &BTreeMap::from([
                    ("user-agent".to_string(), "pi/0.80.2".to_string()),
                    ("session_id".to_string(), "measurement-session".to_string()),
                ]),
                RequestTags::default(),
            );
            let setup_elapsed = setup_started.elapsed();
            assert!(context.request_json.is_some());
            eprintln!(
                "payload_bounding scenario={scenario} input_bytes={bytes} stored_bytes={stored_size} max_bytes={max_bytes} truncated={truncated} helper_us={} request_setup_us={}",
                helper_elapsed.as_micros(),
                setup_elapsed.as_micros(),
            );
        }
    }
    stored_sizes.sort_unstable();
    let p95_index = stored_sizes.len().saturating_mul(95).div_ceil(100) - 1;
    let p95_stored_bytes = stored_sizes[p95_index];
    assert!(p95_stored_bytes <= max_bytes);
    eprintln!(
        "payload_bounding stress_fixture_p95_stored_bytes={p95_stored_bytes} preferred_p95_bytes={} normal_cap_bytes={max_bytes} absolute_inline_bytes={MAX_INLINE_REQUEST_BYTES}",
        64 * 1024,
    );
}
