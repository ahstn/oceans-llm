use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    Extension, Json,
    body::{Bytes, to_bytes},
    extract::State,
    http::HeaderMap,
    response::Response,
};
use futures_util::StreamExt;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ModelRoutingPolicy,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    RoutingStrategy, SeedModel, SeedModelRoute, SeedProvider, SessionAffinityPolicy,
};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{AppError, InferenceAuth, tests::seed_stream_cancellation_test};
use crate::http::{state::AppState, test_support::app_state};

#[derive(Clone, Debug)]
struct ProviderCall {
    provider: &'static str,
    response_id: String,
    previous_response_id: Option<String>,
}

struct RoutingProvider {
    key: &'static str,
    calls: Arc<Mutex<Vec<ProviderCall>>>,
    fail: bool,
    stream_end: StreamEnd,
}

#[derive(Clone, Copy)]
enum StreamEnd {
    Completed,
    EarlyEof,
    Pending,
}

impl RoutingProvider {
    fn answer(&self, previous_response_id: Option<&Value>) -> Result<Value, ProviderError> {
        let mut calls = self.calls.lock().unwrap();
        let response_id = format!("resp_{}_{}", self.key, calls.len());
        calls.push(ProviderCall {
            provider: self.key,
            response_id: response_id.clone(),
            previous_response_id: previous_response_id
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
        if self.fail {
            return Err(ProviderError::Transport("upstream unavailable".to_string()));
        }
        Ok(json!({
            "id": response_id,
            "model": "fast-upstream",
            "status": "completed",
            "output": [],
            "choices": [{"message": {"role": "assistant", "content": self.key}}],
        }))
    }
}

#[async_trait]
impl ProviderClient for RoutingProvider {
    fn provider_key(&self) -> &str {
        self.key
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
        self.answer(None)
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        self.answer(None)?;
        panic!("these tests only stream Responses requests")
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.answer(request.extra.get("previous_response_id"))
    }

    async fn responses_stream(
        &self,
        request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let mut response = self.answer(request.extra.get("previous_response_id"))?;
        let event_type = match self.stream_end {
            StreamEnd::Completed => "response.completed",
            StreamEnd::EarlyEof | StreamEnd::Pending => {
                response["status"] = json!("in_progress");
                "response.created"
            }
        };
        let event = json!({"type": event_type, "response": response});
        let stream = futures_util::stream::iter([Ok(Bytes::from(format!(
            "event: {event_type}\ndata: {event}\n\n"
        )))]);
        if matches!(self.stream_end, StreamEnd::Pending) {
            Ok(Box::pin(stream.chain(futures_util::stream::pending())))
        } else {
            Ok(Box::pin(stream))
        }
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("these tests do not dispatch embeddings")
    }
}

struct RoutingHarness {
    _directory: tempfile::TempDir,
    state: AppState,
    calls: Arc<Mutex<Vec<ProviderCall>>>,
}

impl RoutingHarness {
    async fn new(fail: bool) -> Self {
        Self::with_stream_end(fail, StreamEnd::Completed).await
    }

    async fn with_stream_end(fail: bool, stream_end: StreamEnd) -> Self {
        let (directory, mut state) = app_state().await;
        seed_stream_cancellation_test(&state.store).await;
        let provider_keys = ["vertex", "secondary"];
        let providers = provider_keys.map(|key| SeedProvider {
            provider_key: key.to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({}),
            secrets: None,
        });
        let model = SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            max_reasoning_effort: None,
            description: None,
            tags: Vec::new(),
            rank: 0,
            routing: Some(ModelRoutingPolicy {
                strategy: RoutingStrategy::RoundRobin,
                affinity: Some(SessionAffinityPolicy::default()),
            }),
            routes: provider_keys.into_iter().map(seed_route).collect(),
            allowlist: None,
        };
        state
            .store
            .seed_from_inputs(&providers, &[model], &[], &[], &[], &[], &[], &[])
            .await
            .expect("seed two routes for the existing test model");
        let calls = Arc::new(Mutex::new(Vec::new()));
        for key in provider_keys {
            state.providers.register_with_routing_identity(
                Arc::new(RoutingProvider {
                    key,
                    calls: calls.clone(),
                    fail,
                    stream_end,
                }),
                format!("mock-{key}-v1"),
            );
        }
        Self {
            _directory: directory,
            state,
            calls,
        }
    }

    async fn auth(&self) -> InferenceAuth {
        InferenceAuth(
            self.state
                .service
                .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
                .await
                .expect("authenticate service account"),
        )
    }

    async fn chat(&self, headers: HeaderMap) -> Result<Response, AppError> {
        let request = serde_json::from_value(json!({
            "model": "fast", "messages": [{"role": "user", "content": "hello"}],
        }))
        .unwrap();
        super::v1_chat_completions(
            State(self.state.clone()),
            Some(request_id()),
            headers,
            self.auth().await,
            Json(request),
        )
        .await
    }

    async fn responses(
        &self,
        stream: bool,
        previous_response_id: Option<&str>,
    ) -> Result<Response, AppError> {
        let mut request = json!({"model": "fast", "input": "hello", "stream": stream});
        if let Some(id) = previous_response_id {
            request["previous_response_id"] = json!(id);
        }
        super::v1_responses(
            State(self.state.clone()),
            Some(request_id()),
            HeaderMap::new(),
            self.auth().await,
            Json(serde_json::from_value(request).unwrap()),
        )
        .await
    }
}

fn seed_route(provider_key: &str) -> SeedModelRoute {
    SeedModelRoute {
        route_key: Some(provider_key.to_string()),
        provider_key: provider_key.to_string(),
        upstream_model: "fast-upstream".to_string(),
        priority: 0,
        weight: 1.0,
        enabled: true,
        context_window_tokens: None,
        pricing_override: None,
        extra_headers: Default::default(),
        extra_body: Default::default(),
        capabilities: ProviderCapabilities::all_enabled(),
        compatibility: Default::default(),
    }
}

fn session_headers(session: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-oceans-session-id", session.parse().unwrap());
    headers
}

fn request_id() -> Extension<RequestId> {
    Extension(RequestId::new(
        uuid::Uuid::new_v4().to_string().parse().unwrap(),
    ))
}

async fn consume(response: Response) -> Bytes {
    assert_eq!(response.status(), 200);
    to_bytes(response.into_body(), usize::MAX).await.unwrap()
}

#[tokio::test]
async fn round_robin_allocates_new_sessions_and_keeps_existing_sessions_on_their_route() {
    let harness = RoutingHarness::new(false).await;
    for session in ["session-a", "session-a", "session-b", "session-a"] {
        consume(
            harness
                .chat(session_headers(session))
                .await
                .map_err(|error| error.0)
                .unwrap(),
        )
        .await;
    }

    let calls = harness.calls.lock().unwrap();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].provider, calls[1].provider);
    assert_ne!(calls[0].provider, calls[2].provider);
    assert_eq!(calls[0].provider, calls[3].provider);
}

#[tokio::test]
async fn responses_continuations_reuse_the_origin_route_without_a_session_header() {
    for stream in [false, true] {
        let harness = RoutingHarness::new(false).await;
        let body = consume(
            harness
                .responses(stream, None)
                .await
                .map_err(|error| error.0)
                .unwrap(),
        )
        .await;
        let first = harness.calls.lock().unwrap()[0].clone();
        assert!(String::from_utf8_lossy(&body).contains(&first.response_id));

        consume(
            harness
                .responses(false, Some(&first.response_id))
                .await
                .map_err(|error| error.0)
                .unwrap(),
        )
        .await;
        // A fresh request proves that the continuation did not consume the next route.
        consume(
            harness
                .responses(false, None)
                .await
                .map_err(|error| error.0)
                .unwrap(),
        )
        .await;

        let calls = harness.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].provider, calls[1].provider);
        assert_ne!(calls[0].provider, calls[2].provider);
        assert_eq!(
            calls[1].previous_response_id.as_deref(),
            Some(first.response_id.as_str())
        );
    }
}

#[tokio::test]
async fn replacing_a_runtime_provider_invalidates_its_response_origins() {
    let mut harness = RoutingHarness::new(false).await;
    consume(
        harness
            .responses(false, None)
            .await
            .map_err(|error| error.0)
            .unwrap(),
    )
    .await;
    let first = harness.calls.lock().unwrap()[0].clone();

    // The stored provider config is unchanged; only the loaded client identity changes.
    harness.state.providers.register_with_routing_identity(
        Arc::new(RoutingProvider {
            key: first.provider,
            calls: harness.calls.clone(),
            fail: false,
            stream_end: StreamEnd::Completed,
        }),
        format!("mock-{}-v2", first.provider),
    );

    let error = harness
        .responses(false, Some(&first.response_id))
        .await
        .unwrap_err();

    assert_eq!(error.0.http_status_code(), 400);
    assert!(error.0.to_string().contains("no longer eligible"));
    assert_eq!(harness.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn configured_routing_pool_rejects_providers_without_runtime_identities() {
    let mut harness = RoutingHarness::new(false).await;
    for key in ["vertex", "secondary"] {
        harness.state.providers.register(Arc::new(RoutingProvider {
            key,
            calls: harness.calls.clone(),
            fail: false,
            stream_end: StreamEnd::Completed,
        }));
        assert_eq!(harness.state.providers.routing_identity(key), None);
    }

    let error = harness
        .chat(session_headers("session-a"))
        .await
        .unwrap_err();

    assert_eq!(error.0.http_status_code(), 500);
    assert!(error.0.to_string().contains("no runtime provider identity"));
    assert!(harness.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn responses_stream_without_a_terminal_event_does_not_record_an_origin() {
    let harness = RoutingHarness::with_stream_end(false, StreamEnd::EarlyEof).await;
    let body = consume(
        harness
            .responses(true, None)
            .await
            .map_err(|error| error.0)
            .unwrap(),
    )
    .await;
    let first = harness.calls.lock().unwrap()[0].clone();
    assert!(String::from_utf8_lossy(&body).contains(&first.response_id));

    let error = harness
        .responses(false, Some(&first.response_id))
        .await
        .unwrap_err();

    assert_eq!(error.0.http_status_code(), 400);
    assert!(error.0.to_string().contains("no known origin"));
    assert_eq!(harness.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancelling_a_responses_stream_does_not_record_an_origin() {
    let harness = RoutingHarness::with_stream_end(false, StreamEnd::Pending).await;
    let response = harness
        .responses(true, None)
        .await
        .map_err(|error| error.0)
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body().into_data_stream();
    let created = body.next().await.unwrap().unwrap();
    let first = harness.calls.lock().unwrap()[0].clone();
    assert!(String::from_utf8_lossy(&created).contains(&first.response_id));
    drop(body);
    assert_eq!(
        harness
            .state
            .metrics
            .test_snapshot()
            .request_outcomes
            .get("client_cancelled"),
        Some(&1)
    );

    let error = harness
        .responses(false, Some(&first.response_id))
        .await
        .unwrap_err();

    assert_eq!(error.0.http_status_code(), 400);
    assert!(error.0.to_string().contains("no known origin"));
    assert_eq!(harness.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn provider_failure_does_not_retry_another_route() {
    for stream in [false, true] {
        let harness = RoutingHarness::new(true).await;
        let error = harness.responses(stream, None).await.unwrap_err();
        assert_eq!(error.0.error_code(), "upstream_transport");
        assert_eq!(harness.calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn duplicate_session_headers_are_rejected_before_provider_dispatch() {
    let harness = RoutingHarness::new(false).await;
    let mut headers = session_headers("session-a");
    headers.append("x-oceans-session-id", "session-b".parse().unwrap());

    let error = harness.chat(headers).await.unwrap_err();

    assert_eq!(error.0.http_status_code(), 400);
    assert!(error.0.to_string().contains("may only be sent once"));
    assert!(harness.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn service_account_requests_skip_providers_that_require_a_user_credential() {
    let mut harness = RoutingHarness::new(false).await;
    harness.state.copilot_user_provider_keys = Arc::new(vec!["vertex".to_string()]);
    for session in ["session-a", "session-b", "session-c"] {
        consume(
            harness
                .chat(session_headers(session))
                .await
                .map_err(|error| error.0)
                .unwrap(),
        )
        .await;
    }

    let calls = harness.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.provider == "secondary"));
}
