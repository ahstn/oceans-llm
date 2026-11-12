use std::collections::BTreeMap;

use axum::{
    Json, Router,
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::StreamExt;
use gateway_core::{
    CoreChatRequest, CoreResponsesRequest, ModelRoute, OpenAiCompatDecisionsApi,
    OpenAiCompatRouteCompatibility, ProviderClient, ProviderRequestContext, ServiceTier,
    SseEventParser, served_service_tier,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::{OpenAiCompatConfig, OpenAiCompatProvider};

fn provider(base_url: &str) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new(OpenAiCompatConfig::new(
        "openai".to_string(),
        base_url.to_string(),
    ))
    .expect("provider")
}

fn context(upstream_model: &str, service_tier: Option<ServiceTier>) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "req-tier".to_string(),
        model_key: "gpt".to_string(),
        provider_key: "openai".to_string(),
        upstream_model: upstream_model.to_string(),
        owner_user_id: None,
        expected_provider_credential_id: None,
        chat_protocol: Default::default(),
        extra_headers: serde_json::Map::new(),
        extra_body: serde_json::Map::new(),
        request_headers: BTreeMap::new(),
        compatibility: Default::default(),
        service_tier,
    }
}

fn chat_request(stream: bool) -> CoreChatRequest {
    CoreChatRequest {
        model: "gpt".to_string(),
        messages: Vec::new(),
        stream,
        extra: BTreeMap::new(),
    }
}

fn responses_request() -> CoreResponsesRequest {
    CoreResponsesRequest {
        model: "gpt".to_string(),
        input: json!("hello"),
        stream: false,
        instructions: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: BTreeMap::new(),
    }
}

fn body_json(request: &reqwest::Request) -> Value {
    let body = request
        .body()
        .and_then(reqwest::Body::as_bytes)
        .expect("buffered body");
    serde_json::from_slice(body).expect("json body")
}

#[test]
fn chat_and_responses_send_the_effective_tier_as_body_service_tier() {
    let provider = provider("https://api.openai.com/v1");
    for tier in ServiceTier::ALL {
        let context = context("gpt-5.5", Some(tier));
        let chat = provider
            .build_chat_request(&chat_request(false), &context)
            .expect("chat request");
        assert_eq!(body_json(&chat)["service_tier"], tier.as_str());
        let responses = provider
            .build_responses_request(&responses_request(), &context)
            .expect("responses request");
        assert_eq!(body_json(&responses)["service_tier"], tier.as_str());
    }

    let untiered = provider
        .build_chat_request(&chat_request(false), &context("gpt-5.5", None))
        .expect("chat request");
    assert!(body_json(&untiered).get("service_tier").is_none());
}

#[test]
fn openrouter_messages_take_the_tier_as_body_service_tier() {
    let provider = provider("https://openrouter.ai/api/v1");
    let mut context = context("anthropic/claude-opus-5.5", Some(ServiceTier::Priority));
    context.compatibility.openrouter = Some(serde_json::from_value(json!({})).expect("compat"));
    let request: CoreChatRequest = serde_json::from_value(json!({
        "model": "claude",
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "hello"}]
    }))
    .expect("chat request");

    let wire = provider
        .build_chat_request(&request, &context)
        .expect("messages request");
    assert_eq!(wire.url().as_str(), "https://openrouter.ai/api/v1/messages");
    let body = body_json(&wire);
    assert_eq!(body["service_tier"], "priority");
    assert!(body.get("speed").is_none());
    assert!(wire.headers().get("anthropic-beta").is_none());
}

#[test]
fn decisions_routes_serve_only_the_default_tier() {
    let mut route: ModelRoute = serde_json::from_value(json!({
        "id": "00000000-0000-0000-0000-000000000001",
        "model_id": "00000000-0000-0000-0000-000000000002",
        "provider_key": "openai",
        "upstream_model": "gpt-5.5",
        "priority": 0,
        "weight": 1.0,
        "enabled": true
    }))
    .expect("route");
    let provider = provider("https://api.openai.com/v1");
    assert_eq!(provider.service_tiers(&route), ServiceTier::ALL);

    route.compatibility.openai_compat = Some(OpenAiCompatRouteCompatibility {
        decisions_api: Some(OpenAiCompatDecisionsApi::OpenAi),
        ..Default::default()
    });
    assert_eq!(provider.service_tiers(&route), [ServiceTier::Default]);
}

/// Chat Completions that demands a priority request and reports serving it at the default
/// tier, as OpenAI does when priority capacity is unavailable.
async fn downgrading_server() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<Value>| async move {
            if body["service_tier"] != "priority" {
                return (StatusCode::BAD_REQUEST, "expected a priority request").into_response();
            }
            let usage = json!({"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3});
            if body["stream"] != true {
                return Json(json!({
                    "id": "chatcmpl-tier",
                    "object": "chat.completion",
                    "service_tier": "default",
                    "choices": [{"index": 0, "finish_reason": "stop",
                        "message": {"role": "assistant", "content": "hi"}}],
                    "usage": usage
                }))
                .into_response();
            }
            let content = json!({"id": "chatcmpl-tier", "service_tier": "default",
                "choices": [{"index": 0, "delta": {"content": "hi"}, "finish_reason": "stop"}]});
            let usage = json!({"id": "chatcmpl-tier", "service_tier": "default",
                "choices": [], "usage": usage});
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(format!(
                    "data: {content}\n\ndata: {usage}\n\ndata: [DONE]\n\n"
                )))
                .expect("response")
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    format!("http://{addr}/v1")
}

#[tokio::test]
async fn chat_reports_the_tier_upstream_served_rather_than_the_one_requested() {
    let provider = provider(&downgrading_server().await);
    let context = context("gpt-5.5", Some(ServiceTier::Priority));

    let completion = provider
        .chat_completions(&chat_request(false), &context)
        .await
        .expect("completion");
    assert_eq!(served_service_tier(&completion), Some(ServiceTier::Default));

    let mut stream = provider
        .chat_completions_stream(&chat_request(true), &context)
        .await
        .expect("stream");
    let mut parser = SseEventParser::default();
    let mut usage_chunk = None;
    while let Some(chunk) = stream.next().await {
        for event in parser.push_bytes(&chunk.expect("chunk")).expect("sse") {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data)
                && value.get("usage").is_some()
            {
                usage_chunk = Some(value);
            }
        }
    }
    let usage_chunk = usage_chunk.expect("usage chunk");
    assert_eq!(
        served_service_tier(&usage_chunk),
        Some(ServiceTier::Default)
    );
}
