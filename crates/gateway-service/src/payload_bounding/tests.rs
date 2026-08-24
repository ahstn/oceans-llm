use serde_json::json;

use super::{bound_request_payload, bound_request_payload_after_known_fields, serialized_size};

#[test]
fn under_cap_payload_is_unchanged() {
    let payload = json!({"headers": {"x": "y"}, "body": {"input": "hello"}});

    let (stored, truncated) = bound_request_payload(payload.clone(), 1024);

    assert!(!truncated);
    assert_eq!(stored, payload);
}

#[test]
fn known_large_field_truncation_gets_top_level_metadata_under_cap() {
    let payload = json!({
        "headers": {},
        "body": {
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": {"truncated": true, "size_bytes": 4096, "preview": "data:"}
                }]
            }]
        }
    });

    let (stored, truncated) =
        bound_request_payload_after_known_fields(payload, 1024, Some(5000), 1);

    assert!(truncated);
    assert_eq!(stored["truncation"]["original_size_bytes"], 5000);
    assert_eq!(stored["truncation"]["truncated_field_count"], 1);
    assert_eq!(stored["truncation"]["known_large_fields_truncated"], 1);
    assert!(serialized_size(&stored).expect("stored size") <= 1024);
}

#[test]
fn bounds_message_text_and_preserves_structure_and_utf8() {
    let payload = json!({
        "headers": {"session_id": "session-1"},
        "body": {
            "model": "gpt-test",
            "input": [
                {"type": "message", "id": "item-1", "role": "user", "content": "🙂".repeat(6000)},
                {"type": "future_item", "id": "unknown-1", "custom": {"keep": true}, "content": [
                    {"type": "input_text", "text": "é".repeat(5000)},
                    {"type": "input_image", "image_url": "data:image/png;base64,".to_string() + &"A".repeat(5000)}
                ]}
            ],
            "reasoning": {"effort": "high"},
            "stream": true
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 4096);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 4096);
    assert_eq!(stored["headers"]["session_id"], "session-1");
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(stored["body"]["reasoning"]["effort"], "high");
    assert_eq!(stored["body"]["input"][1]["type"], "future_item");
    assert_eq!(stored["body"]["input"][1]["id"], "unknown-1");
    assert_eq!(stored["body"]["input"][1]["custom"]["keep"], true);
    assert!(
        stored["body"]["input"][0]["content"]
            .as_str()
            .expect("string content")
            .contains("gateway truncated")
    );
    assert!(
        stored["body"]["input"][1]["content"][0]["text"]
            .as_str()
            .expect("array text")
            .contains("gateway truncated")
    );
    assert!(
        stored["truncation"]["truncated_field_count"]
            .as_u64()
            .is_some_and(|count| count >= 2)
    );
}

#[test]
fn bounds_chat_message_content_and_preserves_message_shape() {
    let payload = json!({
        "headers": {"x-client-request-id": "chat-session"},
        "body": {
            "model": "gpt-test",
            "messages": [
                {"role": "system", "name": "policy", "content": "rules ".repeat(2000)},
                {"role": "user", "content": [
                    {"type": "text", "text": "prompt ".repeat(2000)},
                    {"type": "image_url", "image_url": {"url": "https://example.test/image.png"}}
                ]}
            ]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 4096);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 4096);
    assert_eq!(stored["body"]["messages"][0]["role"], "system");
    assert_eq!(stored["body"]["messages"][0]["name"], "policy");
    assert_eq!(stored["body"]["messages"][1]["role"], "user");
    assert_eq!(stored["body"]["messages"][1]["content"][0]["type"], "text");
    assert_eq!(
        stored["body"]["messages"][1]["content"][1]["image_url"]["url"],
        "https://example.test/image.png"
    );
    assert!(
        stored["body"]["messages"][0]["content"]
            .as_str()
            .expect("system content")
            .contains("gateway truncated")
    );
    assert!(
        stored["body"]["messages"][1]["content"][0]["text"]
            .as_str()
            .expect("user text")
            .contains("gateway truncated")
    );
}

#[test]
fn compacts_tool_envelope_before_hard_fallback() {
    let payload = json!({
        "headers": {"session_id": "session-1"},
        "body": {
            "model": "gpt-test",
            "input": [{"role": "user", "content": "hello"}],
            "tools": [{
                "type": "function",
                "name": "search",
                "description": "description ".repeat(1000),
                "parameters": {
                    "type": "object",
                    "properties": {"query": {"type": "string", "description": "query ".repeat(1000), "examples": ["large ".repeat(1000)]}},
                    "default": {"query": "large ".repeat(1000)}
                }
            }]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 3072);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 3072);
    assert_eq!(stored["body"]["tools"][0]["name"], "search");
    assert_eq!(stored["body"]["tools"][0]["parameters"]["type"], "object");
    assert!(
        stored["truncation"]["tool_fields_compacted"]
            .as_u64()
            .is_some_and(|count| count >= 3)
    );
}
