use gateway_core::{ServiceTier, served_service_tier};

use super::*;
use crate::bedrock::response::normalize_converse_response;

fn runtime_bearer_provider() -> BedrockProvider {
    BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock".to_string(),
        region: "us-east-1".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockRuntime,
        endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
        auth: BedrockAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider")
}

fn chat() -> CoreChatRequest {
    CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    }
}

fn tiered(
    upstream_model: &str,
    api_style: AwsBedrockApiStyle,
    tier: ServiceTier,
) -> ProviderRequestContext {
    let mut context = context_with_api_style(upstream_model, api_style, None);
    context.service_tier = Some(tier);
    context
}

fn body_json(request: &reqwest::Request) -> Value {
    serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).expect("json body")
}

#[tokio::test]
async fn converse_takes_a_service_tier_object_and_invoke_takes_a_header() {
    let provider = runtime_bearer_provider();
    let converse = tiered(
        "amazon.nova-pro-v1:0",
        AwsBedrockApiStyle::RuntimeConverse,
        ServiceTier::Flex,
    );
    let built = provider
        .build_chat_request(&chat(), &converse)
        .await
        .expect("converse");
    assert_eq!(body_json(&built)["serviceTier"], json!({"type": "flex"}));
    assert!(built.headers().get("x-amzn-bedrock-service-tier").is_none());

    let invoke = tiered(
        "us.anthropic.claude-3-5-sonnet-20241022-v2:0",
        AwsBedrockApiStyle::RuntimeAnthropicInvoke,
        ServiceTier::Priority,
    );
    let built = provider
        .build_chat_request(&chat(), &invoke)
        .await
        .expect("invoke");
    assert_eq!(
        built.headers().get("x-amzn-bedrock-service-tier").unwrap(),
        "priority"
    );
    let body = body_json(&built);
    assert!(body.get("serviceTier").is_none() && body.get("service_tier").is_none());
}

#[tokio::test]
async fn unsupported_tiers_are_rejected_before_signing() {
    for (provider, upstream_model, api_style, tier) in [
        (
            mantle_bearer_provider(),
            "anthropic.claude-opus-4-8",
            AwsBedrockApiStyle::MantleAnthropicMessages,
            ServiceTier::Priority,
        ),
        (
            runtime_bearer_provider(),
            "amazon.nova-pro-v1:0",
            AwsBedrockApiStyle::RuntimeConverse,
            ServiceTier::Ultrafast,
        ),
    ] {
        let context = tiered(upstream_model, api_style, tier);
        let error = provider
            .build_chat_request(&chat(), &context)
            .await
            .expect_err("unsupported tier");
        let expected = format!("provider `bedrock` route does not support service tier `{tier}`");
        assert!(
            matches!(&error, ProviderError::InvalidRequest(message) if *message == expected),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn converse_reports_the_served_tier_in_responses_and_stream_metadata() {
    let response = json!({
        "output": {"message": {"role": "assistant", "content": [{"text": "hi"}]}},
        "stopReason": "end_turn",
        "usage": {"inputTokens": 2, "outputTokens": 1, "totalTokens": 3},
        "serviceTier": {"type": "priority"}
    });
    let completion = normalize_converse_response(&response, &context("nova")).expect("completion");
    assert_eq!(
        served_service_tier(&completion),
        Some(ServiceTier::Priority)
    );

    let transcript = collect_bedrock_stream(vec![
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStop")],
            json!({"stopReason": "end_turn"}).to_string().as_bytes(),
        ),
        eventstream_frame(
            &[(":message-type", "event"), (":event-type", "metadata")],
            json!({"usage": {"inputTokens": 2, "outputTokens": 1}, "serviceTier": {"type": "flex"}})
                .to_string()
                .as_bytes(),
        ),
    ])
    .await;
    let usage_chunk = transcript
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|chunk| chunk.get("usage").is_some())
        .expect("usage chunk");
    assert_eq!(served_service_tier(&usage_chunk), Some(ServiceTier::Flex));
}
