//! Router-level tests for Messages token counting and the unversioned inference aliases.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use gateway_core::{
    BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings, CoreChatRequest,
    CoreEmbeddingsRequest, CoreResponsesRequest, Money4, ProviderCapabilities, ProviderClient,
    ProviderError, ProviderRequestContext, ProviderStream, RequestLogQuery,
};
use gateway_guardrails::{
    FailureDisposition, GuardPhase, GuardrailEngine, ManagedCheckConfig, ManagedCheckKind,
    ManagedEvaluator, ManagedOutcome, ManagedService, PolicyMode, ReasonCode,
    test_utils::StubManagedEvaluator,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::http::{
    build_router,
    state::AppState,
    test_support::{app_state, seed_stream_cancellation_test},
};

const BEARER: &str = "Bearer gwk_streamtest.cancel-secret";

struct RecordingProvider {
    counts_tokens: bool,
    calls: Mutex<Vec<&'static str>>,
}

impl RecordingProvider {
    fn record(&self, operation: &'static str) {
        self.calls.lock().unwrap().push(operation);
    }
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
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.record("chat");
        Ok(json!({
            "model": "fast-upstream",
            "choices": [{"message": {"role": "assistant", "content": "ok"}}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3},
        }))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("route tests do not stream")
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.record("responses");
        Ok(json!({
            "model": "fast-upstream", "output": [],
            "usage": {"input_tokens": 2, "output_tokens": 1, "total_tokens": 3},
        }))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("route tests do not stream")
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.record("embeddings");
        Ok(json!({
            "object": "list",
            "model": "fast-upstream",
            "data": [{"object": "embedding", "index": 0, "embedding": [0.5]}],
            "usage": {"prompt_tokens": 1, "total_tokens": 1},
        }))
    }

    async fn count_tokens(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.record("count_tokens");
        if !self.counts_tokens {
            return Err(ProviderError::NotImplemented(
                "openai_compat does not support token counting".to_string(),
            ));
        }
        assert!(!request.stream, "a count never streams");
        for dropped in [
            "temperature",
            "store",
            "stream_options",
            "max_completion_tokens",
        ] {
            assert!(!request.extra.contains_key(dropped), "{dropped} is dropped");
        }
        assert_eq!(
            request.extra["max_tokens"], 4096,
            "caller limit is replaced"
        );
        assert_eq!(context.upstream_model, "fast-upstream");
        Ok(json!({"input_tokens": 7}))
    }
}

async fn setup(
    counts_tokens: bool,
) -> (tempfile::TempDir, AppState, Arc<RecordingProvider>, Router) {
    setup_with(counts_tokens, |_| {}).await
}

async fn setup_with(
    counts_tokens: bool,
    configure: impl FnOnce(&mut AppState),
) -> (tempfile::TempDir, AppState, Arc<RecordingProvider>, Router) {
    let (directory, mut state) = app_state().await;
    configure(&mut state);
    seed_stream_cancellation_test(&state.store).await;
    let provider = Arc::new(RecordingProvider {
        counts_tokens,
        calls: Mutex::default(),
    });
    state.providers.register(provider.clone());
    let router = build_router(state.clone(), admin_ui::AdminUiConfig::default());
    (directory, state, provider, router)
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    auth: (&str, &str),
    body: Option<Value>,
) -> (StatusCode, Option<String>, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header(auth.0, auth.1)
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        request_id,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn count_body() -> Value {
    json!({
        "model": "fast",
        "max_tokens": 1024,
        "max_completion_tokens": 2048,
        "temperature": 0.5,
        "store": true,
        "stream_options": {"include_obfuscation": true},
        "stream": true,
        "messages": [{"role": "user", "content": "hello"}],
    })
}

async fn request_log_total(state: &AppState) -> u64 {
    state
        .service
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 10,
            ..Default::default()
        })
        .await
        .unwrap()
        .total
}

#[tokio::test]
async fn count_tokens_returns_upstream_count_without_accounting() {
    let (_directory, state, provider, router) = setup(true).await;

    for (path, auth) in [
        ("/v1/messages/count_tokens", ("authorization", BEARER)),
        (
            "/messages/count_tokens",
            ("x-api-key", "gwk_streamtest.cancel-secret"),
        ),
    ] {
        let (status, request_id, body) =
            send(&router, "POST", path, auth, Some(count_body())).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body, json!({"input_tokens": 7}), "{path}");
        let request_id = request_id.expect("x-request-id");
        let auth = state.service.authenticate(Some(BEARER)).await.unwrap();
        let scope = format!("service_account:{}", auth.owner_service_account_id.unwrap());
        assert!(
            state
                .store
                .get_usage_ledger_by_request_and_scope(&request_id, &scope)
                .await
                .unwrap()
                .is_none(),
            "{path} must not write a ledger row"
        );
    }

    assert_eq!(*provider.calls.lock().unwrap(), ["count_tokens"; 2]);
    assert_eq!(request_log_total(&state).await, 0);
    assert_eq!(state.metrics.test_snapshot().requests, 0);
}

#[tokio::test]
async fn count_tokens_is_not_blocked_by_an_exhausted_budget() {
    let (_directory, state, provider, router) = setup(true).await;
    let auth = state.service.authenticate(Some(BEARER)).await.unwrap();
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

    let (status, _, body) = send(
        &router,
        "POST",
        "/v1/messages/count_tokens",
        ("authorization", BEARER),
        Some(count_body()),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(*provider.calls.lock().unwrap(), ["count_tokens"]);
}

/// A prompt guard in deny mode that rejects every prompt.
fn deny_prompts(state: &mut AppState) {
    let config = Arc::make_mut(&mut state.guardrail_config);
    config.default.enabled = true;
    config.default.mode = PolicyMode::Deny;
    config.default.managed_checks = vec!["deny".into()];
    config.managed_checks.insert(
        "deny".into(),
        ManagedCheckConfig {
            kind: ManagedCheckKind::GoogleModelArmor,
            phases: BTreeSet::from([GuardPhase::Prompt]),
            timeout_ms: 1_000,
            failure_disposition: FailureDisposition::FailClosed,
            max_content_bytes: 4_096,
            bedrock: None,
            model_armor: None,
        },
    );
    state.guardrail_engine = Arc::new(GuardrailEngine::new(
        Vec::new(),
        BTreeMap::from([(
            "deny".into(),
            Arc::new(StubManagedEvaluator::new(
                "deny",
                ManagedService::GoogleModelArmor,
                [Ok(ManagedOutcome::Intervention {
                    reason_code: ReasonCode::new("test.denied").unwrap(),
                    metadata: Default::default(),
                })],
            )) as Arc<dyn ManagedEvaluator>,
        )]),
    ));
}

#[tokio::test]
async fn count_tokens_runs_the_prompt_guard_before_the_provider() {
    let (_directory, _state, provider, router) = setup_with(true, deny_prompts).await;

    let (status, _, body) = send(
        &router,
        "POST",
        "/v1/messages/count_tokens",
        ("authorization", BEARER),
        Some(count_body()),
    )
    .await;

    assert!(status.is_client_error(), "{status}: {body}");
    assert_eq!(body["type"], "error");
    assert!(
        provider.calls.lock().unwrap().is_empty(),
        "a denied prompt must not reach the provider"
    );
}

#[tokio::test]
async fn count_tokens_without_upstream_api_returns_typed_501() {
    let (_directory, state, _provider, router) = setup(false).await;

    let (status, _, body) = send(
        &router,
        "POST",
        "/v1/messages/count_tokens",
        ("authorization", BEARER),
        Some(count_body()),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "not_implemented_error");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("`vertex`") && message.contains("fast-upstream"),
        "{message}"
    );
    assert_eq!(request_log_total(&state).await, 0);
}

#[tokio::test]
async fn count_tokens_rejects_missing_credentials_with_anthropic_envelope() {
    let (_directory, _state, provider, router) = setup(true).await;

    let (status, _, body) = send(
        &router,
        "POST",
        "/messages/count_tokens",
        ("x-api-key", "gwk_streamtest.wrong-secret"),
        Some(count_body()),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["type"], "error");
    assert!(provider.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn count_tokens_rejects_malformed_bodies_with_anthropic_envelope() {
    let (_directory, _state, provider, router) = setup(true).await;

    for body in [
        Body::from("{not json"),
        Body::from(json!({"messages": []}).to_string()),
        Body::from(json!({"model": 7, "messages": []}).to_string()),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages/count_tokens")
            .header("content-type", "application/json")
            .header("authorization", BEARER)
            .body(body)
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).expect("JSON error body");

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "invalid_request_error");
    }
    assert!(provider.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unversioned_aliases_share_the_versioned_handlers() {
    let (_directory, _state, provider, router) = setup(true).await;
    let chat = json!({"model": "fast", "messages": [{"role": "user", "content": "hi"}]});
    let responses = json!({"model": "fast", "input": "hi"});
    let embeddings = json!({"model": "fast", "input": "hi"});
    let decisions = json!({"model": "fast", "state": {}});

    for (method, versioned, alias, body, operation) in [
        (
            "POST",
            "/v1/chat/completions",
            "/chat/completions",
            Some(chat),
            Some("chat"),
        ),
        (
            "POST",
            "/v1/responses",
            "/responses",
            Some(responses),
            Some("responses"),
        ),
        (
            "POST",
            "/v1/embeddings",
            "/embeddings",
            Some(embeddings),
            Some("embeddings"),
        ),
        ("POST", "/v1/decisions", "/decisions", Some(decisions), None),
        ("GET", "/v1/models", "/models", None, None),
    ] {
        provider.calls.lock().unwrap().clear();
        let (versioned_status, _, versioned_body) = send(
            &router,
            method,
            versioned,
            ("authorization", BEARER),
            body.clone(),
        )
        .await;
        let (alias_status, _, alias_body) =
            send(&router, method, alias, ("authorization", BEARER), body).await;

        assert_eq!(alias_status, versioned_status, "{alias}: {alias_body}");
        assert!(
            ![StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED].contains(&alias_status),
            "{alias} is not routed: {alias_status}"
        );
        if let Some(operation) = operation {
            assert_eq!(alias_status, StatusCode::OK, "{alias}: {alias_body}");
            assert_eq!(*provider.calls.lock().unwrap(), [operation; 2], "{alias}");
        }
        if alias == "/models" {
            assert_eq!(alias_status, StatusCode::OK);
            assert_eq!(alias_body, versioned_body);
        }
    }
}
