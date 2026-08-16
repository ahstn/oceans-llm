use super::*;

#[test]
fn normalizes_google_response_into_openai_shape() {
    let response = json!({
        "responseId": "resp-123",
        "candidates":[
            {"index":0, "content":{"parts":[{"text":"hello"}]}, "finishReason":"STOP"}
        ],
        "usageMetadata":{
            "promptTokenCount":10,
            "cachedContentTokenCount":4,
            "candidatesTokenCount":3,
            "thoughtsTokenCount":2,
            "totalTokenCount":15
        }
    });
    let normalized = normalize_google_response(&response, &context("google/gemini-2.0-flash"));
    assert_eq!(normalized["object"], "chat.completion");
    assert_eq!(normalized["choices"][0]["message"]["content"], "hello");
    assert_eq!(normalized["usage"]["total_tokens"], 15);
    assert_eq!(normalized["usage"]["usage_source"], "vertex_google");
    assert_eq!(
        normalized["usage"]["provider_usage"]["cachedContentTokenCount"],
        4
    );
    assert_eq!(
        normalized["usage"]["provider_usage"]["thoughtsTokenCount"],
        2
    );
}

#[test]
fn google_usage_preserves_malformed_optional_fields_for_normalization() {
    let response = json!({
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 3,
            "cachedContentTokenCount": "unknown"
        }
    });
    let normalized = map_google_usage(&response).expect("usage metadata");

    assert_eq!(normalized["prompt_tokens"], 10);
    assert_eq!(normalized["completion_tokens"], 3);
    assert!(normalized.get("total_tokens").is_none());
    assert_eq!(
        normalized["provider_usage"]["cachedContentTokenCount"],
        "unknown"
    );
}

#[test]
fn normalizes_anthropic_response_into_openai_shape() {
    let response = json!({
        "id":"msg_123",
        "content":[{"type":"text","text":"hello"}],
        "stop_reason":"end_turn",
        "usage":{
            "input_tokens":5,
            "output_tokens":7,
            "cache_read_input_tokens":3,
            "cache_creation_input_tokens":2
        }
    });
    let normalized =
        normalize_anthropic_response(&response, &context("anthropic/claude-sonnet-4-6"));
    assert_eq!(normalized["choices"][0]["message"]["content"], "hello");
    assert_eq!(normalized["usage"]["prompt_tokens"], 5);
    assert_eq!(normalized["usage"]["completion_tokens"], 7);
    assert_eq!(normalized["usage"]["total_tokens"], 17);
    assert_eq!(normalized["usage"]["usage_source"], "vertex_anthropic");
    assert_eq!(
        normalized["usage"]["provider_usage"]["cache_read_input_tokens"],
        3
    );
    assert_eq!(
        normalized["usage"]["provider_usage"]["cache_creation_input_tokens"],
        2
    );
}

#[test]
fn normalizes_anthropic_tool_use_response_into_openai_tool_calls() {
    let response = json!({
        "id":"msg_123",
        "content":[{
            "type":"tool_use",
            "id":"toolu_123",
            "name":"lookup",
            "input":{"city":"London"}
        }],
        "stop_reason":"tool_use",
        "usage":{"input_tokens":5,"output_tokens":7}
    });
    let normalized =
        normalize_anthropic_response(&response, &context("anthropic/claude-sonnet-4-6"));

    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        normalized["choices"][0]["message"]["tool_calls"][0],
        json!({
            "id": "toolu_123",
            "type": "function",
            "function": {
                "name": "lookup",
                "arguments": "{\"city\":\"London\"}"
            }
        })
    );
}

#[test]
fn normalizes_anthropic_thinking_metadata_without_leaking_into_content() {
    let response = json!({
        "id":"msg_123",
        "content":[
            {"type":"thinking","thinking":"summarized hidden reasoning","signature":"sig-thinking"},
            {"type":"redacted_thinking","data":"encrypted-redacted"},
            {"type":"text","text":"visible answer"}
        ],
        "stop_reason":"end_turn",
        "usage":{"input_tokens":5,"output_tokens":7}
    });
    let normalized = normalize_anthropic_response(&response, &context("anthropic/claude-opus-4-7"));
    let message = &normalized["choices"][0]["message"];

    assert_eq!(message["content"], "visible answer");
    assert_eq!(
        message["provider_metadata"]["gcp_vertex"]["reasoning"]["source"],
        "anthropic_messages"
    );
    assert_eq!(
        message["provider_metadata"]["gcp_vertex"]["reasoning"]["blocks"],
        json!([
            {"type":"thinking","thinking":"summarized hidden reasoning","signature":"sig-thinking"},
            {"type":"redacted_thinking","data":"encrypted-redacted"}
        ])
    );
}
