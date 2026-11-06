use super::*;

fn normalize(response: &Value) -> Value {
    normalize_google_response(response, &context("google/gemini-3.7-flash")).expect("normalized")
}

#[test]
fn normalizes_google_response_into_openai_shape() {
    let response = json!({
        "responseId": "resp-123",
        "candidates": [{
            "content": { "role": "model", "parts": [{ "text": "hel" }, { "text": "lo" }] },
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 5,
            "totalTokenCount": 15
        }
    });
    let normalized = normalize_google_response(&response, &context("google/gemini-2.0-flash"))
        .expect("normalized");

    assert_eq!(normalized["id"], "resp-123");
    assert_eq!(normalized["object"], "chat.completion");
    assert_eq!(normalized["model"], "fast");
    assert_eq!(normalized["choices"][0]["index"], 0);
    assert_eq!(normalized["choices"][0]["message"]["role"], "assistant");
    assert_eq!(normalized["choices"][0]["message"]["content"], "hello");
    assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
    assert!(
        normalized["choices"][0]["message"]
            .get("reasoning_content")
            .is_none()
    );
    assert!(
        normalized["choices"][0]["message"]
            .get("tool_calls")
            .is_none()
    );
    assert_eq!(normalized["usage"]["prompt_tokens"], 10);
    assert_eq!(normalized["usage"]["completion_tokens"], 5);
    assert_eq!(normalized["usage"]["total_tokens"], 15);
    assert_eq!(normalized["usage"]["usage_source"], "vertex_google");
    assert_eq!(normalized["usage"]["provider_usage"]["totalTokenCount"], 15);
    assert!(
        normalized["usage"]
            .get("completion_tokens_details")
            .is_none()
    );
    assert!(normalized["usage"].get("prompt_tokens_details").is_none());
}

#[test]
fn thought_parts_become_reasoning_content_and_never_leak_into_content() {
    let response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "text": "Let me think", "thought": true },
                    { "text": " harder.", "thought": true },
                    { "text": "The answer is 4." }
                ]
            },
            "finishReason": "STOP"
        }]
    });
    let message = &normalize(&response)["choices"][0]["message"];

    assert_eq!(message["reasoning_content"], "Let me think harder.");
    assert_eq!(message["content"], "The answer is 4.");
}

#[test]
fn text_part_thought_signature_is_surfaced_on_message_and_provider_metadata() {
    let response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{ "text": "hello", "thoughtSignature": "sig-text" }]
            },
            "finishReason": "STOP"
        }]
    });
    let message = &normalize(&response)["choices"][0]["message"];

    assert_eq!(message["content"], "hello");
    assert_eq!(message["thought_signature"], "sig-text");
    assert_eq!(
        message["provider_metadata"]["gcp_vertex"]["thought_signature"],
        "sig-text"
    );
    assert!(message.get("tool_calls").is_none());
}

#[test]
fn empty_text_part_thought_signature_is_ignored() {
    let response = json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "text": "hello", "thoughtSignature": "" }] },
            "finishReason": "STOP"
        }]
    });
    let message = &normalize(&response)["choices"][0]["message"];

    assert!(message.get("thought_signature").is_none());
}

#[test]
fn function_call_thought_signature_rides_on_the_tool_call() {
    let response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": { "id": "fc-1", "name": "weather", "args": { "city": "London" } },
                    "thoughtSignature": "sig-call"
                }]
            },
            "finishReason": "STOP"
        }]
    });
    let normalized = normalize(&response);
    let message = &normalized["choices"][0]["message"];

    assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(message["content"], Value::Null);
    assert!(message.get("thought_signature").is_none());
    let tool_calls = message["tool_calls"].as_array().expect("tool_calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"], "fc-1");
    assert_eq!(tool_calls[0]["type"], "function");
    assert_eq!(tool_calls[0]["function"]["name"], "weather");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        r#"{"city":"London"}"#
    );
    assert_eq!(tool_calls[0]["thought_signature"], "sig-call");
}

#[test]
fn function_call_without_id_gets_a_generated_call_id() {
    let response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{ "functionCall": { "name": "weather" } }]
            },
            "finishReason": "STOP"
        }]
    });
    let tool_call = &normalize(&response)["choices"][0]["message"]["tool_calls"][0];

    let id = tool_call["id"].as_str().expect("call id");
    assert!(id.starts_with("call_"), "unexpected id `{id}`");
    assert_eq!(tool_call["function"]["arguments"], "{}");
    assert!(tool_call.get("thought_signature").is_none());
}

#[test]
fn usage_adds_thought_tokens_to_completion_tokens_and_reports_details() {
    let response = json!({
        "candidates": [{ "content": { "role": "model", "parts": [{ "text": "ok" }] }, "finishReason": "STOP" }],
        "usageMetadata": {
            "promptTokenCount": 100,
            "cachedContentTokenCount": 40,
            "candidatesTokenCount": 7,
            "thoughtsTokenCount": 30,
            "totalTokenCount": 137
        }
    });
    let usage = &normalize(&response)["usage"];

    assert_eq!(usage["prompt_tokens"], 100);
    assert_eq!(usage["completion_tokens"], 37);
    assert_eq!(usage["total_tokens"], 137);
    assert_eq!(usage["completion_tokens_details"]["reasoning_tokens"], 30);
    assert_eq!(usage["prompt_tokens_details"]["cached_tokens"], 40);
    assert_eq!(usage["usage_source"], "vertex_google");
    assert_eq!(usage["provider_usage"]["thoughtsTokenCount"], 30);
    assert_eq!(usage["provider_usage"]["cachedContentTokenCount"], 40);
}

#[test]
fn usage_with_only_thought_tokens_counts_them_as_completion_tokens() {
    let response = json!({
        "usageMetadata": { "promptTokenCount": 3, "thoughtsTokenCount": 12 }
    });
    let usage = map_google_usage(&response).expect("usage metadata");

    assert_eq!(usage["completion_tokens"], 12);
    assert_eq!(usage["completion_tokens_details"]["reasoning_tokens"], 12);
}

#[test]
fn google_usage_preserves_malformed_optional_fields_for_normalization() {
    let response = json!({
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 3,
            "totalTokenCount": "13",
            "cachedContentTokenCount": "not-a-number"
        }
    });
    let normalized = map_google_usage(&response).expect("usage metadata");

    assert_eq!(normalized["prompt_tokens"], 10);
    assert_eq!(normalized["completion_tokens"], 3);
    assert!(normalized.get("total_tokens").is_none());
    assert!(normalized.get("prompt_tokens_details").is_none());
    assert_eq!(normalized["provider_usage"]["totalTokenCount"], "13");
    assert_eq!(
        normalized["provider_usage"]["cachedContentTokenCount"],
        "not-a-number"
    );
}

#[test]
fn response_without_usage_metadata_omits_usage() {
    let response = json!({
        "candidates": [{ "content": { "role": "model", "parts": [{ "text": "ok" }] }, "finishReason": "STOP" }]
    });

    assert!(normalize(&response).get("usage").is_none());
}

#[test]
fn maps_finish_reasons_to_openai_vocabulary() {
    let cases = [
        ("STOP", "stop"),
        ("MAX_TOKENS", "length"),
        ("SAFETY", "content_filter"),
        ("RECITATION", "content_filter"),
        ("LANGUAGE", "content_filter"),
        ("BLOCKLIST", "content_filter"),
        ("PROHIBITED_CONTENT", "content_filter"),
        ("SPII", "content_filter"),
        ("MODEL_ARMOR", "content_filter"),
    ];
    for (upstream, expected) in cases {
        let response = json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "x" }] },
                "finishReason": upstream
            }]
        });
        assert_eq!(
            normalize(&response)["choices"][0]["finish_reason"],
            expected,
            "finishReason {upstream}"
        );
    }
}

#[test]
fn malformed_function_call_is_an_error() {
    let response = json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{ "text": "" }] },
            "finishReason": "MALFORMED_FUNCTION_CALL",
            "finishMessage": "could not parse call"
        }]
    });
    let err = normalize_google_response(&response, &context("google/gemini-3.7-flash"))
        .expect_err("malformed function call must fail");

    assert!(matches!(err, VertexAdapterError::MalformedFunctionCall(_)));
    assert!(err.to_string().contains("could not parse call"));
    let provider_err: ProviderError = err.into();
    assert!(matches!(provider_err, ProviderError::Transport(_)));
}

#[test]
fn blocked_prompt_without_candidates_yields_content_filter_choice() {
    let response = json!({
        "promptFeedback": { "blockReason": "PROHIBITED_CONTENT" },
        "usageMetadata": { "promptTokenCount": 9, "totalTokenCount": 9 }
    });
    let normalized = normalize(&response);

    let choices = normalized["choices"].as_array().expect("choices");
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0]["finish_reason"], "content_filter");
    assert_eq!(choices[0]["message"]["content"], Value::Null);
    assert_eq!(normalized["usage"]["prompt_tokens"], 9);
}

#[test]
fn inline_error_object_maps_to_transport_error() {
    let response = json!({
        "error": { "code": 429, "status": "RESOURCE_EXHAUSTED", "message": "quota exceeded" }
    });
    let err = normalize_google_response(&response, &context("google/gemini-3.7-flash"))
        .expect_err("inline error must fail");

    assert!(matches!(err, VertexAdapterError::StreamError(_)));
    let rendered = err.to_string();
    assert!(rendered.contains("RESOURCE_EXHAUSTED"));
    assert!(rendered.contains("429"));
    assert!(rendered.contains("quota exceeded"));
    let provider_err: ProviderError = err.into();
    assert!(matches!(provider_err, ProviderError::Transport(_)));
}

#[test]
fn candidate_index_is_taken_from_upstream_when_present() {
    let response = json!({
        "candidates": [
            { "index": 1, "content": { "role": "model", "parts": [{ "text": "b" }] }, "finishReason": "STOP" },
            { "index": 0, "content": { "role": "model", "parts": [{ "text": "a" }] }, "finishReason": "STOP" }
        ]
    });
    let normalized = normalize(&response);

    assert_eq!(normalized["choices"][0]["index"], 1);
    assert_eq!(normalized["choices"][0]["message"]["content"], "b");
    assert_eq!(normalized["choices"][1]["index"], 0);
    assert_eq!(normalized["choices"][1]["message"]["content"], "a");
}

#[test]
fn incomplete_google_envelopes_are_upstream_failures() {
    for response in [
        json!({}),
        json!({ "candidates": [] }),
        json!({ "usageMetadata": { "promptTokenCount": 5 } }),
        json!({ "candidates": [{ "content": { "parts": [{ "text": "partial" }] } }] }),
        json!({ "candidates": [{ "finishReason": null }] }),
        json!({ "candidates": [{ "finishReason": 42 }] }),
        json!({ "promptFeedback": { "blockReason": "BLOCK_REASON_UNSPECIFIED" } }),
    ] {
        let error = normalize_google_response(&response, &context("google/gemini-2.5-flash"))
            .expect_err("incomplete output is not a successful completion");
        assert!(
            matches!(error, VertexAdapterError::IncompleteResponse(_)),
            "{response}"
        );
        assert!(matches!(
            ProviderError::from(error),
            ProviderError::Transport(_)
        ));
    }
}

#[test]
fn truncation_and_filter_terminals_are_not_overwritten_by_tool_calls() {
    for (reason, expected) in [
        ("MAX_TOKENS", "length"),
        ("SAFETY", "content_filter"),
        ("MODEL_ARMOR", "content_filter"),
        ("IMAGE_SAFETY", "content_filter"),
    ] {
        let response = json!({ "candidates": [{
            "content": { "parts": [{ "functionCall": { "name": "lookup", "args": {} } }] },
            "finishReason": reason
        }] });
        let normalized = normalize(&response);
        assert_eq!(
            normalized["choices"][0]["finish_reason"], expected,
            "{reason}"
        );
        assert_eq!(
            normalized["choices"][0]["message"]["provider_metadata"]["gcp_vertex"]["finish_reason"],
            reason
        );
        assert_eq!(
            normalized["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
    }
}

#[test]
fn explicit_failure_and_unknown_finish_reasons_never_become_successful_calls() {
    for reason in [
        "UNEXPECTED_TOOL_CALL",
        "OTHER",
        "FINISH_REASON_UNSPECIFIED",
        "FUTURE_FAILURE",
        "NO_IMAGE",
    ] {
        let response = json!({ "candidates": [{
            "content": { "parts": [{ "functionCall": { "name": "lookup", "args": {} } }] },
            "finishReason": reason
        }] });
        let error = normalize_google_response(&response, &context("google/gemini-2.5-flash"))
            .expect_err("provider failure must not turn into tool_calls");
        assert!(matches!(
            error,
            VertexAdapterError::UnsuccessfulFinishReason(_)
        ));
        assert!(error.to_string().contains(reason));
        assert!(matches!(
            ProviderError::from(error),
            ProviderError::Transport(_)
        ));
    }
}

#[test]
fn returned_logprobs_keep_chosen_tokens_separate_from_top_candidates() {
    let response = json!({ "candidates": [{
        "content": { "parts": [{ "text": "cat!" }] },
        "finishReason": "STOP",
        "logprobsResult": {
            "chosenCandidates": [
                { "token": "cat", "tokenId": 10, "logProbability": -1.5 },
                { "token": "!", "tokenId": 12, "logProbability": -0.25 }
            ],
            "topCandidates": [
                { "candidates": [
                    { "token": "dog", "tokenId": 11, "logProbability": -0.5 },
                    { "token": "cat", "tokenId": 10, "logProbability": -1.5 }
                ] },
                { "candidates": [] }
            ]
        }
    }] });
    let normalized = normalize(&response);
    let content = &normalized["choices"][0]["logprobs"]["content"];
    assert_eq!(content[0]["token"], "cat");
    assert_eq!(content[0]["logprob"], -1.5);
    assert_eq!(content[0]["top_logprobs"][0]["token"], "dog");
    assert_eq!(content[0]["top_logprobs"][0]["logprob"], -0.5);
    assert_eq!(content[1]["token"], "!");
    assert_eq!(content[1]["logprob"], -0.25);
    assert_eq!(content[1]["top_logprobs"], json!([]));
}
