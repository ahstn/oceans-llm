//! Router-level checks that every inference endpoint returns the `x-oceans-*` headers
//! and that the cost header matches the usage ledger row for the same request.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::RETRY_AFTER},
    response::Response,
};
use gateway_core::{
    BudgetRepository, CoreChatRequest, CoreDecisionsRequest, CoreEmbeddingsRequest,
    CoreResponsesRequest, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream, RequestLogQuery, UsagePricingStatus,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::http::{
    state::AppState,
    test_support::{app_state, seed_priced_fast_route},
};

const API_KEY: &str = "gwk_streamtest.cancel-secret";

/// Answers every operation with fixed usage: 1000 input and 100 output tokens, which
/// the seeded route prices at $0.0045. Set `failure` to fail every call instead.
#[derive(Default)]
struct FixedUsageProvider {
    failure: Mutex<Option<Failure>>,
}

#[derive(Clone, Copy)]
enum Failure {
    Transport,
    RateLimited,
}

impl FixedUsageProvider {
    fn fail_with(&self, failure: Failure) {
        *self.failure.lock().unwrap() = Some(failure);
    }

    fn respond(&self, body: Value) -> Result<Value, ProviderError> {
        match *self.failure.lock().unwrap() {
            Some(Failure::Transport) => {
                Err(ProviderError::Transport("connection reset".to_string()))
            }
            Some(Failure::RateLimited) => Err(ProviderError::UpstreamHttp {
                status: 429,
                body: "slow down".to_string(),
                retry_after: Some("7".to_string()),
            }),
            None => Ok(body),
        }
    }
}

fn chat_usage() -> Value {
    json!({"prompt_tokens": 1000, "completion_tokens": 100, "total_tokens": 1100})
}

#[async_trait]
impl ProviderClient for FixedUsageProvider {
    fn provider_key(&self) -> &str {
        "vertex"
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all_enabled()
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.respond(json!({
            "id": "chatcmpl-1", "object": "chat.completion", "model": "fast-upstream",
            "choices": [{
                "index": 0, "finish_reason": "stop",
                "message": {"role": "assistant", "content": "ok"},
            }],
            "usage": chat_usage(),
        }))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::Transport(
            "streaming is not under test".to_string(),
        ))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.respond(json!({
            "object": "list", "model": "fast-upstream",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.1, 0.2]}],
            "usage": {"prompt_tokens": 1000, "total_tokens": 1000},
        }))
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.respond(json!({
            "id": "resp_1", "object": "response", "model": "fast-upstream", "output": [],
            "usage": {"input_tokens": 1000, "output_tokens": 100, "total_tokens": 1100},
        }))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::Transport(
            "streaming is not under test".to_string(),
        ))
    }

    async fn decisions(
        &self,
        _request: &CoreDecisionsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.respond(json!({
            "id": "dec_1", "model": "fast-upstream",
            "answers": {"q1": {"type": "noul", "value": 0.5}},
            "usage": chat_usage(),
        }))
    }
}

#[derive(Clone, Copy, Debug)]
enum Endpoint {
    Chat,
    Responses,
    Messages,
    Embeddings,
    Decisions,
}

const ENDPOINTS: [Endpoint; 5] = [
    Endpoint::Chat,
    Endpoint::Responses,
    Endpoint::Messages,
    Endpoint::Embeddings,
    Endpoint::Decisions,
];

impl Endpoint {
    fn request(self, request_id: &str) -> Request<Body> {
        let (path, body) = match self {
            Self::Chat => (
                "/v1/chat/completions",
                json!({"model": "fast", "messages": [{"role": "user", "content": "hello"}]}),
            ),
            Self::Responses => ("/v1/responses", json!({"model": "fast", "input": "hello"})),
            Self::Messages => (
                "/v1/messages",
                json!({
                    "model": "fast", "max_tokens": 16,
                    "messages": [{"role": "user", "content": "hello"}],
                }),
            ),
            Self::Embeddings => ("/v1/embeddings", json!({"model": "fast", "input": "hello"})),
            Self::Decisions => (
                "/v1/decisions",
                json!({
                    "model": "fast", "state": {"text": "hello"},
                    "questions": {"q1": {"type": "noul", "instructions": "Is this a greeting?"}},
                }),
            ),
        };
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {API_KEY}"))
            .header("x-request-id", request_id)
            .body(Body::from(body.to_string()))
            .unwrap()
    }
}

struct Harness {
    _directory: tempfile::TempDir,
    state: AppState,
    provider: Arc<FixedUsageProvider>,
}

impl Harness {
    async fn new() -> Self {
        let (directory, mut state) = app_state().await;
        seed_priced_fast_route(&state.store).await;
        let provider = Arc::new(FixedUsageProvider::default());
        state.providers.register(provider.clone());
        Self {
            _directory: directory,
            state,
            provider,
        }
    }

    async fn send(&self, endpoint: Endpoint, request_id: &str) -> Response {
        crate::http::build_router(self.state.clone(), admin_ui::AdminUiConfig::default())
            .oneshot(endpoint.request(request_id))
            .await
            .unwrap()
    }

    async fn ledger_cost(&self, request_id: &str) -> String {
        let auth = self
            .state
            .service
            .authenticate(Some(&format!("Bearer {API_KEY}")))
            .await
            .unwrap();
        let scope = format!(
            "service_account:{}",
            auth.owner_service_account_id
                .expect("service account owner")
        );
        let ledger = self
            .state
            .store
            .get_usage_ledger_by_request_and_scope(request_id, &scope)
            .await
            .expect("read usage ledger")
            .expect("usage ledger row");
        assert_eq!(ledger.pricing_status, UsagePricingStatus::Priced);
        ledger.computed_cost_usd.format_4dp()
    }

    async fn attempt_route_id(&self, request_id: &str) -> String {
        let page = self
            .state
            .service
            .list_request_logs(&RequestLogQuery {
                page: 1,
                page_size: 10,
                request_id: Some(request_id.to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(page.total, 1, "{request_id}");
        let detail = self
            .state
            .service
            .get_request_log_detail(page.items[0].request_log_id)
            .await
            .unwrap();
        assert_eq!(detail.attempts.len(), 1);
        detail.attempts[0].route_id.to_string()
    }
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().unwrap())
}

#[tokio::test]
async fn success_responses_carry_routing_and_ledger_cost_headers() {
    for endpoint in ENDPOINTS {
        let harness = Harness::new().await;
        let request_id = format!("headers-{endpoint:?}");
        let response = harness.send(endpoint, &request_id).await;
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{endpoint:?}: {body:?}");

        let route_id = harness.attempt_route_id(&request_id).await;
        let expected = [
            ("x-request-id", request_id.clone()),
            ("x-oceans-model", "fast".to_string()),
            ("x-oceans-route-id", route_id),
            ("x-oceans-provider", "vertex".to_string()),
            ("x-oceans-attempt-count", "1".to_string()),
            (
                "x-oceans-response-cost-usd",
                harness.ledger_cost(&request_id).await,
            ),
        ];
        for (name, value) in expected {
            assert_eq!(
                headers.get(name).map(|value| value.to_str().unwrap()),
                Some(value.as_str()),
                "{endpoint:?} {name}"
            );
        }
        assert_ne!(headers["x-oceans-response-cost-usd"], "0.0000");
        assert!(
            headers
                .keys()
                .all(|name| !name.as_str().starts_with("x-litellm-")),
            "{endpoint:?} must not send x-litellm-* response headers"
        );
    }
}

#[tokio::test]
async fn provider_errors_keep_route_headers_and_drop_success_headers() {
    for endpoint in ENDPOINTS {
        let harness = Harness::new().await;
        harness.provider.fail_with(Failure::Transport);
        let request_id = format!("error-headers-{endpoint:?}");
        let response = harness.send(endpoint, &request_id).await;
        assert!(response.status().is_server_error(), "{endpoint:?}");
        assert_eq!(header(&response, "x-request-id"), Some(request_id.as_str()));
        assert_eq!(header(&response, "x-oceans-provider"), Some("vertex"));
        assert_eq!(
            header(&response, "x-oceans-route-id"),
            Some(harness.attempt_route_id(&request_id).await.as_str())
        );
        assert_eq!(header(&response, "x-oceans-attempt-count"), None);
        assert_eq!(header(&response, "x-oceans-response-cost-usd"), None);
        assert_eq!(header(&response, "retry-after"), None);
    }
}

#[tokio::test]
async fn upstream_retry_after_passes_through_on_rate_limits() {
    for endpoint in ENDPOINTS {
        let harness = Harness::new().await;
        harness.provider.fail_with(Failure::RateLimited);
        let response = harness
            .send(endpoint, &format!("retry-after-{endpoint:?}"))
            .await;
        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{endpoint:?}"
        );
        assert_eq!(response.headers()[RETRY_AFTER], "7", "{endpoint:?}");
        assert_eq!(header(&response, "x-oceans-provider"), Some("vertex"));
    }
}

#[tokio::test]
async fn requests_rejected_before_routing_have_no_route_headers() {
    let harness = Harness::new().await;
    let mut request = Endpoint::Chat.request("unknown-model");
    *request.body_mut() = Body::from(
        json!({"model": "missing", "messages": [{"role": "user", "content": "hi"}]}).to_string(),
    );
    let response = crate::http::build_router(harness.state.clone(), Default::default())
        .oneshot(request)
        .await
        .unwrap();
    assert!(response.status().is_client_error());
    assert_eq!(header(&response, "x-request-id"), Some("unknown-model"));
    for name in [
        "x-oceans-model",
        "x-oceans-route-id",
        "x-oceans-provider",
        "x-oceans-attempt-count",
        "x-oceans-response-cost-usd",
    ] {
        assert_eq!(header(&response, name), None, "{name}");
    }
}
