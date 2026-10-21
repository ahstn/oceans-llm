use super::*;

use gateway_core::{
    anthropic_messages_request_to_core, protocol::anthropic::AnthropicMessagesRequest,
};

type CapturedRequests = Arc<Mutex<Vec<(String, Value)>>>;

#[test]
fn vertex_native_opus_4_5_effort_beta_is_independent_of_thinking_mode() {
    for (controls, needs_beta) in [
        (json!({"output_config": {"effort": "high"}}), true),
        (
            json!({"thinking": {"type": "disabled"}, "output_config": {"effort": "high"}}),
            true,
        ),
        (json!({"output_config": {"effort": null}}), false),
        (
            json!({"thinking": {"type": "disabled"}, "output_config": {"effort": null}}),
            false,
        ),
    ] {
        let mut payload = json!({
            "model": "claude", "max_tokens": 4096,
            "messages": [{"role": "user", "content": "ping"}]
        });
        payload
            .as_object_mut()
            .unwrap()
            .extend(controls.as_object().unwrap().clone());
        let native: AnthropicMessagesRequest = serde_json::from_value(payload).unwrap();
        let request = anthropic_messages_request_to_core(&native);
        for stream in [false, true] {
            let mapped = anthropic_body(&request, &context("anthropic/claude-opus-4-5"), stream)
                .expect("native effort mapping");
            assert_eq!(mapped.get("thinking"), controls.get("thinking"));
            assert_eq!(mapped.get("output_config"), controls.get("output_config"));
            if needs_beta {
                assert_eq!(mapped["anthropic_beta"], json!(["effort-2025-11-24"]));
            } else {
                assert!(mapped.get("anthropic_beta").is_none());
            }
        }
    }
}

async fn capture_anthropic_request(
    Path(path): Path<String>,
    State(captured): State<CapturedRequests>,
    Json(payload): Json<Value>,
) -> Response {
    let streaming = payload["stream"] == true;
    captured.lock().await.push((path, payload));
    let message = json!({
        "id": "msg_opus", "type": "message", "role": "assistant",
        "model": "claude-opus-5-5", "content": [{"type": "text", "text": "pong"}],
        "stop_reason": "end_turn", "usage": {"input_tokens": 2, "output_tokens": 1}
    });
    let (content_type, body) = if streaming {
        let events = [
            json!({"type": "message_start", "message": {
                "id": "msg_opus", "usage": {"input_tokens": 2, "output_tokens": 0}
            }}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "pong"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 1}}),
            json!({"type": "message_stop"}),
        ];
        let frames = events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap()
                )
            })
            .collect::<String>();
        ("text/event-stream", frames)
    } else {
        ("application/json", message.to_string())
    };
    Response::builder()
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn vertex_native_messages_preserves_opus_5_5_thinking_controls_over_http() {
    let captured: CapturedRequests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/{*path}", post(capture_anthropic_request))
        .with_state(captured.clone());
    let host = start_router(app).await;
    let provider = vertex_provider_for_test(format!("http://{host}"));
    let context = context("anthropic/claude-opus-5-5");
    let mut cases = vec![
        json!({}),
        json!({"thinking": {"type": "adaptive", "display": "summarized"}}),
        json!({"output_config": {"effort": "high"}}),
    ];
    for display in ["summarized", "omitted"] {
        for effort in ["low", "medium", "high", "xhigh", "max"] {
            cases.push(json!({
                "thinking": {"type": "adaptive", "display": display},
                "output_config": {"effort": effort}
            }));
        }
    }

    for controls in cases {
        for streaming in [false, true] {
            let mut payload = json!({
                "model": "claude-opus-5.5", "max_tokens": 128000, "stream": streaming,
                "messages": [{"role": "user", "content": "ping"}]
            });
            payload
                .as_object_mut()
                .unwrap()
                .extend(controls.as_object().unwrap().clone());
            let native: AnthropicMessagesRequest = serde_json::from_value(payload).unwrap();
            let request = anthropic_messages_request_to_core(&native);
            if streaming {
                let chunks: Vec<_> = provider
                    .chat_completions_stream(&request, &context)
                    .await
                    .expect("Vertex streaming request")
                    .collect()
                    .await;
                assert!(!chunks.is_empty());
                assert!(chunks.iter().all(Result::is_ok));
            } else {
                let response = provider
                    .chat_completions(&request, &context)
                    .await
                    .expect("Vertex request");
                assert_eq!(response["choices"][0]["message"]["content"], "pong");
            }
            let (path, sent) = captured.lock().await.pop().expect("outbound HTTP request");
            let operation = if streaming {
                "streamRawPredict"
            } else {
                "rawPredict"
            };
            assert_eq!(
                path,
                format!(
                    "projects/proj-123/locations/global/publishers/anthropic/models/claude-opus-5-5:{operation}"
                )
            );
            assert_eq!(sent["anthropic_version"], "vertex-2023-10-16");
            assert_eq!(sent["max_tokens"], 128000);
            assert_eq!(sent["stream"], streaming);
            assert!(sent.get("model").is_none());
            assert_eq!(sent.get("thinking"), controls.get("thinking"));
            assert_eq!(sent.get("output_config"), controls.get("output_config"));
        }
    }
}
