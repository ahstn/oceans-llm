use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, GatewayError, GatewayModel, IdentityRepository, ModelRepository,
    ModelRoute, PricingCatalogRepository, PricingResolution, PricingUnpricedReason,
    ProviderRepository, RequestLogRecord, RequestLogRepository, RouteError, RoutePlanner,
    StoreHealth,
};
use tracing::warn;

use crate::{Authenticator, ModelAccess, PricingCatalog, RequestLogging};

#[derive(Debug, Clone)]
pub struct ResolvedRequest {
    pub auth: AuthenticatedApiKey,
    pub requested_model: GatewayModel,
    pub execution_model: GatewayModel,
    pub routes: Vec<ModelRoute>,
}

#[derive(Clone)]
pub struct GatewayService<S, P> {
    store: Arc<S>,
    authenticator: Authenticator<S>,
    model_access: ModelAccess<S>,
    pricing_catalog: PricingCatalog<S>,
    request_logging: RequestLogging<S>,
    planner: Arc<P>,
}

impl<S, P> GatewayService<S, P>
where
    S: gateway_core::ApiKeyRepository
        + ModelRepository
        + IdentityRepository
        + PricingCatalogRepository
        + RequestLogRepository
        + ProviderRepository
        + StoreHealth
        + Send
        + Sync
        + 'static,
    P: RoutePlanner + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(store: Arc<S>, planner: Arc<P>) -> Self {
        let authenticator = Authenticator::new(store.clone());
        let model_access = ModelAccess::new(store.clone());
        let pricing_catalog = PricingCatalog::new(store.clone());
        let request_logging = RequestLogging::new(store.clone());

        Self {
            store,
            authenticator,
            model_access,
            pricing_catalog,
            request_logging,
            planner,
        }
    }

    pub async fn check_readiness(&self) -> Result<(), GatewayError> {
        self.store.ping().await?;
        Ok(())
    }

    #[must_use]
    pub fn store(&self) -> &Arc<S> {
        &self.store
    }

    pub async fn authenticate(
        &self,
        authorization_header: Option<&str>,
    ) -> Result<AuthenticatedApiKey, GatewayError> {
        self.authenticator
            .authenticate_authorization_header(authorization_header)
            .await
    }

    pub async fn list_models_for_api_key(
        &self,
        auth: &AuthenticatedApiKey,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        self.model_access.list_models_for_api_key(auth).await
    }

    pub async fn resolve_request(
        &self,
        auth: &AuthenticatedApiKey,
        requested_model: &str,
    ) -> Result<ResolvedRequest, GatewayError> {
        let requested_model = self
            .model_access
            .resolve_requested_model(auth, requested_model)
            .await?;
        let execution_model = self
            .resolve_execution_model(&requested_model)
            .await?;

        let routes = self.store.list_routes_for_model(execution_model.id).await?;
        let planned_routes = self.planner.plan_routes(&routes)?;

        let mut viable_routes = Vec::new();
        for route in planned_routes {
            let exists = self
                .store
                .get_provider_by_key(&route.provider_key)
                .await?
                .is_some();
            if exists {
                viable_routes.push(route);
            } else {
                warn!(
                    provider_key = %route.provider_key,
                    requested_model_key = %requested_model.model_key,
                    execution_model_key = %execution_model.model_key,
                    "route references missing provider"
                );
            }
        }

        if viable_routes.is_empty() {
            return Err(RouteError::NoRoutesAvailable(requested_model.model_key.clone()).into());
        }

        Ok(ResolvedRequest {
            auth: auth.clone(),
            requested_model,
            execution_model,
            routes: viable_routes,
        })
    }

    pub async fn log_request_if_enabled(
        &self,
        auth: &AuthenticatedApiKey,
        log: RequestLogRecord,
    ) -> Result<bool, GatewayError> {
        self.request_logging.log_request_if_enabled(auth, log).await
    }

    pub async fn refresh_pricing_catalog_if_stale(&self) -> Result<(), GatewayError> {
        self.pricing_catalog.refresh_if_stale().await
    }

    pub async fn resolve_route_pricing(
        &self,
        route: &ModelRoute,
    ) -> Result<PricingResolution, GatewayError> {
        let Some(provider) = self.store.get_provider_by_key(&route.provider_key).await? else {
            return Ok(PricingResolution::Unpriced {
                reason: PricingUnpricedReason::UnsupportedPricingProviderId(
                    route.provider_key.clone(),
                ),
            });
        };

        self.pricing_catalog
            .resolve_for_provider_connection(&provider, route)
            .await
    }

    async fn resolve_execution_model(
        &self,
        requested_model: &GatewayModel,
    ) -> Result<GatewayModel, GatewayError> {
        const MAX_ALIAS_DEPTH: usize = 8;

        let mut current = requested_model.clone();
        let mut seen_keys = std::collections::BTreeSet::new();
        seen_keys.insert(current.model_key.clone());

        for _ in 0..MAX_ALIAS_DEPTH {
            let Some(alias_target_model_key) = current.alias_target_model_key.clone() else {
                return Ok(current);
            };

            let next = self
                .store
                .get_model_by_key(&alias_target_model_key)
                .await?
                .ok_or_else(|| RouteError::ModelNotFound(requested_model.model_key.clone()))?;

            if !seen_keys.insert(next.model_key.clone()) {
                return Err(RouteError::Policy(format!(
                    "model alias cycle detected for requested model `{}`",
                    requested_model.model_key
                ))
                .into());
            }

            current = next;
        }

        Err(RouteError::Policy(format!(
            "model alias depth exceeded for requested model `{}`",
            requested_model.model_key
        ))
        .into())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, ApiKeyRepository, AuthMode, AuthenticatedApiKey, GatewayError, GatewayModel,
        GlobalRole, IdentityRepository, ModelAccessMode, ModelRepository, ModelRoute,
        PricingCatalogCacheRecord, PricingCatalogRepository, ProviderConnection, ProviderRepository,
        RequestLogRecord, RequestLogRepository, RouteError, RoutePlanner, StoreError, StoreHealth,
        TeamMembershipRecord, TeamRecord, UserRecord,
    };
    use serde_json::{Map, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::GatewayService;

    #[derive(Default)]
    struct InMemoryStore {
        models: HashMap<String, GatewayModel>,
        grants: HashMap<Uuid, Vec<String>>,
        routes: HashMap<Uuid, Vec<ModelRoute>>,
        providers: HashMap<String, ProviderConnection>,
    }

    #[async_trait]
    impl ApiKeyRepository for InMemoryStore {
        async fn get_api_key_by_public_id(
            &self,
            _public_id: &str,
        ) -> Result<Option<gateway_core::ApiKeyRecord>, StoreError> {
            Ok(None)
        }

        async fn touch_api_key_last_used(&self, _api_key_id: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ModelRepository for InMemoryStore {
        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self.models.get(model_key).cloned())
        }

        async fn list_models_for_api_key(
            &self,
            api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            let model_keys = self.grants.get(&api_key_id).cloned().unwrap_or_default();
            Ok(model_keys
                .into_iter()
                .filter_map(|model_key| self.models.get(&model_key).cloned())
                .collect())
        }

        async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError> {
            Ok(self.routes.get(&model_id).cloned().unwrap_or_default())
        }
    }

    #[async_trait]
    impl IdentityRepository for InMemoryStore {
        async fn get_user_by_id(&self, _user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(Some(UserRecord {
                user_id: Uuid::new_v4(),
                name: "test".to_string(),
                email: "user@example.com".to_string(),
                email_normalized: "user@example.com".to_string(),
                global_role: GlobalRole::User,
                auth_mode: AuthMode::Password,
                status: "active".to_string(),
                must_change_password: false,
                request_logging_enabled: true,
                model_access_mode: ModelAccessMode::All,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }))
        }

        async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(Some(TeamRecord {
                team_id,
                team_key: "team".to_string(),
                team_name: "Team".to_string(),
                status: "active".to_string(),
                model_access_mode: ModelAccessMode::All,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }))
        }

        async fn get_team_membership_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(None)
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl PricingCatalogRepository for InMemoryStore {
        async fn get_pricing_catalog_cache(
            &self,
            _catalog_key: &str,
        ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
            Ok(None)
        }

        async fn upsert_pricing_catalog_cache(
            &self,
            _cache: &PricingCatalogCacheRecord,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn touch_pricing_catalog_cache_fetched_at(
            &self,
            _catalog_key: &str,
            _fetched_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[async_trait]
    impl RequestLogRepository for InMemoryStore {
        async fn insert_request_log(&self, _log: &RequestLogRecord) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ProviderRepository for InMemoryStore {
        async fn get_provider_by_key(
            &self,
            provider_key: &str,
        ) -> Result<Option<ProviderConnection>, StoreError> {
            Ok(self.providers.get(provider_key).cloned())
        }
    }

    #[async_trait]
    impl StoreHealth for InMemoryStore {
        async fn ping(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    struct PassthroughPlanner;

    impl RoutePlanner for PassthroughPlanner {
        fn plan_routes(&self, routes: &[ModelRoute]) -> Result<Vec<ModelRoute>, RouteError> {
            Ok(routes.to_vec())
        }
    }

    fn model(model_key: &str, alias_target_model_key: Option<&str>) -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: model_key.to_string(),
            alias_target_model_key: alias_target_model_key.map(ToString::to_string),
            description: None,
            tags: Vec::new(),
            rank: 10,
        }
    }

    fn route(model_id: Uuid, provider_key: &str, upstream_model: &str) -> ModelRoute {
        ModelRoute {
            id: Uuid::new_v4(),
            model_id,
            provider_key: provider_key.to_string(),
            upstream_model: upstream_model.to_string(),
            priority: 10,
            weight: 1.0,
            enabled: true,
            extra_headers: Map::new(),
            extra_body: Map::new(),
        }
    }

    fn auth(api_key_id: Uuid, team_id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        }
    }

    fn provider(provider_key: &str) -> ProviderConnection {
        ProviderConnection {
            provider_key: provider_key.to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({"base_url":"https://api.example.com/v1"}),
            secrets: None,
        }
    }

    #[tokio::test]
    async fn resolve_request_follows_alias_chain_to_execution_model() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let requested = model("fast", Some("fast-v2"));
        let intermediate = model("fast-v2", Some("fast-v3"));
        let target = model("fast-v3", None);

        let target_route = route(target.id, "openai-prod", "gpt-5");
        let mut store = InMemoryStore::default();
        store.models.insert(requested.model_key.clone(), requested.clone());
        store.models.insert(intermediate.model_key.clone(), intermediate);
        store.models.insert(target.model_key.clone(), target.clone());
        store.grants.insert(api_key_id, vec!["fast".to_string()]);
        store.routes.insert(target.id, vec![target_route.clone()]);
        store
            .providers
            .insert("openai-prod".to_string(), provider("openai-prod"));

        let service = GatewayService::new(Arc::new(store), Arc::new(PassthroughPlanner));
        let resolved = service
            .resolve_request(&auth(api_key_id, team_id), "fast")
            .await
            .expect("alias should resolve");

        assert_eq!(resolved.requested_model.model_key, "fast");
        assert_eq!(resolved.execution_model.model_key, "fast-v3");
        assert_eq!(resolved.routes.len(), 1);
        assert_eq!(resolved.routes[0].upstream_model, "gpt-5");
    }

    #[tokio::test]
    async fn resolve_request_rejects_runtime_alias_cycles() {
        let api_key_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let fast = model("fast", Some("fast-v2"));
        let fast_v2 = model("fast-v2", Some("fast"));

        let mut store = InMemoryStore::default();
        store.models.insert(fast.model_key.clone(), fast);
        store.models.insert(fast_v2.model_key.clone(), fast_v2);
        store.grants.insert(api_key_id, vec!["fast".to_string()]);

        let service = GatewayService::new(Arc::new(store), Arc::new(PassthroughPlanner));
        let error = service
            .resolve_request(&auth(api_key_id, team_id), "fast")
            .await
            .expect_err("cycle should fail");

        assert!(matches!(error, GatewayError::Route(RouteError::Policy(_))));
    }
}
