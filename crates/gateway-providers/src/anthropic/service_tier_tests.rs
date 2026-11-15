use std::collections::BTreeMap;

use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    ProviderError, ProviderRequestContext, ServiceTier, SseEventParser, served_service_tier,
};
use serde_json::{Value, json};

use super::response::normalize_anthropic_response;
use super::streaming::normalize_anthropic_stream;
use crate::anthropic_compat::{AnthropicCompatConfig, AnthropicCompatProvider};

fn context(upstream_model: &str, service_tier: Option<ServiceTier>) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "req_tier".to_string(),
        model_key: "claude".to_string(),
        provider_key: "anthropic".to_string(),
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

/// The Messages body and `anthropic-beta` header the Claude API adapter sends.
fn built_request(
    upstream_model: &str,
    service_tier: Option<ServiceTier>,
) -> Result<(Value, Option<String>), ProviderError> {
    let provider = AnthropicCompatProvider::new(AnthropicCompatConfig::new(
        "anthropic".to_string(),
        "https://api.anthropic.com".to_string(),
    ))
    .expect("provider");
    let body = json!({
        "model": upstream_model,
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "hello"}]
    });
    let request = provider.build_request(body, &context(upstream_model, service_tier))?;
    let beta = request
        .headers()
        .get("anthropic-beta")
        .map(|value| value.to_str().expect("ascii header").to_string());
    let body = request
        .body()
        .and_then(reqwest::Body::as_bytes)
        .expect("buffered body");
    Ok((serde_json::from_slice(body).expect("json body"), beta))
}

#[test]
fn priority_is_fast_mode_with_the_preview_beta_only_on_opus_5_and_opus_4_8() {
    for (model, beta) in [
        ("claude-opus-5", Some("fast-mode-2026-02-01")),
        ("claude-opus-5-20260301", Some("fast-mode-2026-02-01")),
        ("claude-opus-4-8", Some("fast-mode-2026-02-01")),
        ("claude-opus-5-5", None),
        ("claude-fable-5-2", None),
    ] {
        let (body, header) = built_request(model, Some(ServiceTier::Priority)).expect("request");
        assert_eq!(body["speed"], "fast", "{model}");
        assert!(body.get("service_tier").is_none(), "{model}");
        assert!(body.get("anthropic_beta").is_none(), "{model}");
        assert_eq!(header.as_deref(), beta, "{model}");
    }
}

#[test]
fn explicit_default_pins_standard_capacity_and_no_tier_sends_nothing() {
    let (body, header) =
        built_request("claude-opus-5", Some(ServiceTier::Default)).expect("request");
    assert_eq!(body["service_tier"], "standard_only");
    assert!(body.get("speed").is_none());
    assert_eq!(header, None);

    let (body, _) = built_request("claude-opus-5", None).expect("request");
    assert!(body.get("service_tier").is_none());
    assert!(body.get("speed").is_none());
}

#[test]
fn flex_and_ultrafast_are_rejected_before_the_request_is_built() {
    for tier in [ServiceTier::Flex, ServiceTier::Ultrafast] {
        let error = built_request("claude-opus-5-5", Some(tier)).expect_err("unsupported tier");
        let expected = format!("provider `anthropic` route does not support service tier `{tier}`");
        assert!(
            matches!(&error, ProviderError::InvalidRequest(message) if *message == expected),
            "{error:?}"
        );
    }
}

#[test]
fn priority_is_rejected_on_claude_api_models_without_fast_mode() {
    for model in [
        "claude-haiku-5-5",
        "claude-sonnet-5-5",
        "claude-opus-latest",
    ] {
        let error = built_request(model, Some(ServiceTier::Priority)).expect_err("no fast mode");
        assert!(
            matches!(
                &error,
                ProviderError::InvalidRequest(message)
                    if message == "provider `anthropic` route does not support service tier `priority`"
            ),
            "{model}: {error:?}"
        );
        let (body, _) = built_request(model, Some(ServiceTier::Default)).expect("default");
        assert_eq!(body["service_tier"], "standard_only", "{model}");
    }
    for model in ["claude-opus-5-5", "claude-opus-5-5-20261101"] {
        let (body, _) = built_request(model, Some(ServiceTier::Priority)).expect("fast mode");
        assert_eq!(body["speed"], "fast", "{model}");
    }
}

#[test]
fn translated_response_reports_the_served_speed_at_the_top_level() {
    for (usage, served) in [
        (
            json!({"speed": "fast", "service_tier": "standard"}),
            Some("priority"),
        ),
        (json!({"speed": "standard"}), Some("default")),
        (json!({"service_tier": "batch"}), None),
    ] {
        let mut usage = usage;
        usage["input_tokens"] = json!(3);
        usage["output_tokens"] = json!(2);
        let response = json!({
            "id": "msg_tier",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn",
            "usage": usage
        });
        let completion = normalize_anthropic_response(
            &response,
            &context("claude-opus-5-5", None),
            "anthropic_compat",
            "anthropic_compat",
        );
        assert_eq!(
            completion.get("service_tier").and_then(Value::as_str),
            served,
            "{usage}"
        );
    }
}

#[tokio::test]
async fn stream_usage_chunks_report_the_speed_from_message_start() {
    let events = [
        json!({"type": "message_start", "message": {"usage": {
            "input_tokens": 3, "output_tokens": 0, "speed": "fast"
        }}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 2}}),
        json!({"type": "message_stop"}),
    ];
    let upstream = futures_util::stream::iter(events.map(|event| {
        Ok(Bytes::from(format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().expect("event type")
        )))
    }));
    let chunks = normalize_anthropic_stream(
        upstream,
        "chatcmpl-tier".to_string(),
        0,
        "claude".to_string(),
        "anthropic_compat",
        "anthropic_compat",
    )
    .collect::<Vec<_>>()
    .await;

    let mut parser = SseEventParser::default();
    let mut usage_chunks = Vec::new();
    for chunk in chunks {
        for event in parser.push_bytes(&chunk.expect("chunk")).expect("sse") {
            if let Ok(value) = serde_json::from_str::<Value>(&event.data)
                && value.get("usage").is_some()
            {
                usage_chunks.push(value);
            }
        }
    }
    assert_eq!(
        usage_chunks.len(),
        2,
        "message_start and finish carry usage"
    );
    for chunk in &usage_chunks {
        assert_eq!(chunk["service_tier"], "priority");
        assert_eq!(served_service_tier(chunk), Some(ServiceTier::Priority));
    }
}
