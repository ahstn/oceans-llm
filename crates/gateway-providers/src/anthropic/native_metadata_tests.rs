use bytes::Bytes;
use futures_util::{StreamExt, stream};
use gateway_core::{
    AnthropicMessagesRequest, ProviderRequestContext, SseEventParser,
    anthropic_messages_request_to_core, protocol::anthropic::anthropic_message_from_openai_chat,
};
use serde_json::{Value, json};

use super::{AnthropicRequestOptions, map_anthropic_request, normalize_anthropic_response};
use crate::bedrock::{
    AnthropicMessagesTarget, map_chat_request_to_anthropic_messages,
    normalize_anthropic_messages_response, normalize_anthropic_messages_stream,
};

fn context() -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "native-metadata-test".into(),
        model_key: "claude".into(),
        provider_key: "anthropic".into(),
        upstream_model: "claude-sonnet-4-6".into(),
        owner_user_id: None,
        expected_provider_credential_id: None,
        chat_protocol: gateway_core::ChatRequestProtocol::AnthropicMessages,
        extra_headers: Default::default(),
        extra_body: Default::default(),
        request_headers: Default::default(),
        compatibility: Default::default(),
    }
}

#[test]
fn native_effort_messages_and_safeguards_survive_provider_mapping() {
    for content in [
        json!([]),
        json!([{ "type": "text", "text": "Use lower effort now." }]),
    ] {
        let messages = json!([
            {"role": "user", "content": [{"type": "text", "text": "hello"}]},
            {"role": "system", "content": content, "output_config": {"effort": "low"}}
        ]);
        let safeguards = json!({"opaque": {"classifier": "requested"}});
        let native: AnthropicMessagesRequest = serde_json::from_value(json!({
            "model": "claude", "max_tokens": 1024,
            "system": [{"type": "text", "text": "System instruction"}],
            "messages": messages, "safeguards": safeguards,
            "anthropic_beta": ["mid-conversation-output-config-2026-07-01"]
        }))
        .unwrap();
        let request = anthropic_messages_request_to_core(&native);
        let context = ProviderRequestContext {
            upstream_model: "claude-opus-5-5".into(),
            ..context()
        };
        let mapped = [
            map_anthropic_request(
                &request,
                &context,
                false,
                &AnthropicRequestOptions::default(),
            )
            .unwrap(),
            map_chat_request_to_anthropic_messages(
                &request,
                &context,
                AnthropicMessagesTarget::RuntimeInvoke,
            )
            .unwrap(),
            map_chat_request_to_anthropic_messages(
                &request,
                &context,
                AnthropicMessagesTarget::MantleMessages,
            )
            .unwrap(),
        ];
        for body in mapped {
            assert_eq!(body["messages"].as_array().unwrap().len(), 2);
            assert_eq!(body["messages"][0]["role"], "user");
            let user_content = &body["messages"][0]["content"];
            assert!(user_content == "hello" || user_content == &messages[0]["content"]);
            assert_eq!(body["messages"][1], messages[1]);
            assert_eq!(body["system"], native.system.as_ref().unwrap().clone());
            assert_eq!(body["safeguards"], safeguards);
            assert_eq!(
                body["anthropic_beta"],
                json!(["mid-conversation-output-config-2026-07-01"])
            );
        }
    }
}

#[test]
fn native_response_round_trip_preserves_opaque_safeguard_results_and_tool_ids() {
    let upstream = json!({
        "id": "msg_native", "type": "message", "role": "assistant",
        "content": [
            {"type": "thinking", "thinking": "", "signature": "signed"},
            {"type": "tool_use", "id": "toolu_exact", "name": "read", "input": {"path": "src"}}
        ],
        "stop_reason": "tool_use", "usage": {"input_tokens": 4, "output_tokens": 2},
        "safeguard_results": {"opaque": [{"tool_use_id": "toolu_exact", "verdict": "test"}]}
    });
    for namespace in [
        "anthropic_compat",
        "gcp_vertex",
        "github_copilot",
        "aws_bedrock",
    ] {
        let context = context();
        for normalized in [
            normalize_anthropic_response(&upstream, &context, namespace, "anthropic"),
            normalize_anthropic_messages_response(&upstream, &context, namespace),
        ] {
            let response = anthropic_message_from_openai_chat(&normalized, "claude");
            assert_eq!(response["safeguard_results"], upstream["safeguard_results"]);
            assert_eq!(response["content"][0], upstream["content"][0]);
            assert_eq!(response["content"][1], upstream["content"][1]);
            assert!(response.get("provider_metadata").is_none());
        }
    }
}

#[tokio::test]
async fn safeguard_streams_start_with_assistant_role_and_usage() {
    let results = json!({"opaque": [{"tool_use_id": "toolu_exact", "verdict": "test"}]});
    for shared in [false, true] {
        let events = [
            json!({"type": "message_start", "message": {
                "id": "msg_1", "safeguard_results": results,
                "usage": {"input_tokens": 7, "output_tokens": 0}
            }}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 2}}),
            json!({"type": "message_stop"}),
        ];
        let upstream = stream::iter(
            events
                .into_iter()
                .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
        );
        let normalized = if shared {
            super::normalize_anthropic_stream(
                upstream,
                "msg_1".into(),
                0,
                "claude".into(),
                "anthropic_compat",
                "anthropic",
            )
        } else {
            normalize_anthropic_messages_stream(upstream, context(), "anthropic_compat")
        };
        let chunks = normalized.collect::<Vec<_>>().await;
        let mut parser = SseEventParser::default();
        let mut parsed = Vec::new();
        for chunk in chunks {
            for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
                if event.data != "[DONE]" {
                    parsed.push(serde_json::from_str::<Value>(&event.data).unwrap());
                }
            }
        }
        assert_eq!(parsed[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(parsed[0]["usage"]["prompt_tokens"], 7);
        assert_eq!(parsed[0]["usage"]["completion_tokens"], 0);
        assert_eq!(
            parsed[0]["choices"][0]["delta"]["provider_metadata"]["anthropic_compat"]["message"]["safeguard_results"],
            results
        );
        assert_eq!(
            parsed
                .iter()
                .filter(|chunk| chunk["choices"][0]["delta"]["role"] == "assistant")
                .count(),
            1
        );
        assert_eq!(parsed.last().unwrap()["usage"]["prompt_tokens"], 7);
        assert_eq!(parsed.last().unwrap()["usage"]["completion_tokens"], 2);
    }
}

#[tokio::test]
async fn bedrock_stream_preserves_safeguards_without_usage_or_stop_reason() {
    let events = [
        json!({"type": "message_start", "message": {"id": "msg_1", "safeguard_results": []}}),
        json!({"type": "message_delta", "delta": {"safeguard_results": {"opaque": true}}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
        json!({"type": "message_stop"}),
    ];
    let upstream = stream::iter(
        events
            .into_iter()
            .map(|event| Ok(Bytes::from(format!("data: {event}\n\n")))),
    );
    let chunks = normalize_anthropic_messages_stream(upstream, context(), "aws_bedrock")
        .collect::<Vec<_>>()
        .await;
    let mut parser = SseEventParser::default();
    let mut metadata = Vec::new();
    for chunk in chunks {
        for event in parser.push_bytes(&chunk.unwrap()).unwrap() {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                metadata
                    .push(value["choices"][0]["delta"]["provider_metadata"]["aws_bedrock"].clone());
            }
        }
    }
    assert!(metadata.contains(&json!({"message": {"safeguard_results": []}})));
    assert!(
        metadata.contains(
            &json!({"message_delta": {"delta": {"safeguard_results": {"opaque": true}}}})
        )
    );
}
