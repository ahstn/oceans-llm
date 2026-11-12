//! Token estimates for a successful request the provider reported no usage for: a token
//! per four bytes of request body, and of generated response text.

use serde_json::Value;

use crate::payload_bounding::serialized_size;

/// The sizes that estimate a request's tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageEstimateInput {
    /// The request body's size in bytes. Without it, nothing is estimated.
    pub request_body_bytes: Option<u64>,
    /// The generated text's size in bytes, from [`response_text_bytes`] or the stream.
    pub response_text_bytes: Option<u64>,
}

impl UsageEstimateInput {
    /// Estimated prompt and completion tokens; `None` without the request size.
    pub(crate) fn tokens(self) -> Option<(i64, i64)> {
        let prompt_tokens = estimated_tokens(self.request_body_bytes?);
        let completion_tokens = self.response_text_bytes.map_or(0, estimated_tokens);
        Some((prompt_tokens, completion_tokens))
    }
}

fn estimated_tokens(bytes: u64) -> i64 {
    i64::try_from(bytes.div_ceil(4)).unwrap_or(i64::MAX)
}

/// Keys whose string values are generated text in Chat Completions, Responses and
/// Messages output.
const TEXT_KEYS: [&str; 9] = [
    "content",
    "text",
    "refusal",
    "reasoning",
    "reasoning_content",
    "thinking",
    "arguments",
    "partial_json",
    "input",
];

/// Keys whose arrays or objects hold generated text.
const CONTAINER_KEYS: [&str; 8] = [
    "choices",
    "message",
    "delta",
    "content",
    "tool_calls",
    "function",
    "output",
    "summary",
];

/// Bytes of generated text in a non-streaming Chat Completions, Responses or Messages
/// response body: message text, reasoning, refusals and tool call arguments.
#[must_use]
pub fn response_text_bytes(body: &Value) -> u64 {
    ["choices", "output", "content"]
        .into_iter()
        .filter_map(|key| body.get(key))
        .map(text_bytes)
        .sum()
}

/// Bytes of generated text in one streamed event: a Chat Completions chunk, a Responses
/// `*.delta` event or a Messages `content_block_delta`.
pub(crate) fn stream_event_text_bytes(event: &Value) -> u64 {
    match event.get("delta") {
        Some(Value::String(delta)) => {
            let audio = event.get("type").and_then(Value::as_str) == Some("response.audio.delta");
            if audio { 0 } else { delta.len() as u64 }
        }
        Some(delta @ Value::Object(_)) => text_bytes(delta),
        _ => event.get("choices").map_or(0, text_bytes),
    }
}

fn text_bytes(value: &Value) -> u64 {
    match value {
        Value::Array(items) => items.iter().map(text_bytes).sum(),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| field_text_bytes(key, value))
            .sum(),
        _ => 0,
    }
}

fn field_text_bytes(key: &str, value: &Value) -> u64 {
    match value {
        Value::String(text) if TEXT_KEYS.contains(&key) => text.len() as u64,
        // A Messages `tool_use` block's arguments are a JSON object.
        Value::Object(_) if key == "input" => serialized_size(value).map_or(0, |size| size as u64),
        Value::Array(_) | Value::Object(_) if CONTAINER_KEYS.contains(&key) => text_bytes(value),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tokens_round_each_size_up_to_a_token_per_four_bytes() {
        let estimate = UsageEstimateInput {
            request_body_bytes: Some(4_001),
            response_text_bytes: Some(7),
        };
        assert_eq!(estimate.tokens(), Some((1_001, 2)));

        let without_response = UsageEstimateInput {
            response_text_bytes: None,
            ..estimate
        };
        assert_eq!(without_response.tokens(), Some((1_001, 0)));
        assert_eq!(UsageEstimateInput::default().tokens(), None);
    }

    #[test]
    fn response_text_counts_generated_text_in_each_api_shape() {
        let chat = json!({
            "choices": [{"message": {
                "content": "hello",
                "reasoning_content": "abc",
                "tool_calls": [{"id": "call_1", "function": {"name": "f", "arguments": "{}"}}]
            }}],
            "usage": null
        });
        assert_eq!(response_text_bytes(&chat), 10);

        let responses = json!({
            "instructions": "not output",
            "output": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "why"}]},
                {"type": "message", "content": [{"type": "output_text", "text": "hi"}]},
                {"type": "function_call", "name": "f", "arguments": "{\"a\":1}"}
            ]
        });
        assert_eq!(response_text_bytes(&responses), 12);

        let messages = json!({
            "content": [
                {"type": "thinking", "thinking": "hmm", "signature": "opaque"},
                {"type": "text", "text": "done"},
                {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
            ]
        });
        assert_eq!(response_text_bytes(&messages), 14);
    }

    #[test]
    fn stream_events_count_only_their_deltas() {
        let chat = json!({"choices": [{"delta": {"content": "hey"}}]});
        let responses = json!({"type": "response.output_text.delta", "delta": "four"});
        let audio = json!({"type": "response.audio.delta", "delta": "AAAA"});
        let done = json!({"type": "response.output_text.done", "text": "four"});
        let messages = json!({
            "type": "content_block_delta",
            "delta": {"type": "input_json_delta", "partial_json": "{\"a\""}
        });

        assert_eq!(stream_event_text_bytes(&chat), 3);
        assert_eq!(stream_event_text_bytes(&responses), 4);
        assert_eq!(stream_event_text_bytes(&audio), 0);
        assert_eq!(stream_event_text_bytes(&done), 0);
        assert_eq!(stream_event_text_bytes(&messages), 4);
    }
}
