use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;

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

fn count_request() -> CoreChatRequest {
    CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("system", "Be brief."), message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([
            ("max_tokens".to_string(), json!(64)),
            ("temperature".to_string(), json!(0.2)),
        ]),
    }
}

fn request_body(request: &reqwest::Request) -> Value {
    serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).expect("json body")
}

#[tokio::test]
async fn invoke_route_wraps_the_invoke_body_in_count_tokens() {
    let provider = runtime_bearer_provider();
    let context = context_with_api_style(
        "us.anthropic.claude-sonnet-4-6",
        AwsBedrockApiStyle::RuntimeAnthropicInvoke,
        None,
    );

    let built = provider
        .build_count_tokens_request(&count_request(), &context)
        .await
        .expect("request");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-sonnet-4-6/count-tokens",
        "CountTokens rejects inference profile IDs, so the geographic prefix is dropped"
    );
    assert_eq!(
        built.headers().get("authorization").unwrap(),
        "Bearer test-token"
    );
    let body = request_body(&built);
    let encoded = body["input"]["invokeModel"]["body"]
        .as_str()
        .expect("base64 invoke body");
    let invoke: Value = serde_json::from_slice(&STANDARD.decode(encoded).expect("base64"))
        .expect("invoke body json");
    assert_eq!(invoke["anthropic_version"], "bedrock-2023-05-31");
    assert_eq!(invoke["max_tokens"], 64);
    assert_eq!(invoke["messages"][0]["role"], "user");
    assert_eq!(invoke["system"][0]["text"], "Be brief.");
}

#[tokio::test]
async fn converse_route_sends_only_prompt_fields() {
    let provider = runtime_bearer_provider();
    let context = context_with_api_style(
        "anthropic.claude-sonnet-4-6",
        AwsBedrockApiStyle::RuntimeConverse,
        None,
    );

    let built = provider
        .build_count_tokens_request(&count_request(), &context)
        .await
        .expect("request");

    assert!(
        built
            .url()
            .as_str()
            .ends_with("/model/anthropic.claude-sonnet-4-6/count-tokens")
    );
    let converse = &request_body(&built)["input"]["converse"];
    assert_eq!(converse["messages"][0]["role"], "user");
    assert_eq!(converse["system"][0]["text"], "Be brief.");
    assert!(converse.get("inferenceConfig").is_none(), "{converse}");
}

#[tokio::test]
async fn other_api_styles_report_not_implemented() {
    let provider = mantle_bearer_provider();
    let context = context_with_api_style(
        "anthropic.claude-sonnet-4-6",
        AwsBedrockApiStyle::MantleAnthropicMessages,
        None,
    );

    let error = provider
        .count_tokens(&count_request(), &context)
        .await
        .expect_err("mantle has no count API");

    assert!(
        matches!(&error, ProviderError::NotImplemented(message) if message.contains("token counting")),
        "{error:?}"
    );
}

#[tokio::test]
async fn count_tokens_uses_the_foundation_model_behind_a_profile() {
    let provider = runtime_bearer_provider();
    let mut request = count_request();
    request.extra.remove("temperature");
    for (upstream_model, expected_path) in [
        (
            "global.anthropic.claude-opus-4-7",
            "/model/anthropic.claude-opus-4-7/count-tokens",
        ),
        (
            "anthropic.claude-sonnet-4-6",
            "/model/anthropic.claude-sonnet-4-6/count-tokens",
        ),
        (
            "arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-sonnet-4-6",
            "/model/arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-sonnet-4-6/count-tokens",
        ),
    ] {
        let context = context_with_api_style(
            upstream_model,
            AwsBedrockApiStyle::RuntimeAnthropicInvoke,
            None,
        );

        let built = provider
            .build_count_tokens_request(&request, &context)
            .await
            .expect("request");

        assert_eq!(built.url().path(), expected_path, "{upstream_model}");
    }
}
