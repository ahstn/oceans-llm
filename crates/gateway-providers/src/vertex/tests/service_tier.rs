use gateway_core::{ServiceTier, served_service_tier};

use crate::vertex::vertex_service_tiers;

use super::*;

const ENDPOINT: &str = "https://aiplatform.googleapis.com/v1/generate";

fn tiered(upstream_model: &str, tier: ServiceTier) -> ProviderRequestContext {
    let mut context = context(upstream_model);
    context.service_tier = Some(tier);
    context
}

fn provider_at(location: &str) -> VertexProvider {
    VertexProvider::new(VertexProviderConfig {
        provider_key: "vertex-prod".to_string(),
        project_id: "proj-123".to_string(),
        location: location.to_string(),
        api_host: vertex_api_host_for_location(location),
        auth: VertexAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 5_000,
        batch: None,
    })
    .expect("provider")
}

#[tokio::test]
async fn gemini_tiers_follow_the_endpoint_location() {
    use ServiceTier::{Default, Flex, Priority, Ultrafast};
    for (location, sold) in [
        ("global", &[Default, Flex, Priority][..]),
        ("us", &[Default, Priority]),
        ("eu", &[Default, Priority]),
        ("us-central1", &[Default]),
        ("europe-west4", &[Default]),
    ] {
        assert_eq!(
            vertex_service_tiers(location, "google/gemini-2.5-flash"),
            sold,
            "{location}"
        );
        let provider = provider_at(location);
        for tier in [Default, Flex, Priority, Ultrafast] {
            let result = provider
                .build_request(
                    ENDPOINT,
                    &json!({}),
                    &tiered("google/gemini-2.5-flash", tier),
                    PublisherFamily::Google,
                )
                .await;
            if sold.contains(&tier) {
                result.unwrap_or_else(|error| panic!("{tier} at {location}: {error:?}"));
            } else {
                let error = result.expect_err("tier not sold at this location");
                assert!(
                    matches!(&error, ProviderError::InvalidRequest(message)
                        if *message == format!("provider `vertex-prod` route does not support service tier `{tier}`")),
                    "{tier} at {location}: {error:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn claude_fast_mode_is_sold_only_on_the_models_vertex_lists() {
    let request = chat_request(vec![CoreChatMessage {
        role: "user".to_string(),
        content: json!("hello"),
        name: None,
        extra: BTreeMap::new(),
    }]);
    for location in ["global", "us-east5"] {
        let provider = provider_at(location);
        for (model, fast_mode) in [
            ("anthropic/claude-fable-5-2", true),
            ("anthropic/claude-mythos-5-2@20261001", true),
            ("anthropic/claude-opus-5-5", false),
            ("anthropic/claude-haiku-5-5", false),
        ] {
            let context = tiered(model, ServiceTier::Priority);
            let body = anthropic_body(&request, &context, false).expect("Claude body");
            let result = provider
                .build_request(ENDPOINT, &body, &context, PublisherFamily::Anthropic)
                .await;
            assert_eq!(
                result.is_ok(),
                fast_mode,
                "{model} at {location}: {result:?}"
            );
            if !fast_mode {
                assert!(
                    matches!(&result, Err(ProviderError::InvalidRequest(message))
                        if message == "provider `vertex-prod` route does not support service tier `priority`"),
                    "{model} at {location}"
                );
            }
        }
    }
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
    let context = tiered("anthropic/claude-fable-5-2", ServiceTier::Priority);
    let body = anthropic_body(&request, &context, false).expect("Claude body");
    assert_eq!(body["speed"], "fast");

    let provider = vertex_provider_for_test("aiplatform.googleapis.com".to_string());
    let context = tiered("anthropic/claude-fable-5-2", ServiceTier::Flex);
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
