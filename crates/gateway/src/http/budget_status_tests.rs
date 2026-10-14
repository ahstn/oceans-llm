//! Budget rejections use the configured status on every route that enforces a
//! budget, in both the OpenAI and the Anthropic error envelope.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use gateway_core::{
    BatchCapabilities, BudgetCadence, BudgetExceededStatus, BudgetRepository, BudgetScope,
    BudgetSettings, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, Money4,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
};
use gateway_service::{GatewayService, WeightedRoutePlanner};
use http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::http::test_support::{app_state, seed_stream_cancellation_test};

const API_KEY: &str = "gwk_streamtest.cancel-secret";

/// Registered so routes resolve; budget enforcement must stop every request
/// before it gets here.
struct UnreachableProvider;

#[async_trait]
impl ProviderClient for UnreachableProvider {
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
        BatchCapabilities {
            chat_completions: true,
            ..BatchCapabilities::NONE
        }
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("over-budget chat request reached the provider")
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("over-budget chat stream reached the provider")
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("over-budget embeddings request reached the provider")
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("over-budget responses request reached the provider")
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("over-budget responses stream reached the provider")
    }
}

struct Case {
    path: &'static str,
    body: Value,
    anthropic_envelope: bool,
}

fn cases() -> Vec<Case> {
    let chat_messages = json!([{"role": "user", "content": "hello"}]);
    vec![
        Case {
            path: "/v1/chat/completions",
            body: json!({"model": "fast", "messages": chat_messages}),
            anthropic_envelope: false,
        },
        Case {
            path: "/v1/responses",
            body: json!({"model": "fast", "input": "hello"}),
            anthropic_envelope: false,
        },
        Case {
            path: "/v1/embeddings",
            body: json!({"model": "fast", "input": "hello"}),
            anthropic_envelope: false,
        },
        Case {
            path: "/v1/decisions",
            body: json!({
                "model": "fast",
                "state": {"ticket": "refund request"},
                "questions": {
                    "approve": {"type": "noul", "instructions": "Should this be approved?"}
                },
            }),
            anthropic_envelope: false,
        },
        Case {
            path: "/v1/messages",
            body: json!({"model": "fast", "max_tokens": 16, "messages": chat_messages}),
            anthropic_envelope: true,
        },
        Case {
            path: "/api/v1/batches",
            body: json!({
                "endpoint": "chat_completions",
                "model": "fast",
                "items": [{"custom_id": "one", "body": {"messages": chat_messages}}],
            }),
            anthropic_envelope: false,
        },
    ]
}

async fn over_budget_router(
    status: Option<BudgetExceededStatus>,
) -> (tempfile::TempDir, axum::Router) {
    let (directory, mut state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    state.providers.register(Arc::new(UnreachableProvider));
    let service = GatewayService::new(
        state.store.clone(),
        Arc::new(WeightedRoutePlanner::default()),
    );
    state.service = Arc::new(match status {
        Some(status) => service.with_budget_exceeded_status(status),
        None => service,
    });

    let auth = state
        .service
        .authenticate(Some(&format!("Bearer {API_KEY}")))
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
                amount_usd: Money4::ZERO,
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            gateway_service::offset_now(),
        )
        .await
        .unwrap();
    let router = crate::http::build_router(state, admin_ui::AdminUiConfig::default());
    (directory, router)
}

async fn assert_every_route_returns(status: Option<BudgetExceededStatus>, expected: StatusCode) {
    let (_directory, router) = over_budget_router(status).await;
    for (index, case) in cases().into_iter().enumerate() {
        let mut request = Request::builder()
            .method("POST")
            .uri(case.path)
            .header("content-type", "application/json")
            .header("x-request-id", format!("budget-status-{index}"))
            .header("idempotency-key", format!("budget-status-{index}"));
        request = if case.anthropic_envelope {
            request.header("x-api-key", API_KEY)
        } else {
            request.header("authorization", format!("Bearer {API_KEY}"))
        };
        let response = router
            .clone()
            .oneshot(request.body(Body::from(case.body.to_string())).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(status, expected, "{}: {body}", case.path);
        assert_eq!(body["error"]["type"], "budget_error", "{}", case.path);
        assert_eq!(body["error"]["code"], "budget_exceeded", "{}", case.path);
        if case.anthropic_envelope {
            assert_eq!(body["type"], "error", "{}", case.path);
        } else {
            assert!(body.get("type").is_none(), "{}: {body}", case.path);
        }
        let message = body["error"]["message"].as_str().unwrap();
        assert!(
            message.starts_with("daily service account budget exhausted"),
            "{}: {message}",
            case.path
        );
        assert!(!message.contains("budget:v1"), "{}: {message}", case.path);
    }
}

#[tokio::test]
async fn over_budget_requests_return_422_by_default() {
    assert_every_route_returns(None, StatusCode::UNPROCESSABLE_ENTITY).await;
}

#[tokio::test]
async fn over_budget_status_opt_out_restores_429() {
    assert_every_route_returns(
        Some(BudgetExceededStatus::TooManyRequests),
        StatusCode::TOO_MANY_REQUESTS,
    )
    .await;
}

#[tokio::test]
async fn over_budget_status_can_be_402() {
    assert_every_route_returns(
        Some(BudgetExceededStatus::PaymentRequired),
        StatusCode::PAYMENT_REQUIRED,
    )
    .await;
}
