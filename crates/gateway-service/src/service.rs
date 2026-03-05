use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, GatewayError, GatewayModel, IdentityRepository, ModelRepository,
    ModelRoute, ProviderRepository, RequestLogRecord, RequestLogRepository, RouteError,
    RoutePlanner, StoreHealth,
};
use tracing::warn;

use crate::{Authenticator, ModelAccess, RequestLogging};

#[derive(Debug, Clone)]
pub struct ResolvedRequest {
    pub auth: AuthenticatedApiKey,
    pub model: GatewayModel,
    pub routes: Vec<ModelRoute>,
}

#[derive(Clone)]
pub struct GatewayService<S, P> {
    store: Arc<S>,
    authenticator: Authenticator<S>,
    model_access: ModelAccess<S>,
    request_logging: RequestLogging<S>,
    planner: Arc<P>,
}

impl<S, P> GatewayService<S, P>
where
    S: gateway_core::ApiKeyRepository
        + ModelRepository
        + IdentityRepository
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
        let request_logging = RequestLogging::new(store.clone());

        Self {
            store,
            authenticator,
            model_access,
            request_logging,
            planner,
        }
    }

    pub async fn check_readiness(&self) -> Result<(), GatewayError> {
        self.store.ping().await?;
        Ok(())
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
        let model = self
            .model_access
            .resolve_requested_model(auth, requested_model)
            .await?;

        let routes = self.store.list_routes_for_model(model.id).await?;
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
                    model_key = %model.model_key,
                    "route references missing provider"
                );
            }
        }

        if viable_routes.is_empty() {
            return Err(RouteError::NoRoutesAvailable(model.model_key.clone()).into());
        }

        Ok(ResolvedRequest {
            auth: auth.clone(),
            model,
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
}
