//! Embeddings requests record one `gateway.embeddings.requests` sample on
//! every exit after model resolution, and never a chat request sample.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use axum::{Extension, Json, extract::State, http::HeaderMap, response::Response};
use gateway_core::{
    BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings, CoreChatRequest,
    CoreEmbeddingsRequest, CoreResponsesRequest, Money4, ProviderCapabilities, ProviderClient,
    ProviderError, ProviderRequestContext, ProviderStream,
};
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{AppError, InferenceAuth, v1_embeddings};
use crate::{
    http::{
        state::AppState,
        test_support::{app_state, seed_stream_cancellation_test},
    },
    observability::TestEmbeddingsRequest,
};

const API_KEY: &str = "Bearer gwk_streamtest.cancel-secret";

#[derive(Clone, Copy)]
enum Upstream {
    Vectors,
    /// Fails after billable usage, like a truncated upstream body.
    PartialUsage,
    /// Registered without the embeddings capability, so no route is eligible.
    NoEmbeddings,
}

struct EmbeddingsProvider(Upstream);

#[async_trait]
impl ProviderClient for EmbeddingsProvider {
    fn provider_key(&self) -> &str {
        "vertex"
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            embeddings: !matches!(self.0, Upstream::NoEmbeddings),
            ..ProviderCapabilities::all_enabled()
        }
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        match self.0 {
            Upstream::Vectors => Ok(json!({
                "object": "list",
                "model": "fast-upstream",
                "data": [{"object": "embedding", "index": 0, "embedding": [0.1, 0.2]}],
                "usage": {"prompt_tokens": 2, "total_tokens": 2},
            })),
            Upstream::PartialUsage => Err(ProviderError::PartialUsage {
                source: Box::new(ProviderError::Transport("body truncated".to_string())),
                provider_usage: Some(json!({"prompt_tokens": 2, "total_tokens": 2})),
            }),
            Upstream::NoEmbeddings => panic!("a route without embeddings support was selected"),
        }
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("embeddings requests must not dispatch chat completions")
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("embeddings requests must not dispatch chat streams")
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("embeddings requests must not dispatch responses")
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("embeddings requests must not dispatch response streams")
    }
}

async fn setup(upstream: Upstream) -> (tempfile::TempDir, AppState) {
    let (directory, mut state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    state
        .providers
        .register(Arc::new(EmbeddingsProvider(upstream)));
    (directory, state)
}

async fn call(state: &AppState) -> Result<Response, AppError> {
    let request_id = Some(Extension(RequestId::new(
        "embeddings-test".parse().unwrap(),
    )));
    let auth = InferenceAuth(state.service.authenticate(Some(API_KEY)).await.unwrap());
    let request = serde_json::from_value(json!({"model": "fast", "input": "hello"})).unwrap();
    v1_embeddings(
        State(state.clone()),
        request_id,
        HeaderMap::new(),
        Default::default(),
        auth,
        Json(request),
    )
    .await
}

fn recorded(state: &AppState) -> BTreeMap<TestEmbeddingsRequest, u64> {
    let snapshot = state.metrics.test_snapshot();
    assert_eq!(
        snapshot.requests, 0,
        "embeddings must not record chat requests"
    );
    assert!(snapshot.decisions_requests.is_empty());
    snapshot.embeddings_requests
}

/// Each sample recorded exactly once.
fn once<const N: usize>(
    samples: [TestEmbeddingsRequest; N],
) -> BTreeMap<TestEmbeddingsRequest, u64> {
    samples.into_iter().map(|sample| (sample, 1)).collect()
}

fn sample(provider_key: &str, status_code: i64, outcome: &str) -> TestEmbeddingsRequest {
    TestEmbeddingsRequest {
        provider_key: provider_key.to_string(),
        status_code,
        outcome: outcome.to_string(),
    }
}

#[tokio::test]
async fn success_records_provider() {
    let (_directory, state) = setup(Upstream::Vectors).await;
    let response = call(&state)
        .await
        .unwrap_or_else(|error| panic!("embeddings request failed: {}", error.0));
    assert_eq!(response.status(), 200);

    assert_eq!(recorded(&state), once([sample("vertex", 200, "success")]));
}

#[tokio::test]
async fn no_compatible_route_records_unavailable_provider() {
    let (_directory, state) = setup(Upstream::NoEmbeddings).await;
    let error = call(&state).await.unwrap_err();
    assert!(
        error.0.to_string().contains("no configured route supports"),
        "{}",
        error.0
    );

    assert_eq!(
        recorded(&state),
        once([sample(
            "unavailable",
            i64::from(error.0.http_status_code()),
            "invalid_request_error",
        )])
    );
}

#[tokio::test]
async fn provider_failure_after_billable_usage_records_upstream_error() {
    let (_directory, state) = setup(Upstream::PartialUsage).await;
    let error = call(&state).await.unwrap_err();
    assert_eq!(error.0.http_status_code(), 502);

    assert_eq!(
        recorded(&state),
        once([sample("vertex", 502, "upstream_error")])
    );
}

#[tokio::test]
async fn budget_rejection_records_budget_error() {
    let (_directory, state) = setup(Upstream::Vectors).await;
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

    let error = call(&state).await.unwrap_err();
    assert_eq!(error.0.error_type(), "budget_error");

    assert_eq!(
        recorded(&state),
        once([sample(
            "vertex",
            i64::from(error.0.http_status_code()),
            "budget_error",
        )])
    );
}
