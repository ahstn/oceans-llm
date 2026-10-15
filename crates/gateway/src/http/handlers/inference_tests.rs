use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{Extension, Json, body::to_bytes, extract::State, http::HeaderMap, response::Response};
use gateway_core::{
    BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings, CoreChatRequest,
    CoreEmbeddingsRequest, CoreResponsesRequest, Money4, ProviderCapabilities, ProviderClient,
    ProviderError, ProviderRequestContext, ProviderStream, RequestAttemptStatus, RequestLogDetail,
    RequestLogQuery,
};
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{
    AppError, v1_chat_completions, v1_decisions, v1_embeddings, v1_messages_inner, v1_responses,
};
use crate::http::{
    state::AppState,
    test_support::{app_state, seed_stream_cancellation_test},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endpoint {
    Chat,
    Responses,
    Messages,
    Embeddings,
    Decisions,
}

impl Endpoint {
    async fn call(self, state: AppState, stream: bool) -> Result<Response, AppError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "Bearer gwk_streamtest.cancel-secret".parse().unwrap(),
        );
        let request_id = Some(Extension(RequestId::new("inference-test".parse().unwrap())));
        let auth = super::InferenceAuth(
            state
                .service
                .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
                .await
                .unwrap(),
        );
        match self {
            Self::Chat => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "messages": [{"role": "user", "content": "hello"}],
                    "stream": stream,
                }))
                .unwrap();
                v1_chat_completions(State(state), request_id, headers, auth, Json(request)).await
            }
            Self::Responses => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "input": "hello", "stream": stream,
                }))
                .unwrap();
                v1_responses(State(state), request_id, headers, auth, Json(request)).await
            }
            Self::Messages => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "max_tokens": 16, "stream": stream,
                    "messages": [{"role": "user", "content": "hello"}],
                }))
                .unwrap();
                v1_messages_inner(state, request_id, headers, auth.0, request).await
            }
            Self::Embeddings => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "input": "hello",
                }))
                .unwrap();
                v1_embeddings(State(state), request_id, headers, auth, Json(request)).await
            }
            Self::Decisions => {
                let request = serde_json::from_value(json!({
                    "model": "fast", "state": {"ticket": "hello"},
                    "questions": {"urgent": {"type": "noul", "instructions": "Is it urgent?"}},
                }))
                .unwrap();
                v1_decisions(State(state), request_id, headers, auth, Json(request)).await
            }
        }
    }
}

#[derive(Default)]
struct RecordingProvider {
    calls: Mutex<Vec<(Endpoint, bool)>>,
}

#[async_trait]
impl ProviderClient for RecordingProvider {
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
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        assert_eq!(request.model, "fast");
        assert_eq!(context.upstream_model, "fast-upstream");
        self.calls.lock().unwrap().push((Endpoint::Chat, false));
        Ok(json!({
            "model": "fast-upstream", "choices": [{"message": {"role": "assistant", "content": "ok"}}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3},
        }))
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        assert_eq!(request.model, "fast");
        assert_eq!(context.upstream_model, "fast-upstream");
        self.calls
            .lock()
            .unwrap()
            .push((Endpoint::Responses, false));
        Ok(json!({
            "model": "fast-upstream", "output": [],
            "usage": {"input_tokens": 2, "output_tokens": 1, "total_tokens": 3},
        }))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        self.calls.lock().unwrap().push((Endpoint::Chat, true));
        Err(ProviderError::Transport("connection reset".to_string()))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        self.calls.lock().unwrap().push((Endpoint::Responses, true));
        Err(ProviderError::Transport("connection reset".to_string()))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("inference endpoints must not dispatch embeddings")
    }
}

async fn setup() -> (tempfile::TempDir, AppState, Arc<RecordingProvider>) {
    let (directory, mut state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    let provider = Arc::new(RecordingProvider::default());
    state.providers.register(provider.clone());
    (directory, state, provider)
}

async fn request_detail(state: &AppState) -> RequestLogDetail {
    let page = state
        .service
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 10,
            request_id: Some("inference-test".to_string()),
            ..Default::default()
        })
        .await
        .expect("request log persisted before endpoint returns");
    assert_eq!(page.total, 1);
    state
        .service
        .get_request_log_detail(page.items[0].request_log_id)
        .await
        .unwrap()
}

#[tokio::test]
async fn inference_endpoints_normalize_models_and_account_for_success() {
    for endpoint in [Endpoint::Chat, Endpoint::Responses] {
        let (_directory, state, provider) = setup().await;
        let response = endpoint
            .call(state.clone(), false)
            .await
            .unwrap_or_else(|error| panic!("{endpoint:?} request failed: {}", error.0));
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-request-id"], "inference-test");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["model"], "fast");
        assert_eq!(*provider.calls.lock().unwrap(), [(endpoint, false)]);

        let detail = request_detail(&state).await;
        assert_eq!(detail.log.status_code, Some(200));
        assert_eq!(detail.log.model_key, "fast");
        assert_eq!(detail.log.provider_key, "vertex");
        assert_eq!(detail.log.prompt_tokens, Some(2));
        assert_eq!(detail.log.completion_tokens, Some(1));
        assert_eq!(detail.log.total_tokens, Some(3));
        assert_eq!(detail.attempts.len(), 1);
        assert_eq!(detail.attempts[0].status, RequestAttemptStatus::Success);
        let scope = format!("service_account:{}", detail.log.service_account_id.unwrap());
        let ledger = state
            .store
            .get_usage_ledger_by_request_and_scope("inference-test", &scope)
            .await
            .unwrap()
            .expect("successful request records usage");
        assert_eq!(ledger.prompt_tokens, Some(2));
        assert_eq!(ledger.completion_tokens, Some(1));
        assert_eq!(ledger.total_tokens, Some(3));
        assert_eq!(ledger.upstream_model, "fast-upstream");
        assert_eq!(
            state
                .metrics
                .test_snapshot()
                .request_outcomes
                .get("success"),
            Some(&1)
        );
    }
}

#[tokio::test]
async fn inference_endpoints_log_stream_start_failure_without_charging() {
    for endpoint in [Endpoint::Chat, Endpoint::Responses] {
        let (_directory, state, provider) = setup().await;
        let error = endpoint.call(state.clone(), true).await.unwrap_err();
        assert_eq!(error.0.error_code(), "upstream_transport");
        assert_eq!(*provider.calls.lock().unwrap(), [(endpoint, true)]);
        let detail = request_detail(&state).await;
        assert_eq!(detail.log.error_code.as_deref(), Some("upstream_transport"));
        assert_eq!(detail.attempts.len(), 1);
        assert_eq!(
            detail.attempts[0].status,
            RequestAttemptStatus::StreamStartError
        );
        let scope = format!("service_account:{}", detail.log.service_account_id.unwrap());
        assert!(
            state
                .store
                .get_usage_ledger_by_request_and_scope("inference-test", &scope)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(state.metrics.test_snapshot().requests, 1);
    }
}

#[tokio::test]
async fn inference_endpoints_reject_exhausted_budget_before_dispatch() {
    for (endpoint, stream) in [
        (Endpoint::Chat, false),
        (Endpoint::Chat, true),
        (Endpoint::Responses, false),
        (Endpoint::Responses, true),
        (Endpoint::Messages, false),
        (Endpoint::Messages, true),
        (Endpoint::Embeddings, false),
        (Endpoint::Decisions, false),
    ] {
        let (_directory, state, provider) = setup().await;
        let auth = state
            .service
            .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
            .await
            .unwrap();
        state
            .store
            .upsert_active_budget(
                &BudgetScope::ServiceAccount {
                    service_account_id: auth.owner_service_account_id.unwrap(),
                },
                &BudgetSettings {
                    cadence: BudgetCadence::Daily,
                    amount_usd: Money4::from_scaled(0),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                },
                gateway_service::offset_now(),
            )
            .await
            .unwrap();
        let error = endpoint.call(state.clone(), stream).await.unwrap_err();
        assert_eq!(
            error.0.error_code(),
            "budget_exceeded",
            "{endpoint:?}: {}",
            error.0
        );
        assert_eq!(error.0.http_status_code(), 422);
        assert!(provider.calls.lock().unwrap().is_empty());
        let metrics = state.metrics.test_snapshot();
        let request_outcomes = if endpoint == Endpoint::Decisions {
            // Decisions records its own request metric, never the chat one.
            assert_eq!(metrics.requests, 0);
            metrics
                .decisions_requests
                .iter()
                .map(|(request, count)| {
                    assert_eq!(*count, 1);
                    request.outcome.as_str()
                })
                .collect::<Vec<_>>()
        } else {
            assert_eq!(metrics.requests, 1);
            assert!(metrics.decisions_requests.is_empty());
            metrics
                .request_outcomes
                .keys()
                .map(String::as_str)
                .collect()
        };
        assert_eq!(request_outcomes, ["budget_error"], "{endpoint:?}");
        let detail = request_detail(&state).await;
        assert_eq!(
            detail.log.status_code,
            Some(i64::from(error.0.http_status_code())),
            "{endpoint:?} logs the returned status"
        );
        assert_eq!(detail.log.error_code.as_deref(), Some("budget_exceeded"));
        assert_eq!(detail.log.provider_key, "vertex");
        assert_eq!(
            detail.log.metadata["stream"],
            Value::Bool(stream),
            "{endpoint:?} keeps the requested stream flag"
        );
        assert!(
            detail.attempts.is_empty(),
            "{endpoint:?} budget rejection must not record a provider attempt"
        );
    }
}
