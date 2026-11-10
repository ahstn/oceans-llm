use std::{
    collections::BTreeMap,
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
use gateway_core::{
    BudgetRepository, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ModelRoute,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    RequestLogDetail, RequestLogQuery, SERVICE_TIER_REQUEST_FIELDS, ServiceTier,
};
use serde_json::{Value, json};
use tower_http::request_id::RequestId;

use super::{AppError, messages::v1_messages_inner, v1_chat_completions, v1_responses};
use crate::http::{
    state::AppState,
    test_support::{app_state, seed_test_models, test_model},
};

const REQUEST_ID: &str = "tier-test";

#[derive(Clone, Copy, Debug)]
enum Endpoint {
    Chat,
    Responses,
    Messages,
}

impl Endpoint {
    /// A minimal request for `model` with `tier_fields` merged in.
    fn body(self, model: &str, stream: bool, tier_fields: Value) -> Value {
        let mut body = match self {
            Self::Chat => json!({
                "model": model, "stream": stream,
                "messages": [{"role": "user", "content": "hello"}],
            }),
            Self::Responses => json!({"model": model, "input": "hello", "stream": stream}),
            Self::Messages => json!({
                "model": model, "max_tokens": 16, "stream": stream,
                "messages": [{"role": "user", "content": "hello"}],
            }),
        };
        let Value::Object(tier_fields) = tier_fields else {
            panic!("tier fields must be an object");
        };
        body.as_object_mut().unwrap().extend(tier_fields);
        body
    }

    async fn call(
        self,
        state: &AppState,
        request_id: &str,
        body: Value,
    ) -> Result<Response, AppError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "Bearer gwk_streamtest.cancel-secret".parse().unwrap(),
        );
        let request_id = Some(Extension(RequestId::new(request_id.parse().unwrap())));
        let auth = state
            .service
            .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
            .await
            .unwrap();
        let state = state.clone();
        match self {
            Self::Chat => {
                v1_chat_completions(
                    State(state),
                    request_id,
                    headers,
                    Default::default(),
                    super::InferenceAuth(auth),
                    Json(serde_json::from_value(body).unwrap()),
                )
                .await
            }
            Self::Responses => {
                v1_responses(
                    State(state),
                    request_id,
                    headers,
                    Default::default(),
                    super::InferenceAuth(auth),
                    Json(serde_json::from_value(body).unwrap()),
                )
                .await
            }
            Self::Messages => {
                v1_messages_inner(
                    state,
                    request_id,
                    headers,
                    Default::default(),
                    auth,
                    serde_json::from_value(body).unwrap(),
                )
                .await
            }
        }
    }
}

type RecordedCall = (Option<ServiceTier>, BTreeMap<String, Value>);

/// An upstream that records the tier and extension fields of each call, and reports
/// `served` as the tier it served.
struct TierProvider {
    key: &'static str,
    tiers: &'static [ServiceTier],
    served: Option<&'static str>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl TierProvider {
    fn record(&self, context: &ProviderRequestContext, extra: &BTreeMap<String, Value>) {
        self.calls
            .lock()
            .unwrap()
            .push((context.service_tier, extra.clone()));
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    fn with_served_tier(&self, mut body: Value) -> Value {
        if let Some(served) = self.served {
            body["service_tier"] = json!(served);
        }
        body
    }
}

#[async_trait]
impl ProviderClient for TierProvider {
    fn provider_key(&self) -> &str {
        self.key
    }

    fn provider_type(&self) -> &str {
        "openai_compat"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all_enabled()
    }

    fn service_tiers(&self, _route: &ModelRoute) -> &'static [ServiceTier] {
        self.tiers
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.record(context, &request.extra);
        Ok(self.with_served_tier(json!({
            "id": "chatcmpl-tier", "model": context.upstream_model,
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3},
        })))
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        self.record(context, &request.extra);
        let chunks = [
            json!({"id": "chatcmpl-tier", "choices": [{"index": 0,
                "delta": {"role": "assistant", "content": "ok"}, "finish_reason": null}]}),
            json!({"id": "chatcmpl-tier", "choices": [{"index": 0, "delta": {},
                "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3}}),
        ];
        let mut sse: String = chunks
            .into_iter()
            .map(|chunk| format!("data: {}\n\n", self.with_served_tier(chunk)))
            .collect();
        sse.push_str("data: [DONE]\n\n");
        Ok(Box::pin(futures_util::stream::iter([Ok(Bytes::from(sse))])))
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.record(context, &request.extra);
        Ok(self.with_served_tier(json!({
            "id": "resp-tier", "object": "response", "model": context.upstream_model,
            "status": "completed", "output": [],
            "usage": {"input_tokens": 2, "output_tokens": 1, "total_tokens": 3},
        })))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("tier tests do not stream Responses")
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("tier tests do not request embeddings")
    }
}

/// Seeds `luna` (any tier), `luna-fast` (route forced to priority), `flex-only` (allows
/// only flex) on the `tiers` provider, and `plain` on a provider that sends only the
/// default tier. Both providers report `served`.
async fn setup(served: Option<&'static str>) -> (tempfile::TempDir, AppState, Arc<TierProvider>) {
    let (directory, mut state) = app_state().await;
    let mut forced = test_model("luna-fast", "tiers", "luna-upstream");
    forced.routes[0].service_tier = Some(ServiceTier::Priority);
    let mut flex_only = test_model("flex-only", "tiers", "luna-upstream");
    flex_only.service_tiers = Some(vec![ServiceTier::Flex]);
    seed_test_models(
        &state.store,
        &["tiers", "plain"],
        vec![
            test_model("luna", "tiers", "luna-upstream"),
            forced,
            flex_only,
            test_model("plain", "plain", "plain-upstream"),
        ],
    )
    .await;
    let provider = Arc::new(TierProvider {
        key: "tiers",
        tiers: &ServiceTier::ALL,
        served,
        calls: Mutex::default(),
    });
    state.providers.register(provider.clone());
    state.providers.register(Arc::new(TierProvider {
        key: "plain",
        tiers: &[ServiceTier::Default],
        served,
        calls: Mutex::default(),
    }));
    (directory, state, provider)
}

async fn request_detail(state: &AppState, request_id: &str) -> RequestLogDetail {
    let page = state
        .service
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 10,
            request_id: Some(request_id.to_string()),
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

/// The ledger's (requested, served) tiers for the logged request.
async fn ledger_tiers(
    state: &AppState,
    detail: &RequestLogDetail,
) -> (Option<ServiceTier>, Option<ServiceTier>) {
    let scope = format!("service_account:{}", detail.log.service_account_id.unwrap());
    let ledger = state
        .store
        .get_usage_ledger_by_request_and_scope(&detail.log.request_id, &scope)
        .await
        .unwrap()
        .expect("usage ledger row");
    (ledger.requested_service_tier, ledger.served_service_tier)
}

#[tokio::test]
async fn requested_tier_reaches_the_provider_context_but_not_the_upstream_body() {
    for (endpoint, tier_fields, expected) in [
        (
            Endpoint::Chat,
            json!({"service_tier": "fast"}),
            ServiceTier::Priority,
        ),
        (
            Endpoint::Responses,
            json!({"serviceTier": "flex"}),
            ServiceTier::Flex,
        ),
        (
            Endpoint::Messages,
            json!({"speed": "fast"}),
            ServiceTier::Priority,
        ),
    ] {
        let (_directory, state, provider) = setup(None).await;
        endpoint
            .call(
                &state,
                REQUEST_ID,
                endpoint.body("luna", false, tier_fields),
            )
            .await
            .unwrap_or_else(|error| panic!("{endpoint:?} failed: {}", error.0));

        let calls = provider.calls();
        assert_eq!(calls.len(), 1, "{endpoint:?}");
        let (tier, extra) = &calls[0];
        assert_eq!(*tier, Some(expected), "{endpoint:?}");
        assert!(
            SERVICE_TIER_REQUEST_FIELDS
                .iter()
                .all(|field| !extra.contains_key(*field)),
            "{endpoint:?} forwarded tier fields upstream: {extra:?}"
        );
        let detail = request_detail(&state, REQUEST_ID).await;
        assert_eq!(
            detail.log.metadata["service_tier"],
            expected.as_str(),
            "{endpoint:?}"
        );
    }
}

#[tokio::test]
async fn route_forced_tier_overrides_the_callers_tier() {
    for tier_fields in [json!({}), json!({"service_tier": "flex"})] {
        let (_directory, state, provider) = setup(Some("priority")).await;
        Endpoint::Chat
            .call(
                &state,
                REQUEST_ID,
                Endpoint::Chat.body("luna-fast", false, tier_fields.clone()),
            )
            .await
            .unwrap_or_else(|error| panic!("{tier_fields} failed: {}", error.0));

        assert_eq!(provider.calls()[0].0, Some(ServiceTier::Priority));
        let detail = request_detail(&state, REQUEST_ID).await;
        assert_eq!(detail.log.metadata["service_tier"], "priority");
        assert_eq!(
            ledger_tiers(&state, &detail).await,
            (Some(ServiceTier::Priority), Some(ServiceTier::Priority))
        );
    }
}

#[tokio::test]
async fn model_allow_list_rejects_other_tiers() {
    let (_directory, state, provider) = setup(None).await;
    let error = Endpoint::Chat
        .call(
            &state,
            "tier-denied",
            Endpoint::Chat.body("flex-only", false, json!({"service_tier": "priority"})),
        )
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 403);
    assert_eq!(error.0.error_code(), "service_tier_not_allowed");
    assert!(provider.calls().is_empty());

    Endpoint::Chat
        .call(
            &state,
            "tier-allowed",
            Endpoint::Chat.body("flex-only", false, json!({"service_tier": "flex"})),
        )
        .await
        .unwrap_or_else(|error| panic!("flex failed: {}", error.0));
    assert_eq!(provider.calls()[0].0, Some(ServiceTier::Flex));
}

#[tokio::test]
async fn rejects_a_tier_that_no_route_can_serve() {
    let (_directory, state, _provider) = setup(None).await;
    let error = Endpoint::Chat
        .call(
            &state,
            "tier-unserved",
            Endpoint::Chat.body("plain", false, json!({"service_tier": "flex"})),
        )
        .await
        .unwrap_err();
    assert_eq!(error.0.http_status_code(), 400);
    assert_eq!(
        error.0.to_string(),
        "invalid request: no configured route for model `plain` supports service tier `flex`"
    );

    Endpoint::Chat
        .call(
            &state,
            "tier-default",
            Endpoint::Chat.body("plain", false, json!({"service_tier": "default"})),
        )
        .await
        .unwrap_or_else(|error| panic!("default tier failed: {}", error.0));
}

#[tokio::test]
async fn rejects_unknown_and_conflicting_tiers() {
    let (_directory, state, provider) = setup(None).await;
    for tier_fields in [
        json!({"service_tier": "turbo"}),
        json!({"service_tier": "flex", "speed": "fast"}),
    ] {
        let error = Endpoint::Chat
            .call(
                &state,
                REQUEST_ID,
                Endpoint::Chat.body("luna", false, tier_fields.clone()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.0.http_status_code(), 400, "{tier_fields}");
        assert_eq!(error.0.error_code(), "invalid_request", "{tier_fields}");
    }
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn records_the_requested_and_served_tiers() {
    for (endpoint, stream) in [
        (Endpoint::Chat, false),
        (Endpoint::Chat, true),
        (Endpoint::Responses, false),
        (Endpoint::Messages, false),
        (Endpoint::Messages, true),
    ] {
        let (_directory, state, _provider) = setup(Some("default")).await;
        let response = endpoint
            .call(
                &state,
                REQUEST_ID,
                endpoint.body("luna", stream, json!({"service_tier": "priority"})),
            )
            .await
            .unwrap_or_else(|error| panic!("{endpoint:?} failed: {}", error.0));
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        if matches!(endpoint, Endpoint::Messages) {
            // Messages callers read the served tier in Anthropic terms.
            let body = String::from_utf8(body.to_vec()).unwrap();
            assert!(
                body.contains(r#""service_tier":"standard""#),
                "stream={stream}: {body}"
            );
        }

        let detail = request_detail(&state, REQUEST_ID).await;
        let case = format!("{endpoint:?} stream={stream}");
        assert_eq!(detail.log.metadata["service_tier"], "priority", "{case}");
        assert_eq!(
            detail.log.metadata["served_service_tier"], "default",
            "{case}"
        );
        assert_eq!(
            ledger_tiers(&state, &detail).await,
            (Some(ServiceTier::Priority), Some(ServiceTier::Default)),
            "{case}"
        );
    }
}
