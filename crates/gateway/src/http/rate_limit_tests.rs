//! Rate limits through the real router: error envelopes, headers, recovery
//! after the window, precedence, and token limits charged after a response.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::HeaderMap,
};
use gateway_core::{
    BatchCapabilities, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    RateLimitMetric, RateLimitOperation, RateLimitPeriod, RateLimitScopeKind,
};
use gateway_service::ConfiguredRateLimitPolicy;
use http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::http::test_support::{
    TestClock, app_state, install_rate_limits, seed_stream_cancellation_test,
};

const API_KEY: &str = "gwk_streamtest.cancel-secret";

/// Answers chat and embeddings with fixed usage, and counts calls.
#[derive(Default)]
struct CountingProvider {
    calls: AtomicUsize,
    /// When set, chat fails after reporting usage, as a cut-off upstream does.
    fail_with_partial_usage: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl ProviderClient for CountingProvider {
    fn provider_key(&self) -> &str {
        "vertex"
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all_enabled()
    }

    fn batch_capabilities(&self) -> BatchCapabilities {
        BatchCapabilities::NONE
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_with_partial_usage.load(Ordering::SeqCst) {
            return Err(ProviderError::PartialUsage {
                source: Box::new(ProviderError::Transport("connection reset".to_string())),
                provider_usage: Some(
                    json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}),
                ),
            });
        }
        Ok(json!({
            "id": "chatcmpl-rate-limit",
            "object": "chat.completion",
            "created": 1,
            "model": "fast-upstream",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop",
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
        }))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented("stream".to_string()))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({
            "object": "list",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.1]}],
            "model": "fast-upstream",
            "usage": {"prompt_tokens": 2, "total_tokens": 2},
        }))
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented("responses".to_string()))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented("responses".to_string()))
    }
}

fn policy(
    name: &str,
    operation: RateLimitOperation,
    scope_kind: RateLimitScopeKind,
    metric: RateLimitMetric,
    period: RateLimitPeriod,
    limit: i64,
) -> ConfiguredRateLimitPolicy {
    ConfiguredRateLimitPolicy {
        name: name.to_string(),
        operation,
        scope_kind,
        model_key: None,
        mcp_server_key: None,
        metric,
        period,
        limit,
    }
}

struct Harness {
    _directory: tempfile::TempDir,
    store: Arc<gateway_store::AnyStore>,
    router: Router,
    metrics: Arc<crate::observability::GatewayMetrics>,
    clock: TestClock,
    provider: Arc<CountingProvider>,
}

async fn harness(policies: Vec<ConfiguredRateLimitPolicy>) -> Harness {
    let (directory, mut state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    let provider = Arc::new(CountingProvider::default());
    state.providers.register(provider.clone());
    let clock = install_rate_limits(&mut state, policies);
    Harness {
        _directory: directory,
        store: state.store.clone(),
        metrics: state.metrics.clone(),
        router: crate::http::build_router(state, admin_ui::AdminUiConfig::default()),
        clock,
        provider,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|value| value.to_str().unwrap())
    }
}

impl Harness {
    async fn post(&self, path: &str, body: Value) -> Reply {
        let anthropic = path.contains("messages");
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        let request = if anthropic {
            request.header("x-api-key", API_KEY)
        } else {
            request.header("authorization", format!("Bearer {API_KEY}"))
        };
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn chat(&self) -> Reply {
        self.post(
            "/v1/chat/completions",
            json!({"model": "fast", "messages": [{"role": "user", "content": "hello"}]}),
        )
        .await
    }
}

#[tokio::test]
async fn scoped_keys_keep_their_key_limits_and_scope_refusals_count_nothing() {
    use gateway_core::{AdminApiKeyRepository, ApiKeyAccess, ApiKeyRepository};

    let harness = harness(vec![policy(
        "per key",
        RateLimitOperation::Inference,
        RateLimitScopeKind::ApiKey,
        RateLimitMetric::Requests,
        RateLimitPeriod::Minute,
        1,
    )])
    .await;
    let key_id = harness
        .store
        .get_api_key_by_public_id("streamtest")
        .await
        .unwrap()
        .unwrap()
        .id;
    let set_scopes = |scope: &str| ApiKeyAccess::new(Some(vec![scope.to_string()]), None).unwrap();

    // Refused for scope before rate limiting, so it uses none of the limit.
    harness
        .store
        .replace_api_key_access(key_id, &set_scopes("mcp:proxy"))
        .await
        .unwrap();
    let refused = harness.chat().await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(refused.body["error"]["code"], "insufficient_scope");
    assert_eq!(harness.provider.calls.load(Ordering::SeqCst), 0);

    // Narrowed to `inference`, the key is still the policy's subject.
    harness
        .store
        .replace_api_key_access(key_id, &set_scopes("inference"))
        .await
        .unwrap();
    let first = harness.chat().await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(first.header("x-ratelimit-remaining"), Some("0"));
    let limited = harness.chat().await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(harness.provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn requests_over_the_limit_get_429_with_headers_until_the_window_slides() {
    let harness = harness(vec![policy(
        "per key",
        RateLimitOperation::Inference,
        RateLimitScopeKind::ApiKey,
        RateLimitMetric::Requests,
        RateLimitPeriod::Minute,
        2,
    )])
    .await;

    let first = harness.chat().await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(first.header("x-ratelimit-limit"), Some("2"));
    assert_eq!(first.header("x-ratelimit-remaining"), Some("1"));
    // Reset is when the whole limit is free again: the end of the next minute,
    // once this window has slid out. `Retry-After` below is when one more fits.
    assert_eq!(first.header("x-ratelimit-reset"), Some("110"));
    assert_eq!(first.header("retry-after"), None);
    let second = harness.chat().await;
    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(second.header("x-ratelimit-remaining"), Some("0"));

    let limited = harness.chat().await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.body["error"]["type"], "rate_limit_error");
    assert_eq!(limited.body["error"]["code"], "rate_limited");
    assert_eq!(
        limited.body["error"]["message"],
        "rate limited: API key rate limit exceeded: 2 inference requests per minute. Retry after 51 seconds"
    );
    assert_eq!(limited.header("retry-after"), Some("51"));
    assert_eq!(limited.header("x-ratelimit-remaining"), Some("0"));
    // The rejected request never reached the provider.
    assert_eq!(harness.provider.calls.load(Ordering::SeqCst), 2);

    // Still limited a little before the retry time, admitted after it.
    harness.clock.advance(50);
    assert_eq!(harness.chat().await.status, StatusCode::TOO_MANY_REQUESTS);
    harness.clock.advance(1);
    let recovered = harness.chat().await;
    assert_eq!(recovered.status, StatusCode::OK, "{}", recovered.body);
    assert_eq!(recovered.header("x-ratelimit-remaining"), Some("0"));
}

#[tokio::test]
async fn every_inference_route_shares_the_limit_in_its_own_envelope() {
    let harness = harness(vec![
        policy(
            "team inference",
            RateLimitOperation::Inference,
            RateLimitScopeKind::Team,
            RateLimitMetric::Requests,
            RateLimitPeriod::Day,
            1,
        ),
        policy(
            "key counts",
            RateLimitOperation::CountTokens,
            RateLimitScopeKind::ApiKey,
            RateLimitMetric::Requests,
            RateLimitPeriod::Day,
            1,
        ),
    ])
    .await;
    assert_eq!(harness.chat().await.status, StatusCode::OK);

    let chat_messages = json!([{"role": "user", "content": "hello"}]);
    let cases = [
        (
            "/v1/chat/completions",
            json!({"model": "fast", "messages": chat_messages}),
        ),
        (
            "/chat/completions",
            json!({"model": "fast", "messages": chat_messages}),
        ),
        ("/v1/responses", json!({"model": "fast", "input": "hello"})),
        ("/v1/embeddings", json!({"model": "fast", "input": "hello"})),
        (
            "/v1/decisions",
            json!({
                "model": "fast",
                "state": {"ticket": "refund"},
                "questions": {"approve": {"type": "noul", "instructions": "Approve?"}},
            }),
        ),
        (
            "/v1/messages",
            json!({"model": "fast", "max_tokens": 16, "messages": chat_messages}),
        ),
    ];
    for (path, body) in cases {
        let reply = harness.post(path, body).await;
        assert_eq!(
            reply.status,
            StatusCode::TOO_MANY_REQUESTS,
            "{path}: {}",
            reply.body
        );
        assert_eq!(reply.body["error"]["type"], "rate_limit_error", "{path}");
        let message = reply.body["error"]["message"].as_str().unwrap();
        assert!(
            message
                .contains("Team rate limit exceeded: 1 inference requests per calendar day (UTC)"),
            "{path}: {message}"
        );
        assert!(reply.header("retry-after").is_some(), "{path}");
        assert_eq!(
            reply.body["type"] == "error",
            path == "/v1/messages",
            "{path}"
        );
    }
    assert_eq!(harness.provider.calls.load(Ordering::SeqCst), 1);

    // Token counts are their own operation, with the Anthropic envelope.
    let count = json!({"model": "fast", "messages": chat_messages});
    let first = harness
        .post("/v1/messages/count_tokens", count.clone())
        .await;
    assert_ne!(
        first.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        first.body
    );
    assert_eq!(first.header("x-ratelimit-remaining"), Some("0"));
    let limited = harness.post("/v1/messages/count_tokens", count).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.body["type"], "error");
    assert_eq!(limited.body["error"]["type"], "rate_limit_error");
    assert!(
        limited.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("API key rate limit exceeded: 1 token counts per calendar day (UTC)")
    );
}

#[tokio::test]
async fn headers_report_the_tightest_policy_and_any_rejection_wins() {
    let harness = harness(vec![
        policy(
            "gateway",
            RateLimitOperation::Inference,
            RateLimitScopeKind::Global,
            RateLimitMetric::Requests,
            RateLimitPeriod::Hour,
            100,
        ),
        policy(
            "team",
            RateLimitOperation::Inference,
            RateLimitScopeKind::Team,
            RateLimitMetric::Requests,
            RateLimitPeriod::Minute,
            3,
        ),
        // Other operations and models never apply to chat on `fast`.
        policy(
            "mcp",
            RateLimitOperation::McpToolCall,
            RateLimitScopeKind::Global,
            RateLimitMetric::Requests,
            RateLimitPeriod::Minute,
            1,
        ),
        ConfiguredRateLimitPolicy {
            model_key: Some("slow".to_string()),
            ..policy(
                "slow model",
                RateLimitOperation::Inference,
                RateLimitScopeKind::Global,
                RateLimitMetric::Requests,
                RateLimitPeriod::Minute,
                1,
            )
        },
    ])
    .await;

    let first = harness.chat().await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.header("x-ratelimit-limit"), Some("3"));
    assert_eq!(first.header("x-ratelimit-remaining"), Some("2"));
    harness.chat().await;
    harness.chat().await;
    let limited = harness.chat().await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.header("x-ratelimit-limit"), Some("3"));
    assert!(
        limited.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Team rate limit exceeded")
    );
}

#[tokio::test]
async fn token_limits_are_charged_after_the_response() {
    let harness = harness(vec![policy(
        "team tokens",
        RateLimitOperation::Inference,
        RateLimitScopeKind::Team,
        RateLimitMetric::Tokens,
        RateLimitPeriod::Hour,
        20,
    )])
    .await;

    let first = harness.chat().await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.header("x-ratelimit-remaining"), Some("20"));
    // The 15 tokens are charged before the response is returned, so the very
    // next sequential request already sees them.

    let second = harness.chat().await;
    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(second.header("x-ratelimit-remaining"), Some("5"));

    let limited = harness.chat().await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        limited.body["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("rate limited: Team rate limit exceeded: 20 tokens per hour")
    );
    assert_eq!(limited.header("x-ratelimit-remaining"), Some("0"));
    assert_eq!(harness.provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn requests_without_a_matching_policy_carry_no_rate_limit_headers() {
    let harness = harness(Vec::new()).await;
    let reply = harness.chat().await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header("x-ratelimit-limit"), None);
}

#[tokio::test]
async fn rejected_requests_keep_one_request_metric_sample_per_route() {
    let harness = harness(vec![policy(
        "team inference",
        RateLimitOperation::Inference,
        RateLimitScopeKind::Team,
        RateLimitMetric::Requests,
        RateLimitPeriod::Day,
        1,
    )])
    .await;
    assert_eq!(harness.chat().await.status, StatusCode::OK);
    let before = harness.metrics.test_snapshot();

    let chat_messages = json!([{"role": "user", "content": "hello"}]);
    for (path, body) in [
        (
            "/v1/chat/completions",
            json!({"model": "fast", "messages": chat_messages}),
        ),
        ("/v1/responses", json!({"model": "fast", "input": "hello"})),
        (
            "/v1/messages",
            json!({"model": "fast", "max_tokens": 16, "messages": chat_messages}),
        ),
        ("/v1/embeddings", json!({"model": "fast", "input": "hello"})),
        (
            "/v1/decisions",
            json!({
                "model": "fast",
                "state": {"ticket": "refund"},
                "questions": {"approve": {"type": "noul", "instructions": "Approve?"}},
            }),
        ),
    ] {
        let reply = harness.post(path, body).await;
        assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS, "{path}");
    }

    let after = harness.metrics.test_snapshot();
    // Chat Completions, Responses and Messages share the chat request metric.
    assert_eq!(after.requests - before.requests, 3);
    assert_eq!(
        after
            .request_outcomes
            .get("rate_limit_error")
            .copied()
            .unwrap_or(0),
        3
    );
    let rate_limited = |samples: Vec<(i64, String)>| {
        samples
            .into_iter()
            .filter(|(status, outcome)| *status == 429 && outcome == "rate_limit_error")
            .count()
    };
    assert_eq!(
        rate_limited(
            after
                .embeddings_requests
                .iter()
                .flat_map(|(sample, count)| {
                    std::iter::repeat_n(
                        (sample.status_code, sample.outcome.clone()),
                        *count as usize,
                    )
                })
                .collect()
        ),
        1
    );
    assert_eq!(
        rate_limited(
            after
                .decisions_requests
                .iter()
                .flat_map(|(sample, count)| {
                    std::iter::repeat_n(
                        (sample.status_code, sample.outcome.clone()),
                        *count as usize,
                    )
                })
                .collect()
        ),
        1
    );
}

#[tokio::test]
async fn partial_usage_on_a_failed_request_is_charged_to_token_limits() {
    let harness = harness(vec![policy(
        "team tokens",
        RateLimitOperation::Inference,
        RateLimitScopeKind::Team,
        RateLimitMetric::Tokens,
        RateLimitPeriod::Hour,
        100,
    )])
    .await;
    harness
        .provider
        .fail_with_partial_usage
        .store(true, Ordering::SeqCst);
    let chat_messages = json!([{"role": "user", "content": "hello"}]);
    for (path, body) in [
        (
            "/v1/chat/completions",
            json!({"model": "fast", "messages": chat_messages}),
        ),
        (
            "/v1/messages",
            json!({"model": "fast", "max_tokens": 16, "messages": chat_messages}),
        ),
    ] {
        let reply = harness.post(path, body).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_GATEWAY,
            "{path}: {}",
            reply.body
        );
    }

    // Both failures reported 15 tokens each before failing.
    harness
        .provider
        .fail_with_partial_usage
        .store(false, Ordering::SeqCst);
    let next = harness.chat().await;
    assert_eq!(next.status, StatusCode::OK);
    assert_eq!(next.header("x-ratelimit-remaining"), Some("70"));
}
