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
    provider_with_headers(kind, base_url, default_headers)
}

fn provider_with_headers(
    kind: &str,
    base_url: &str,
    default_headers: BTreeMap<String, String>,
) -> Box<dyn ProviderClient> {
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

#[test]
fn beta_merge_rejects_resource_limit_overflows() {
    let context = context("direct");
    let defaults = BTreeMap::new();
    for value in [
        json!(
            (0..1_025)
                .map(|index| format!("future-beta-{index}"))
                .collect::<Vec<_>>()
        ),
        json!(vec!["duplicate"; 1_025]),
        json!(["a".repeat(257)]),
        json!(["é".repeat(129)]),
        json!(",".repeat(256 * 1024 + 1)),
    ] {
        let mut body = json!({"anthropic_beta": value});
        let error = super::beta::take_request_betas(&mut body, &context, &defaults)
            .expect_err("oversized beta metadata must fail before HTTP dispatch");
        assert!(matches!(
            error,
            gateway_core::ProviderError::InvalidRequest(_)
        ));
        assert!(error.to_string().contains("beta"));
        assert!(error.to_string().contains("limit"));
    }
}

#[test]
fn beta_merge_preserves_order_and_checks_combined_boundaries() {
    let mut context = context("direct");
    context.extra_headers.clear();
    context.request_headers.clear();
    let mut defaults = BTreeMap::new();
    let names = (0..1_024)
        .map(|index| format!("future-beta-{index}"))
        .collect::<Vec<_>>();
    let body = json!(names);
    context
        .request_headers
        .insert("anthropic-beta".into(), names.join(","));
    let merged =
        super::beta::merged_request_betas(Some(&body), &context, Some(&defaults), &[]).unwrap();
    assert_eq!(
        merged, names,
        "canonical names repeated in headers must not consume the distinct-name limit twice"
    );
    defaults.insert("anthropic-beta".into(), "another-future-beta".into());
    let error =
        super::beta::merged_request_betas(Some(&body), &context, Some(&defaults), &[]).unwrap_err();
    assert!(error.to_string().contains("distinct name count"));
    let error =
        super::beta::merged_request_betas(Some(&body), &context, None, &["effort-2025-11-24"])
            .unwrap_err();
    assert!(error.to_string().contains("distinct name count"));

    defaults.insert("anthropic-beta".into(), "second,first".into());
    context
        .extra_headers
        .insert("Anthropic-Beta".into(), json!("third,second"));
    context
        .request_headers
        .insert("anthropic-beta".into(), "fourth,first".into());
    let body = json!(["first", "first"]);
    assert_eq!(
        super::beta::merged_request_betas(
            Some(&body),
            &context,
            Some(&defaults),
            &["fifth", "second"]
        )
        .unwrap(),
        ["first", "second", "third", "fourth", "fifth"]
    );
    let body = json!(["é".repeat(128)]);
    assert!(super::beta::merged_request_betas(Some(&body), &context, None, &[]).is_ok());

    defaults.insert("anthropic-beta".into(), ",".repeat(128 * 1_024));
    context
        .extra_headers
        .insert("Anthropic-Beta".into(), json!(",".repeat(128 * 1_024)));
    context.request_headers.clear();
    assert!(
        super::beta::merged_request_betas(None, &context, Some(&defaults), &[])
            .unwrap()
            .is_empty()
    );
    context
        .request_headers
        .insert("anthropic-beta".into(), ",".into());
    let error =
        super::beta::merged_request_betas(None, &context, Some(&defaults), &[]).unwrap_err();
    assert!(error.to_string().contains("combined header byte length"));
}

#[tokio::test]
async fn beta_resource_limits_prevent_provider_http_dispatch() {
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
        for source in [
            "body-array",
            "body-name",
            "body-string",
            "route-body",
            "caller-header",
            "route-header",
            "provider-header",
            "merged-count",
            "vertex-effort",
        ] {
            if source == "vertex-effort" && kind != "vertex" {
                continue;
            }
            let mut context = context(kind);
            context.extra_headers.clear();
            context.request_headers.clear();
            let mut defaults = BTreeMap::new();
            let native: AnthropicMessagesRequest = serde_json::from_value(json!({
                "model": "claude", "max_tokens": 1024,
                "messages": [{"role": "user", "content": "never dispatch"}]
            }))
            .unwrap();
            let mut request = anthropic_messages_request_to_core(&native);
            match source {
                "body-array" => {
                    request
                        .extra
                        .insert("anthropic_beta".into(), json!(vec!["duplicate"; 1_025]));
                }
                "body-name" => {
                    request
                        .extra
                        .insert("anthropic_beta".into(), json!(["x".repeat(257)]));
                }
                "body-string" => {
                    request
                        .extra
                        .insert("anthropic_beta".into(), json!(",".repeat(256 * 1_024 + 1)));
                }
                "route-body" => {
                    context
                        .extra_body
                        .insert("anthropic_beta".into(), json!(vec!["duplicate"; 1_025]));
                }
                "caller-header" => {
                    context
                        .request_headers
                        .insert("anthropic-beta".into(), "x".repeat(257));
                }
                "route-header" => {
                    context
                        .extra_headers
                        .insert("anthropic-beta".into(), json!("x".repeat(257)));
                }
                "provider-header" => {
                    defaults.insert("anthropic-beta".into(), "x".repeat(257));
                }
                "merged-count" => {
                    request.extra.insert(
                        "anthropic_beta".into(),
                        json!(
                            (0..700)
                                .map(|index| format!("beta-{index}"))
                                .collect::<Vec<_>>()
                        ),
                    );
                    defaults.insert(
                        "anthropic-beta".into(),
                        (700..1_025)
                            .map(|index| format!("beta-{index}"))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
                "vertex-effort" => {
                    context.upstream_model = "anthropic/claude-opus-4-5".into();
                    request.extra.insert(
                        "anthropic_beta".into(),
                        json!(
                            (0..1_024)
                                .map(|index| format!("beta-{index}"))
                                .collect::<Vec<_>>()
                        ),
                    );
                    request
                        .extra
                        .insert("output_config".into(), json!({"effort":"high"}));
                }
                _ => unreachable!(),
            }
            let provider = provider_with_headers(kind, &url, defaults);
            let result = provider.chat_completions(&request, &context).await;
            assert!(
                matches!(result, Err(gateway_core::ProviderError::InvalidRequest(_))),
                "{kind}/{source}: {result:?}"
            );
            if kind != "runtime" {
                let result = provider.chat_completions_stream(&request, &context).await;
                assert!(
                    matches!(result, Err(gateway_core::ProviderError::InvalidRequest(_))),
                    "{kind}/{source} stream"
                );
            }
            if kind != "mantle" {
                let result = provider.count_tokens(&request, &context).await;
                assert!(
                    matches!(result, Err(gateway_core::ProviderError::InvalidRequest(_))),
                    "{kind}/{source} count: {result:?}"
                );
            }
            assert!(
                captured.lock().await.is_none(),
                "{kind}/{source} made an upstream HTTP call"
            );
        }
    }
    server.abort();
}

#[tokio::test]
async fn bedrock_beta_array_boundary_allows_generated_effort_beta() {
    let captured: Captured = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/{*path}", post(capture))
        .with_state(captured.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    for kind in ["runtime", "mantle"] {
        let provider = provider_with_headers(kind, &url, BTreeMap::new());
        let mut context = context(kind);
        context.upstream_model = "anthropic.claude-opus-4-5".into();
        context.extra_headers.clear();
        context.request_headers.clear();
        let native: AnthropicMessagesRequest = serde_json::from_value(json!({
            "model":"claude", "max_tokens":1024,
            "messages":[{"role":"user", "content":"hello"}],
            "anthropic_beta":vec!["future-capability";1_024], "output_config":{"effort":"high"}
        }))
        .unwrap();
        let request = anthropic_messages_request_to_core(&native);
        provider.chat_completions(&request, &context).await.unwrap();
        let (_, headers, body) = captured.lock().await.take().unwrap();
        if kind == "runtime" {
            assert_eq!(
                body["anthropic_beta"],
                json!(["future-capability", "effort-2025-11-24"])
            );
        } else {
            assert_eq!(
                headers["anthropic-beta"],
                "future-capability,effort-2025-11-24"
            );
        }
    }
    server.abort();
}
