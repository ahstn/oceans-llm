use axum::{Json, Router, http::HeaderMap, routing::post};
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use gateway_core::{OpenAiCompatReasoningEffort, OpenAiCompatRouteCompatibility, ServiceTier};
use serde_json::json;
use tempfile::NamedTempFile;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::*;

fn context() -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "gateway-test-request".to_string(),
        model_key: "chatgpt-test".to_string(),
        provider_key: "chatgpt_plan".to_string(),
        upstream_model: "upstream-test-model".to_string(),
        owner_user_id: None,
        expected_provider_credential_id: None,
        chat_protocol: Default::default(),
        extra_headers: Default::default(),
        extra_body: Default::default(),
        request_headers: Default::default(),
        compatibility: Default::default(),
        service_tier: None,
    }
}

fn request() -> CoreResponsesRequest {
    CoreResponsesRequest {
        model: "chatgpt-test".to_string(),
        input: json!("Hello"),
        stream: true,
        instructions: Some(json!("Keep the caller's instructions.")),
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: Default::default(),
    }
}

fn provider(base_url: &str, strip: bool) -> (ChatGptPlanProvider, NamedTempFile) {
    let credentials_file = NamedTempFile::new().unwrap();
    let mut credentials: Value =
        serde_json::from_str(include_str!("testdata/credential.json")).unwrap();
    credentials["saved_at"] = json!(OffsetDateTime::now_utc().format(&Rfc3339).unwrap());
    std::fs::write(
        credentials_file.path(),
        serde_json::to_vec(&credentials).unwrap(),
    )
    .unwrap();
    let provider = ChatGptPlanProvider::new(ChatGptPlanConfig {
        provider_key: "chatgpt_plan".to_string(),
        credentials_file: credentials_file.path().to_path_buf(),
        base_url: base_url.to_string(),
        strip_unsupported_fields: strip,
        default_headers: Default::default(),
        request_timeout_ms: 5_000,
    })
    .unwrap();
    (provider, credentials_file)
}

fn body(request: &reqwest::Request) -> Value {
    serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap()
}

#[test]
fn required_rewrites_apply_with_both_policy_settings() {
    for strip in [false, true] {
        let mut body = json!({
            "input": "Hello", "store": true, "stream": false,
            "instructions": "Unchanged instructions", "prompt_cache_key": "caller-cache-key"
        });
        policy::apply_request_policy(&mut body, strip);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["input"],
            json!([{
                "type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "Hello"}]
            }])
        );
        assert_eq!(body["instructions"], "Unchanged instructions");
        assert_eq!(body["prompt_cache_key"], "caller-cache-key");

        let mut body = json!({"input": [
            {"type": "message", "role": "system", "content": "Policy"},
            {"role": "system", "content": "More policy"},
            {"role": "user", "content": "Hello"}
        ]});
        policy::apply_request_policy(&mut body, strip);
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["role"], "developer");
        assert_eq!(body["input"][2]["role"], "user");
        assert_eq!(body["input"][0]["content"], "Policy");
    }
}

#[test]
fn optional_policy_strips_exact_fields_and_hosted_tools_without_namespacing() {
    let fields = [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ];
    let hosted_types = [
        "image_generation",
        "file_search",
        "code_interpreter",
        "computer_use",
        "computer_use_preview",
        "computer",
        "mcp",
        "tool_search",
    ];
    let mut tools = vec![
        json!({"type": "function", "name": "lookup", "parameters": {"type": "object"}}),
        json!({"type": "custom", "name": "execute"}),
        json!({"type": "web_search"}),
    ];
    tools.extend(hosted_types.map(|kind| json!({"type": kind})));
    let mut original = json!({
        "input": [{"type": "additional_tools", "tools": tools}],
        "tools": tools,
        "tool_choice": "auto", "service_tier": "caller-value", "instructions": "Do not change"
    });
    for field in fields {
        original[field] = json!({"caller": "value"});
    }
    let mut stripped = original.clone();
    policy::apply_request_policy(&mut stripped, true);
    for field in fields {
        assert!(stripped.get(field).is_none(), "{field} must be stripped");
    }
    assert_eq!(
        stripped["tools"],
        json!([
            {"type": "function", "name": "lookup", "parameters": {"type": "object"}},
            {"type": "custom", "name": "execute"},
            {"type": "web_search"}
        ])
    );
    assert_eq!(stripped["input"][0]["tools"], stripped["tools"]);
    assert_eq!(stripped["tool_choice"], "auto");
    assert_eq!(stripped["service_tier"], "caller-value");
    assert_eq!(stripped["instructions"], "Do not change");

    let mut preserved = original.clone();
    policy::apply_request_policy(&mut preserved, false);
    for field in fields {
        assert_eq!(preserved[field], original[field]);
    }
    assert_eq!(preserved["tools"], original["tools"]);
    assert_eq!(preserved["input"], original["input"]);
}

#[test]
fn programmatic_tool_calling_is_stripped_only_from_top_level_tools() {
    let original = json!({
        "input": [{"type": "additional_tools", "tools": [{"type": "programmatic_tool_calling"}]}],
        "tools": [{"type": "programmatic_tool_calling"}],
        "tool_choice": "auto"
    });
    let mut stripped = original.clone();
    policy::apply_request_policy(&mut stripped, true);
    assert!(stripped.get("tools").is_none());
    assert!(stripped.get("tool_choice").is_none());
    assert_eq!(stripped["input"], original["input"]);
    let mut preserved = original.clone();
    policy::apply_request_policy(&mut preserved, false);
    assert_eq!(preserved["tools"], original["tools"]);
    assert_eq!(preserved["input"], original["input"]);
}

#[test]
fn stripping_all_top_level_tools_removes_tool_choice() {
    let mut body = json!({
        "input": [{"type": "additional_tools", "tools": [{"type": "mcp"}]}],
        "tools": [{"type": "tool_search"}], "tool_choice": "required"
    });
    policy::apply_request_policy(&mut body, true);
    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
    assert_eq!(body["input"][0]["tools"], json!([]));
}

#[test]
fn wire_request_applies_shared_profiles_then_required_policy_and_bearer_only() {
    let (mut provider, _credentials) = provider(DEFAULT_CHATGPT_PLAN_BASE_URL, true);
    provider.config.default_headers = BTreeMap::from([
        (
            "authorization".to_string(),
            "Bearer wrong-default".to_string(),
        ),
        ("account-id".to_string(), "not-used".to_string()),
        ("chatgpt-account-id".to_string(), "not-used".to_string()),
        ("originator".to_string(), "not-used".to_string()),
        ("OpenAI-Beta".to_string(), "not-used".to_string()),
    ]);
    let mut context = context();
    context
        .extra_headers
        .insert("Authorization".to_string(), json!("Bearer wrong-extra"));
    for forbidden in [
        "Account-Id",
        "ChatGPT-Account-Id",
        "Originator",
        "OpenAI-Beta",
    ] {
        context
            .extra_headers
            .insert(forbidden.to_string(), json!("not-used"));
    }
    context.extra_body.insert("store".to_string(), json!(true));
    context
        .extra_body
        .insert("stream".to_string(), json!(false));
    context
        .extra_body
        .insert("temperature".to_string(), json!(0.5));
    context
        .extra_body
        .insert("reasoning_effort".to_string(), json!("high"));
    context.compatibility.openai_compat = Some(OpenAiCompatRouteCompatibility {
        supports_store: false,
        reasoning_effort: OpenAiCompatReasoningEffort::ReasoningObject,
        ..Default::default()
    });
    let built = provider
        .build_responses_stream_request(&request(), &context, "fake-token")
        .unwrap();
    assert_eq!(built.url().as_str(), "https://api.openai.com/v1/responses");
    assert_eq!(built.method(), reqwest::Method::POST);
    assert_eq!(built.headers()["authorization"], "Bearer fake-token");
    assert_eq!(built.headers().get_all("authorization").iter().count(), 1);
    assert!(built.headers()["authorization"].is_sensitive());
    for forbidden in [
        "account-id",
        "chatgpt-account-id",
        "originator",
        "openai-beta",
    ] {
        assert!(!built.headers().contains_key(forbidden));
    }
    let body = body(&built);
    assert_eq!(body["model"], "upstream-test-model");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["reasoning"], json!({"effort": "high"}));
    assert!(body.get("temperature").is_none());
    assert_eq!(body["instructions"], "Keep the caller's instructions.");
}

#[test]
fn wire_request_preserves_unsupported_values_when_stripping_is_disabled() {
    let (provider, _credentials) = provider(DEFAULT_CHATGPT_PLAN_BASE_URL, false);
    let mut request = request();
    request.extra.insert("temperature".to_string(), json!(0.3));
    request
        .extra
        .insert("previous_response_id".to_string(), json!("resp_prior"));
    request.tools = Some(json!([{"type": "code_interpreter"}]));
    let built = provider
        .build_responses_stream_request(&request, &context(), "fake-token")
        .unwrap();
    let body = body(&built);
    assert_eq!(body["temperature"], 0.3);
    assert_eq!(body["previous_response_id"], "resp_prior");
    assert_eq!(body["tools"], json!([{"type": "code_interpreter"}]));
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn rejects_nonstream_and_nonresponses_apis_without_reading_credentials() {
    let (provider, credentials) = provider(DEFAULT_CHATGPT_PLAN_BASE_URL, true);
    drop(credentials);
    let context = context();
    let error = provider.responses(&request(), &context).await.unwrap_err();
    assert!(matches!(error, ProviderError::InvalidRequest(message)
        if message == "chatgpt_plan routes require stream: true"));
    let chat = CoreChatRequest {
        model: "chatgpt-test".to_string(),
        messages: vec![],
        stream: false,
        extra: Default::default(),
    };
    assert!(matches!(
        provider.chat_completions(&chat, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    assert!(matches!(
        provider.chat_completions_stream(&chat, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    assert!(matches!(
        provider.messages(&chat, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    assert!(matches!(
        provider.messages_stream(&chat, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    let embeddings = CoreEmbeddingsRequest {
        model: "chatgpt-test".to_string(),
        input: json!("Hello"),
        extra: Default::default(),
    };
    assert!(matches!(
        provider.embeddings(&embeddings, &context).await,
        Err(ProviderError::NotImplemented(_))
    ));
    let capabilities = provider.capabilities();
    assert!(
        capabilities.responses && capabilities.stream && capabilities.tools && capabilities.vision
    );
    assert!(
        !capabilities.chat_completions
            && !capabilities.embeddings
            && !capabilities.images
            && !capabilities.decisions
    );
    assert_eq!(
        provider.batch_capabilities(),
        gateway_core::BatchCapabilities::NONE
    );
    let mut context = context;
    context.service_tier = Some(ServiceTier::Priority);
    assert!(matches!(
        provider.responses_stream(&request(), &context).await,
        Err(ProviderError::InvalidRequest(_))
    ));
}

fn normalize_fixture(data: String) -> ProviderStream {
    // Split across SSE/JSON boundaries to exercise the real shared byte normalizer.
    let bytes = Bytes::from(data);
    let middle = bytes.len() / 2;
    let upstream = stream::iter(vec![
        Ok::<_, reqwest::Error>(bytes.slice(..middle)),
        Ok(bytes.slice(middle..)),
    ]);
    errors::require_terminal(
        normalize_openai_compat_responses_stream(upstream),
        Some("upstream-request".to_string()),
    )
}

fn sse(value: &Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        value["type"].as_str().unwrap(),
        value
    )
}

#[tokio::test]
async fn completed_stream_preserves_output_usage_and_normalized_done() {
    let completed = json!({"type": "response.completed", "response": {
        "id": "resp_test", "status": "completed", "output": [],
        "usage": {"input_tokens": 12, "output_tokens": 3, "total_tokens": 15,
            "input_tokens_details": {"cached_tokens": 4}}
    }});
    let chunks: Vec<_> = normalize_fixture(format!(
        "{}{}",
        sse(&json!({"type": "response.output_text.delta", "delta": "Hello"})),
        sse(&completed)
    ))
    .collect()
    .await;
    let text: String = chunks
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect();
    assert!(text.contains("Hello"));
    assert!(text.contains(&completed.to_string()));
    assert_eq!(text.matches("data: [DONE]").count(), 1);
}

#[tokio::test]
async fn incomplete_stream_preserves_terminal_event_usage_and_normalized_done() {
    let incomplete = json!({"type": "response.incomplete", "response": {
        "id": "resp_incomplete", "status": "incomplete", "output": [],
        "incomplete_details": {"reason": "max_output_tokens"},
        "usage": {"input_tokens": 12, "output_tokens": 3, "total_tokens": 15}
    }});
    let chunks: Vec<_> = normalize_fixture(format!(
        "{}{}",
        sse(&json!({"type": "response.output_text.delta", "delta": "Partial"})),
        sse(&incomplete)
    ))
    .collect()
    .await;
    let text: String = chunks
        .into_iter()
        .map(|chunk| {
            String::from_utf8(chunk.expect("incomplete is not a transport error").to_vec()).unwrap()
        })
        .collect();
    assert!(text.contains(&incomplete.to_string()));
    assert!(text.contains(r#""usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}"#));
    assert_eq!(text.matches("data: [DONE]").count(), 1);
}

#[tokio::test]
async fn failed_stream_preserves_all_siwc_codes_params_request_ids_and_event_shape() {
    for (code, status) in [
        ("subscription_sharing_user_not_eligible", 403),
        ("subscription_sharing_usage_limit_exceeded", 429),
        ("subscription_sharing_usage_unavailable", 503),
        ("subscription_sharing_unsupported_capability", 400),
        ("subscription_sharing_route_not_supported", 403),
        ("subscription_sharing_invalid_user", 401),
        ("chatpass_v2_scope_not_authorized", 403),
        ("chatpass_v2_invalid_authorization_context", 403),
        ("subscription_sharing_user_unavailable", 503),
        ("future_unknown_error", 502),
    ] {
        let failed = json!({"type": "response.failed", "response": {
            "status": "failed", "error": {"code": code, "param": "tools[0]", "message": "Diagnostic"}
        }});
        let mut stream = normalize_fixture(format!(
            "{}{}",
            sse(&json!({"type": "response.output_text.delta", "delta": "partial"})),
            sse(&failed)
        ));
        assert!(
            stream
                .next()
                .await
                .unwrap()
                .unwrap()
                .starts_with(b"event: response.output_text.delta")
        );
        let error = stream.next().await.unwrap().unwrap_err();
        let ProviderError::UpstreamHttp {
            status: actual,
            body,
            request_id,
        } = error
        else {
            panic!("expected preserved upstream error, got {error:?}");
        };
        assert_eq!(actual, status);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), failed);
        assert_eq!(request_id.as_deref(), Some("upstream-request"));
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test]
async fn explicit_error_and_missing_terminal_are_failures() {
    let explicit = json!({"type": "error", "error": {
        "code": "subscription_sharing_usage_limit_exceeded", "message": "Limit", "param": null
    }});
    let mut stream = normalize_fixture(sse(&explicit));
    assert!(matches!(
        stream.next().await,
        Some(Err(ProviderError::UpstreamHttp { .. }))
    ));
    assert!(stream.next().await.is_none());
    for data in [
        String::new(),
        sse(&json!({"type": "response.output_text.delta", "delta": "partial"})),
    ] {
        let chunks: Vec<_> = normalize_fixture(data).collect().await;
        assert!(chunks.iter().any(Result::is_err));
        assert!(
            !chunks
                .iter()
                .filter_map(|chunk| chunk.as_ref().ok())
                .any(|chunk| chunk
                    .windows(b"[DONE]".len())
                    .any(|window| window == b"[DONE]"))
        );
    }
}

#[tokio::test]
async fn http_admission_errors_preserve_status_detail_and_request_id_without_oauth() {
    for (status, payload) in [
        (401, json!({"detail": "Signed identity not accepted"})),
        (
            403,
            json!({"detail": {"message": "Region restriction", "context": "keep-shape"}}),
        ),
        (503, json!({"detail": "Direct routing unavailable"})),
        (
            400,
            json!({"error": {"code": "subscription_sharing_unsupported_capability", "param": "tools", "message": "Not supported"}}),
        ),
    ] {
        let raw = payload.to_string();
        let expected = raw.clone();
        let app = Router::new().route(
            "/v1/responses",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let raw = raw.clone();
                async move {
                    assert_eq!(
                        headers["authorization"],
                        "Bearer fake-access-token-not-a-credential"
                    );
                    assert_eq!(body["store"], false);
                    assert_eq!(body["stream"], true);
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("x-request-id", "admission-request")],
                        raw,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (provider, _credentials) = provider(&format!("http://{address}/v1"), true);
        let error = match provider.responses_stream(&request(), &context()).await {
            Err(error) => error,
            Ok(_) => panic!("admission error must fail before opening a stream"),
        };
        server.abort();
        let ProviderError::UpstreamHttp {
            status: actual,
            body,
            request_id,
        } = error
        else {
            panic!("expected preserved HTTP failure, got {error:?}");
        };
        assert_eq!(actual, status);
        assert_eq!(body, expected);
        assert_eq!(request_id.as_deref(), Some("admission-request"));
    }
}

#[tokio::test]
async fn provider_streams_completed_usage_from_public_responses_route() {
    let completed = json!({"type": "response.completed", "response": {
        "id": "resp_local", "status": "completed", "output": [],
        "usage": {"input_tokens": 8, "output_tokens": 2, "total_tokens": 10}
    }});
    let wire = format!(
        "{}{}",
        sse(&json!({
            "type": "response.output_text.delta", "delta": "Local fixture"
        })),
        sse(&completed)
    );
    let app = Router::new().route(
        "/v1/responses",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let wire = wire.clone();
            async move {
                assert_eq!(
                    headers["authorization"],
                    "Bearer fake-access-token-not-a-credential"
                );
                assert_eq!(headers["accept"], "text/event-stream");
                assert_eq!(body["model"], "upstream-test-model");
                assert_eq!(body["input"][0]["role"], "user");
                assert_eq!(body["store"], false);
                assert_eq!(body["stream"], true);
                ([("content-type", "text/event-stream; charset=utf-8")], wire)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (provider, _credentials) = provider(&format!("http://{address}/v1"), true);
    let chunks: Vec<_> = provider
        .responses_stream(&request(), &context())
        .await
        .unwrap()
        .collect()
        .await;
    server.abort();
    let output: String = chunks
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect();
    assert!(output.contains("Local fixture"));
    assert!(output.contains(&completed.to_string()));
    assert_eq!(output.matches("data: [DONE]").count(), 1);
}
