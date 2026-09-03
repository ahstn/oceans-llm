use super::*;

#[test]
fn provider_capabilities_include_responses_for_mantle_routes() {
    let capabilities = mantle_bearer_provider().capabilities();

    assert!(capabilities.chat_completions);
    assert!(capabilities.responses);
    assert!(capabilities.stream);
    assert!(!capabilities.embeddings);
}

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
async fn builds_bearer_anthropic_invoke_request_with_authoritative_envelope() {
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

    let mut context = context_with_api_style(
        "us.anthropic.claude-3-5-sonnet-20241022-v2:0",
        AwsBedrockApiStyle::RuntimeAnthropicInvoke,
        None,
    );
    context
        .extra_body
        .insert("model".to_string(), json!("route-override"));
    context.extra_body.insert("stream".to_string(), json!(true));
    context
        .extra_body
        .insert("anthropic_version".to_string(), json!("hostile-version"));

    let built = provider
        .build_chat_request(&request, &context)
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
    assert!(body.get("model").is_none());
    assert!(body.get("stream").is_none());
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
async fn normalizes_mantle_openai_responses_replay_ids() {
    let provider = mantle_bearer_provider();
    let foreign_call_id = format!("copilot|{}", "x".repeat(100));
    let mut request = responses_request(false);
    request.input = json!([
        {
            "type": "function_call",
            "id": "foreign-provider-item",
            "call_id": foreign_call_id,
            "name": "lookup",
            "arguments": "{}"
        },
        {
            "type": "function_call_output",
            "call_id": foreign_call_id,
            "output": "ok"
        },
        {
            "type": "reasoning",
            "id": format!("rs_{}", "x".repeat(100)),
            "summary": []
        },
        {
            "type": "message",
            "id": "msg_native123",
            "role": "assistant",
            "content": []
        }
    ]);
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");
    let input = body["input"].as_array().expect("input");

    let call_id = input[0]["call_id"].as_str().expect("call id");
    assert!(input[0]["id"].as_str().unwrap().starts_with("fc_"));
    assert!(call_id.starts_with("call_"));
    assert!(call_id.len() <= 64);
    assert_eq!(input[1]["call_id"], call_id);
    assert!(input[2]["id"].as_str().unwrap().starts_with("rs_"));
    assert!(input[2]["id"].as_str().unwrap().len() <= 64);
    assert_eq!(input[3]["id"], "msg_native123");
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
async fn builds_mantle_openai_responses_request_strips_opportunistic_image_generation_tool() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([
        {"type": "image_generation"},
        {"type": "function", "name": "lookup", "parameters": {"type": "object"}},
        {"type": "custom", "name": "custom_tool"},
        {"type": "namespace", "namespace": "browser"},
        {"type": "tool_search"},
        {"type": "mcp", "server_label": "tools"}
    ]));
    request.tool_choice = Some(json!("auto"));
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");
    let tools = body["tools"].as_array().expect("tools");
    let tool_types = tools
        .iter()
        .filter_map(|tool| tool.get("type").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert_eq!(body["model"], "openai.gpt-5.5");
    assert!(!tool_types.contains(&"image_generation"));
    assert!(tool_types.contains(&"function"));
    assert!(tool_types.contains(&"custom"));
    assert!(tool_types.contains(&"namespace"));
    assert!(tool_types.contains(&"tool_search"));
    assert!(tool_types.contains(&"mcp"));
}

#[tokio::test]
async fn builds_mantle_openai_responses_request_omits_only_opportunistic_image_generation_tool() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([{"type": "image_generation"}]));
    request.tool_choice = Some(json!("auto"));
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
}

#[tokio::test]
async fn rejects_mantle_openai_responses_when_tool_choice_requires_image_generation() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([{"type": "image_generation"}]));
    request.tool_choice = Some(json!({"type": "image_generation"}));
    let mut context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );
    context.provider_key = "bedrock-mantle".to_string();

    let error = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect_err("explicit image generation is rejected")
        .to_string();

    assert!(error.contains("image_generation"));
    assert!(error.contains("bedrock-mantle"));
    assert!(error.contains("openai.gpt-5.5"));
}

#[tokio::test]
async fn rejects_mantle_openai_responses_when_image_generation_tool_forces_action() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([{"type": "image_generation", "action": "generate"}]));
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let error = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect_err("forced image generation is rejected")
        .to_string();

    assert!(error.contains("image_generation"));
    assert!(error.contains("openai.gpt-5.5"));
}

#[tokio::test]
async fn rejects_mantle_openai_responses_when_allowed_tools_requires_image_generation() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([
        {"type": "image_generation"},
        {"type": "function", "name": "lookup", "parameters": {"type": "object"}}
    ]));
    request.tool_choice = Some(json!({
        "type": "allowed_tools",
        "mode": "required",
        "tools": [{"type": "image_generation"}]
    }));
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let error = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect_err("required allowed_tools image generation is rejected")
        .to_string();

    assert!(error.contains("image_generation"));
    assert!(error.contains("openai.gpt-5.5"));
}

#[tokio::test]
async fn builds_mantle_openai_responses_request_prunes_allowed_image_generation_tools() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([
        {"type": "image_generation"},
        {"type": "function", "name": "lookup", "parameters": {"type": "object"}}
    ]));
    request.tool_choice = Some(json!({
        "type": "allowed_tools",
        "mode": "auto",
        "tools": [
            {"type": "image_generation"},
            {"type": "function", "name": "lookup"}
        ]
    }));
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");
    let tools = body["tools"].as_array().expect("tools");
    let allowed_tools = body["tool_choice"]["tools"]
        .as_array()
        .expect("allowed tools");

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(allowed_tools.len(), 1);
    assert_eq!(allowed_tools[0]["type"], "function");
}

#[tokio::test]
async fn builds_mantle_openai_responses_request_with_function_named_image_generation() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request.tools = Some(json!([
        {"type": "function", "name": "image_generation", "parameters": {"type": "object"}}
    ]));
    request.tool_choice = Some(json!({"type": "function", "name": "image_generation"}));
    let context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "image_generation");
    assert_eq!(body["tool_choice"]["type"], "function");
    assert_eq!(body["tool_choice"]["name"], "image_generation");
}

#[tokio::test]
async fn preserves_matching_route_and_caller_prompt_cache_intent() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request
        .extra
        .insert("prompt_cache_key".to_string(), json!("session-1"));
    request
        .extra
        .insert("prompt_cache_retention".to_string(), json!("in_memory"));
    let mut context = context_with_api_style(
        "openai.gpt-5.6-sol",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );
    context
        .extra_body
        .insert("prompt_cache_key".to_string(), json!("session-1"));

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("matching cache intent");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert_eq!(body["prompt_cache_key"], "session-1");
    assert_eq!(body["prompt_cache_retention"], "in_memory");
}

#[tokio::test]
async fn rejects_route_override_that_conflicts_with_caller_prompt_cache_intent() {
    let provider = mantle_bearer_provider();
    let mut request = responses_request(false);
    request
        .extra
        .insert("prompt_cache_key".to_string(), json!("caller-session"));
    let mut context = context_with_api_style(
        "openai.gpt-5.6-sol",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );
    context
        .extra_body
        .insert("prompt_cache_key".to_string(), json!("route-session"));

    let error = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect_err("conflicting cache intent must fail")
        .to_string();

    assert!(error.contains("conflicts with caller prompt-cache intent"));
}

#[tokio::test]
async fn strips_image_generation_added_by_route_extra_body() {
    let provider = mantle_bearer_provider();
    let request = responses_request(false);
    let mut context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );
    context.extra_body.insert(
        "tools".to_string(),
        json!([
            {"type": "image_generation"},
            {"type": "function", "name": "lookup", "parameters": {"type": "object"}}
        ]),
    );

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");
    let tools = body["tools"].as_array().expect("tools");

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
}

#[tokio::test]
async fn strips_single_object_image_generation_added_by_route_extra_body() {
    let provider = mantle_bearer_provider();
    let request = responses_request(false);
    let mut context = context_with_api_style(
        "openai.gpt-5.5",
        AwsBedrockApiStyle::MantleOpenaiResponses,
        Some("/openai/v1"),
    );
    context
        .extra_body
        .insert("tools".to_string(), json!({"type": "image_generation"}));

    let built = provider
        .build_responses_request(&request, &context, false)
        .await
        .expect("request");
    let body: Value =
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
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
    assert!(!authorization.contains("x-request-id"));
    assert_eq!(built.headers().get("x-request-id").unwrap(), "req-test");
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
async fn builds_mantle_anthropic_messages_request_with_authoritative_envelope() {
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
    context
        .extra_body
        .insert("model".to_string(), json!("route-override"));
    context
        .extra_body
        .insert("anthropic_version".to_string(), json!("hostile-version"));
    context.extra_body.insert("stream".to_string(), json!(true));

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
    assert_eq!(body["stream"], false);
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
