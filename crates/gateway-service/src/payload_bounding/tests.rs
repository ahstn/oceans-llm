use serde_json::{Value, json};

use super::{
    MAX_ORDINARY_MESSAGE_BYTES, MAX_SOLITARY_MESSAGE_BYTES, MAX_TOTAL_CONTENT_BYTES,
    bound_request_payload, bound_request_payload_after_known_fields, serialized_size,
};
use crate::redaction::MAX_INLINE_REQUEST_BYTES;

#[test]
fn under_cap_payload_is_unchanged() {
    let payload = json!({"headers": {"x": "y"}, "body": {"input": "hello"}});

    let (stored, truncated) = bound_request_payload(payload.clone(), 1024);

    assert!(!truncated);
    assert_eq!(stored, payload);
}

#[test]
fn bounds_string_responses_input_without_discarding_the_envelope() {
    let payload = json!({
        "headers": {"session_id": "session-string-input"},
        "body": {
            "model": "gpt-test",
            "input": "prompt ".repeat(4000),
            "reasoning": {"effort": "high"}
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 4096);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 4096);
    assert_eq!(stored["headers"]["session_id"], "session-string-input");
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(stored["body"]["reasoning"]["effort"], "high");
    assert!(
        stored["body"]["input"]
            .as_str()
            .expect("string input")
            .contains("gateway truncated")
    );
    assert_eq!(stored["truncation"]["affected_paths"][0], "/body/input");
}

#[test]
fn bounds_responses_instructions_without_discarding_the_envelope() {
    let payload = json!({
        "headers": {"session_id": "session-instructions"},
        "body": {
            "model": "gpt-test",
            "instructions": "system prompt 🙂 ".repeat(3000),
            "input": "short user input",
            "reasoning": {"effort": "high"}
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 4096);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 4096);
    assert_eq!(stored["headers"]["session_id"], "session-instructions");
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(stored["body"]["reasoning"]["effort"], "high");
    assert_eq!(stored["body"]["input"], "short user input");
    assert!(
        stored["body"]["instructions"]
            .as_str()
            .expect("instructions")
            .contains("gateway truncated")
    );
    assert!(
        stored["truncation"]["affected_paths"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(|path| path == "/body/instructions"))
    );
}

#[test]
fn bounds_function_call_output_without_discarding_the_envelope() {
    let payload = json!({
        "headers": {"session_id": "session-function-output"},
        "body": {
            "model": "gpt-test",
            "input": [
                {"type": "message", "role": "user", "content": "short user input"},
                {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": "tool result 🙂 ".repeat(3000)
                }
            ],
            "reasoning": {"effort": "high"}
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 4096);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 4096);
    assert_eq!(stored["headers"]["session_id"], "session-function-output");
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(stored["body"]["reasoning"]["effort"], "high");
    assert_eq!(stored["body"]["input"][1]["type"], "function_call_output");
    assert_eq!(stored["body"]["input"][1]["call_id"], "call-1");
    assert!(
        stored["body"]["input"][1]["output"]
            .as_str()
            .expect("function output")
            .contains("gateway truncated")
    );
    assert!(
        stored["truncation"]["affected_paths"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(|path| path == "/body/input/1/output"))
    );
}

#[test]
fn bounds_embeddings_input_strings_without_discarding_the_envelope() {
    let payload = json!({
        "headers": {"x-client-request-id": "embedding-request"},
        "body": {
            "model": "embedding-test",
            "input": ["short input", "embedding input 🙂 ".repeat(3000)],
            "encoding_format": "float"
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 4096);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 4096);
    assert_eq!(
        stored["headers"]["x-client-request-id"],
        "embedding-request"
    );
    assert_eq!(stored["body"]["model"], "embedding-test");
    assert_eq!(stored["body"]["encoding_format"], "float");
    assert_eq!(stored["body"]["input"][0], "short input");
    assert!(
        stored["body"]["input"][1]
            .as_str()
            .expect("embedding input")
            .contains("gateway truncated")
    );
    assert!(
        stored["truncation"]["affected_paths"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(|path| path == "/body/input/1"))
    );
}

#[test]
fn embedding_array_uses_hard_fallback_when_empty_structure_cannot_fit() {
    let max_bytes = 4096;
    let payload = json!({
        "headers": {"x-client-request-id": "high-cardinality-embedding"},
        "body": {
            "model": "embedding-test",
            "input": vec!["x"; max_bytes / 3 + 1]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, max_bytes);

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= max_bytes);
    assert_eq!(stored["truncated"], true);
    assert!(stored.get("preview").is_some());
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
    assert_eq!(stored["truncation"]["affected_path_count"], 0);
    assert_eq!(stored["truncation"]["affected_paths"], json!([]));
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

#[test]
fn tool_compaction_preserves_schema_properties_named_like_keywords() {
    let payload = json!({
        "headers": {"session_id": "session-schema-properties"},
        "body": {
            "model": "gpt-test",
            "input": [{"role": "user", "content": "hello"}],
            "tools": [{
                "type": "function",
                "name": "configure",
                "description": "description ".repeat(2000),
                "parameters": {
                    "type": "object",
                    "$defs": {
                        "default": {
                            "type": "string",
                            "description": "definition ".repeat(1000),
                            "default": "definition value ".repeat(1000)
                        }
                    },
                    "properties": {
                        "default": {
                            "$ref": "#/$defs/default",
                            "type": "string",
                            "description": "default property ".repeat(1000),
                            "default": "value ".repeat(1000)
                        },
                        "example": {
                            "type": "string",
                            "examples": ["example value ".repeat(1000)]
                        },
                        "examples": {
                            "type": "array",
                            "items": {"type": "string"},
                            "example": ["item ".repeat(1000)]
                        }
                    }
                }
            }]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 8192);
    let properties = &stored["body"]["tools"][0]["parameters"]["properties"];

    assert!(truncated);
    assert!(serialized_size(&stored).expect("stored size") <= 8192);
    assert_eq!(properties["default"]["type"], "string");
    assert_eq!(properties["default"]["$ref"], "#/$defs/default");
    assert_eq!(properties["example"]["type"], "string");
    assert_eq!(properties["examples"]["type"], "array");
    assert_eq!(properties["examples"]["items"]["type"], "string");
    assert_eq!(
        stored["body"]["tools"][0]["parameters"]["$defs"]["default"]["type"],
        "string"
    );
    assert_eq!(
        stored["body"]["tools"][0]["parameters"]["$defs"]["default"]["default"],
        "[omitted by gateway storage bound]"
    );
    assert_eq!(
        properties["default"]["default"],
        "[omitted by gateway storage bound]"
    );
    assert_eq!(
        properties["example"]["examples"],
        "[omitted by gateway storage bound]"
    );
    assert_eq!(
        properties["examples"]["example"],
        "[omitted by gateway storage bound]"
    );
}

#[test]
fn absolute_inline_ceiling_applies_above_configured_limit() {
    let payload = json!({
        "headers": {"session_id": "session-absolute"},
        "body": {
            "model": "gpt-test",
            "input": [{"type": "message", "role": "user", "content": "x".repeat(400 * 1024)}]
        }
    });
    let original_size = serialized_size(&payload).expect("original size");

    let (stored, truncated) = bound_request_payload(payload, 512 * 1024);
    let stored_size = serialized_size(&stored).expect("stored size");

    assert!(truncated);
    assert!(stored_size <= MAX_INLINE_REQUEST_BYTES);
    assert_eq!(stored["headers"]["session_id"], "session-absolute");
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(
        stored["truncation"]["strategy_version"],
        "structured-request-v2"
    );
    assert_eq!(stored["truncation"]["original_size_bytes"], original_size);
    assert_eq!(stored["truncation"]["stored_size_bytes"], stored_size);
    assert_eq!(
        stored["truncation"]["omitted_bytes"],
        original_size - stored_size
    );
    assert!(
        stored["truncation"]["truncated_field_count"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(
        stored["truncation"]["affected_paths"]
            .as_array()
            .is_some_and(|paths| !paths.is_empty())
    );
}

#[test]
fn solitary_message_can_use_the_larger_content_budget() {
    let payload = json!({
        "headers": {"session_id": "session-solitary"},
        "body": {
            "model": "gpt-test",
            "input": [{"type": "message", "role": "user", "content": "x".repeat(200 * 1024)}]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 128 * 1024);
    let retained = stored["body"]["input"][0]["content"]
        .as_str()
        .expect("retained content");

    assert!(truncated);
    assert!(retained.contains("gateway truncated"));
    assert!(retained.len() > MAX_ORDINARY_MESSAGE_BYTES);
    assert!(retained.len() <= MAX_SOLITARY_MESSAGE_BYTES + 64);
    assert!(serialized_size(&stored).expect("stored size") <= 128 * 1024);
}

#[test]
fn many_messages_share_the_total_content_budget() {
    let input = (0..12)
        .map(|index| {
            json!({
                "type": "message",
                "id": format!("item-{index}"),
                "role": "user",
                "content": "x".repeat(32 * 1024),
            })
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "headers": {"session_id": "session-many"},
        "body": {"model": "gpt-test", "input": input}
    });

    let (stored, truncated) = bound_request_payload(payload, 128 * 1024);
    let retained_content_bytes = stored["body"]["input"]
        .as_array()
        .expect("input array")
        .iter()
        .map(|item| item["content"].as_str().expect("message content").len())
        .sum::<usize>();

    assert!(truncated);
    assert!(retained_content_bytes <= MAX_TOTAL_CONTENT_BYTES + (12 * 64));
    assert!(
        stored["body"]["input"]
            .as_array()
            .expect("input array")
            .iter()
            .all(|item| item["id"].as_str().is_some())
    );
    assert!(serialized_size(&stored).expect("stored size") <= 128 * 1024);
}

#[test]
fn important_message_can_use_the_larger_content_budget() {
    let payload = json!({
        "headers": {"session_id": "session-important"},
        "body": {
            "model": "gpt-test",
            "messages": [
                {"role": "system", "content": "s".repeat(160 * 1024)},
                {"role": "user", "content": "u".repeat(160 * 1024)},
                {"role": "assistant", "content": "a".repeat(160 * 1024)}
            ]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 128 * 1024);
    let messages = stored["body"]["messages"]
        .as_array()
        .expect("message array");
    let system_bytes = messages[0]["content"]
        .as_str()
        .expect("system content")
        .len();
    let user_bytes = messages[1]["content"].as_str().expect("user content").len();

    assert!(truncated);
    assert!(system_bytes > MAX_ORDINARY_MESSAGE_BYTES);
    assert!(system_bytes <= MAX_SOLITARY_MESSAGE_BYTES + 64);
    assert!(user_bytes <= MAX_ORDINARY_MESSAGE_BYTES + 64);
    assert!(serialized_size(&stored).expect("stored size") <= 128 * 1024);
}

#[test]
fn oversized_envelope_compacts_verbose_tool_fields_before_content() {
    let payload = json!({
        "headers": {
            "session_id": "session-envelope",
            "x-client-request-id": "lineage-envelope"
        },
        "body": {
            "model": "gpt-test",
            "reasoning": {"effort": "high"},
            "tool_choice": "auto",
            "stream": true,
            "include": ["reasoning.encrypted_content"],
            "input": [{"type": "message", "id": "item-1", "role": "user", "content": "prompt".repeat(20 * 1024)}],
            "tools": [{
                "type": "function",
                "name": "search",
                "description": "description ".repeat(2000),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "query description ".repeat(1000),
                            "examples": ["example ".repeat(1000)]
                        }
                    },
                    "required": ["query"]
                }
            }]
        }
    });

    let (stored, truncated) = bound_request_payload(payload, 128 * 1024);

    assert!(truncated);
    assert_eq!(stored["headers"]["session_id"], "session-envelope");
    assert_eq!(stored["headers"]["x-client-request-id"], "lineage-envelope");
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(stored["body"]["reasoning"]["effort"], "high");
    assert_eq!(stored["body"]["tools"][0]["name"], "search");
    assert_eq!(
        stored["body"]["tools"][0]["parameters"]["properties"]["query"]["type"],
        "string"
    );
    assert!(
        stored["truncation"]["tool_fields_compacted"]
            .as_u64()
            .is_some_and(|count| count >= 3)
    );
    assert!(serialized_size(&stored).expect("stored size") <= 128 * 1024);
}

#[test]
fn hard_fallback_is_reserved_for_an_envelope_that_cannot_fit() {
    let max_bytes = 32 * 1024;
    let payload = json!({
        "headers": {"session_id": "x🙂\"\\\n".repeat(12 * 1024)},
        "body": {
            "model": "gpt-test",
            "input": [{"type": "future_item", "id": "unknown", "metadata": "y".repeat(48 * 1024)}]
        }
    });
    let original_size = serialized_size(&payload).expect("original size");
    let expected = legacy_hard_fallback_marker(&payload, original_size, max_bytes);

    let (stored, truncated) = bound_request_payload(payload, max_bytes);

    assert!(truncated);
    assert_eq!(stored, expected);
    assert!(serialized_size(&stored).expect("stored size") <= max_bytes);
}

fn legacy_hard_fallback_marker(value: &Value, original_size: usize, max_bytes: usize) -> Value {
    let bytes = serde_json::to_vec(value).expect("serialize legacy fallback input");
    let mut preview_bytes = max_bytes.min(bytes.len());
    loop {
        let marker = json!({
            "truncated": true,
            "size_bytes": original_size,
            "preview": String::from_utf8_lossy(&bytes[..preview_bytes]),
        });
        if serialized_size(&marker).is_ok_and(|size| size <= max_bytes) {
            return marker;
        }
        preview_bytes /= 2;
    }
}
