use super::beta_transport_tests::{context, provider};
use axum::{
    Json, Router,
    body::Body,
    extract::{OriginalUri, State},
    http::HeaderMap,
    response::Response,
    routing::post,
};
use futures_util::StreamExt;
use gateway_core::{
    AnthropicMessagesRequest, ProviderClient, ProviderRequestContext,
    anthropic_messages_request_to_core,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Mutex;

type Capture = Arc<Mutex<Option<(String, HeaderMap, Value)>>>;

fn native_response() -> Value {
    json!({"id":"msg_native", "type":"message", "role":"assistant", "model":"upstream-model",
        "content":[
            {"type":"text","text":"Checking", "citations":[{"type":"future_citation","opaque":true}]},
            {"type":"thinking","thinking":"","signature":"signed"},
            {"type":"server_tool_use","id":"srvtoolu_exact","name":"web_search","input":{"query":"synthetic"}},
            {"type":"future_block","opaque":[1,2,3]}
        ],
        "stop_reason":"pause_turn", "stop_sequence":null,
        "usage":{"input_tokens":3,"output_tokens":2,"cache_read_input_tokens":100,"cache_creation_input_tokens":7,"future_usage":true},
        "container":{"id":"opaque"}, "input_transformations":[{"opaque":true}], "future_response":true})
}

const NATIVE_SSE: &str = concat!(
    ": keepalive\r\n\r\n",
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_native\",\"model\":\"upstream-model\",\"usage\":{\"input_tokens\":3,\"cache_read_input_tokens\":100}}}\n\n",
    "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":4,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":4,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"signed\"}}\n\n",
    "id: future-id\nretry: 250\nevent: future_event\ndata: {\"type\":\"future_event\",\"opaque\":true}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":4}\n\n",
    "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\",\"future_delta\":true},\"usage\":{\"output_tokens\":2}}\n\n",
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
);

async fn capture(
    OriginalUri(uri): OriginalUri,
    State(captured): State<Capture>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let stream = body["stream"] == true;
    *captured.lock().await = Some((uri.path().to_string(), headers, body));
    Response::builder()
        .header(
            "content-type",
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .header("anthropic-ratelimit-requests-remaining", "17")
        .header("anthropic-future-header", "opaque")
        .header("x-openrouter-future", "opaque")
        .header("x-ratelimit-remaining", "17")
        .header("ratelimit-reset", "2")
        .header("x-should-retry", "false")
        .header("retry-after", "2")
        .header("set-cookie", "must-not-forward")
        .body(Body::from(if stream {
            NATIVE_SSE.to_string()
        } else {
            native_response().to_string()
        }))
        .unwrap()
}

fn native_request(stream: bool) -> AnthropicMessagesRequest {
    serde_json::from_value(json!({"model":"claude","max_tokens":1024,"stream":stream,
        "system":[{"type":"text","text":"Root", "cache_control":{"type":"ephemeral"}}],
        "messages":[
            {"role":"user","content":"hello","cache_control":{"type":"ephemeral"},"future_message":true},
            {"role":"system","content":"Stay in sequence","future_system":true},
            {"role":"assistant","content":[{"type":"text","text":"Before"},{"type":"thinking","thinking":"","signature":"signed"}]},
            {"role":"user","content":"continue"}
        ],
        "thinking":{"type":"adaptive","display":"summarized","block_binding":{"prefix_mismatch_behavior":"drop_block"}},
        "output_config":{"effort":"high","future_option":true},
        "context_management":{"future_edit":true},"future_request":true
    })).unwrap()
}

fn openrouter(base: &str) -> Box<dyn ProviderClient> {
    let mut config = crate::OpenAiCompatConfig::new("openrouter".into(), format!("{base}/api/v1"));
    config.bearer_token = Some("upstream-secret".into());
    Box::new(crate::OpenAiCompatProvider::new(config).unwrap())
}

fn openrouter_context() -> ProviderRequestContext {
    let mut context = context("openrouter");
    context.upstream_model = "~anthropic/claude-opus-latest".into();
    context.compatibility.openrouter = Some(gateway_core::OpenRouterRouteCompatibility {
        provider: Default::default(),
        api: Default::default(),
    });
    context
}

#[tokio::test]
async fn native_messages_preserve_bodies_headers_and_sse_across_transports() {
    for kind in [
        "direct",
        "vertex",
        "runtime",
        "mantle",
        "openrouter",
        "copilot",
    ] {
        let captured = Capture::default();
        let router = Router::new()
            .route("/{*path}", post(capture))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (provider, mut context) = if kind == "openrouter" {
            (openrouter(&base), openrouter_context())
        } else if kind == "copilot" {
            let mut config = crate::CopilotProviderConfig::new(
                "copilot".into(),
                crate::CopilotAuthConfig::Bearer {
                    token: "upstream-secret".into(),
                },
            );
            config.base_url = base.clone();
            let mut context = context("copilot");
            context.compatibility.github_copilot =
                Some(gateway_core::GitHubCopilotRouteCompatibility {
                    chat_api: Some(gateway_core::GitHubCopilotChatApi::AnthropicMessages),
                    supports_responses: false,
                    supports_embeddings: false,
                    upstream_supports: gateway_core::GitHubCopilotUpstreamSupports {
                        streaming: true,
                        ..Default::default()
                    },
                });
            (
                Box::new(crate::CopilotProvider::new(config).unwrap()) as Box<dyn ProviderClient>,
                context,
            )
        } else {
            (provider(kind, &base), context(kind))
        };
        context
            .request_headers
            .insert("anthropic-future-capability".into(), "opaque".into());
        for streaming in [false, true] {
            if kind == "runtime" && streaming {
                continue;
            }
            let native = native_request(streaming);
            let request = anthropic_messages_request_to_core(&native);
            assert!(provider.supports_native_messages(&context), "{kind}");
            let headers = if streaming {
                let response = provider.messages_stream(&request, &context).await.unwrap();
                let chunks = response.body.collect::<Vec<_>>().await;
                let bytes: Vec<_> = chunks
                    .into_iter()
                    .flat_map(|chunk| chunk.unwrap().to_vec())
                    .collect();
                assert_eq!(
                    bytes,
                    NATIVE_SSE.as_bytes(),
                    "{kind}: native stream changed"
                );
                response.headers
            } else {
                let response = provider.messages(&request, &context).await.unwrap();
                assert_eq!(
                    response.body,
                    native_response(),
                    "{kind}: native response changed"
                );
                response.headers
            };
            assert_eq!(headers["anthropic-future-header"], "opaque");
            assert_eq!(headers["x-should-retry"], "false");
            assert_eq!(headers["retry-after"], "2");
            assert_eq!(headers["x-ratelimit-remaining"], "17");
            assert_eq!(headers["ratelimit-reset"], "2");
            assert!(!headers.contains_key("set-cookie"));
            let (path, sent_headers, body) = captured.lock().await.take().unwrap();
            if kind == "openrouter" {
                assert_eq!(path, "/api/v1/messages");
                assert_eq!(sent_headers["authorization"], "Bearer upstream-secret");
            }
            if kind != "runtime" {
                assert_eq!(sent_headers["anthropic-future-capability"], "opaque");
                assert!(body.get("anthropic_beta").is_none());
                assert!(
                    sent_headers["anthropic-beta"]
                        .to_str()
                        .unwrap()
                        .contains("future-capability-2099-01-01")
                );
            }
            assert_eq!(
                body["messages"],
                serde_json::to_value(&native.messages).unwrap(),
                "{kind}"
            );
            assert_eq!(body["system"], native.system.unwrap(), "{kind}");
            assert_eq!(body["thinking"], native.thinking.unwrap(), "{kind}");
            assert_eq!(body["future_request"], true);
            assert_eq!(body["context_management"], json!({"future_edit":true}));
            assert!(!sent_headers.contains_key("cookie"));
            assert_ne!(
                sent_headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok()),
                Some("Bearer gateway-secret")
            );
        }
        server.abort();
    }
}

#[test]
fn rolling_native_controls_match_explicit_models_and_unknown_fields_reach_upstream() {
    for stream in [false, true] {
        for effort in [
            None,
            Some(Value::Null),
            Some(json!("low")),
            Some(json!("medium")),
            Some(json!("high")),
            Some(json!("xhigh")),
            Some(json!("max")),
        ] {
            let mut native = native_request(stream);
            if let Some(effort) = effort {
                native
                    .extra
                    .insert("output_config".into(), json!({"effort":effort}));
            } else {
                native.extra.remove("output_config");
            }
            let request = anthropic_messages_request_to_core(&native);
            let mut alias = openrouter_context();
            let mut body =
                super::map_anthropic_request(&request, &alias, stream, &Default::default())
                    .unwrap();
            alias.upstream_model = "anthropic/claude-opus-5.5".into();
            let mut explicit =
                super::map_anthropic_request(&request, &alias, stream, &Default::default())
                    .unwrap();
            body.as_object_mut().unwrap().remove("model");
            explicit.as_object_mut().unwrap().remove("model");
            assert_eq!(body, explicit);
        }
    }
    let mut native = native_request(false);
    native.thinking = Some(json!({"type":"future_thinking","display":"future_display"}));
    native
        .extra
        .insert("output_config".into(), json!({"effort":"future_effort"}));
    let request = anthropic_messages_request_to_core(&native);
    for model in [
        "~anthropic/claude-opus-latest",
        "private-deployment",
        "claude-opus-9-1",
    ] {
        let mut context = context("direct");
        context.upstream_model = model.into();
        let body =
            super::map_anthropic_request(&request, &context, false, &Default::default()).unwrap();
        assert_eq!(body["thinking"], native.thinking.as_ref().unwrap().clone());
        assert_eq!(body["output_config"], native.extra["output_config"]);
    }
}

#[tokio::test]
async fn native_provider_failures_preserve_error_body_and_retry_headers() {
    let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"synthetic limit","future_error":true}}"#;
    let router = Router::new().route(
        "/{*path}",
        post(move || async move {
            Response::builder()
                .status(429)
                .header("retry-after", "3")
                .header("x-should-retry", "false")
                .header("anthropic-future-header", "opaque")
                .header("set-cookie", "must-not-forward")
                .body(Body::from(body))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let provider = provider("direct", &base);
    let context = context("direct");
    for stream in [false, true] {
        let request = anthropic_messages_request_to_core(&native_request(stream));
        let error = if stream {
            provider
                .messages_stream(&request, &context)
                .await
                .err()
                .unwrap()
        } else {
            provider.messages(&request, &context).await.err().unwrap()
        };
        assert!(!error.is_retryable());
        match error {
            gateway_core::ProviderError::NativeMessagesHttp {
                status,
                body: actual,
                headers,
            } => {
                assert_eq!(status, 429);
                assert_eq!(actual, body);
                assert_eq!(headers["retry-after"], "3");
                assert_eq!(headers["anthropic-future-header"], "opaque");
                assert!(!headers.contains_key("set-cookie"));
            }
            error => panic!("unexpected error: {error}"),
        }
    }
    server.abort();
}
