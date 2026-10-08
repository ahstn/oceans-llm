use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

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
    BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings, CoreChatRequest,
    CoreDecisionsRequest, CoreEmbeddingsRequest, CoreResponsesRequest, GitHubCopilotChatApi,
    GitHubCopilotRouteCompatibility, GitHubCopilotUpstreamSupports, ModelRoutingPolicy, Money4,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderFailoverPolicy,
    ProviderRequestContext, ProviderStream, RequestAttemptStatus, RequestLogDetail,
    RequestLogQuery, RoutePricingOverride, RoutingStrategy, SeedModel, SeedModelRoute,
    SeedProvider, SessionAffinityPolicy, UsageLedgerRecord,
};
use gateway_store::{AnyStore, GatewayStore};
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{AppError, InferenceAuth, tests::seed_stream_cancellation_test};
use crate::http::{state::AppState, test_support::app_state};

#[path = "failover_safety_tests.rs"]
mod safety;

#[derive(Clone, Copy, Debug)]
enum Endpoint {
    Chat,
    Messages,
    Responses,
    Embeddings,
    Decisions,
}

const NON_STREAM_ENDPOINTS: [Endpoint; 5] = [
    Endpoint::Chat,
    Endpoint::Messages,
    Endpoint::Responses,
    Endpoint::Embeddings,
    Endpoint::Decisions,
];

#[derive(Clone, Copy)]
enum Outcome {
    Success,
    Transient,
    PartialUsage,
    StreamFailure,
    PendingStream,
    Quota,
    QuotaExhaustsBudget,
    LongRetryAfter,
}

#[derive(Clone, Debug)]
struct ProviderCall {
    provider: &'static str,
    request_id: String,
    upstream_model: String,
    previous_response_id: Option<String>,
}

struct ScriptedProvider {
    key: &'static str,
    provider_type: &'static str,
    calls: Arc<Mutex<Vec<ProviderCall>>>,
    outcomes: Arc<Mutex<VecDeque<Outcome>>>,
    store: Arc<AnyStore>,
    budget_scope: BudgetScope,
}

impl ScriptedProvider {
    async fn answer(
        &self,
        context: &ProviderRequestContext,
        previous_response_id: Option<&Value>,
    ) -> Result<(Value, Outcome), ProviderError> {
        self.calls.lock().unwrap().push(ProviderCall {
            provider: self.key,
            request_id: context.request_id.clone(),
            upstream_model: context.upstream_model.clone(),
            previous_response_id: previous_response_id
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
        let outcome = self
            .outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("provider must not run more attempts than the test permits");
        match outcome {
            Outcome::Transient => Err(transient_error()),
            Outcome::LongRetryAfter => Err(ProviderError::UpstreamHttp {
                status: 503,
                body: r#"{"error":{"code":"unavailable"}}"#.to_string(),
                retry_after: Some(std::time::Duration::from_secs(3_600)),
            }),
            Outcome::Quota | Outcome::QuotaExhaustsBudget => {
                if matches!(outcome, Outcome::QuotaExhaustsBudget) {
                    self.store
                        .upsert_active_budget(
                            &self.budget_scope,
                            &BudgetSettings {
                                cadence: BudgetCadence::Daily,
                                amount_usd: Money4::ZERO,
                                hard_limit: true,
                                timezone: "UTC".to_string(),
                            },
                            gateway_service::offset_now(),
                        )
                        .await
                        .unwrap();
                }
                Err(ProviderError::UpstreamHttp {
                    status: 402,
                    body: r#"{"error":{"code":"quota_exceeded"}}"#.to_string(),
                    retry_after: None,
                })
            }
            Outcome::PartialUsage => Err(ProviderError::PartialUsage {
                source: Box::new(transient_error()),
                provider_usage: Some(usage()),
            }),
            Outcome::Success | Outcome::StreamFailure | Outcome::PendingStream => Ok((
                json!({
                    "id": format!("resp_{}", context.request_id),
                    "model": context.upstream_model,
                    "status": "completed",
                    "output": [],
                    "choices": [{"message": {"role": "assistant", "content": "ok"}}],
                    "data": [{"object": "embedding", "index": 0, "embedding": [0.25]}],
                    "usage": usage(),
                }),
                outcome,
            )),
        }
    }
}

#[async_trait]
impl ProviderClient for ScriptedProvider {
    fn provider_key(&self) -> &str {
        self.key
    }

    fn provider_type(&self) -> &str {
        self.provider_type
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all_enabled()
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.answer(context, None).await.map(|(value, _)| value)
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let (response, outcome) = self.answer(context, None).await?;
        assert!(matches!(outcome, Outcome::Success));
        let content = json!({
            "id": response["id"], "object": "chat.completion.chunk", "created": 1,
            "model": response["model"],
            "choices": [{"index": 0, "delta": {"role": "assistant", "content": "ok"}, "finish_reason": null}],
        });
        let completed = json!({
            "id": response["id"], "object": "chat.completion.chunk", "created": 1,
            "model": response["model"],
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": usage(),
        });
        Ok(Box::pin(futures_util::stream::iter([
            Ok(Bytes::from(format!("data: {content}\n\n"))),
            Ok(Bytes::from(format!("data: {completed}\n\n"))),
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
        ])))
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.answer(context, request.extra.get("previous_response_id"))
            .await
            .map(|(value, _)| value)
    }

    async fn responses_stream(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let (mut response, outcome) = self
            .answer(context, request.extra.get("previous_response_id"))
            .await?;
        let event_type = if matches!(outcome, Outcome::StreamFailure | Outcome::PendingStream) {
            response["status"] = json!("in_progress");
            "response.created"
        } else {
            "response.completed"
        };
        let event = json!({"type": event_type, "response": response});
        let mut chunks = vec![Ok(Bytes::from(format!(
            "event: {event_type}\ndata: {event}\n\n"
        )))];
        if matches!(outcome, Outcome::StreamFailure) {
            chunks.push(Err(ProviderError::Transport(
                "stream interrupted".to_string(),
            )));
        }
        let stream = futures_util::stream::iter(chunks);
        if matches!(outcome, Outcome::PendingStream) {
            Ok(Box::pin(stream.chain(futures_util::stream::pending())))
        } else {
            Ok(Box::pin(stream))
        }
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.answer(context, None).await.map(|(value, _)| value)
    }

    async fn decisions(
        &self,
        _request: &CoreDecisionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.answer(context, None).await.map(|(value, _)| value)
    }
}

struct FailoverHarness {
    _directory: tempfile::TempDir,
    state: AppState,
    calls: Arc<Mutex<Vec<ProviderCall>>>,
}

impl FailoverHarness {
    async fn new(outcomes: impl IntoIterator<Item = Outcome>) -> Self {
        Self::with_provider_type(outcomes, "openai_compat").await
    }

    async fn with_provider_type(
        outcomes: impl IntoIterator<Item = Outcome>,
        provider_type: &'static str,
    ) -> Self {
        Self::with_max_attempts(
            outcomes,
            provider_type,
            ProviderFailoverPolicy::default().max_attempts,
        )
        .await
    }

    async fn with_max_attempts(
        outcomes: impl IntoIterator<Item = Outcome>,
        provider_type: &'static str,
        max_attempts: u32,
    ) -> Self {
        let (directory, mut state) = app_state().await;
        seed_stream_cancellation_test(&state.store).await;
        let keys = ["vertex", "secondary"];
        let providers = keys.map(|key| SeedProvider {
            provider_key: key.to_string(),
            provider_type: provider_type.to_string(),
            config: json!({}),
            secrets: None,
        });
        let routing = ModelRoutingPolicy {
            strategy: RoutingStrategy::RoundRobin,
            affinity: Some(SessionAffinityPolicy::default()),
            failover: Some(ProviderFailoverPolicy {
                max_attempts,
                initial_backoff_ms: 1,
                max_backoff_ms: 1,
                ..Default::default()
            }),
        };
        let model = SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            max_reasoning_effort: None,
            description: None,
            tags: Vec::new(),
            rank: 0,
            routing: Some(routing),
            routes: keys
                .into_iter()
                .map(|key| seed_route(key, provider_type))
                .collect(),
            allowlist: None,
        };
        state
            .store
            .seed_from_inputs(&providers, &[model], &[], &[], &[], &[], &[], &[])
            .await
            .expect("seed a model with two failover routes");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(outcomes.into_iter().collect()));
        let auth = state
            .service
            .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
            .await
            .unwrap();
        let budget_scope = BudgetScope::ServiceAccount {
            service_account_id: auth.owner_service_account_id.unwrap(),
        };
        for key in keys {
            state.providers.register_with_routing_identity(
                Arc::new(ScriptedProvider {
                    key,
                    provider_type,
                    calls: calls.clone(),
                    outcomes: outcomes.clone(),
                    store: state.store.clone(),
                    budget_scope: budget_scope.clone(),
                }),
                format!("failover-{key}-v1"),
            );
        }
        Self {
            _directory: directory,
            state,
            calls,
        }
    }

    async fn call(
        &self,
        endpoint: Endpoint,
        request_id: &str,
        stream: bool,
        previous_response_id: Option<&str>,
    ) -> Result<Response, AppError> {
        let auth = InferenceAuth(
            self.state
                .service
                .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
                .await
                .unwrap(),
        );
        let request_id = Some(Extension(RequestId::new(request_id.parse().unwrap())));
        let mut body = json!({"model": "fast", "input": "hello"});
        match endpoint {
            Endpoint::Chat => {
                body = json!({
                    "model": "fast", "messages": [{"role": "user", "content": "hello"}],
                    "stream": stream,
                });
                super::v1_chat_completions(
                    State(self.state.clone()),
                    request_id,
                    HeaderMap::new(),
                    auth,
                    Json(serde_json::from_value(body).unwrap()),
                )
                .await
            }
            Endpoint::Messages => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "max_tokens": 10,
                    "messages": [{"role": "user", "content": "hello"}],
                    "stream": stream,
                }))
                .unwrap();
                super::v1_messages_inner(
                    self.state.clone(),
                    request_id,
                    HeaderMap::new(),
                    auth.0,
                    request,
                )
                .await
            }
            Endpoint::Responses => {
                body["stream"] = json!(stream);
                if let Some(id) = previous_response_id {
                    body["previous_response_id"] = json!(id);
                }
                super::v1_responses(
                    State(self.state.clone()),
                    request_id,
                    HeaderMap::new(),
                    auth,
                    Json(serde_json::from_value(body).unwrap()),
                )
                .await
            }
            Endpoint::Embeddings => {
                super::v1_embeddings(
                    State(self.state.clone()),
                    request_id,
                    HeaderMap::new(),
                    auth,
                    Json(serde_json::from_value(body).unwrap()),
                )
                .await
            }
            Endpoint::Decisions => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "state": {"content": "hello"},
                    "questions": {"allowed": {"type": "noul", "instructions": "Is this allowed?"}},
                }))
                .unwrap();
                super::v1_decisions(
                    State(self.state.clone()),
                    request_id,
                    HeaderMap::new(),
                    auth,
                    Json(request),
                )
                .await
            }
        }
    }

    async fn detail(&self, request_id: &str) -> RequestLogDetail {
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
        assert_eq!(
            page.total, 1,
            "one final request log must include every attempt"
        );
        let detail = self
            .state
            .service
            .get_request_log_detail(page.items[0].request_log_id)
            .await
            .unwrap();
        let calls = self.calls.lock().unwrap();
        let calls: Vec<_> = calls
            .iter()
            .filter(|call| call.request_id == request_id)
            .collect();
        assert_eq!(detail.attempts.len(), calls.len());
        for (attempt, call) in detail.attempts.iter().zip(calls) {
            assert_eq!(attempt.provider_key, call.provider);
            assert_eq!(attempt.upstream_model, call.upstream_model);
        }
        detail
    }

    async fn ledgers(&self, request_id: &str) -> Vec<UsageLedgerRecord> {
        let auth = self
            .state
            .service
            .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
            .await
            .unwrap();
        let scope = format!("service_account:{}", auth.owner_service_account_id.unwrap());
        self.state
            .store
            .get_usage_ledgers_by_request_ids_and_scope(&[request_id.to_string()], &scope)
            .await
            .unwrap()
    }

    async fn assert_usage(&self, request_id: &str, provider: &str) {
        let records = self.ledgers(request_id).await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].provider_key, provider);
        assert_eq!(records[0].prompt_tokens, Some(5));
        assert_eq!(records[0].completion_tokens, Some(2));
        assert_eq!(records[0].total_tokens, Some(7));
        assert_eq!(records[0].computed_cost_usd, Money4::from_scaled(7));
        let detail = self.detail(request_id).await;
        assert_eq!(
            records[0].model_route_id,
            Some(detail.attempts.last().unwrap().route_id)
        );
    }
}

fn seed_route(provider_key: &str, provider_type: &str) -> SeedModelRoute {
    let mut route = SeedModelRoute {
        route_key: Some(provider_key.to_string()),
        provider_key: provider_key.to_string(),
        upstream_model: format!("{provider_key}-upstream"),
        priority: 0,
        weight: 1.0,
        enabled: true,
        context_window_tokens: None,
        pricing_override: Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(1_000_000),
            output_cost_per_million_tokens: Money4::from_scaled(1_000_000),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
        }),
        extra_headers: Default::default(),
        extra_body: Default::default(),
        capabilities: ProviderCapabilities::all_enabled(),
        compatibility: Default::default(),
    };
    if provider_type == "github_copilot" {
        route.compatibility.github_copilot = Some(GitHubCopilotRouteCompatibility {
            chat_api: Some(GitHubCopilotChatApi::ChatCompletions),
            supports_responses: true,
            supports_embeddings: false,
            upstream_supports: GitHubCopilotUpstreamSupports {
                streaming: true,
                ..Default::default()
            },
        });
    }
    route
}

fn transient_error() -> ProviderError {
    ProviderError::UpstreamHttp {
        status: 503,
        body: r#"{"error":{"code":"unavailable"}}"#.to_string(),
        retry_after: None,
    }
}

fn usage() -> Value {
    json!({"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7})
}

async fn consume(response: Response) -> Value {
    assert_eq!(response.status(), 200);
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

fn assert_attempts(detail: &RequestLogDetail, statuses: &[RequestAttemptStatus], success: bool) {
    assert_eq!(detail.attempts.len(), statuses.len());
    for (index, (attempt, status)) in detail.attempts.iter().zip(statuses).enumerate() {
        assert_eq!(attempt.attempt_number, i64::try_from(index + 1).unwrap());
        assert_eq!(attempt.status, *status);
        assert_eq!(attempt.terminal, index + 1 == statuses.len());
        assert_eq!(
            attempt.produced_final_response,
            success && index + 1 == statuses.len()
        );
        assert_eq!(attempt.request_id, detail.log.request_id);
        assert!(attempt.completed_at.is_some());
    }
}

#[tokio::test]
async fn transient_failure_retries_same_route_and_records_one_success() {
    for endpoint in NON_STREAM_ENDPOINTS {
        let harness = FailoverHarness::new([Outcome::Transient, Outcome::Success]).await;
        consume(
            harness
                .call(endpoint, "retry-success", false, None)
                .await
                .unwrap_or_else(|error| panic!("request failed: {}", error.0)),
        )
        .await;
        let calls = harness.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "{endpoint:?}");
        assert_eq!(calls[0].provider, calls[1].provider);
        assert!(calls.iter().all(|call| call.request_id == "retry-success"));
        let detail = harness.detail("retry-success").await;
        assert_attempts(
            &detail,
            &[
                RequestAttemptStatus::ProviderError,
                RequestAttemptStatus::Success,
            ],
            true,
        );
        assert!(detail.attempts[0].retryable);
        assert_eq!(detail.log.status_code, Some(200));
        harness
            .assert_usage("retry-success", calls[1].provider)
            .await;
        assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
    }
}

#[tokio::test]
async fn exhausted_route_retries_advance_to_an_alternate_provider() {
    for endpoint in NON_STREAM_ENDPOINTS {
        let harness = FailoverHarness::new([
            Outcome::Transient,
            Outcome::Transient,
            Outcome::Transient,
            Outcome::Success,
        ])
        .await;
        consume(
            harness
                .call(endpoint, "alternate-success", false, None)
                .await
                .unwrap_or_else(|error| panic!("request failed: {}", error.0)),
        )
        .await;
        let calls = harness.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 4, "{endpoint:?}");
        assert!(
            calls[..3]
                .iter()
                .all(|call| call.provider == calls[0].provider)
        );
        assert_ne!(calls[2].provider, calls[3].provider);
        let detail = harness.detail("alternate-success").await;
        assert_attempts(
            &detail,
            &[
                RequestAttemptStatus::ProviderError,
                RequestAttemptStatus::ProviderError,
                RequestAttemptStatus::ProviderError,
                RequestAttemptStatus::Success,
            ],
            true,
        );
        assert_eq!(detail.log.provider_key, calls[3].provider);
        harness
            .assert_usage("alternate-success", calls[3].provider)
            .await;
        assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
    }
}

#[tokio::test]
async fn partial_usage_stops_failover_and_records_incurred_usage() {
    let harness = FailoverHarness::new([Outcome::PartialUsage]).await;
    let error = harness
        .call(Endpoint::Embeddings, "partial-usage", false, None)
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 503);
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let detail = harness.detail("partial-usage").await;
    assert_attempts(&detail, &[RequestAttemptStatus::ProviderError], false);
    assert!(!detail.attempts[0].retryable);
    harness
        .assert_usage("partial-usage", calls[0].provider)
        .await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn accepted_stream_failure_never_dispatches_another_attempt() {
    let harness = FailoverHarness::new([Outcome::StreamFailure]).await;
    let response = harness
        .call(Endpoint::Responses, "stream-failure", true, None)
        .await
        .unwrap_or_else(|error| panic!("request failed: {}", error.0));
    assert_eq!(response.status(), 200);
    let mut body = response.into_body().into_data_stream();
    assert!(body.next().await.unwrap().is_ok());
    assert!(body.next().await.unwrap().is_err());
    assert!(body.next().await.is_none());
    drop(body);
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let detail = harness.detail("stream-failure").await;
    assert_attempts(&detail, &[RequestAttemptStatus::StreamError], false);
    harness
        .assert_usage("stream-failure", calls[0].provider)
        .await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 1);
}

#[tokio::test]
async fn strict_responses_continuation_retries_only_its_origin_route() {
    let harness = FailoverHarness::new([
        Outcome::Success,
        Outcome::Transient,
        Outcome::Transient,
        Outcome::Transient,
    ])
    .await;
    let origin = consume(
        harness
            .call(Endpoint::Responses, "origin", false, None)
            .await
            .unwrap_or_else(|error| panic!("request failed: {}", error.0)),
    )
    .await;
    let origin_id = origin["id"].as_str().unwrap();
    let error = harness
        .call(Endpoint::Responses, "continuation", false, Some(origin_id))
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 503);
    let calls = harness.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|call| call.provider == calls[0].provider));
    assert!(
        calls[1..]
            .iter()
            .all(|call| call.previous_response_id.as_deref() == Some(origin_id))
    );
    let detail = harness.detail("continuation").await;
    assert_attempts(
        &detail,
        &[
            RequestAttemptStatus::ProviderError,
            RequestAttemptStatus::ProviderError,
            RequestAttemptStatus::ProviderError,
        ],
        false,
    );
    assert!(harness.ledgers("continuation").await.is_empty());
    harness.assert_usage("origin", calls[0].provider).await;
    assert_eq!(harness.state.metrics.test_snapshot().requests, 2);
}
