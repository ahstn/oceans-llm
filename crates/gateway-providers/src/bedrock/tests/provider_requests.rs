use super::*;

#[tokio::test]
async fn builds_bearer_converse_request_with_encoded_model_path_and_headers() {
    let provider = BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock".to_string(),
        region: "us-east-1".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockRuntime,
        endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
        auth: BedrockAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
        default_headers: BTreeMap::from([(
            "x-amzn-bedrock-trace".to_string(),
            "ENABLED".to_string(),
        )]),
        request_timeout_ms: 1_000,
    })
    .expect("provider");
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };

    let built = provider
        .build_chat_request(
            &request,
            &context_with_api_style(
                "amazon.nova-pro-v1:0",
                AwsBedrockApiStyle::RuntimeConverse,
                None,
            ),
        )
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/amazon.nova-pro-v1%3A0/converse"
    );
    assert_eq!(
        built.headers().get("authorization").unwrap(),
        "Bearer test-token"
    );
    assert_eq!(built.headers().get("x-request-id").unwrap(), "req-test");
    assert_eq!(
        built.headers().get("x-amzn-bedrock-trace").unwrap(),
        "ENABLED"
    );
    assert_eq!(
        body,
        json!({
            "messages": [{
                "role": "user",
                "content": [{"text": "Hello"}]
            }]
        })
    );
}

#[tokio::test]
async fn builds_bearer_anthropic_invoke_request_with_encoded_model_path() {
    let provider = BedrockProvider::new(BedrockProviderConfig {
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
    .expect("provider");
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };

    let built = provider
        .build_chat_request(
            &request,
            &context_with_api_style(
                "us.anthropic.claude-3-5-sonnet-20241022-v2:0",
                AwsBedrockApiStyle::RuntimeAnthropicInvoke,
                None,
            ),
        )
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/invoke"
    );
    assert_eq!(
        built.headers().get("authorization").unwrap(),
        "Bearer test-token"
    );
    assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
    assert_eq!(body["max_tokens"], 64);
}

#[tokio::test]
async fn builds_bearer_converse_stream_request_with_eventstream_accept_header() {
    let provider = BedrockProvider::new(BedrockProviderConfig {
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
    .expect("provider");
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::new(),
    };

    let built = provider
        .build_converse_stream_request(
            &request,
            &context_with_api_style(
                "us.anthropic.claude-3-5-sonnet-20241022-v2:0",
                AwsBedrockApiStyle::RuntimeConverse,
                None,
            ),
        )
        .await
        .expect("request");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse-stream"
    );
    assert_eq!(
        built.headers().get("accept").unwrap(),
        "application/vnd.amazon.eventstream"
    );
}

#[tokio::test]
async fn builds_static_credentials_converse_request_with_sigv4_headers() {
    let provider = static_credentials_provider(Some("test-session-token"));
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };

    let built = provider
        .build_chat_request(
            &request,
            &context_with_api_style(
                "amazon.nova-pro-v1:0",
                AwsBedrockApiStyle::RuntimeConverse,
                None,
            ),
        )
        .await
        .expect("request");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/amazon.nova-pro-v1%3A0/converse"
    );
    let authorization = built
        .headers()
        .get("authorization")
        .expect("authorization")
        .to_str()
        .expect("authorization utf8");
    assert!(authorization.starts_with("AWS4-HMAC-SHA256 "));
    assert!(authorization.contains("Credential=test-access-key/"));
    assert!(authorization.contains("/us-east-1/bedrock/aws4_request"));
    assert!(authorization.contains("SignedHeaders="));
    assert!(built.headers().get("x-amz-date").is_some());
    assert_eq!(
        built.headers().get("x-amz-security-token").unwrap(),
        "test-session-token"
    );
}

#[tokio::test]
async fn builds_static_credentials_invoke_and_converse_stream_requests_with_sigv4_headers() {
    let provider = static_credentials_provider(None);
    let invoke_request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };
    let stream_request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: true,
        extra: BTreeMap::new(),
    };

    let invoke = provider
        .build_chat_request(
            &invoke_request,
            &context_with_api_style(
                "us.anthropic.claude-3-5-sonnet-20241022-v2:0",
                AwsBedrockApiStyle::RuntimeAnthropicInvoke,
                None,
            ),
        )
        .await
        .expect("invoke request");
    let stream = provider
        .build_converse_stream_request(
            &stream_request,
            &context_with_api_style(
                "amazon.nova-pro-v1:0",
                AwsBedrockApiStyle::RuntimeConverse,
                None,
            ),
        )
        .await
        .expect("stream request");

    assert_eq!(
        invoke.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/invoke"
    );
    assert!(invoke.headers().get("authorization").is_some());
    assert!(invoke.headers().get("x-amz-date").is_some());
    assert!(invoke.headers().get("x-amz-security-token").is_none());
    assert_eq!(
        stream.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/amazon.nova-pro-v1%3A0/converse-stream"
    );
    assert!(stream.headers().get("authorization").is_some());
    assert!(stream.headers().get("x-amz-date").is_some());
    assert_eq!(
        stream.headers().get("accept").unwrap(),
        "application/vnd.amazon.eventstream"
    );
}

#[tokio::test]
#[serial]
async fn default_chain_uses_aws_provider_chain_for_sigv4_signing() {
    let _env = AwsCredentialEnvGuard::set();
    let provider = BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock".to_string(),
        region: "us-east-1".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockRuntime,
        endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
        auth: BedrockAuthConfig::DefaultChain,
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider");
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };

    let built = provider
        .build_chat_request(
            &request,
            &context_with_api_style(
                "amazon.nova-pro-v1:0",
                AwsBedrockApiStyle::RuntimeConverse,
                None,
            ),
        )
        .await
        .expect("request");
    let authorization = built
        .headers()
        .get("authorization")
        .expect("authorization")
        .to_str()
        .expect("authorization utf8");

    assert!(authorization.contains("Credential=chain-access-key/"));
    assert!(authorization.contains("/us-east-1/bedrock/aws4_request"));
    assert_eq!(
        built.headers().get("x-amz-security-token").unwrap(),
        "chain-session-token"
    );
    assert!(built.headers().get("x-amz-date").is_some());
}

#[tokio::test]
async fn builds_runtime_openai_chat_request_with_route_headers() {
    let provider = BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock-runtime".to_string(),
        region: "us-east-1".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockRuntime,
        endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
        auth: BedrockAuthConfig::Bearer {
            token: "runtime-token".to_string(),
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider");
    let request = CoreChatRequest {
        model: "gpt".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };
    let mut context = context_with_api_style(
        "openai.gpt-oss-120b-1:0",
        AwsBedrockApiStyle::RuntimeOpenaiChat,
        Some("/openai/v1"),
    );
    context.extra_headers.insert(
        "OpenAI-Project".to_string(),
        Value::String("proj_runtime".to_string()),
    );

    let built = provider
        .build_chat_request(&request, &context)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1/chat/completions"
    );
    assert_eq!(body["model"], "openai.gpt-oss-120b-1:0");
    assert_eq!(
        built.headers().get("OpenAI-Project").unwrap(),
        "proj_runtime"
    );
    assert_eq!(
        built.headers().get("authorization").unwrap(),
        "Bearer runtime-token"
    );
}

#[tokio::test]
async fn builds_mantle_openai_responses_bearer_request_for_gpt_55() {
    let provider = mantle_bearer_provider();
    let request = responses_request(false);
    let mut context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );
    context.extra_headers.insert(
        "OpenAI-Project".to_string(),
        Value::String("proj_123".to_string()),
    );
    context.extra_headers.insert(
        "authorization".to_string(),
        Value::String("Bearer route-should-not-win".to_string()),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-mantle.us-east-2.api.aws/openai/v1/responses"
    );
    assert_eq!(body["model"], "openai.gpt-5.5");
    assert_eq!(built.headers().get("OpenAI-Project").unwrap(), "proj_123");
    assert_eq!(
        built.headers().get("authorization").unwrap(),
        "Bearer mantle-token"
    );
    assert!(built.headers().get("x-api-key").is_none());
}

#[tokio::test]
async fn builds_mantle_openai_responses_stream_request() {
    let provider = mantle_bearer_provider();
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&responses_request(false), &context, true)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-mantle.us-east-2.api.aws/openai/v1/responses"
    );
    assert_eq!(built.headers().get("accept").unwrap(), "text/event-stream");
    assert_eq!(body["stream"], true);
}

#[tokio::test]
#[serial]
async fn mantle_openai_responses_default_chain_uses_mantle_sigv4_service() {
    let _env = AwsCredentialEnvGuard::set();
    let provider = BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock-mantle".to_string(),
        region: "us-east-2".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockMantle,
        endpoint_url: "https://bedrock-mantle.us-east-2.api.aws".to_string(),
        auth: BedrockAuthConfig::DefaultChain,
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider");
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&responses_request(false), &context, false)
        .await
        .expect("request");
    let authorization = built
        .headers()
        .get("authorization")
        .expect("authorization")
        .to_str()
        .expect("authorization utf8");

    assert!(authorization.contains("Credential=chain-access-key/"));
    assert!(authorization.contains("/us-east-2/bedrock-mantle/aws4_request"));
    assert!(built.headers().get("x-api-key").is_none());
}

#[tokio::test]
async fn builds_mantle_openai_chat_request() {
    let provider = mantle_bearer_provider();
    let request = CoreChatRequest {
        model: "gpt".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };
    let context = context_with_api_style(
        "openai.gpt-5.4",
        AwsBedrockApiStyle::MantleOpenaiChat,
        Some("/openai/v1"),
    );

    let built = provider
        .build_chat_request(&request, &context)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-mantle.us-east-2.api.aws/openai/v1/chat/completions"
    );
    assert_eq!(body["model"], "openai.gpt-5.4");
}

#[tokio::test]
async fn builds_mantle_anthropic_messages_api_key_request() {
    let provider = mantle_bearer_provider();
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };
    let mut context = context_with_api_style(
        "anthropic.claude-sonnet-4-5",
        AwsBedrockApiStyle::MantleAnthropicMessages,
        None,
    );
    context.extra_headers.insert(
        "x-api-key".to_string(),
        Value::String("route-should-not-win".to_string()),
    );

    let built = provider
        .build_chat_request(&request, &context)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(
        built.url().as_str(),
        "https://bedrock-mantle.us-east-2.api.aws/anthropic/v1/messages"
    );
    assert_eq!(built.headers().get("x-api-key").unwrap(), "mantle-token");
    assert!(built.headers().get("authorization").is_none());
    assert_eq!(
        built.headers().get("anthropic-version").unwrap(),
        "2023-06-01"
    );
    assert_eq!(body["model"], "anthropic.claude-sonnet-4-5");
    assert!(body.get("anthropic_version").is_none());
    assert_eq!(body["max_tokens"], 64);
}

#[tokio::test]
async fn builds_mantle_anthropic_messages_sigv4_request() {
    let provider = BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock-mantle".to_string(),
        region: "us-east-2".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockMantle,
        endpoint_url: "https://bedrock-mantle.us-east-2.api.aws".to_string(),
        auth: BedrockAuthConfig::StaticCredentials {
            access_key_id: "test-access-key".to_string(),
            secret_access_key: "test-secret-key".to_string(),
            session_token: None,
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider");
    let request = CoreChatRequest {
        model: "claude".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
    };
    let context = context_with_api_style(
        "anthropic.claude-sonnet-4-5",
        AwsBedrockApiStyle::MantleAnthropicMessages,
        None,
    );

    let built = provider
        .build_chat_request(&request, &context)
        .await
        .expect("request");
    let authorization = built
        .headers()
        .get("authorization")
        .expect("authorization")
        .to_str()
        .expect("authorization utf8");

    assert!(authorization.contains("/us-east-2/bedrock-mantle/aws4_request"));
    assert!(built.headers().get("x-api-key").is_none());
}

#[tokio::test]
async fn rejects_incompatible_endpoint_and_api_style_pair() {
    let provider = mantle_bearer_provider();
    let request = CoreChatRequest {
        model: "nova".to_string(),
        messages: vec![message("user", "Hello")],
        stream: false,
        extra: BTreeMap::new(),
    };
    let error = provider
        .build_chat_request(
            &request,
            &context_with_api_style(
                "amazon.nova-pro-v1:0",
                AwsBedrockApiStyle::RuntimeConverse,
                None,
            ),
        )
        .await
        .expect_err("incompatible route rejected")
        .to_string();

    assert!(error.contains("not compatible with endpoint_kind `bedrock_mantle`"));
}

#[tokio::test]
async fn rejects_missing_bedrock_route_compatibility() {
    let provider = mantle_bearer_provider();
    let error = provider
        .build_responses_request(&responses_request(false), &context("openai.gpt-5.5"), false)
        .await
        .expect_err("compatibility required")
        .to_string();

    assert!(error.contains("compatibility.aws_bedrock.api_style"));
}
