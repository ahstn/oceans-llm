//! Gemini thought signatures across a Chat Completions round trip: the shapes the gateway
//! returns, and the shapes Pi, OMP and OpenCode send back on the next request.

use super::*;

const GEMINI_4: &str = "google/gemini-4-hydrogen";
const SKIP: &str = "skip_thought_signature_validator";

/// Maps Chat messages, decoded exactly as the gateway decodes a request body, to Gemini contents.
fn map_contents(messages: Value, upstream_model: &str) -> Value {
    let request = chat_request(serde_json::from_value(messages).expect("chat messages"));
    google_body(&request, &context(upstream_model), false).expect("mapped")["contents"].clone()
}

fn function_call(id: &str, command: &str, signature: &str) -> Value {
    json!({
        "functionCall": { "id": id, "name": "bash", "args": { "command": command } },
        "thoughtSignature": signature
    })
}

fn bash_call(id: &str, command: &str) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": { "name": "bash", "arguments": json!({ "command": command }).to_string() }
    })
}

#[test]
fn extra_content_signature_replays_onto_its_own_function_call() {
    // Google's documented OpenAI-compatible parallel history: only the first call is signed.
    let contents = map_contents(
        json!([
            { "role": "user", "content": "Check the weather in Paris and London." },
            { "role": "assistant", "content": null, "tool_calls": [
                { "id": "fc-paris", "type": "function",
                  "function": { "name": "weather", "arguments": "{\"city\":\"Paris\"}" },
                  "extra_content": { "google": { "thought_signature": "sig-paris" } } },
                { "id": "fc-london", "type": "function",
                  "function": { "name": "weather", "arguments": "{\"city\":\"London\"}" } }
            ] },
            { "role": "tool", "tool_call_id": "fc-paris", "content": "{\"temp\":\"15C\"}" },
            { "role": "tool", "tool_call_id": "fc-london", "content": "{\"temp\":\"12C\"}" }
        ]),
        GEMINI_4,
    );

    let parts = &contents[1]["parts"];
    assert_eq!(parts[0]["thoughtSignature"], "sig-paris");
    assert_eq!(parts[0]["functionCall"]["id"], "fc-paris");
    assert!(
        parts[1].get("thoughtSignature").is_none(),
        "a signed step leaves its later parallel calls bare"
    );
}

#[test]
fn reasoning_details_signatures_replay_onto_the_calls_with_their_ids() {
    let contents = map_contents(
        json!([
            { "role": "user", "content": "List both directories." },
            { "role": "assistant", "content": null,
              "tool_calls": [bash_call("fc-1", "ls a"), bash_call("fc-2", "ls b")],
              "reasoning_details": [
                  { "type": "reasoning.text", "text": "thinking", "format": "google-gemini-v1" },
                  { "type": "reasoning.encrypted", "id": "fc-2", "data": "sig-2", "format": "google-gemini-v1" },
                  { "type": "reasoning.encrypted", "id": "fc-1", "data": "sig-1", "format": "google-gemini-v1", "index": 0 }
              ] },
            { "role": "tool", "tool_call_id": "fc-1", "content": "a.txt" },
            { "role": "tool", "tool_call_id": "fc-2", "content": "b.txt" }
        ]),
        GEMINI_4,
    );

    let parts = &contents[1]["parts"];
    assert_eq!(parts[0]["thoughtSignature"], "sig-1");
    assert_eq!(parts[1]["thoughtSignature"], "sig-2");
}

#[test]
fn foreign_reasoning_details_never_become_gemini_signatures() {
    let contents = map_contents(
        json!([
            { "role": "user", "content": "List files." },
            { "role": "assistant", "content": "Checking.",
              "tool_calls": [bash_call("fc-1", "ls")],
              "reasoning_details": [
                  { "type": "reasoning.encrypted", "id": "fc-1", "data": "claude", "format": "anthropic-claude-v1" },
                  { "type": "reasoning.encrypted", "data": "gAAAA-openai", "format": "openai-responses-v1" },
                  { "type": "reasoning.text", "id": "fc-1", "text": "plain", "format": "google-gemini-v1" },
                  { "type": "reasoning.encrypted", "id": "fc-other", "data": "sig-other", "format": "google-gemini-v1" },
                  { "type": "reasoning.encrypted", "id": "fc-1", "data": "", "format": "google-gemini-v1" },
                  { "type": "reasoning.encrypted", "id": 7, "data": "sig-numeric-id", "format": "google-gemini-v1" }
              ] },
            { "role": "tool", "tool_call_id": "fc-1", "content": "a.txt" }
        ]),
        GEMINI_4,
    );

    assert_eq!(
        contents[1]["parts"],
        json!([
            { "text": "Checking." },
            { "functionCall": { "id": "fc-1", "name": "bash", "args": { "command": "ls" } },
              "thoughtSignature": SKIP }
        ])
    );
}

#[test]
fn unbound_gemini_reasoning_detail_signs_the_text_part() {
    let contents = map_contents(
        json!([
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": "Hello.",
              "reasoning_details": [
                  { "type": "reasoning.encrypted", "data": "sig-text", "format": "google-gemini-v1" }
              ] },
            { "role": "user", "content": "again" }
        ]),
        GEMINI_4,
    );

    assert_eq!(
        contents[1]["parts"],
        json!([{ "text": "Hello.", "thoughtSignature": "sig-text" }])
    );
}

/// History from another provider: an earlier turn, then a current turn with two steps.
fn unsigned_history() -> Value {
    json!([
        { "role": "user", "content": "first task" },
        { "role": "assistant", "content": null, "tool_calls": [bash_call("old", "ls")] },
        { "role": "tool", "tool_call_id": "old", "content": "a.txt" },
        { "role": "assistant", "content": "Done." },
        { "role": "user", "content": "second task" },
        { "role": "assistant", "content": "Two at once.",
          "tool_calls": [bash_call("a", "ls a"), bash_call("b", "ls b")] },
        { "role": "tool", "tool_call_id": "a", "content": "a.txt" },
        { "role": "tool", "tool_call_id": "b", "content": "b.txt" },
        { "role": "assistant", "content": null, "tool_calls": [bash_call("c", "cat a.txt")] },
        { "role": "tool", "tool_call_id": "c", "content": "hello" }
    ])
}

#[test]
fn unsigned_current_turn_steps_get_the_documented_bypass_on_gemini_3_and_later() {
    for model in [
        GEMINI_4,
        "google/gemini-3.1-pro-preview",
        "google/gemini-3.7-flash",
    ] {
        let contents = map_contents(unsigned_history(), model);
        assert!(
            contents[1]["parts"][0].get("thoughtSignature").is_none(),
            "{model}: earlier turns aren't validated, so they stay unsigned"
        );
        let step_one = &contents[5]["parts"];
        assert_eq!(step_one[0], json!({ "text": "Two at once." }), "{model}");
        assert_eq!(step_one[1]["thoughtSignature"], SKIP, "{model}");
        assert!(
            step_one[2].get("thoughtSignature").is_none(),
            "{model}: only the first call of a step needs a signature"
        );
        assert_eq!(contents[7]["parts"][0]["thoughtSignature"], SKIP, "{model}");
    }
}

#[test]
fn models_that_do_not_validate_signatures_never_get_the_bypass() {
    for model in [
        "google/gemini-2.5-flash",
        "google/gemini-2.0-flash",
        "google/custom-tuned",
    ] {
        let contents = map_contents(unsigned_history(), model);
        assert!(!contents.to_string().contains(SKIP), "{model}");
    }
}

#[test]
fn messages_tool_use_history_gets_the_bypass_in_the_current_turn() {
    let contents = map_contents(
        json!([
            { "role": "user", "content": "List files." },
            { "role": "assistant", "content": [
                { "type": "text", "text": "Listing." },
                { "type": "tool_use", "id": "toolu_1", "name": "bash", "input": { "command": "ls" } }
            ] },
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "toolu_1", "content": "a.txt" }
            ] }
        ]),
        GEMINI_4,
    );

    assert_eq!(contents[1]["parts"][1]["thoughtSignature"], SKIP);
    assert!(contents[2]["parts"][0].get("functionResponse").is_some());
}

/// One signed tool call as the gateway streams it, with the fields clients keep.
struct StreamedCall {
    id: Value,
    function: Value,
    extra_content: Value,
    reasoning_detail: Value,
}

/// How one harness client sends a streamed tool-call turn back.
type ClientReplay = fn(&StreamedCall) -> Value;

fn stream_step(part: Value) -> StreamedCall {
    let mut state = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string());
    let chunks = state.on_response(
        &json!({ "candidates": [{ "content": { "role": "model", "parts": [part] } }] }),
    );
    state.on_response(
        &json!({ "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": "STOP" }] }),
    );
    let finish = state.finish().expect("finish");
    assert_eq!(finish["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        chunks.len(),
        1,
        "the call and its signature share one delta"
    );
    let delta = &chunks[0]["choices"][0]["delta"];
    let call = &delta["tool_calls"][0];
    StreamedCall {
        id: call["id"].clone(),
        function: call["function"].clone(),
        extra_content: call["extra_content"].clone(),
        reasoning_detail: delta["reasoning_details"][0].clone(),
    }
}

/// `@earendil-works/pi-ai` openai-completions: stores the `reasoning_details` entry whose `id`
/// matches the call as its `thoughtSignature`, and sends the parsed entry back in
/// `reasoning_details`; the call itself goes back bare.
fn pi_assistant(call: &StreamedCall) -> Value {
    json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{ "id": call.id, "type": "function", "function": call.function }],
        "reasoning_details": [call.reasoning_detail]
    })
}

/// `@oh-my-pi/pi-ai` openai-completions: records `extra_content.google` first, then the
/// `reasoning_details` entry for the same call replaces it, so the entry goes back in
/// `reasoning_details`. Null content goes back as an empty string.
fn omp_assistant(call: &StreamedCall) -> Value {
    json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [{ "id": call.id, "type": "function", "function": call.function }],
        "reasoning_details": [call.reasoning_detail]
    })
}

/// `@opencode/ai/providers/openai-compatible` (OpenAI Chat protocol): keeps
/// `extra_content.google` on the call and the reasoning details on a reasoning part, and sends
/// both back.
fn opencode_assistant(call: &StreamedCall) -> Value {
    json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": call.id,
            "type": "function",
            "function": call.function,
            "extra_content": call.extra_content
        }],
        "reasoning_details": [call.reasoning_detail],
        "reasoning_content": ""
    })
}

#[test]
fn pi_omp_and_opencode_replays_restore_every_signed_step() {
    let ls = function_call("fc-ls", "ls", "sig-ls");
    let cat = function_call("fc-cat", "cat a.txt", "sig-cat");
    let streamed_ls = stream_step(ls.clone());
    let streamed_cat = stream_step(cat.clone());
    assert_eq!(
        streamed_ls.extra_content,
        json!({ "google": { "thought_signature": "sig-ls" } })
    );
    assert_eq!(
        streamed_ls.reasoning_detail,
        json!({ "type": "reasoning.encrypted", "id": "fc-ls", "data": "sig-ls", "format": "google-gemini-v1" })
    );

    let clients: [(&str, ClientReplay); 3] = [
        ("pi", pi_assistant),
        ("omp", omp_assistant),
        ("opencode", opencode_assistant),
    ];
    for (client, assistant) in clients {
        let contents = map_contents(
            json!([
                { "role": "user", "content": "List the files, then show a.txt." },
                assistant(&streamed_ls),
                { "role": "tool", "tool_call_id": "fc-ls", "content": "a.txt" },
                assistant(&streamed_cat),
                { "role": "tool", "tool_call_id": "fc-cat", "content": "hello" }
            ]),
            GEMINI_4,
        );
        assert_eq!(contents[1]["parts"], json!([ls]), "{client}");
        assert_eq!(contents[3]["parts"], json!([cat]), "{client}");
        assert!(!contents.to_string().contains(SKIP), "{client}");
    }
}

#[test]
fn json_replay_without_provider_metadata_restores_the_signed_call() {
    // An OpenAI SDK client that keeps unknown message fields but drops `provider_metadata`.
    let part = function_call("fc-ls", "ls", "sig-ls");
    let response = json!({ "candidates": [{
        "content": { "role": "model", "parts": [part.clone()] }, "finishReason": "STOP"
    }] });
    let mut message = normalize_google_response(&response, &context(GEMINI_4)).expect("normalized")
        ["choices"][0]["message"]
        .clone();
    message
        .as_object_mut()
        .expect("message")
        .remove("provider_metadata");

    let contents = map_contents(
        json!([
            { "role": "user", "content": "List files." },
            message,
            { "role": "tool", "tool_call_id": "fc-ls", "content": "a.txt" }
        ]),
        GEMINI_4,
    );

    assert_eq!(contents[1]["parts"], json!([part]));
}

#[test]
fn opencode_replays_the_streamed_text_part_signature() {
    let mut state = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string());
    state.on_response(
        &json!({ "candidates": [{ "content": { "role": "model", "parts": [
        { "text": "Done." },
        { "text": "", "thoughtSignature": "sig-answer" }
    ] }, "finishReason": "STOP" }] }),
    );
    let finish = state.finish().expect("finish");
    let details = finish["choices"][0]["delta"]["reasoning_details"].clone();

    let contents = map_contents(
        json!([
            { "role": "user", "content": "hi" },
            { "role": "assistant", "content": "Done.", "reasoning_details": details,
              "reasoning_content": "" },
            { "role": "user", "content": "thanks" }
        ]),
        GEMINI_4,
    );

    assert_eq!(
        contents[1]["parts"],
        json!([{ "text": "Done.", "thoughtSignature": "sig-answer" }])
    );
}
