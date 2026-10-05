use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{Extension, Json, body::to_bytes, extract::State, http::HeaderMap};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey,
    BudgetRepository, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest,
    GitHubCopilotChatApi, GitHubCopilotRouteCompatibility, GlobalRole, ModelRoutingPolicy,
    NewApiKeyRecord, ProviderCapabilities, ProviderClient, ProviderError, ProviderFailoverPolicy,
    ProviderRequestContext, ProviderStream, ProviderUserCredentialRepository, RequestAttemptStatus,
    RequestLogQuery, RouteCompatibility, RoutingStrategy, SeedModel, SeedModelRoute, SeedProvider,
    SessionAffinityPolicy, UpsertProviderUserCredentialRecord, UserStatus,
};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use tower_http::request_id::RequestId;
use uuid::Uuid;

use super::InferenceAuth;
use crate::http::{state::AppState, test_support::app_state};

#[derive(Debug, PartialEq, Eq)]
struct CredentialCall {
    provider: &'static str,
    request_id: String,
    owner_user_id: Option<Uuid>,
    credential_id: Option<Uuid>,
}

struct CredentialProvider {
    key: &'static str,
    provider_type: &'static str,
    exhausted_credential_id: Uuid,
    calls: Arc<Mutex<Vec<CredentialCall>>>,
}

#[async_trait]
impl ProviderClient for CredentialProvider {
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
        self.calls.lock().unwrap().push(CredentialCall {
            provider: self.key,
            request_id: context.request_id.clone(),
            owner_user_id: context.owner_user_id,
            credential_id: context.expected_provider_credential_id,
        });
        if self.provider_type == "github_copilot"
            && context.expected_provider_credential_id == Some(self.exhausted_credential_id)
        {
            return Err(ProviderError::UpstreamHttp {
                status: 402,
                body: r#"{"error":{"code":"quota_exceeded"}}"#.to_string(),
                retry_after: None,
            });
        }
        Ok(json!({
            "id": format!("chatcmpl_{}", context.request_id),
            "model": context.upstream_model,
            "choices": [{"message": {"role": "assistant", "content": self.key}}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7},
        }))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("this test does not stream chat completions")
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("this test does not dispatch Responses requests")
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        panic!("this test does not stream Responses requests")
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        panic!("this test does not dispatch embeddings")
    }
}

async fn linked_caller(state: &AppState, name: &str) -> (AuthenticatedApiKey, Uuid) {
    let email = format!("{name}@example.test");
    let user = state
        .store
        .create_identity_user(
            name,
            &email,
            &email,
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .unwrap();
    state
        .store
        .create_api_key(&NewApiKeyRecord {
            name: name.to_string(),
            public_id: name.to_string(),
            secret_hash: gateway_core::hash_gateway_key_secret("test-secret").unwrap(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            created_at: gateway_service::offset_now(),
        })
        .await
        .unwrap();
    state
        .store
        .upsert_provider_user_credential(&UpsertProviderUserCredentialRecord {
            provider_key: "copilot".to_string(),
            user_id: user.user_id,
            secret_ciphertext: "mock-ciphertext".to_string(),
            secret_nonce: "mock-nonce".to_string(),
            secret_key_id: "mock-key".to_string(),
            updated_at: gateway_service::offset_now(),
        })
        .await
        .unwrap();
    let credential = state
        .store
        .get_provider_user_credential("copilot", user.user_id)
        .await
        .unwrap()
        .unwrap();
    let auth = state
        .service
        .authenticate(Some(&format!("Bearer gwk_{name}.test-secret")))
        .await
        .unwrap();
    (auth, credential.credential_id)
}

async fn seed_routes(state: &AppState) {
    let providers =
        [("copilot", "github_copilot"), ("paid", "openai_compat")].map(|(key, provider_type)| {
            SeedProvider {
                provider_key: key.to_string(),
                provider_type: provider_type.to_string(),
                config: json!({}),
                secrets: None,
            }
        });
    let routes = ["copilot", "paid"]
        .into_iter()
        .enumerate()
        .map(|(priority, provider_key)| SeedModelRoute {
            route_key: Some(provider_key.to_string()),
            provider_key: provider_key.to_string(),
            upstream_model: format!("{provider_key}-upstream"),
            priority: i32::try_from(priority).unwrap(),
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities: ProviderCapabilities::all_enabled(),
            compatibility: RouteCompatibility {
                github_copilot: (provider_key == "copilot").then_some(
                    GitHubCopilotRouteCompatibility {
                        chat_api: Some(GitHubCopilotChatApi::ChatCompletions),
                        supports_responses: false,
                        supports_embeddings: false,
                        upstream_supports: Default::default(),
                    },
                ),
                ..Default::default()
            },
        })
        .collect();
    let model = SeedModel {
        model_key: "fast".to_string(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: Vec::new(),
        rank: 0,
        routing: Some(ModelRoutingPolicy {
            strategy: RoutingStrategy::Preferred,
            affinity: Some(SessionAffinityPolicy::default()),
            failover: Some(ProviderFailoverPolicy {
                initial_backoff_ms: 1,
                max_backoff_ms: 1,
                quota_cooldown_seconds: 1,
                transient_cooldown_seconds: 1,
                max_cooldown_seconds: 1,
                ..Default::default()
            }),
        }),
        routes,
        allowlist: None,
    };
    state
        .store
        .seed_from_inputs(&providers, &[model], &[], &[], &[], &[], &[], &[])
        .await
        .unwrap();
}

async fn chat(state: &AppState, auth: &AuthenticatedApiKey, request_id: &str) -> Value {
    let mut headers = HeaderMap::new();
    headers.insert("x-oceans-session-id", "shared-session".parse().unwrap());
    let request = serde_json::from_value(json!({
        "model": "fast", "messages": [{"role": "user", "content": "hello"}],
    }))
    .unwrap();
    let response = super::v1_chat_completions(
        State(state.clone()),
        Some(Extension(RequestId::new(request_id.parse().unwrap()))),
        headers,
        InferenceAuth(auth.clone()),
        Json(request),
    )
    .await
    .unwrap_or_else(|error| panic!("request failed: {}", error.0));
    assert_eq!(response.status(), 200);
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn assert_request_result(
    state: &AppState,
    caller: &AuthenticatedApiKey,
    request_id: &str,
    expected_providers: &[&str],
) {
    let page = state
        .service
        .list_request_logs(&RequestLogQuery {
            page: 1,
            page_size: 10,
            request_id: Some(request_id.to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let detail = state
        .service
        .get_request_log_detail(page.items[0].request_log_id)
        .await
        .unwrap();
    assert_eq!(detail.attempts.len(), expected_providers.len());
    for (index, (attempt, provider)) in detail.attempts.iter().zip(expected_providers).enumerate() {
        let terminal = index + 1 == expected_providers.len();
        assert_eq!(attempt.provider_key, *provider);
        assert_eq!(attempt.attempt_number, i64::try_from(index + 1).unwrap());
        assert_eq!(attempt.terminal, terminal);
        assert_eq!(attempt.produced_final_response, terminal);
        assert_eq!(
            attempt.status,
            if terminal {
                RequestAttemptStatus::Success
            } else {
                RequestAttemptStatus::ProviderError
            }
        );
    }
    let final_provider = *expected_providers.last().unwrap();
    assert_eq!(detail.log.provider_key, final_provider);
    let scope = format!("user:{}", caller.owner_user_id.unwrap());
    let ledgers = state
        .store
        .get_usage_ledgers_by_request_ids_and_scope(&[request_id.to_string()], &scope)
        .await
        .unwrap();
    assert_eq!(ledgers.len(), 1);
    assert_eq!(ledgers[0].provider_key, final_provider);
    assert_eq!(ledgers[0].total_tokens, Some(7));
    assert_eq!(
        ledgers[0].model_route_id,
        Some(detail.attempts.last().unwrap().route_id)
    );
}

#[tokio::test]
async fn copilot_quota_keeps_fallback_affinity_and_isolates_other_user_credentials() {
    let (_directory, mut state) = app_state().await;
    seed_routes(&state).await;
    let (caller_a, credential_a) = linked_caller(&state, "caller-a").await;
    let (caller_b, credential_b) = linked_caller(&state, "caller-b").await;
    assert_ne!(credential_a, credential_b);
    state.copilot_user_provider_keys = Arc::new(vec!["copilot".to_string()]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    for (key, provider_type) in [("copilot", "github_copilot"), ("paid", "openai_compat")] {
        state.providers.register_with_routing_identity(
            Arc::new(CredentialProvider {
                key,
                provider_type,
                exhausted_credential_id: credential_a,
                calls: calls.clone(),
            }),
            format!("credential-test-{key}-v1"),
        );
    }

    for (caller, request_id, expected) in [
        (&caller_a, "caller-a-fallback", "paid"),
        (&caller_b, "caller-b-healthy", "copilot"),
    ] {
        let body = chat(&state, caller, request_id).await;
        assert_eq!(body["choices"][0]["message"]["content"], expected);
    }

    // Copilot is eligible again, so only the saved session binding keeps the paid route.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let followup = chat(&state, &caller_a, "caller-a-followup").await;
    assert_eq!(followup["choices"][0]["message"]["content"], "paid");

    let expected = [
        (
            "copilot",
            "caller-a-fallback",
            &caller_a,
            Some(credential_a),
        ),
        ("paid", "caller-a-fallback", &caller_a, None),
        ("copilot", "caller-b-healthy", &caller_b, Some(credential_b)),
        ("paid", "caller-a-followup", &caller_a, None),
    ]
    .map(
        |(provider, request_id, caller, credential_id)| CredentialCall {
            provider,
            request_id: request_id.to_string(),
            owner_user_id: caller.owner_user_id,
            credential_id,
        },
    );
    assert_eq!(*calls.lock().unwrap(), expected);
    assert_request_result(&state, &caller_a, "caller-a-fallback", &["copilot", "paid"]).await;
    assert_request_result(&state, &caller_a, "caller-a-followup", &["paid"]).await;
    assert_request_result(&state, &caller_b, "caller-b-healthy", &["copilot"]).await;
    assert_eq!(state.metrics.test_snapshot().requests, 3);
}
