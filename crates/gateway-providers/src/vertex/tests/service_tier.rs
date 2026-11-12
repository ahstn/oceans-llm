use gateway_core::{ServiceTier, served_service_tier};

use super::*;

const ENDPOINT: &str = "https://aiplatform.googleapis.com/v1/generate";

fn tiered(upstream_model: &str, tier: ServiceTier) -> ProviderRequestContext {
    let mut context = context(upstream_model);
    context.service_tier = Some(tier);
    context
}

#[tokio::test]
async fn gemini_takes_flex_and_priority_as_the_shared_request_type_header() {
    let provider = vertex_provider_for_test("aiplatform.googleapis.com".to_string());
    for (tier, header) in [
        (ServiceTier::Priority, Some("priority")),
        (ServiceTier::Flex, Some("flex")),
        (ServiceTier::Default, None),
    ] {
        let context = tiered("google/gemini-2.5-flash", tier);
        let request = provider
            .build_request(ENDPOINT, &json!({}), &context, PublisherFamily::Google)
            .await
            .expect("request");
        let sent = request
            .headers()
            .get("x-vertex-ai-llm-shared-request-type")
            .map(|value| value.to_str().expect("ascii"));
        assert_eq!(sent, header, "{tier}");
    }
}

#[tokio::test]
async fn claude_is_fast_mode_only_and_rejects_other_tiers_before_the_token_fetch() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!("hello"),
        name: None,
        extra: BTreeMap::new(),
    }]);
    let context = tiered("anthropic/claude-opus-5-5", ServiceTier::Priority);
    let body = anthropic_body(&request, &context, false).expect("Claude body");
    assert_eq!(body["speed"], "fast");

    let provider = vertex_provider_for_test("aiplatform.googleapis.com".to_string());
    let context = tiered("anthropic/claude-opus-5-5", ServiceTier::Flex);
    let error = provider
        .build_request(ENDPOINT, &body, &context, PublisherFamily::Anthropic)
        .await
        .expect_err("Claude on Vertex has no flex tier");
    assert!(
        matches!(&error, ProviderError::InvalidRequest(message)
            if message == "provider `vertex-prod` route does not support service tier `flex`"),
        "{error:?}"
    );
}

fn gemini_response(traffic_type: &str) -> Value {
    json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": "hi"}]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {
            "promptTokenCount": 2,
            "candidatesTokenCount": 1,
            "totalTokenCount": 3,
            "trafficType": traffic_type
        }
    })
}

#[test]
fn gemini_traffic_type_reports_the_served_tier() {
    for (traffic_type, served) in [
        ("ON_DEMAND_PRIORITY", Some(ServiceTier::Priority)),
        ("ON_DEMAND_FLEX", Some(ServiceTier::Flex)),
        ("ON_DEMAND", Some(ServiceTier::Default)),
        ("PROVISIONED_THROUGHPUT", None),
    ] {
        let response = gemini_response(traffic_type);
        let completion = normalize_google_response(&response, &context("google/gemini-2.5-flash"))
            .expect("completion");
        assert_eq!(served_service_tier(&completion), served, "{traffic_type}");

        let mut state = GoogleStreamState::new("chatcmpl-test".to_string(), 1, "fast".to_string());
        state.on_response(&response);
        let finish = state.finish().expect("finish");
        assert_eq!(
            served_service_tier(&finish),
            served,
            "{traffic_type} stream"
        );
    }
}
