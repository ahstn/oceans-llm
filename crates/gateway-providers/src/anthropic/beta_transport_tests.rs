use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{OriginalUri, State},
    http::HeaderMap,
    response::Response,
    routing::post,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures_util::StreamExt;
use gateway_core::{
    AnthropicMessagesRequest, AwsBedrockApiStyle, AwsBedrockRouteCompatibility,
    ChatRequestProtocol, ProviderClient, ProviderRequestContext, RouteCompatibility,
    anthropic_messages_request_to_core,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    AnthropicCompatAuth, AnthropicCompatAuthKind, AnthropicCompatConfig, AnthropicCompatProvider,
    BedrockAuthConfig, BedrockEndpointKind, BedrockProvider, BedrockProviderConfig,
    VertexAuthConfig, VertexProvider, VertexProviderConfig,
};

const CALLER_BETAS: &str = "mid-conversation-output-config-2026-07-01, context-management-2025-06-27, future-capability-2099-01-01";
const EXPECTED_BETAS: &str = "files-api-2025-04-14,context-management-2025-06-27,thinking-display-updates-2026-08-18,mid-conversation-output-config-2026-07-01,future-capability-2099-01-01";
type Captured = Arc<Mutex<Option<(String, HeaderMap, Value)>>>;

async fn capture(
    OriginalUri(uri): OriginalUri,
    State(captured): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let streaming = body["stream"] == true;
    *captured.lock().await = Some((uri.path().to_string(), headers, body));
    let (content_type, response) = if uri.path().contains("count") {
        (
            "application/json",
            json!({"input_tokens": 3, "inputTokens": 3}).to_string(),
        )
    } else if streaming {
        ("text/event-stream", concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_beta\",\"usage\":{\"input_tokens\":3}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ).to_string())
    } else {
        (
            "application/json",
            json!({
                "id": "msg_beta", "type": "message", "role": "assistant",
                "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn",
                "usage": {"input_tokens": 3, "output_tokens": 1}
            })
            .to_string(),
        )
    };
    Response::builder()
        .header("content-type", content_type)
        .body(Body::from(response))
        .unwrap()
}

fn context(kind: &str) -> ProviderRequestContext {
    let upstream_model = match kind {
        "vertex" => "anthropic/claude-opus-5-5",
        "runtime" | "mantle" => "anthropic.claude-opus-5-5",
        _ => "claude-opus-5-5",
    };
    let api_style = match kind {
        "runtime" => Some(AwsBedrockApiStyle::RuntimeAnthropicInvoke),
        "mantle" => Some(AwsBedrockApiStyle::MantleAnthropicMessages),
        _ => None,
    };
    ProviderRequestContext {
        request_id: "req-beta".into(),
        model_key: "claude".into(),
        provider_key: kind.into(),
        upstream_model: upstream_model.into(),
        owner_user_id: None,
        expected_provider_credential_id: None,
        chat_protocol: ChatRequestProtocol::AnthropicMessages,
        extra_headers: serde_json::from_value(json!({
            "ANTHROPIC-BETA": "thinking-display-updates-2026-08-18, context-management-2025-06-27"
        }))
        .unwrap(),
        extra_body: Default::default(),
        request_headers: BTreeMap::from([
            ("Anthropic-Beta".into(), CALLER_BETAS.into()),
            ("authorization".into(), "Bearer gateway-secret".into()),
            ("x-api-key".into(), "gateway-secret".into()),
            ("cookie".into(), "private-cookie".into()),
            ("x-private-header".into(), "private-value".into()),
            ("host".into(), "untrusted.invalid".into()),
        ]),
        compatibility: RouteCompatibility {
            aws_bedrock: api_style.map(|api_style| AwsBedrockRouteCompatibility {
                api_style,
                openai_base_path: None,
                supports_strict_tools: None,
            }),
            ..Default::default()
        },
    }
}

fn provider(kind: &str, base_url: &str) -> Box<dyn ProviderClient> {
    let default_headers = BTreeMap::from([(
        "anthropic-beta".into(),
        "context-management-2025-06-27".into(),
    )]);
    match kind {
        "direct" => {
            let mut config = AnthropicCompatConfig::new(kind.into(), base_url.into());
            config.auth = Some(AnthropicCompatAuth {
                kind: AnthropicCompatAuthKind::XApiKey,
                token: "upstream-secret".into(),
            });
            config.default_headers = default_headers;
            Box::new(AnthropicCompatProvider::new(config).unwrap())
        }
        "vertex" => Box::new(
            VertexProvider::new(VertexProviderConfig {
                provider_key: kind.into(),
                project_id: "test-project".into(),
                location: "global".into(),
                api_host: base_url.into(),
                auth: VertexAuthConfig::Bearer {
                    token: "upstream-secret".into(),
                },
                default_headers,
                request_timeout_ms: 5_000,
                batch: None,
            })
            .unwrap(),
        ),
        _ => Box::new(
            BedrockProvider::new(BedrockProviderConfig {
                provider_key: kind.into(),
                region: "us-east-1".into(),
                endpoint_url: base_url.into(),
                endpoint_kind: if kind == "runtime" {
                    BedrockEndpointKind::BedrockRuntime
                } else {
                    BedrockEndpointKind::BedrockMantle
                },
                auth: BedrockAuthConfig::Bearer {
                    token: "upstream-secret".into(),
                },
                default_headers,
                request_timeout_ms: 5_000,
            })
            .unwrap(),
        ),
    }
}

fn assert_transport(kind: &str, headers: &HeaderMap, body: &Value) {
    assert!(!headers.contains_key("cookie"));
    assert!(!headers.contains_key("x-private-header"));
    assert_ne!(headers["host"], "untrusted.invalid");
    if matches!(kind, "direct" | "mantle") {
        assert_eq!(headers["x-api-key"], "upstream-secret");
        assert!(!headers.contains_key("authorization"));
    } else {
        assert_eq!(headers["authorization"], "Bearer upstream-secret");
        assert!(!headers.contains_key("x-api-key"));
    }
    if kind == "runtime" {
        assert!(!headers.contains_key("anthropic-beta"));
        assert_eq!(
            body["anthropic_beta"],
            json!(EXPECTED_BETAS.split(',').collect::<Vec<_>>())
        );
    } else {
        assert_eq!(headers.get_all("anthropic-beta").iter().count(), 1);
        assert_eq!(headers["anthropic-beta"], EXPECTED_BETAS);
        assert!(body.get("anthropic_beta").is_none());
    }
}

#[tokio::test]
async fn native_messages_beta_controls_reach_each_provider_over_http() {
    let captured: Captured = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(captured.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    for kind in ["direct", "vertex", "runtime", "mantle"] {
        let provider = provider(kind, &url);
        let context = context(kind);
        for streaming in [false, true] {
            if kind == "runtime" && streaming {
                continue;
            }
            let native: AnthropicMessagesRequest = serde_json::from_value(json!({
                "model": "claude", "max_tokens": 1024, "stream": streaming,
                "messages": [
                    {"role": "user", "content": "hello"},
                    {"role": "system", "content": [], "output_config": {"effort": "low"}}
                ],
                "thinking": {"type": "adaptive", "display": "updates"},
                "context_management": {"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]},
                "anthropic_beta": ["files-api-2025-04-14", "files-api-2025-04-14"]
            })).unwrap();
            let request = anthropic_messages_request_to_core(&native);
            if streaming {
                let chunks: Vec<_> = provider
                    .chat_completions_stream(&request, &context)
                    .await
                    .unwrap()
                    .collect()
                    .await;
                assert!(chunks.iter().all(Result::is_ok), "{kind}");
            } else {
                provider.chat_completions(&request, &context).await.unwrap();
            }
            let (path, headers, body) = captured.lock().await.take().unwrap();
            assert_transport(kind, &headers, &body);
            assert_eq!(body["thinking"]["display"], "updates");
            assert_eq!(body["messages"][1]["output_config"]["effort"], "low");
            assert_eq!(
                body["context_management"],
                request.extra["context_management"]
            );
            assert!(path.contains(if kind == "runtime" {
                "/invoke"
            } else if kind == "vertex" {
                "Predict"
            } else {
                "/messages"
            }));
        }
    }
    server.abort();
}

#[tokio::test]
async fn native_count_tokens_forwards_caller_beta_headers_over_http() {
    let captured: Captured = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(captured.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    for kind in ["direct", "vertex", "runtime"] {
        let provider = provider(kind, &url);
        let context = context(kind);
        let native: AnthropicMessagesRequest = serde_json::from_value(json!({
            "model": "claude", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hello"}],
            "anthropic_beta": ["files-api-2025-04-14"]
        }))
        .unwrap();
        let request = anthropic_messages_request_to_core(&native);
        provider.count_tokens(&request, &context).await.unwrap();
        let (path, headers, mut body) = captured.lock().await.take().unwrap();
        assert!(path.contains("count"));
        if kind == "runtime" {
            assert!(body.get("anthropic_beta").is_none());
            let encoded = body["input"]["invokeModel"]["body"].as_str().unwrap();
            body = serde_json::from_slice(&BASE64_STANDARD.decode(encoded).unwrap()).unwrap();
        }
        assert_transport(kind, &headers, &body);
    }
    server.abort();
}

#[tokio::test]
async fn native_messages_body_beta_preserves_context_management_over_http() {
    let captured: Captured = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(captured.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let provider =
        AnthropicCompatProvider::new(AnthropicCompatConfig::new("direct".into(), url)).unwrap();
    let mut context = context("direct");
    context.extra_headers.clear();
    context.request_headers.clear();
    let native: AnthropicMessagesRequest = serde_json::from_value(json!({
        "model": "claude", "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hello"}],
        "anthropic_beta": ["context-management-2025-06-27"],
        "context_management": {"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]}
    }))
    .unwrap();
    let request = anthropic_messages_request_to_core(&native);
    provider.chat_completions(&request, &context).await.unwrap();
    let (_, headers, body) = captured.lock().await.take().unwrap();
    assert_eq!(headers["anthropic-beta"], "context-management-2025-06-27");
    assert_eq!(
        body["context_management"],
        request.extra["context_management"]
    );
    assert!(body.get("anthropic_beta").is_none());
    server.abort();
}
