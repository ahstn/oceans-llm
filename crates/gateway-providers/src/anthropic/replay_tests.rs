use std::collections::BTreeMap;

use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    AnthropicMessagesRequest, CoreChatMessage, CoreChatRequest, ProviderRequestContext,
    SseEventParser, anthropic_messages_request_to_core,
};
use serde_json::{Map, Value, json};

use super::{
    AnthropicRequestOptions, map_anthropic_request, normalize_anthropic_response,
    normalize_anthropic_stream,
};

fn context() -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "replay-test".into(),
        model_key: "claude".into(),
        provider_key: "anthropic".into(),
        upstream_model: "claude-sonnet-4-6".into(),
        owner_user_id: None,
        expected_provider_credential_id: None,
        chat_protocol: Default::default(),
        extra_headers: Default::default(),
        extra_body: Default::default(),
        request_headers: Default::default(),
        compatibility: Default::default(),
        service_tier: None,
    }
}

fn replay(message: &Value) -> Result<Value, gateway_core::ProviderError> {
    let request = CoreChatRequest {
        model: "claude".into(),
        stream: false,
        extra: BTreeMap::new(),
        messages: vec![CoreChatMessage {
            role: "assistant".into(),
            content: message["content"].clone(),
            name: None,
            extra: message
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "role" | "content"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }],
    };
    map_anthropic_request(
        &request,
        &context(),
        false,
        &AnthropicRequestOptions::default(),
    )
}

fn ordered_blocks() -> Vec<Value> {
    vec![
        json!({"type":"thinking","thinking":"first thought","signature":"sig-1","future":true}),
        json!({"type":"tool_use","id":"tool-1","name":"lookup","input":{"q":"one"}}),
        json!({"type":"thinking","thinking":"second thought","signature":"sig-2"}),
        json!({"type":"tool_use","id":"tool-2","name":"lookup","input":{"q":"two"}}),
        json!({"type":"server_tool_use","id":"server-1","name":"web_search","input":{"query":"source"}}),
        json!({"type":"web_search_tool_result","tool_use_id":"server-1","content":[
            {"type":"web_search_result","url":"https://example.org","title":"Source","encrypted_content":"opaque"}
        ]}),
        json!({"type":"text","text":"Answer","citations":[
            {"type":"web_search_result_location","url":"https://example.org","cited_text":"Source"}
        ]}),
        json!({"type":"future_server_state","opaque":{"cursor":"next"}}),
    ]
}

#[test]
fn json_replay_preserves_ordered_signed_tools_server_results_citations_and_pause() {
    let blocks = ordered_blocks();
    let native =
        json!({"id":"message-1","content":blocks,"stop_reason":"pause_turn", "stop_sequence":null});
    let normalized =
        normalize_anthropic_response(&native, &context(), "anthropic_compat", "anthropic");
    let message = &normalized["choices"][0]["message"];
    assert_eq!(message["content"], "Answer");
    assert_eq!(message["tool_calls"].as_array().unwrap().len(), 2);
    assert_eq!(
        message["provider_metadata"]["anthropic_compat"]["message"]["stop_reason"],
        "pause_turn"
    );
    assert_eq!(
        replay(message).unwrap()["messages"][0]["content"],
        json!(blocks)
    );
}

#[test]
fn replay_rejects_edited_public_projection_and_hidden_visible_tool_changes() {
    let native = json!({"content":ordered_blocks(),"stop_reason":"tool_use"});
    let normalized =
        normalize_anthropic_response(&native, &context(), "anthropic_compat", "anthropic");
    let original = normalized["choices"][0]["message"].clone();
    let mut cases = Vec::new();
    let mut edited = original.clone();
    edited["content"] = json!("guardrail-approved replacement");
    cases.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"][0]["function"]["arguments"] = json!("{\"q\":\"edited\"}");
    cases.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"][0]["function"]["name"] = json!("delete_everything");
    cases.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"][0]["id"] = json!("another-id");
    cases.push(edited);
    let mut edited = original.clone();
    edited["tool_calls"] = json!([]);
    cases.push(edited);
    let mut edited = original.clone();
    edited["reasoning_content"] = json!("edited reasoning");
    cases.push(edited);
    let mut edited = original.clone();
    edited["provider_metadata"]["anthropic_compat"]["native_content"]["blocks"][6]["text"] =
        json!("hidden prompt");
    cases.push(edited);
    let mut edited = original.clone();
    edited["provider_metadata"]["anthropic_compat"]["native_content"]["blocks"][1]["input"] =
        json!({"q":"hidden call"});
    cases.push(edited);
    let mut edited = original.clone();
    edited["provider_metadata"]["anthropic_compat"]["native_content"]["blocks"] = json!("invalid");
    cases.push(edited);
    let mut edited = original.clone();
    edited["content"] = json!([{"type":"image","source":{"type":"base64","data":"edited"}}]);
    cases.push(edited);
    let mut edited = original.clone();
    edited["provider_metadata"]["anthropic"] =
        edited["provider_metadata"]["anthropic_compat"].clone();
    edited["provider_metadata"]["anthropic"]["native_content"]["blocks"][0]["signature"] =
        json!("different history");
    cases.push(edited);
    for edited in cases {
        assert!(
            matches!(
                replay(&edited),
                Err(gateway_core::ProviderError::InvalidRequest(_))
            ),
            "{edited}"
        );
    }
    let mut equivalent = original;
    equivalent["tool_calls"][0]["function"]["arguments"] = json!("{ \"q\" : \"one\" }");
    equivalent["reasoning_content"] = json!("first thoughtsecond thought");
    assert!(replay(&equivalent).is_ok());
}

#[test]
fn context_window_exceeded_json_is_length_with_native_stop_state() {
    let normalized = normalize_anthropic_response(
        &json!({
            "content":[{"type":"text","text":"partial"}],
            "stop_reason":"model_context_window_exceeded"
        }),
        &context(),
        "anthropic_compat",
        "anthropic",
    );
    assert_eq!(normalized["choices"][0]["finish_reason"], "length");
    assert_eq!(
        normalized["choices"][0]["message"]["provider_metadata"]["anthropic_compat"]["message"]["stop_reason"],
        "model_context_window_exceeded"
    );
}

fn stream_events(blocks: &[Value], reason: &str) -> Vec<Value> {
    let mut events = vec![
        json!({"type":"message_start","message":{"role":"assistant","content":[],
        "usage":{"input_tokens":10,"output_tokens":0}}}),
    ];
    for (index, block) in blocks.iter().enumerate() {
        let mut start = block.clone();
        let mut deltas = Vec::new();
        match block["type"].as_str().unwrap() {
            "thinking" => {
                start["thinking"] = json!("");
                start.as_object_mut().unwrap().remove("signature");
                deltas.push(json!({"type":"thinking_delta","thinking":block["thinking"]}));
                deltas.push(json!({"type":"signature_delta","signature":block["signature"]}));
            }
            "tool_use" | "server_tool_use" => {
                start["input"] = json!({});
                let input = block["input"].to_string();
                let (first, second) = input.split_at(input.len() / 2);
                deltas.push(json!({"type":"input_json_delta","partial_json":first}));
                deltas.push(json!({"type":"input_json_delta","partial_json":second}));
            }
            "text" => {
                start["text"] = json!("");
                start.as_object_mut().unwrap().remove("citations");
                deltas.push(json!({"type":"text_delta","text":block["text"]}));
                for citation in block["citations"].as_array().unwrap() {
                    deltas.push(json!({"type":"citations_delta","citation":citation}));
                }
            }
            _ => {}
        }
        events.push(json!({"type":"content_block_start","index":index,"content_block":start}));
        events.extend(
            deltas
                .into_iter()
                .map(|delta| json!({"type":"content_block_delta","index":index,"delta":delta})),
        );
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    events.push(json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null},"usage":{"output_tokens":7}}));
    events.push(json!({"type":"message_stop"}));
    events
}

#[tokio::test]
async fn stream_replay_has_indexed_boundaries_complete_ordered_blocks_and_incremental_chat() {
    let blocks = ordered_blocks();
    let events = stream_events(&blocks, "pause_turn");
    let upstream = futures_util::stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let mut stream = normalize_anthropic_stream(
        upstream,
        "test".into(),
        0,
        "claude".into(),
        "anthropic_compat",
        "anthropic",
    );
    let mut parser = SseEventParser::default();
    let mut text = String::new();
    let mut calls = BTreeMap::<u64, Value>::new();
    let mut metadata = Map::new();
    let mut boundaries = Vec::new();
    let mut usage = Value::Null;
    let mut done = false;
    while let Some(chunk) = stream.next().await {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            if event.data == "[DONE]" {
                done = true;
                continue;
            }
            let chunk: Value = serde_json::from_str(&event.data).unwrap();
            assert!(chunk.get("error").is_none(), "{chunk}");
            if let Some(value) = chunk.get("usage") {
                usage = value.clone();
            }
            let delta = &chunk["choices"][0]["delta"];
            if let Some(part) = delta["content"].as_str() {
                text.push_str(part);
            }
            if let Some(tools) = delta["tool_calls"].as_array() {
                for tool in tools {
                    let call = calls
                        .entry(tool["index"].as_u64().unwrap())
                        .or_insert_with(|| json!({"type":"function","function":{"arguments":""}}));
                    for field in ["id", "type"] {
                        if let Some(value) = tool.get(field) {
                            call[field] = value.clone();
                        }
                    }
                    if let Some(name) = tool["function"].get("name") {
                        call["function"]["name"] = name.clone();
                    }
                    if let Some(part) = tool["function"]["arguments"].as_str() {
                        let args = call["function"]["arguments"].as_str().unwrap().to_string();
                        call["function"]["arguments"] = json!(args + part);
                    }
                }
            }
            if let Some(native) = delta["provider_metadata"]["anthropic_compat"].as_object() {
                if let Some(event) = native.get("native_event") {
                    boundaries.push((event["type"].clone(), event["index"].clone()));
                }
                metadata.extend(native.clone());
            }
            if chunk["choices"][0]["finish_reason"] == "stop" {
                assert_eq!(
                    delta["provider_metadata"]["anthropic_compat"]["native_content"]["blocks"],
                    json!(blocks)
                );
            }
        }
    }
    assert!(done);
    assert_eq!(text, "Answer");
    assert_eq!(calls.len(), 2);
    for index in 0..blocks.len() {
        assert!(boundaries.contains(&(json!("content_block_start"), json!(index))));
        assert!(boundaries.contains(&(json!("content_block_stop"), json!(index))));
    }
    assert_eq!(usage["prompt_tokens"], 10);
    assert_eq!(usage["completion_tokens"], 7);
    assert_eq!(metadata["message"]["stop_reason"], "pause_turn");
    let message = json!({"role":"assistant","content":text,"tool_calls":calls.into_values().collect::<Vec<_>>(),
        "provider_metadata":{"anthropic_compat":metadata}});
    assert_eq!(
        replay(&message).unwrap()["messages"][0]["content"],
        json!(blocks)
    );
}

#[tokio::test]
async fn context_window_exceeded_stream_is_length_with_native_stop_state() {
    let events = stream_events(&[], "model_context_window_exceeded");
    let upstream = futures_util::stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let chunks = normalize_anthropic_stream(
        upstream,
        "test".into(),
        0,
        "claude".into(),
        "anthropic_compat",
        "anthropic",
    )
    .collect::<Vec<_>>()
    .await;
    let mut parser = SseEventParser::default();
    let mut finish = None;
    for chunk in chunks {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data)
                && value["choices"][0]["finish_reason"] == "length"
            {
                finish = Some(value);
            }
        }
    }
    let finish = finish.expect("context limit is length, not ordinary stop");
    assert_eq!(
        finish["choices"][0]["delta"]["provider_metadata"]["anthropic_compat"]["message"]["stop_reason"],
        "model_context_window_exceeded"
    );
}

#[tokio::test]
async fn stream_tool_without_argument_deltas_keeps_empty_input_replayable() {
    let events = vec![
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{
            "type":"tool_use","id":"empty-call","name":"get_status","input":{}
        }}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
        json!({"type":"message_stop"}),
    ];
    let upstream = futures_util::stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let chunks = normalize_anthropic_stream(
        upstream,
        "test".into(),
        0,
        "claude".into(),
        "anthropic_compat",
        "anthropic",
    )
    .collect::<Vec<_>>()
    .await;
    let mut parser = SseEventParser::default();
    let mut arguments = String::new();
    for chunk in chunks {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                assert!(value.get("error").is_none(), "{value}");
                if let Some(part) =
                    value["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
                {
                    arguments.push_str(part);
                }
            }
        }
    }
    assert_eq!(
        serde_json::from_str::<Value>(&arguments).unwrap(),
        json!({})
    );
}

#[tokio::test]
async fn unsupported_native_stream_delta_is_error_without_fabricated_replay_or_done() {
    let events = vec![
        json!({"type":"content_block_start","index":0,"content_block":{"type":"future","opaque":true}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"future_delta","opaque":"changed"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_stop"}),
    ];
    let upstream = futures_util::stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let chunks = normalize_anthropic_stream(
        upstream,
        "test".into(),
        0,
        "claude".into(),
        "anthropic_compat",
        "anthropic",
    )
    .collect::<Vec<_>>()
    .await;
    let mut parser = SseEventParser::default();
    let mut error = None;
    for chunk in chunks {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            assert_ne!(event.data, "[DONE]");
            let value: Value = serde_json::from_str(&event.data).unwrap();
            assert!(
                value["choices"][0]["delta"]["provider_metadata"]["anthropic_compat"]
                    .get("native_content")
                    .is_none()
            );
            if let Some(value) = value.get("error") {
                error = Some(value.clone());
            }
        }
    }
    assert_eq!(
        error.expect("unsupported assembly is an explicit failure")["code"],
        "anthropic_replay_error"
    );
}

#[test]
fn foreign_native_history_uses_public_chat_projection_without_replaying_foreign_blocks() {
    for metadata in [
        json!({"gcp_vertex":{"native_content":{"source":"google_generate_content",
            "parts":[{"text":"foreign hidden"}],"tool_call_ids":[]}}}),
        json!({"gcp_vertex":{"native_content":{"parts":[{"text":"legacy hidden"}]}}}),
        json!({"aws_bedrock":{"native_content":{"source":"bedrock_converse",
            "blocks":[{"text":"foreign hidden"}]}}}),
    ] {
        let message =
            json!({"role":"assistant","content":"public content","provider_metadata":metadata});
        assert_eq!(
            replay(&message).unwrap()["messages"][0]["content"],
            "public content"
        );
    }
    let malformed = json!({"role":"assistant","content":"public content","provider_metadata":{
        "gcp_vertex":{"native_content":{"source":"anthropic_messages","parts":[{"text":"hidden"}]}}
    }});
    assert!(matches!(
        replay(&malformed),
        Err(gateway_core::ProviderError::InvalidRequest(_))
    ));
}

async fn compaction_chat_chunks(events: Vec<Value>, bedrock: bool) -> (Vec<Value>, bool) {
    let upstream = futures_util::stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let mut stream = if bedrock {
        super::normalize_anthropic_messages_stream(upstream, context(), "aws_bedrock")
    } else {
        normalize_anthropic_stream(
            upstream,
            "test".into(),
            0,
            "claude".into(),
            "anthropic_compat",
            "anthropic",
        )
    };
    let mut parser = SseEventParser::default();
    let mut chunks = Vec::new();
    let mut done = false;
    while let Some(chunk) = stream.next().await {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            if event.data == "[DONE]" {
                done = true;
            } else {
                chunks.push(serde_json::from_str(&event.data).unwrap());
            }
        }
    }
    (chunks, done)
}

fn compaction_events(start: Value, deltas: Vec<Value>) -> Vec<Value> {
    let mut events = vec![
        json!({"type":"message_start","message":{"role":"assistant","content":[],
            "usage":{"input_tokens":10,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":start}),
    ];
    events.extend(
        deltas
            .into_iter()
            .map(|delta| json!({"type":"content_block_delta","index":0,"delta":delta})),
    );
    events.extend([
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"text","text":"Visible "}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"after compaction"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
            "usage":{"output_tokens":7}}),
        json!({"type":"message_delta","delta":{},"usage":{"output_tokens":9}}),
        json!({"type":"message_stop"}),
    ]);
    events
}

async fn assert_compaction_chat_replay(start: Value, deltas: Vec<Value>, expected: Value) {
    for bedrock in [false, true] {
        let namespace = if bedrock {
            "aws_bedrock"
        } else {
            "anthropic_compat"
        };
        let (chunks, done) =
            compaction_chat_chunks(compaction_events(start.clone(), deltas.clone()), bedrock).await;
        let mut text = String::new();
        let mut metadata = Map::new();
        let mut usage = Value::Null;
        let mut finish = None;
        let mut error = None;
        for chunk in chunks {
            if let Some(value) = chunk.get("error") {
                error = Some(value.clone());
            }
            if let Some(value) = chunk.get("usage") {
                usage = value.clone();
            }
            let delta = &chunk["choices"][0]["delta"];
            if let Some(part) = delta["content"].as_str() {
                text.push_str(part);
            }
            if let Some(native) = delta["provider_metadata"][namespace].as_object() {
                metadata.extend(native.clone());
            }
            if let Some(reason) = chunk["choices"][0]["finish_reason"].as_str() {
                finish = Some(reason.to_string());
            }
        }
        assert_eq!(text, "Visible after compaction", "{namespace}: {error:?}");
        assert_eq!(error, None, "{namespace}");
        assert!(done, "{namespace}");
        assert_eq!(finish.as_deref(), Some("stop"), "{namespace}");
        assert_eq!(usage["completion_tokens"], 9, "{namespace}");
        let expected_blocks = json!([
            expected,
            {"type":"text","text":"Visible after compaction"}
        ]);
        assert_eq!(
            metadata["native_content"]["blocks"], expected_blocks,
            "{namespace}"
        );
        let message = json!({"role":"assistant","content":text,
            "provider_metadata":{namespace:metadata}});
        assert_eq!(
            replay(&message).unwrap()["messages"][0]["content"],
            expected_blocks,
            "{namespace}"
        );
    }
}

// The documented wire sequence is start -> ONE delta containing the complete summary -> stop.
// https://platform.claude.com/docs/en/build-with-claude/compaction-threshold#streaming
#[tokio::test]
async fn compaction_delta_preserves_following_text_and_continuation_in_both_chat_normalizers() {
    assert_compaction_chat_replay(
        json!({"type":"compaction","content":null,"encrypted_content":null,"future":{"keep":true}}),
        vec![
            json!({"type":"compaction_delta","content":"Complete conversation summary.",
            "encrypted_content":"opaque-checkpoint"}),
        ],
        json!({"type":"compaction","content":"Complete conversation summary.",
            "encrypted_content":"opaque-checkpoint","future":{"keep":true}}),
    )
    .await;
}

// Defensive repeated snapshots must replace, not concatenate; they are not text fragments.
// Explicit null means failed compaction, whereas a missing beta-gated checkpoint key is absent.
// https://github.com/anthropics/anthropic-sdk-typescript/blob/832402e8036a2815c9bc13cdce2373b79b0e96cb/src/lib/BetaMessageStream.ts#L806-L815
#[tokio::test]
async fn compaction_delta_replaces_snapshots_and_distinguishes_null_from_absent_checkpoints() {
    for (deltas, expected) in [
        (
            vec![
                json!({"type":"compaction_delta","content":"Superseded summary.",
                    "encrypted_content":"superseded-checkpoint"}),
                json!({"type":"compaction_delta","content":"Final complete summary.",
                    "encrypted_content":"final-checkpoint"}),
            ],
            json!({"type":"compaction","content":"Final complete summary.",
                "encrypted_content":"final-checkpoint"}),
        ),
        (
            vec![
                json!({"type":"compaction_delta","content":"Superseded summary.",
                    "encrypted_content":"superseded-checkpoint"}),
                json!({"type":"compaction_delta","content":null,"encrypted_content":null}),
            ],
            json!({"type":"compaction","content":null,"encrypted_content":null}),
        ),
        (
            vec![json!({"type":"compaction_delta","content":"Complete summary."})],
            json!({"type":"compaction","content":"Complete summary.",
                "encrypted_content":"start-checkpoint"}),
        ),
        (
            vec![json!({"type":"compaction_delta","content":null,
                "encrypted_content":"opaque-checkpoint"})],
            json!({"type":"compaction","content":null,"encrypted_content":"opaque-checkpoint"}),
        ),
    ] {
        assert_compaction_chat_replay(
            json!({"type":"compaction","content":null,"encrypted_content":"start-checkpoint"}),
            deltas,
            expected,
        )
        .await;
    }
}

#[tokio::test]
async fn compaction_delta_rejects_malformed_values_and_mismatched_boundaries_without_replay() {
    let start = json!({"type":"content_block_start","index":0,
        "content_block":{"type":"compaction","content":null,"encrypted_content":null}});
    let delta = json!({"type":"content_block_delta","index":0,
        "delta":{"type":"compaction_delta","content":"Complete summary.","encrypted_content":"opaque"}});
    let stop = json!({"type":"content_block_stop","index":0});
    let mut cases = vec![
        vec![delta.clone()],
        vec![start.clone(), stop.clone(), delta.clone()],
        vec![start.clone(), delta.clone()],
        vec![
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":""}}),
            delta.clone(),
            stop.clone(),
        ],
    ];
    for malformed in [
        json!({"type":"compaction_delta","encrypted_content":"opaque"}),
        json!({"type":"compaction_delta","content":42}),
        json!({"type":"compaction_delta","content":"Summary","encrypted_content":{"opaque":true}}),
    ] {
        cases.push(vec![
            start.clone(),
            json!({"type":"content_block_delta","index":0,"delta":malformed}),
            stop.clone(),
        ]);
    }
    for (case, mut events) in cases.into_iter().enumerate() {
        events.extend([
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"output_tokens":7}}),
            json!({"type":"message_delta","delta":{},"usage":{"output_tokens":9}}),
            json!({"type":"message_stop"}),
        ]);
        for bedrock in [false, true] {
            let namespace = if bedrock {
                "aws_bedrock"
            } else {
                "anthropic_compat"
            };
            let (chunks, done) = compaction_chat_chunks(events.clone(), bedrock).await;
            assert!(!done, "{namespace}: case {case}");
            let mut error = None;
            let mut usage = Value::Null;
            for chunk in chunks {
                if let Some(value) = chunk.get("usage") {
                    usage = value.clone();
                }
                if let Some(value) = chunk.get("error") {
                    assert_eq!(usage["completion_tokens"], 9, "{namespace}: case {case}");
                    error = Some(value.clone());
                }
                assert!(
                    chunk["choices"][0]["delta"]["provider_metadata"][namespace]
                        .get("native_content")
                        .is_none(),
                    "{namespace}: case {case}: {chunk}"
                );
                assert!(
                    chunk["choices"][0]["finish_reason"].is_null(),
                    "{namespace}: case {case}: {chunk}"
                );
            }
            let expected_code = if bedrock {
                "anthropic_messages_replay_error"
            } else {
                "anthropic_replay_error"
            };
            assert_eq!(
                error.expect("malformed compaction cannot become successful replay")["code"],
                expected_code,
                "{namespace}: case {case}"
            );
        }
    }
}

#[test]
fn replay_rejects_explicit_public_text_block_fields_that_would_be_discarded() {
    let native = json!({"content":[{"type":"text","text":"Answer","citations":[{"url":"original"}]}],
        "stop_reason":"end_turn"});
    let normalized =
        normalize_anthropic_response(&native, &context(), "anthropic_compat", "anthropic");
    let original = normalized["choices"][0]["message"].clone();
    for content in [
        json!([{"type":"text","text":"Answer","cache_control":{"type":"ephemeral"}}]),
        json!([{"type":"text","text":"Answer","citations":[{"url":"edited"}]}]),
    ] {
        let mut message = original.clone();
        message["content"] = content;
        assert!(matches!(
            replay(&message),
            Err(gateway_core::ProviderError::InvalidRequest(_))
        ));
    }
    let mut exact = original;
    exact["content"] = native["content"].clone();
    assert_eq!(
        replay(&exact).unwrap()["messages"][0]["content"],
        native["content"]
    );
}

#[tokio::test]
async fn incomplete_tool_replay_drains_final_usage_and_preserves_truthful_terminal_outcome() {
    for explicit_block_stop in [false, true] {
        for (stop, expected) in [
            ("end_turn", None),
            ("tool_use", None),
            ("max_tokens", Some("length")),
            ("model_context_window_exceeded", Some("length")),
            ("refusal", Some("content_filter")),
        ] {
            let mut events = vec![
                json!({"type":"message_start","message":{"role":"assistant","content":[],
                    "usage":{"input_tokens":10,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{
                    "type":"tool_use","id":"truncated","name":"lookup","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{
                    "type":"input_json_delta","partial_json":"{\"query\":"}}),
            ];
            if explicit_block_stop {
                events.push(json!({"type":"content_block_stop","index":0}));
            }
            events.extend([
                json!({"type":"message_delta","delta":{"stop_reason":stop},
                    "usage":{"output_tokens":5}}),
                json!({"type":"message_delta","delta":{},
                    "usage":{"output_tokens":9,"cost":0.125}}),
                json!({"type":"message_stop"}),
            ]);
            let upstream = futures_util::stream::iter(
                events
                    .into_iter()
                    .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
            );
            let chunks = normalize_anthropic_stream(
                upstream,
                "test".into(),
                0,
                "claude".into(),
                "anthropic_compat",
                "anthropic",
            )
            .collect::<Vec<_>>()
            .await;
            let mut parser = SseEventParser::default();
            let mut final_usage = Value::Null;
            let mut finish = None;
            let mut failed = false;
            let mut done = false;
            for chunk in chunks {
                for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
                    if event.data == "[DONE]" {
                        done = true;
                        continue;
                    }
                    let value: Value = serde_json::from_str(&event.data).unwrap();
                    if let Some(usage) = value.get("usage") {
                        final_usage = usage.clone();
                    }
                    assert!(
                        value["choices"][0]["delta"]["provider_metadata"]["anthropic_compat"]
                            .get("native_content")
                            .is_none(),
                        "{stop}: {value}"
                    );
                    if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
                        finish = Some(reason.to_string());
                    }
                    if value.get("error").is_some() {
                        assert_eq!(final_usage["completion_tokens"], 9, "usage precedes error");
                        assert_eq!(final_usage["cost"], 0.125);
                        failed = true;
                    }
                }
            }
            assert_eq!(finish.as_deref(), expected, "{explicit_block_stop}: {stop}");
            assert_eq!(failed, expected.is_none(), "{explicit_block_stop}: {stop}");
            assert_eq!(done, expected.is_some(), "{explicit_block_stop}: {stop}");
            assert_eq!(final_usage["completion_tokens"], 9);
            assert_eq!(final_usage["total_tokens"], 19);
            assert_eq!(final_usage["cost"], 0.125);
        }
    }
}

#[test]
fn native_messages_ignore_chat_replay_metadata_and_preserve_content_and_controls() {
    let blocks = ordered_blocks();
    let messages = json!([{"role":"assistant","content":blocks,
        "provider_metadata":{"anthropic_compat":{"native_content":{"blocks":"invalid"}}}}]);
    let native: AnthropicMessagesRequest = serde_json::from_value(json!({
        "model":"claude", "max_tokens":1024, "messages":messages,
        "tools":[{"type":"web_search_20250305","name":"web_search"}],
        "thinking":{"type":"enabled","budget_tokens":512}
    }))
    .unwrap();
    let mut context = context();
    context.chat_protocol = gateway_core::ChatRequestProtocol::AnthropicMessages;
    let mapped = map_anthropic_request(
        &anthropic_messages_request_to_core(&native),
        &context,
        false,
        &AnthropicRequestOptions::default(),
    )
    .unwrap();
    assert_eq!(mapped["messages"], messages);
    assert_eq!(
        mapped["tools"],
        json!([{"type":"web_search_20250305","name":"web_search"}])
    );
    assert_eq!(
        mapped["thinking"],
        json!({"type":"enabled","budget_tokens":512})
    );
}

async fn compatibility_chat_chunks(
    events: Vec<Value>,
    namespace: &'static str,
) -> (Vec<Value>, bool) {
    let upstream = futures_util::stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let mut stream = super::normalize_anthropic_messages_stream(upstream, context(), namespace);
    let mut parser = SseEventParser::default();
    let mut chunks = Vec::new();
    let mut done = false;
    while let Some(chunk) = stream.next().await {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            if event.data == "[DONE]" {
                done = true;
            } else {
                chunks.push(serde_json::from_str(&event.data).unwrap());
            }
        }
    }
    parser.finish().unwrap();
    (chunks, done)
}

#[tokio::test]
async fn compatibility_chat_requires_message_stop_after_content_and_finish() {
    for namespace in ["aws_bedrock", "github_copilot"] {
        for stop_reason in [None, Some("end_turn")] {
            let mut events = vec![
                json!({"type":"message_start","message":{"usage":{"input_tokens":3,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Partial"}}),
            ];
            if let Some(stop_reason) = stop_reason {
                events.extend([
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":stop_reason},"usage":{"output_tokens":1}}),
                    json!({"type":"message_delta","delta":{},"usage":{"output_tokens":4}}),
                ]);
            }
            let (chunks, done) = compatibility_chat_chunks(events, namespace).await;
            assert!(!done, "{namespace}: {stop_reason:?}");
            assert_eq!(
                chunks.last().unwrap()["error"]["code"],
                "anthropic_messages_stream_truncated",
                "{namespace}: {stop_reason:?}"
            );
            let text = chunks
                .iter()
                .filter_map(|chunk| chunk["choices"][0]["delta"]["content"].as_str())
                .collect::<String>();
            assert_eq!(text, "Partial");
            if stop_reason.is_some() {
                assert_eq!(
                    chunks
                        .iter()
                        .filter_map(|chunk| chunk.get("usage"))
                        .next_back()
                        .unwrap()["completion_tokens"],
                    4
                );
            }
        }
    }
}

#[tokio::test]
async fn compatibility_chat_incomplete_replay_requires_native_terminal_and_keeps_errors() {
    for namespace in ["aws_bedrock", "github_copilot"] {
        for (reason, finish) in [("max_tokens", "length"), ("refusal", "content_filter")] {
            for terminal in ["eof", "message_stop", "error"] {
                let mut events = vec![
                    json!({"type":"message_start","message":{"usage":{"input_tokens":3,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{
                        "type":"tool_use","id":"partial","name":"lookup","input":{}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{
                        "type":"input_json_delta","partial_json":"{\"q\":"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":reason},"usage":{"output_tokens":1}}),
                    json!({"type":"message_delta","delta":{},"usage":{"output_tokens":4,"future_usage":{"opaque":true}}}),
                ];
                match terminal {
                    "message_stop" => events.push(json!({"type":"message_stop"})),
                    "error" => {
                        events.push(json!({"type":"error","error":{
                        "type":"overloaded_error","message":"upstream failed after partial tool"}}))
                    }
                    _ => {}
                }
                let (chunks, done) = compatibility_chat_chunks(events, namespace).await;
                assert_eq!(
                    done,
                    terminal == "message_stop",
                    "{namespace}: {reason}: {terminal}"
                );
                let usage = chunks
                    .iter()
                    .filter_map(|chunk| chunk.get("usage"))
                    .next_back()
                    .unwrap();
                assert_eq!(usage["completion_tokens"], 4);
                assert_eq!(
                    usage["provider_usage"]["future_usage"],
                    json!({"opaque":true})
                );
                assert!(
                    chunks
                        .iter()
                        .any(|chunk| chunk["choices"][0]["finish_reason"] == finish)
                );
                assert!(chunks.iter().all(|chunk| {
                    chunk["choices"][0]["delta"]["provider_metadata"][namespace]
                        .get("native_content")
                        .is_none()
                }));
                if terminal == "message_stop" {
                    assert!(chunks.iter().all(|chunk| chunk.get("error").is_none()));
                } else {
                    let expected = if terminal == "error" {
                        "overloaded_error"
                    } else {
                        "anthropic_messages_stream_truncated"
                    };
                    assert_eq!(chunks.last().unwrap()["error"]["code"], expected);
                }
            }
        }
    }
}

#[tokio::test]
async fn compatibility_chat_cache_totals_merge_usage_snapshots_and_preserve_native_fields() {
    for namespace in ["aws_bedrock", "github_copilot"] {
        let events = vec![
            json!({"type":"message_start","message":{"id":"native-id",
                "usage":{"input_tokens":3,"output_tokens":0,"cache_read_input_tokens":2,
                    "cache_creation_input_tokens":1,"future_usage":{"tier":"start"}}}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":"Answer","future_block":{"opaque":true}}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":"native-stop"},
                "usage":{"output_tokens":1}}),
            json!({"type":"message_delta","delta":{},"usage":{"output_tokens":4,
                "cache_read_input_tokens":5,"cache_creation_input_tokens":2,"future_usage":{"tier":"late"}}}),
            json!({"type":"message_stop"}),
        ];
        let (chunks, done) = compatibility_chat_chunks(events, namespace).await;
        assert!(done, "{namespace}");
        assert!(chunks.iter().all(|chunk| chunk.get("error").is_none()));
        let usages = chunks
            .iter()
            .filter_map(|chunk| chunk.get("usage"))
            .collect::<Vec<_>>();
        assert_eq!(usages.len(), 3);
        assert_eq!(usages[0]["total_tokens"], 6);
        assert_eq!(usages[1]["total_tokens"], 7);
        assert_eq!(usages[2]["total_tokens"], 14);
        assert_eq!(
            usages[0]["provider_usage"]["future_usage"],
            json!({"tier":"start"})
        );
        assert_eq!(usages[1]["provider_usage"]["cache_read_input_tokens"], 2);
        assert_eq!(
            usages[2]["provider_usage"],
            json!({
                "input_tokens":3,"output_tokens":4,"cache_read_input_tokens":5,
                "cache_creation_input_tokens":2,"future_usage":{"tier":"late"}
            })
        );
        let metadata = chunks
            .iter()
            .find_map(|chunk| {
                let metadata = &chunk["choices"][0]["delta"]["provider_metadata"][namespace];
                metadata.get("native_content").map(|_| metadata)
            })
            .unwrap();
        assert_eq!(metadata["message"]["stop_reason"], "end_turn");
        assert_eq!(metadata["message"]["stop_sequence"], "native-stop");
        assert_eq!(
            metadata["native_content"]["blocks"][0]["future_block"],
            json!({"opaque":true})
        );
    }
}

#[tokio::test]
async fn compatibility_chat_usage_overflow_clears_prior_total_without_losing_usage() {
    for namespace in ["aws_bedrock", "github_copilot"] {
        for initial in [
            json!({"input_tokens":i64::MAX,"output_tokens":0}),
            json!({"input_tokens":1,"output_tokens":0,"cache_read_input_tokens":i64::MAX}),
            json!({"input_tokens":1,"output_tokens":0,"cache_creation_input_tokens":i64::MAX}),
        ] {
            let (chunks, done) = compatibility_chat_chunks(vec![
                json!({"type":"message_start","message":{"usage":initial}}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
                json!({"type":"message_stop"}),
            ], namespace).await;
            assert!(done, "{namespace}");
            assert!(chunks.iter().all(|chunk| chunk.get("error").is_none()));
            if initial["input_tokens"] == i64::MAX {
                assert_eq!(
                    chunks.iter().find_map(|chunk| chunk.get("usage")).unwrap()["total_tokens"],
                    i64::MAX
                );
            }
            let usage = chunks
                .iter()
                .filter_map(|chunk| chunk.get("usage"))
                .next_back()
                .unwrap();
            assert_eq!(
                usage.get("total_tokens"),
                Some(&Value::Null),
                "{namespace}: {usage}"
            );
            let mut expected = initial.clone();
            expected["output_tokens"] = json!(1);
            assert_eq!(usage["provider_usage"], expected);
            assert_eq!(usage["completion_tokens"], 1);
        }
    }
}
