use std::{collections::BTreeMap, sync::Arc, time::Instant};

use gateway_core::{RequestTags, ResponsesRequest};
use serde_json::{Value, json};

use crate::{payload_bounding::bound_request_payload, redaction::RequestLogPayloadCaptureMode};

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
    let scenarios = [
        "long_message",
        "many_messages",
        "tool_heavy",
        "content_arrays",
        "binary_blocks",
        "multibyte_utf8",
    ];
    for bytes in [8 * 1024, 64 * 1024, 256 * 1024, 1024 * 1024] {
        let max_bytes = (bytes / 2).max(4096);
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
            let (stored, _) = bound_request_payload(wrapped, max_bytes);
            let helper_elapsed = helper_started.elapsed();
            assert!(serde_json::to_vec(&stored).expect("serialize stored").len() <= max_bytes);

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
                "payload_bounding scenario={scenario} input_bytes={bytes} max_bytes={max_bytes} helper_us={} request_setup_us={}",
                helper_elapsed.as_micros(),
                setup_elapsed.as_micros(),
            );
        }
    }
}
