//! Decisions requests record one `gateway.decisions.requests` sample on every
//! exit after model resolution, and never a chat request sample.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use axum::{Extension, Json, extract::State, http::HeaderMap, response::Response};
use gateway_core::{
    BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings, CoreChatRequest,
    CoreDecisionsRequest, CoreEmbeddingsRequest, CoreResponsesRequest, DecisionsWireFormat, Money4,
    ProviderCapabilities, ProviderClient, ProviderDecisionsResponse, ProviderError,
    ProviderRequestContext, ProviderStream,
};
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{AppError, InferenceAuth, v1_decisions, v1_systemone};
use crate::{
    http::{
        state::AppState,
        test_support::{app_state, seed_stream_cancellation_test},
    },
    observability::TestDecisionsRequest,
};

const API_KEY: &str = "Bearer gwk_streamtest.cancel-secret";

#[derive(Clone, Copy)]
enum Upstream {
    Answers,
    /// Fails after billable usage, like a truncated upstream body.
    PartialUsage,
    /// Registered without the decisions capability, so no route is eligible.
    NoDecisions,
}

struct DecisionsProvider(Upstream);

#[async_trait]
impl ProviderClient for DecisionsProvider {
    fn provider_key(&self) -> &str {
        "vertex"
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            decisions: !matches!(self.0, Upstream::NoDecisions),
            ..ProviderCapabilities::all_enabled()
        }
    }

    async fn decisions(
        &self,
        request: &CoreDecisionsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderDecisionsResponse, ProviderError> {
        match self.0 {
            Upstream::Answers => {
                let body = json!({
                    "model": "fast-upstream",
                    "answers": {"approve": {"type": "noul", "noul": 0.8}},
                    "usage": {"input_tokens": 4, "output_tokens": 1},
                });
                let parsed = DecisionsWireFormat::SystemOne
                    .parse_response(&body, request)
                    .expect("valid upstream answers");
                Ok(ProviderDecisionsResponse {
                    format: DecisionsWireFormat::SystemOne,
                    body,
                    parsed,
                })
            }
            Upstream::PartialUsage => Err(ProviderError::PartialUsage {
                source: Box::new(ProviderError::Transport("body truncated".to_string())),
                provider_usage: Some(json!({"input_tokens": 4, "output_tokens": 1})),
            }),
            Upstream::NoDecisions => panic!("a route without decisions support was selected"),
        }
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("decisions requests must not dispatch chat completions")
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("decisions requests must not dispatch chat streams")
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("decisions requests must not dispatch responses")
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("decisions requests must not dispatch response streams")
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("decisions requests must not dispatch embeddings")
    }
}

async fn setup(upstream: Upstream) -> (tempfile::TempDir, AppState) {
    let (directory, mut state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    state
        .providers
        .register(Arc::new(DecisionsProvider(upstream)));
    (directory, state)
}

fn system_one_body() -> Value {
    json!({
        "model": "fast",
        "state": "refund request",
        "questions": {"approve": {"type": "noul", "instructions": "Approve?"}},
    })
}

async fn call(state: &AppState, path: &str, body: Value) -> Result<Response, AppError> {
    // Usage is recorded once per request id, so each route gets its own.
    let request_id = format!("decisions-test{}", path.replace('/', "-"));
    let request_id = Some(Extension(RequestId::new(request_id.parse().unwrap())));
    let auth = InferenceAuth(state.service.authenticate(Some(API_KEY)).await.unwrap());
    let state = State(state.clone());
    match path {
        "/v1/decisions" => {
            v1_decisions(state, request_id, HeaderMap::new(), auth, Json(body)).await
        }
        "/v1/systemone" => {
            v1_systemone(state, request_id, HeaderMap::new(), auth, Json(body)).await
        }
        other => panic!("unexpected path {other}"),
    }
}

fn recorded(state: &AppState) -> BTreeMap<TestDecisionsRequest, u64> {
    let snapshot = state.metrics.test_snapshot();
    assert_eq!(
        snapshot.requests, 0,
        "decisions must not record chat requests"
    );
    snapshot.decisions_requests
}

/// Each sample recorded exactly once.
fn once<const N: usize>(samples: [TestDecisionsRequest; N]) -> BTreeMap<TestDecisionsRequest, u64> {
    samples.into_iter().map(|sample| (sample, 1)).collect()
}

fn sample(
    http_route: &str,
    request_format: &str,
    provider_key: &str,
    status_code: i64,
    outcome: &str,
) -> TestDecisionsRequest {
    TestDecisionsRequest {
        http_route: http_route.to_string(),
        request_format: request_format.to_string(),
        provider_key: provider_key.to_string(),
        status_code,
        outcome: outcome.to_string(),
    }
}

#[tokio::test]
async fn success_records_route_format_and_provider() {
    let (_directory, state) = setup(Upstream::Answers).await;
    let response = call(&state, "/v1/systemone", system_one_body())
        .await
        .unwrap_or_else(|error| panic!("System One request failed: {}", error.0));
    assert_eq!(response.status(), 200);

    let openai_body = json!({
        "model": "fast",
        "input": "refund request",
        "questions": [{"type": "predicate", "name": "approve", "instructions": "Approve?"}],
    });
    let response = call(&state, "/v1/decisions", openai_body)
        .await
        .unwrap_or_else(|error| panic!("OpenAI request failed: {}", error.0));
    assert_eq!(response.status(), 200);

    assert_eq!(
        recorded(&state),
        once([
            sample("/v1/systemone", "system_one", "vertex", 200, "success"),
            sample("/v1/decisions", "openai", "vertex", 200, "success"),
        ])
    );
}

#[tokio::test]
async fn no_compatible_route_records_unavailable_provider() {
    let (_directory, state) = setup(Upstream::NoDecisions).await;
    let error = call(&state, "/v1/decisions", system_one_body())
        .await
        .unwrap_err();
    assert!(
        error.0.to_string().contains("no configured route supports"),
        "{}",
        error.0
    );

    assert_eq!(
        recorded(&state),
        once([sample(
            "/v1/decisions",
            "system_one",
            "unavailable",
            i64::from(error.0.http_status_code()),
            "invalid_request_error",
        )])
    );
}

#[tokio::test]
async fn provider_failure_after_billable_usage_records_upstream_error() {
    let (_directory, state) = setup(Upstream::PartialUsage).await;
    let error = call(&state, "/v1/systemone", system_one_body())
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 502);

    assert_eq!(
        recorded(&state),
        once([sample(
            "/v1/systemone",
            "system_one",
            "vertex",
            502,
            "upstream_error"
        )])
    );
}

#[tokio::test]
async fn budget_rejection_records_budget_error() {
    let (_directory, state) = setup(Upstream::Answers).await;
    let auth = state.service.authenticate(Some(API_KEY)).await.unwrap();
    state
        .store
        .upsert_active_budget(
            &BudgetScope::ServiceAccount {
                service_account_id: auth.owner_service_account_id.unwrap(),
            },
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

    let error = call(&state, "/v1/decisions", system_one_body())
        .await
        .unwrap_err();
    assert_eq!(error.0.error_type(), "budget_error");

    assert_eq!(
        recorded(&state),
        once([sample(
            "/v1/decisions",
            "system_one",
            "vertex",
            i64::from(error.0.http_status_code()),
            "budget_error",
        )])
    );
}
