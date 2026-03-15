use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetRepository, GatewayError, GatewayModel,
    IdentityRepository, ModelRepository, ModelRoute, Money4, PricingCatalogRepository,
    PricingResolution, PricingUnpricedReason, ProviderRepository, RequestLogRecord,
    RequestLogRepository, ResolvedModelPricing, RouteError, RoutePlanner, StoreHealth,
    UsageLedgerRecord, UsagePricingStatus,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::{
    Authenticator, ModelAccess, ModelResolver, PricingCatalog, RequestLogging,
    ResolvedGatewayRequest,
    budget_guard::{BudgetGuard, BudgetGuardDisposition},
};

#[derive(Clone)]
pub struct GatewayService<S, P> {
    store: Arc<S>,
    authenticator: Authenticator<S>,
    budget_guard: BudgetGuard<S>,
    model_access: ModelAccess<S>,
    model_resolver: ModelResolver<S>,
    pricing_catalog: PricingCatalog<S>,
    request_logging: RequestLogging<S>,
    planner: Arc<P>,
}

impl<S, P> GatewayService<S, P>
where
    S: gateway_core::ApiKeyRepository
        + BudgetRepository
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
        let budget_guard = BudgetGuard::new(store.clone());
        let model_access = ModelAccess::new(store.clone());
        let model_resolver = ModelResolver::new(store.clone());
        let pricing_catalog = PricingCatalog::new(store.clone());
        let request_logging = RequestLogging::new(store.clone());

        Self {
            store,
            authenticator,
            budget_guard,
            model_access,
            model_resolver,
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
    ) -> Result<ResolvedGatewayRequest, GatewayError> {
        let requested_model = self
            .model_access
            .resolve_requested_model(auth, requested_model)
            .await?;
        let selection = self
            .model_resolver
            .canonicalize_requested_model(requested_model)
            .await?;

        let routes = self
            .store
            .list_routes_for_model(selection.execution_model.id)
            .await?;
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
                    requested_model_key = %selection.requested_model.model_key,
                    execution_model_key = %selection.execution_model.model_key,
                    "route references missing provider"
                );
            }
        }

        if viable_routes.is_empty() {
            return Err(
                RouteError::NoRoutesAvailable(selection.requested_model.model_key.clone()).into(),
            );
        }

        Ok(ResolvedGatewayRequest {
            auth: auth.clone(),
            selection,
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
        occurred_at: OffsetDateTime,
    ) -> Result<PricingResolution, GatewayError> {
        let Some(provider) = self.store.get_provider_by_key(&route.provider_key).await? else {
            return Ok(PricingResolution::Unpriced {
                reason: PricingUnpricedReason::UnsupportedPricingProviderId(
                    route.provider_key.clone(),
                ),
            });
        };

        self.pricing_catalog
            .resolve_for_provider_connection(&provider, route, occurred_at)
            .await
    }

    pub async fn record_chat_usage(
        &self,
        auth: &AuthenticatedApiKey,
        model: &GatewayModel,
        route: &ModelRoute,
        request_id: &str,
        provider_usage: Option<Value>,
        occurred_at: OffsetDateTime,
    ) -> Result<BudgetGuardDisposition, GatewayError> {
        let ownership_scope_key = ownership_scope_key(auth, None)?;
        let usage_summary = usage_summary_from_value(provider_usage.as_ref())?;
        let provider_usage = provider_usage.unwrap_or_else(|| json!({}));

        let mut record = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key,
            api_key_id: auth.id,
            user_id: auth.owner_user_id,
            team_id: auth.owner_team_id,
            actor_user_id: None,
            model_id: Some(model.id),
            provider_key: route.provider_key.clone(),
            upstream_model: route.upstream_model.clone(),
            prompt_tokens: usage_summary.prompt_tokens,
            completion_tokens: usage_summary.completion_tokens,
            total_tokens: usage_summary.total_tokens,
            provider_usage,
            pricing_status: UsagePricingStatus::UsageMissing,
            unpriced_reason: None,
            pricing_row_id: None,
            pricing_provider_id: None,
            pricing_model_id: None,
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            computed_cost_usd: Money4::ZERO,
            occurred_at,
        };

        if usage_summary.has_usage() {
            match self.resolve_route_pricing(route, occurred_at).await? {
                PricingResolution::Exact { pricing } => apply_exact_pricing(&mut record, &pricing)?,
                PricingResolution::Unpriced { reason } => {
                    record.pricing_status = UsagePricingStatus::Unpriced;
                    record.unpriced_reason = Some(unpriced_reason_string(&reason));
                    warn!(
                        request_id = %request_id,
                        provider_key = %route.provider_key,
                        model_key = %model.model_key,
                        reason = %record.unpriced_reason.as_deref().unwrap_or("unknown"),
                        "usage ledger recorded without matching pricing"
                    );
                }
            }
        } else {
            warn!(
                request_id = %request_id,
                provider_key = %route.provider_key,
                model_key = %model.model_key,
                "usage ledger recorded without provider usage details"
            );
        }

        let disposition = self
            .budget_guard
            .enforce_and_record_usage(auth, &record)
            .await?;
        if disposition == BudgetGuardDisposition::Duplicate {
            warn!(
                request_id = %request_id,
                ownership_scope_key = %record.ownership_scope_key,
                "duplicate usage ledger write ignored"
            );
        }

        Ok(disposition)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct UsageSummary {
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

impl UsageSummary {
    fn has_usage(self) -> bool {
        self.prompt_tokens.is_some()
            || self.completion_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

fn usage_summary_from_value(value: Option<&Value>) -> Result<UsageSummary, GatewayError> {
    let Some(usage) = value.and_then(Value::as_object) else {
        return Ok(UsageSummary::default());
    };

    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_i64);
    let completion_tokens = usage.get("completion_tokens").and_then(Value::as_i64);
    let total_tokens = match usage.get("total_tokens").and_then(Value::as_i64) {
        some @ Some(_) => some,
        None => match (prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt
                .checked_add(completion)
                .ok_or_else(|| GatewayError::Internal("token total overflow".to_string()))
                .map(Some)?,
            _ => None,
        },
    };

    Ok(UsageSummary {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

fn ownership_scope_key(
    auth: &AuthenticatedApiKey,
    actor_user_id: Option<Uuid>,
) -> Result<String, GatewayError> {
    match auth.owner_kind {
        ApiKeyOwnerKind::User => {
            let user_id = auth.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            Ok(format!("user:{user_id}"))
        }
        ApiKeyOwnerKind::Team => {
            let team_id = auth.owner_team_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            let actor_segment = actor_user_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string());
            Ok(format!("team:{team_id}:actor:{actor_segment}"))
        }
    }
}

fn apply_exact_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &ResolvedModelPricing,
) -> Result<(), GatewayError> {
    record.pricing_row_id = Some(pricing.model_pricing_id);
    record.pricing_provider_id = Some(pricing.pricing_provider_id.clone());
    record.pricing_model_id = Some(pricing.model_id.clone());
    record.pricing_source = Some(pricing.provenance.source.clone());
    record.pricing_source_etag = pricing.provenance.etag.clone();
    record.pricing_source_fetched_at = Some(pricing.provenance.fetched_at);
    record.pricing_last_updated = Some(pricing.last_updated.clone());
    record.input_cost_per_million_tokens = pricing.input_cost_per_million_tokens;
    record.output_cost_per_million_tokens = pricing.output_cost_per_million_tokens;

    if record.prompt_tokens.unwrap_or_default() > 0
        && pricing.input_cost_per_million_tokens.is_none()
    {
        record.pricing_status = UsagePricingStatus::Unpriced;
        record.unpriced_reason = Some("missing_input_rate".to_string());
        return Ok(());
    }
    if record.completion_tokens.unwrap_or_default() > 0
        && pricing.output_cost_per_million_tokens.is_none()
    {
        record.pricing_status = UsagePricingStatus::Unpriced;
        record.unpriced_reason = Some("missing_output_rate".to_string());
        return Ok(());
    }

    record.pricing_status = UsagePricingStatus::Priced;
    record.computed_cost_usd = compute_usage_cost(
        record.prompt_tokens,
        pricing.input_cost_per_million_tokens,
        record.completion_tokens,
        pricing.output_cost_per_million_tokens,
    )?;
    Ok(())
}

fn compute_usage_cost(
    prompt_tokens: Option<i64>,
    input_rate: Option<Money4>,
    completion_tokens: Option<i64>,
    output_rate: Option<Money4>,
) -> Result<Money4, GatewayError> {
    let input_cost = match (prompt_tokens, input_rate) {
        (Some(tokens), Some(rate)) => scaled_cost_for_tokens(tokens, rate)?,
        _ => Money4::ZERO,
    };
    let output_cost = match (completion_tokens, output_rate) {
        (Some(tokens), Some(rate)) => scaled_cost_for_tokens(tokens, rate)?,
        _ => Money4::ZERO,
    };

    input_cost
        .checked_add(output_cost)
        .ok_or_else(|| GatewayError::Internal("usage cost overflow".to_string()))
}

fn scaled_cost_for_tokens(tokens: i64, rate_per_million: Money4) -> Result<Money4, GatewayError> {
    if tokens < 0 {
        return Err(GatewayError::Internal(
            "token count cannot be negative".to_string(),
        ));
    }

    let numerator = i128::from(tokens)
        .checked_mul(i128::from(rate_per_million.as_scaled_i64()))
        .ok_or_else(|| GatewayError::Internal("usage cost overflow".to_string()))?;
    let rounded = numerator
        .checked_add(500_000)
        .ok_or_else(|| GatewayError::Internal("usage cost overflow".to_string()))?
        / 1_000_000;
    let scaled = i64::try_from(rounded)
        .map_err(|_| GatewayError::Internal("usage cost overflow".to_string()))?;
    Ok(Money4::from_scaled(scaled))
}

fn unpriced_reason_string(reason: &PricingUnpricedReason) -> String {
    match reason {
        PricingUnpricedReason::ProviderPricingSourceMissing => {
            "provider_pricing_source_missing".to_string()
        }
        PricingUnpricedReason::UnsupportedPricingProviderId(value) => {
            format!("unsupported_pricing_provider_id:{value}")
        }
        PricingUnpricedReason::UnsupportedVertexPublisher(value) => {
            format!("unsupported_vertex_publisher:{value}")
        }
        PricingUnpricedReason::UnsupportedVertexLocation(value) => {
            format!("unsupported_vertex_location:{value}")
        }
        PricingUnpricedReason::UnsupportedBillingModifier(value) => {
            format!("unsupported_billing_modifier:{value}")
        }
        PricingUnpricedReason::ModelNotFound => "model_not_found".to_string(),
    }
}
