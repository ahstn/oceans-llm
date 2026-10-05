use std::sync::Mutex;

use async_trait::async_trait;
use gateway_core::{
    ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, GatewayModel, ModelRoutingPolicy,
    ProviderCapabilities, RouteSelection, SessionAffinityPolicy, StoreError,
};
use serde_json::Map;
use uuid::Uuid;

use super::*;
use crate::{ResolvedModelSelection, ResolvedProviderConnection};

#[derive(Default)]
struct MockStore {
    state: Mutex<StoreState>,
}

#[derive(Default)]
struct StoreState {
    selections: Vec<RouteSelectionRequest>,
    refreshes: Vec<RouteBindingReceipt>,
    origins: BTreeMap<(String, String), ResponseRouteOrigin>,
    events: Vec<&'static str>,
    fail_refresh: bool,
    fail_origin: bool,
    failures: Vec<RouteFailureRecord>,
    all_cooled: bool,
}

#[async_trait]
impl RoutingRepository for MockStore {
    async fn select_route(
        &self,
        request: &RouteSelectionRequest,
    ) -> Result<Option<RouteSelection>, StoreError> {
        let mut state = self.state.lock().unwrap();
        state.selections.push(request.clone());
        state.events.push("select");
        if state.all_cooled {
            return Ok(None);
        }
        let route_id = request
            .candidates
            .first()
            .expect("eligible candidate")
            .route_id;
        Ok(Some(RouteSelection {
            route_id,
            binding: request
                .affinity_key
                .as_ref()
                .map(|key| RouteBindingReceipt {
                    model_id: request.model_id,
                    affinity_key: key.clone(),
                    token: Uuid::new_v4(),
                    route_id,
                    idle_timeout_seconds: request.idle_timeout_seconds,
                }),
            reused: false,
        }))
    }

    async fn record_route_failure(&self, failure: &RouteFailureRecord) -> Result<(), StoreError> {
        self.state.lock().unwrap().failures.push(failure.clone());
        Ok(())
    }

    async fn refresh_route_binding(
        &self,
        receipt: &RouteBindingReceipt,
        _now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let mut state = self.state.lock().unwrap();
        state.refreshes.push(receipt.clone());
        state.events.push("refresh");
        if state.fail_refresh {
            return Err(StoreError::Unexpected("unavailable".into()));
        }
        Ok(())
    }

    async fn get_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        _now: OffsetDateTime,
    ) -> Result<Option<ResponseRouteOrigin>, StoreError> {
        let mut state = self.state.lock().unwrap();
        state.events.push("lookup");
        Ok(state
            .origins
            .get(&(owner_key.into(), response_id_hash.into()))
            .cloned())
    }

    async fn record_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        origin: &ResponseRouteOrigin,
        _now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let mut state = self.state.lock().unwrap();
        if state.fail_origin {
            return Err(StoreError::Unexpected("unavailable".into()));
        }
        state
            .origins
            .insert((owner_key.into(), response_id_hash.into()), origin.clone());
        state.events.push("origin");
        Ok(())
    }
}

struct TestRequest {
    resolved: ResolvedGatewayRequest,
    extra: BTreeMap<String, Value>,
    headers: BTreeMap<String, String>,
    credential_versions: BTreeMap<String, String>,
    provider_identities: BTreeMap<String, String>,
    harness: &'static str,
    endpoint: RoutingEndpoint,
}

impl TestRequest {
    fn new(policy: Option<ModelRoutingPolicy>) -> Self {
        let execution_model = GatewayModel {
            id: Uuid::from_u128(100),
            model_key: "pooled".into(),
            alias_target_model_key: None,
            max_reasoning_effort: None,
            description: None,
            tags: Vec::new(),
            rank: 10,
            routing: policy,
        };
        let routes: Vec<_> = [2, 1]
            .into_iter()
            .map(|id| ModelRoute {
                id: Uuid::from_u128(id),
                model_id: execution_model.id,
                provider_key: format!("provider-{id}"),
                upstream_model: "same-model".into(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                context_window_tokens: None,
                pricing_override: None,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            })
            .collect();
        let provider_connections = routes
            .iter()
            .map(|route| {
                (
                    route.provider_key.clone(),
                    ResolvedProviderConnection {
                        provider_key: route.provider_key.clone(),
                        provider_type: "openai_compat".into(),
                        config: json!({"base_url": "https://example.invalid/v1"}),
                        redacted_secrets: None,
                    },
                )
            })
            .collect();
        Self {
            provider_identities: routes
                .iter()
                .map(|route| (route.provider_key.clone(), "runtime-identity".into()))
                .collect(),
            resolved: ResolvedGatewayRequest {
                auth: AuthenticatedApiKey {
                    id: Uuid::from_u128(200),
                    public_id: "caller".into(),
                    name: "routing test".into(),
                    model_grant_mode: ApiKeyModelGrantMode::Explicit,
                    owner_kind: ApiKeyOwnerKind::User,
                    owner_user_id: Some(Uuid::from_u128(201)),
                    owner_team_id: None,
                    owner_service_account_id: None,
                },
                selection: ResolvedModelSelection {
                    requested_model: GatewayModel {
                        id: Uuid::from_u128(101),
                        model_key: "alias".into(),
                        alias_target_model_key: Some("pooled".into()),
                        routing: None,
                        ..execution_model.clone()
                    },
                    execution_model,
                    alias_chain: vec!["alias".into(), "pooled".into()],
                    max_reasoning_effort: None,
                },
                routes,
                provider_connections,
            },
            extra: BTreeMap::new(),
            headers: BTreeMap::new(),
            credential_versions: BTreeMap::new(),
            harness: "unknown",
            endpoint: RoutingEndpoint::ChatCompletions,
        }
    }

    fn sticky(strategy: RoutingStrategy) -> Self {
        let mut request = Self::new(Some(ModelRoutingPolicy {
            strategy,
            affinity: Some(SessionAffinityPolicy::default()),
            failover: None,
        }));
        request
            .headers
            .insert("x-oceans-session-id".into(), "shared-session".into());
        request
    }

    fn request(&self) -> RoutingRequest<'_> {
        RoutingRequest {
            resolved: &self.resolved,
            eligible_routes: &self.resolved.routes,
            extra: &self.extra,
            headers: &self.headers,
            credential_versions: &self.credential_versions,
            provider_identities: &self.provider_identities,
            harness_key: self.harness,
            endpoint: self.endpoint,
        }
    }
}

#[tokio::test]
async fn omitted_policy_keeps_legacy_order_without_reading_routing_state() {
    let store = MockStore::default();
    let mut request = TestRequest::new(None);
    // Legacy routing does not require these policy-specific inputs.
    request.resolved.provider_connections.clear();
    request
        .headers
        .insert("x-oceans-session-id".into(), "invalid session".into());
    let selection = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selection.route.id, Uuid::from_u128(2));
    assert!(selection.receipt.is_none());
    assert!(store.state.lock().unwrap().events.is_empty());
}

#[tokio::test]
async fn failover_reads_shared_state_without_affinity_and_records_exact_failure() {
    for strategy in [RoutingStrategy::Preferred, RoutingStrategy::WeightedRandom] {
        let store = MockStore::default();
        let mut request = TestRequest::new(Some(ModelRoutingPolicy {
            strategy,
            failover: Some(gateway_core::ProviderFailoverPolicy::default()),
            ..ModelRoutingPolicy::default()
        }));
        let selected = select_route(&store, request.request())
            .await
            .unwrap()
            .unwrap();
        let receipt = selected.receipt.unwrap();
        receipt
            .record_failure(&store, Duration::from_secs(300))
            .await
            .unwrap();
        {
            let state = store.state.lock().unwrap();
            assert_eq!(state.selections.len(), 1);
            assert!(state.selections[0].affinity_key.is_none());
            assert_eq!(state.failures[0].cooldown_key, receipt.cooldown_key);
            assert!(state.failures[0].binding.is_none());
        }
        request
            .resolved
            .selection
            .execution_model
            .routing
            .as_mut()
            .unwrap()
            .affinity = Some(SessionAffinityPolicy::default());
        request
            .headers
            .insert("x-oceans-session-id".into(), "session".into());
        let selected = select_route(&store, request.request())
            .await
            .unwrap()
            .unwrap();
        let receipt = selected.receipt.unwrap();
        receipt
            .record_failure(&store, Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(
            store.state.lock().unwrap().failures[1].binding,
            receipt.binding
        );
    }
}

#[test]
fn cooldowns_share_user_credentials_across_keys_and_isolate_other_users() {
    let mut request = TestRequest::new(Some(ModelRoutingPolicy {
        failover: Some(gateway_core::ProviderFailoverPolicy::default()),
        ..ModelRoutingPolicy::default()
    }));
    request
        .credential_versions
        .insert("provider-2".into(), "credential-a".into());
    let original = candidates(&request.request()).unwrap();
    request.resolved.auth.id = Uuid::new_v4();
    request.resolved.selection.requested_model.id = Uuid::new_v4();
    request.endpoint = RoutingEndpoint::Responses;
    let same_user = candidates(&request.request()).unwrap();
    assert_eq!(original[0].cooldown_key, same_user[0].cooldown_key);
    request.resolved.auth.owner_user_id = Some(Uuid::new_v4());
    let other_user = candidates(&request.request()).unwrap();
    assert_ne!(same_user[0].cooldown_key, other_user[0].cooldown_key);
    // Shared provider credentials do not acquire a caller-specific cooldown.
    assert_eq!(original[1].cooldown_key, other_user[1].cooldown_key);
    request
        .credential_versions
        .insert("provider-2".into(), "credential-b".into());
    let relinked = candidates(&request.request()).unwrap();
    assert_ne!(other_user[0].cooldown_key, relinked[0].cooldown_key);
}

#[tokio::test]
async fn cooled_continuation_never_chooses_a_different_origin() {
    let store = MockStore::default();
    let mut request = TestRequest::new(Some(ModelRoutingPolicy {
        failover: Some(gateway_core::ProviderFailoverPolicy::default()),
        ..ModelRoutingPolicy::default()
    }));
    request.endpoint = RoutingEndpoint::Responses;
    let selected = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    selected
        .receipt
        .unwrap()
        .complete(&store, Some("resp-owned"))
        .await
        .unwrap();
    request
        .extra
        .insert("previous_response_id".into(), json!("resp-owned"));
    store.state.lock().unwrap().all_cooled = true;
    let error = select_route(&store, request.request()).await.err().unwrap();
    assert_eq!(error.http_status_code(), 503);
    assert_eq!(error.error_code(), "routes_temporarily_unavailable");
    let state = store.state.lock().unwrap();
    let continued = state.selections.last().unwrap();
    assert_eq!(continued.candidates.len(), 1);
    assert_eq!(continued.candidates[0].route_id, selected.route.id);
    assert!(continued.affinity_key.is_none());
}

#[tokio::test]
async fn policies_preserve_weighted_order_and_make_preferred_ties_deterministic() {
    for (strategy, expected_ids, mode) in [
        (
            RoutingStrategy::Preferred,
            vec![3, 1, 2],
            RouteSelectionMode::First,
        ),
        (
            RoutingStrategy::WeightedRandom,
            vec![2, 1, 3],
            RouteSelectionMode::First,
        ),
        (
            RoutingStrategy::RoundRobin,
            vec![3, 1, 2],
            RouteSelectionMode::RoundRobin,
        ),
    ] {
        let store = MockStore::default();
        let mut request = TestRequest::sticky(strategy);
        let mut high_priority = request.resolved.routes[0].clone();
        high_priority.id = Uuid::from_u128(3);
        high_priority.priority = 1;
        request.resolved.routes.push(high_priority);
        let selection = select_route(&store, request.request())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selection.route.id, Uuid::from_u128(expected_ids[0]));
        let state = store.state.lock().unwrap();
        let submitted = &state.selections[0];
        assert_eq!(submitted.mode, mode);
        assert_eq!(submitted.idle_timeout_seconds, 3600);
        assert_eq!(
            submitted
                .candidates
                .iter()
                .map(|route| route.route_id)
                .collect::<Vec<_>>(),
            expected_ids
                .into_iter()
                .map(Uuid::from_u128)
                .collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn affinity_is_scoped_by_api_key_requested_alias_and_harness_namespace() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::Preferred);
    request.headers = BTreeMap::from([("session-id".into(), "shared-session".into())]);
    request.harness = "codex";
    select_route(&store, request.request()).await.unwrap();
    select_route(&store, request.request()).await.unwrap();
    request.resolved.auth.id = Uuid::from_u128(300);
    select_route(&store, request.request()).await.unwrap();
    request.resolved.auth.id = Uuid::from_u128(200);
    request.resolved.selection.requested_model.id = Uuid::from_u128(301);
    select_route(&store, request.request()).await.unwrap();
    request.resolved.selection.requested_model.id = Uuid::from_u128(101);
    request.harness = "pi";
    request.headers = BTreeMap::from([("session_id".into(), "shared-session".into())]);
    select_route(&store, request.request()).await.unwrap();

    let state = store.state.lock().unwrap();
    let keys: Vec<_> = state
        .selections
        .iter()
        .map(|selection| selection.affinity_key.as_ref().unwrap())
        .collect();
    assert_eq!(keys[0], keys[1]);
    let distinct: std::collections::BTreeSet<_> = [keys[0], keys[2], keys[3], keys[4]].into();
    assert_eq!(distinct.len(), 4);
    assert!(
        keys.iter()
            .all(|key| key.len() == 64 && !key.contains("shared-session"))
    );
}

#[tokio::test]
async fn missing_session_does_not_infer_affinity_from_prompt_cache_keys_or_content() {
    for strategy in [RoutingStrategy::Preferred, RoutingStrategy::WeightedRandom] {
        let store = MockStore::default();
        let mut request = TestRequest::sticky(strategy);
        request.headers.clear();
        request
            .extra
            .insert("prompt_cache_key".into(), json!("shared-prefix"));
        request.extra.insert(
            "messages".into(),
            json!([{"content": "session_id: shared-session"}]),
        );
        let selected = select_route(&store, request.request())
            .await
            .unwrap()
            .unwrap();
        assert!(selected.receipt.is_none());
        assert!(store.state.lock().unwrap().events.is_empty());
    }
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::RoundRobin);
    request.headers.clear();
    select_route(&store, request.request()).await.unwrap();
    let state = store.state.lock().unwrap();
    assert!(state.selections[0].affinity_key.is_none());
    assert_eq!(state.selections[0].mode, RouteSelectionMode::RoundRobin);
}

#[tokio::test]
async fn continuations_rebind_the_recorded_origin_without_advancing_round_robin() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::RoundRobin);
    request.endpoint = RoutingEndpoint::Responses;
    let original = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    original
        .receipt
        .unwrap()
        .complete(&store, Some("resp-original"))
        .await
        .unwrap();

    // The mock has no live soft binding. Only durable response ownership remains.
    request.resolved.routes[0].priority = 0;
    request
        .extra
        .insert("previous_response_id".into(), json!("resp-original"));
    let continued = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(continued.route.id, original.route.id);
    continued
        .receipt
        .unwrap()
        .complete(&store, Some("resp-next"))
        .await
        .unwrap();
    let state = store.state.lock().unwrap();
    let rebinding = &state.selections[1];
    assert_eq!(rebinding.mode, RouteSelectionMode::First);
    assert_eq!(rebinding.candidates.len(), 1);
    assert_eq!(rebinding.candidates[0].route_id, original.route.id);
    assert_eq!(state.refreshes.len(), 2);
    assert_eq!(state.origins.len(), 2);
}

#[tokio::test]
async fn unknown_or_ineligible_continuation_origins_never_select_a_replacement() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::Preferred);
    request.endpoint = RoutingEndpoint::Responses;
    request
        .extra
        .insert("previous_response_id".into(), json!("resp-unknown"));
    assert!(matches!(
        select_route(&store, request.request()).await,
        Err(GatewayError::InvalidRequest(_))
    ));
    assert!(store.state.lock().unwrap().selections.is_empty());

    request.extra.clear();
    let original = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    original
        .receipt
        .unwrap()
        .complete(&store, Some("resp-known"))
        .await
        .unwrap();
    store.state.lock().unwrap().selections.clear();
    request
        .resolved
        .routes
        .retain(|route| route.id != original.route.id);
    request
        .extra
        .insert("previous_response_id".into(), json!("resp-known"));
    assert!(matches!(
        select_route(&store, request.request()).await,
        Err(GatewayError::InvalidRequest(_))
    ));
    assert!(store.state.lock().unwrap().selections.is_empty());
}

#[tokio::test]
async fn configuration_and_credential_changes_invalidate_candidate_fingerprints() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::WeightedRandom);
    select_route(&store, request.request()).await.unwrap();
    request.provider_identities.insert(
        "provider-2".into(),
        "changed-runtime-endpoint-or-key".into(),
    );
    select_route(&store, request.request()).await.unwrap();
    request
        .credential_versions
        .insert("provider-2".into(), "new-credential".into());
    select_route(&store, request.request()).await.unwrap();
    let state = store.state.lock().unwrap();
    let selections = &state.selections;
    assert_eq!(selections[0].affinity_key, selections[1].affinity_key);
    assert_eq!(selections[1].affinity_key, selections[2].affinity_key);
    assert_ne!(
        selections[0].candidates[0].fingerprint,
        selections[1].candidates[0].fingerprint
    );
    assert_ne!(
        selections[1].candidates[0].fingerprint,
        selections[2].candidates[0].fingerprint
    );
    assert_eq!(selections[0].candidates[1], selections[2].candidates[1]);
}

#[tokio::test]
async fn successful_completion_records_origin_before_refreshing_its_exact_binding() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::WeightedRandom);
    request.endpoint = RoutingEndpoint::Responses;
    let selected = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    {
        let mut state = store.state.lock().unwrap();
        assert!(state.origins.is_empty());
        assert!(state.refreshes.is_empty());
        state.events.clear();
    }
    let receipt = selected.receipt.expect("completion receipt");
    receipt
        .complete(&store, Some("resp-created"))
        .await
        .unwrap();
    let state = store.state.lock().unwrap();
    assert_eq!(state.events, ["origin", "refresh"]);
    assert_eq!(state.refreshes[0], *receipt.binding.as_ref().unwrap());
    let ((owner_hash, response_hash), origin) = state.origins.first_key_value().unwrap();
    assert_eq!(owner_hash.len(), 64);
    assert_eq!(response_hash.len(), 64);
    assert_ne!(response_hash, "resp-created");
    assert_eq!(
        origin.model_id,
        request.resolved.selection.execution_model.id
    );
    assert_eq!(origin.route_id, selected.route.id);
}

#[tokio::test]
async fn non_responses_endpoints_do_not_interpret_response_ownership_fields() {
    for endpoint in [
        RoutingEndpoint::ChatCompletions,
        RoutingEndpoint::Messages,
        RoutingEndpoint::Embeddings,
        RoutingEndpoint::Decisions,
    ] {
        for extra in [
            BTreeMap::from([("conversation".into(), json!("provider-conversation"))]),
            BTreeMap::from([("conversation".into(), json!({"provider_option": true}))]),
            BTreeMap::from([("previous_response_id".into(), json!("provider-response"))]),
            BTreeMap::from([("previous_response_id".into(), json!(3))]),
        ] {
            let store = MockStore::default();
            let mut request = TestRequest::sticky(RoutingStrategy::RoundRobin);
            request.endpoint = endpoint;
            request.extra = extra;

            let selected = select_route(&store, request.request())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(selected.route.id, Uuid::from_u128(1));
            if let Some(receipt) = selected.receipt {
                receipt
                    .complete(&store, Some("provider-result"))
                    .await
                    .unwrap();
            }

            let state = store.state.lock().unwrap();
            assert_eq!(state.selections.len(), 1);
            assert_eq!(state.selections[0].mode, RouteSelectionMode::RoundRobin);
            assert!(!state.events.contains(&"lookup"));
            assert!(state.origins.is_empty());
        }
    }
}

#[tokio::test]
async fn conflicting_sessions_and_unsupported_conversation_resources_fail_before_selection() {
    for (endpoint, headers, extra) in [
        (
            RoutingEndpoint::ChatCompletions,
            BTreeMap::from([("session-id".into(), "other-session".into())]),
            BTreeMap::new(),
        ),
        (
            RoutingEndpoint::Responses,
            BTreeMap::new(),
            BTreeMap::from([("conversation".into(), json!("conv-owned"))]),
        ),
        (
            RoutingEndpoint::Responses,
            BTreeMap::new(),
            BTreeMap::from([("previous_response_id".into(), json!(3))]),
        ),
    ] {
        let store = MockStore::default();
        let mut request = TestRequest::sticky(RoutingStrategy::Preferred);
        request.endpoint = endpoint;
        request.harness = "codex";
        request.headers.extend(headers);
        request.extra = extra;
        assert!(matches!(
            select_route(&store, request.request()).await,
            Err(GatewayError::InvalidRequest(_))
        ));
        assert!(store.state.lock().unwrap().events.is_empty());
    }
}

#[tokio::test]
async fn refresh_failure_preserves_success_but_origin_failure_remains_fatal() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::Preferred);
    request.endpoint = RoutingEndpoint::Responses;
    let selected = select_route(&store, request.request())
        .await
        .unwrap()
        .unwrap();
    let receipt = selected.receipt.unwrap();
    store.state.lock().unwrap().fail_refresh = true;
    receipt.complete(&store, Some("resp-safe")).await.unwrap();
    assert_eq!(store.state.lock().unwrap().origins.len(), 1);

    store.state.lock().unwrap().fail_origin = true;
    assert!(matches!(
        receipt.complete(&store, Some("resp-unsafe")).await,
        Err(GatewayError::Store(_))
    ));
    assert_eq!(store.state.lock().unwrap().origins.len(), 1);
}

#[tokio::test]
async fn catalog_metadata_changes_do_not_change_runtime_route_identity() {
    let store = MockStore::default();
    let mut request = TestRequest::sticky(RoutingStrategy::Preferred);
    select_route(&store, request.request()).await.unwrap();
    request
        .resolved
        .provider_connections
        .get_mut("provider-1")
        .unwrap()
        .config = json!({
        "display": {"name": "New display name"},
        "pricing_provider_id": "new-catalog-identity",
        "timeouts": {"total_ms": 60_000}
    });
    select_route(&store, request.request()).await.unwrap();
    let state = store.state.lock().unwrap();
    assert_eq!(
        state.selections[0].candidates,
        state.selections[1].candidates
    );
}
