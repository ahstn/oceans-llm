//! Image generation requests log, account and record one
//! `gateway.images.requests` sample on every exit after model resolution, and
//! never a chat request sample.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{Extension, Json, body::to_bytes, extract::State, http::HeaderMap, response::Response};
use gateway_core::{
    BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings, CoreChatRequest,
    CoreEmbeddingsRequest, CoreResponsesRequest, ImagesRequest, Money4, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream, RequestAttemptStatus,
    RequestLogDetail, RequestLogQuery, RouteCompatibility, SeedModel, SeedModelRoute,
};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{AppError, InferenceAuth, v1_images};
use crate::{
    http::{
        state::AppState,
        test_support::{app_state, seed_stream_cancellation_test},
    },
    observability::TestImagesRequest,
};

const API_KEY: &str = "Bearer gwk_streamtest.cancel-secret";
const REQUEST_ID: &str = "images-test";

#[derive(Clone, Copy)]
enum Upstream {
    Image,
    /// Fails after billable usage, like a truncated upstream body.
    PartialUsage,
    /// Registered without the images capability, so no route is eligible.
    NoImages,
}

struct ImagesProvider {
    upstream: Upstream,
    /// Each forwarded request with the upstream model it was sent to.
    calls: Mutex<Vec<(ImagesRequest, String)>>,
}

#[async_trait]
impl ProviderClient for ImagesProvider {
    fn provider_key(&self) -> &str {
        "vertex"
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            images: !matches!(self.upstream, Upstream::NoImages),
            ..ProviderCapabilities::all_enabled()
        }
    }

    async fn images(
        &self,
        request: &ImagesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push((request.clone(), context.upstream_model.clone()));
        match self.upstream {
            Upstream::Image => Ok(json!({
                "created": 1,
                "data": [{"b64_json": "aGk=", "media_type": "image/png"}],
                "usage": {"prompt_tokens": 14, "completion_tokens": 229, "total_tokens": 243},
            })),
            Upstream::PartialUsage => Err(ProviderError::PartialUsage {
                source: Box::new(ProviderError::Transport("body truncated".to_string())),
                provider_usage: Some(
                    json!({"prompt_tokens": 14, "completion_tokens": 229, "total_tokens": 243}),
                ),
            }),
            Upstream::NoImages => panic!("a route without images support was selected"),
        }
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("images requests must not dispatch chat completions")
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("images requests must not dispatch chat streams")
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("images requests must not dispatch embeddings")
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("images requests must not dispatch responses")
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("images requests must not dispatch response streams")
    }
}

async fn setup(upstream: Upstream) -> (tempfile::TempDir, AppState, Arc<ImagesProvider>) {
    let (directory, mut state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    let provider = Arc::new(ImagesProvider {
        upstream,
        calls: Mutex::default(),
    });
    state.providers.register(provider.clone());
    (directory, state, provider)
}

async fn call(state: &AppState, body: Value) -> Result<Response, AppError> {
    let request_id = Some(Extension(RequestId::new(REQUEST_ID.parse().unwrap())));
    let auth = InferenceAuth(state.service.authenticate(Some(API_KEY)).await.unwrap());
    let request = serde_json::from_value(body).unwrap();
    v1_images(
        State(state.clone()),
        request_id,
        HeaderMap::new(),
        Default::default(),
        auth,
        Json(request),
    )
    .await
}

fn generate() -> Value {
    json!({"model": "fast", "prompt": "a lighthouse at dusk"})
}

async fn request_log_count(state: &AppState) -> u64 {
    state
        .service
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 10,
            request_id: Some(REQUEST_ID.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
        .total
}

async fn request_detail(state: &AppState) -> RequestLogDetail {
    let page = state
        .service
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 10,
            request_id: Some(REQUEST_ID.to_string()),
            ..Default::default()
        })
        .await
        .expect("request log persisted before the handler returns");
    assert_eq!(page.total, 1);
    state
        .service
        .get_request_log_detail(page.items[0].request_log_id)
        .await
        .unwrap()
}

async fn ledger_tokens(state: &AppState, detail: &RequestLogDetail) -> Option<[Option<i64>; 3]> {
    let scope = format!("service_account:{}", detail.log.service_account_id.unwrap());
    state
        .store
        .get_usage_ledger_by_request_and_scope(REQUEST_ID, &scope)
        .await
        .unwrap()
        .map(|ledger| {
            assert_eq!(ledger.upstream_model, "fast-upstream");
            [
                ledger.prompt_tokens,
                ledger.completion_tokens,
                ledger.total_tokens,
            ]
        })
}

fn recorded(state: &AppState) -> BTreeMap<TestImagesRequest, u64> {
    let snapshot = state.metrics.test_snapshot();
    assert_eq!(snapshot.requests, 0, "images must not record chat requests");
    assert!(snapshot.decisions_requests.is_empty());
    assert!(snapshot.embeddings_requests.is_empty());
    snapshot.images_requests
}

/// Each sample recorded exactly once.
fn once<const N: usize>(samples: [TestImagesRequest; N]) -> BTreeMap<TestImagesRequest, u64> {
    samples.into_iter().map(|sample| (sample, 1)).collect()
}

fn sample(provider_key: &str, status_code: i64, outcome: &str) -> TestImagesRequest {
    TestImagesRequest {
        provider_key: provider_key.to_string(),
        status_code,
        outcome: outcome.to_string(),
    }
}

#[tokio::test]
async fn success_logs_the_request_and_accounts_usage() {
    let (_directory, state, provider) = setup(Upstream::Image).await;
    let mut body = generate();
    body["aspect_ratio"] = json!("16:9");
    let response = call(&state, body)
        .await
        .unwrap_or_else(|error| panic!("images request failed: {}", error.0));
    assert_eq!(response.status(), 200);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["data"][0]["b64_json"], "aGk=");
    assert!(
        body.get("model").is_none(),
        "the Images API response has no model: {body}"
    );

    let calls = provider.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let (forwarded, upstream_model) = &calls[0];
    assert_eq!(upstream_model, "fast-upstream");
    assert_eq!(forwarded.prompt, "a lighthouse at dusk");
    assert_eq!(
        forwarded.extra["aspect_ratio"], "16:9",
        "untyped fields pass through"
    );

    let detail = request_detail(&state).await;
    assert_eq!(detail.log.status_code, Some(200));
    assert_eq!(detail.log.model_key, "fast");
    assert_eq!(detail.log.provider_key, "vertex");
    assert_eq!(detail.log.metadata["operation"], "images");
    assert_eq!(detail.log.metadata["stream"], false);
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(detail.attempts[0].status, RequestAttemptStatus::Success);
    assert_eq!(
        ledger_tokens(&state, &detail).await,
        Some([Some(14), Some(229), Some(243)]),
        "successful request records usage"
    );

    assert_eq!(recorded(&state), once([sample("vertex", 200, "success")]));
}

#[tokio::test]
async fn no_compatible_route_names_the_images_capability() {
    let (_directory, state, provider) = setup(Upstream::NoImages).await;
    let error = call(&state, generate()).await.unwrap_err();
    assert!(
        error
            .0
            .to_string()
            .contains("no configured route supports requested capabilities (images)"),
        "{}",
        error.0
    );
    assert!(provider.calls.lock().unwrap().is_empty());

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
async fn streaming_images_request_is_rejected_before_routing() {
    let (_directory, state, provider) = setup(Upstream::Image).await;
    let mut body = generate();
    body["stream"] = json!(true);
    let error = call(&state, body).await.unwrap_err();
    assert_eq!(error.0.http_status_code(), 400);
    assert_eq!(error.0.error_type(), "invalid_request_error");
    assert!(
        error.0.to_string().contains("does not support streaming"),
        "{}",
        error.0
    );

    assert!(provider.calls.lock().unwrap().is_empty());
    assert_eq!(request_log_count(&state).await, 0);
    assert!(recorded(&state).is_empty());
}

#[tokio::test]
async fn provider_failure_after_billable_usage_records_upstream_error() {
    let (_directory, state, provider) = setup(Upstream::PartialUsage).await;
    let error = call(&state, generate()).await.unwrap_err();
    assert_eq!(error.0.http_status_code(), 502);
    assert_eq!(provider.calls.lock().unwrap().len(), 1);

    let detail = request_detail(&state).await;
    assert_eq!(detail.log.status_code, Some(502));
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(
        detail.attempts[0].status,
        RequestAttemptStatus::ProviderError
    );
    assert_eq!(
        ledger_tokens(&state, &detail).await,
        Some([Some(14), Some(229), Some(243)]),
        "usage reported before the failure is charged"
    );

    assert_eq!(
        recorded(&state),
        once([sample("vertex", 502, "upstream_error")])
    );
}

#[tokio::test]
async fn budget_rejection_skips_the_provider_and_records_budget_error() {
    let (_directory, state, provider) = setup(Upstream::Image).await;
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

    let error = call(&state, generate()).await.unwrap_err();
    assert_eq!(error.0.error_type(), "budget_error");
    assert!(provider.calls.lock().unwrap().is_empty());

    let detail = request_detail(&state).await;
    assert_eq!(detail.log.error_code.as_deref(), Some("budget_exceeded"));
    assert!(detail.attempts.is_empty());
    assert_eq!(ledger_tokens(&state, &detail).await, None);

    assert_eq!(
        recorded(&state),
        once([sample(
            "vertex",
            i64::from(error.0.http_status_code()),
            "budget_error",
        )])
    );
}

#[tokio::test]
async fn model_not_granted_to_the_key_is_forbidden() {
    let (_directory, state, provider) = setup(Upstream::Image).await;
    state
        .store
        .seed_from_inputs(
            &[],
            &[SeedModel {
                model_key: "locked".to_string(),
                alias_target_model_key: None,
                max_reasoning_effort: None,
                routing: None,
                description: None,
                tags: Vec::new(),
                rank: 0,
                routes: vec![SeedModelRoute {
                    route_key: None,
                    provider_key: "vertex".to_string(),
                    upstream_model: "locked-upstream".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    context_window_tokens: None,
                    pricing_override: None,
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: RouteCompatibility::default(),
                }],
                allowlist: None,
            }],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .await
        .unwrap();

    let error = call(&state, json!({"model": "locked", "prompt": "hello"}))
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 403);
    assert_eq!(error.0.error_code(), "model_not_granted");
    assert!(provider.calls.lock().unwrap().is_empty());
    // Refused during model resolution, before any request metric or log.
    assert_eq!(request_log_count(&state).await, 0);
    assert!(recorded(&state).is_empty());
}
