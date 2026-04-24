use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetAlertRepository, BudgetRepository,
    ChatCompletionsRequest, GatewayError, GatewayModel, IdentityRepository, ModelRepository,
    ModelRoute, Money4, PricingCatalogRepository, PricingResolution, PricingUnpricedReason,
    ProviderRepository, RequestLogDetail, RequestLogPage, RequestLogQuery, RequestLogRecord,
    RequestLogRepository, RequestTags, ResolvedModelPricing, RouteError, RoutePlanner, StoreHealth,
    TeamBudgetRecord, UsageLedgerRecord, UsagePricingStatus, UserBudgetRecord,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::{
    Authenticator, ChatRequestLogContext, LoggedRequest, ModelAccess, ModelResolver,
    PricingCatalog, RequestLogIconMetadata, RequestLogPayloadPolicy, RequestLogging,
    ResolvedGatewayRequest, ResolvedProviderConnection, StreamLogResultInput,
    StreamResponseCollector,
    budget_alerts::{BudgetAlertSender, BudgetAlertService, SinkBudgetAlertSender},
    budget_guard::{BudgetGuard, BudgetGuardDisposition},
};

#[derive(Debug, Clone)]
pub struct RecordedChatUsage {
    pub disposition: BudgetGuardDisposition,
    pub pricing_status: UsagePricingStatus,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
}

#[derive(Clone)]
pub struct GatewayService<S, P> {
    store: Arc<S>,
    authenticator: Authenticator<S>,
    budget_alerts: BudgetAlertService<S>,
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
        + BudgetAlertRepository
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
        Self::new_with_budget_alert_sender(store, planner, Arc::new(SinkBudgetAlertSender))
    }

    #[must_use]
    pub fn new_with_budget_alert_sender(
        store: Arc<S>,
        planner: Arc<P>,
        sender: Arc<dyn BudgetAlertSender>,
    ) -> Self {
        Self::new_with_budget_alert_sender_and_payload_policy(
            store,
            planner,
            sender,
            RequestLogPayloadPolicy::default(),
        )
    }

    #[must_use]
    pub fn new_with_budget_alert_sender_and_payload_policy(
        store: Arc<S>,
        planner: Arc<P>,
        sender: Arc<dyn BudgetAlertSender>,
        payload_policy: RequestLogPayloadPolicy,
    ) -> Self {
        let authenticator = Authenticator::new(store.clone());
        let budget_alerts = BudgetAlertService::new(store.clone(), sender);
        let budget_guard = BudgetGuard::new(store.clone());
        let model_access = ModelAccess::new(store.clone());
        let model_resolver = ModelResolver::new(store.clone());
        let pricing_catalog = PricingCatalog::new(store.clone());
        let request_logging =
            RequestLogging::new_with_payload_policy(store.clone(), payload_policy);

        Self {
            store,
            authenticator,
            budget_alerts,
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
        let mut provider_connections = std::collections::HashMap::new();
        for route in planned_routes {
            if let Some(provider) = self.store.get_provider_by_key(&route.provider_key).await? {
                provider_connections
                    .entry(route.provider_key.clone())
                    .or_insert_with(|| {
                        ResolvedProviderConnection::from_provider_connection(&provider)
                    });
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
            provider_connections,
        })
    }

    #[must_use]
    pub fn begin_chat_request_log(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &ChatCompletionsRequest,
        request_headers: &std::collections::BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> ChatRequestLogContext {
        self.request_logging.begin_chat_request(
            request_id,
            requested_model_key,
            resolved_model_key,
            request,
            request_headers,
            request_tags,
        )
    }

    #[must_use]
    pub fn new_stream_response_collector(&self) -> StreamResponseCollector {
        self.request_logging.new_stream_response_collector()
    }

    pub async fn log_non_stream_success(
        &self,
        auth: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        latency_ms: i64,
        response_body: &Value,
    ) -> Result<LoggedRequest, GatewayError> {
        self.request_logging
            .log_non_stream_success(
                auth,
                context,
                provider_key,
                icon_metadata,
                latency_ms,
                response_body,
            )
            .await
    }

    pub async fn log_non_stream_failure(
        &self,
        auth: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        latency_ms: i64,
        gateway_error: &GatewayError,
    ) -> Result<LoggedRequest, GatewayError> {
        self.request_logging
            .log_non_stream_failure(
                auth,
                context,
                provider_key,
                icon_metadata,
                latency_ms,
                gateway_error,
            )
            .await
    }

    pub async fn log_stream_result(
        &self,
        auth: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        stream_result: StreamLogResultInput,
    ) -> Result<LoggedRequest, GatewayError> {
        self.request_logging
            .log_stream_result(auth, context, stream_result)
            .await
    }

    pub async fn log_request_if_enabled(
        &self,
        auth: &AuthenticatedApiKey,
        log: RequestLogRecord,
    ) -> Result<(), GatewayError> {
        if !self.request_logging.should_log_request(auth).await? {
            return Ok(());
        }

        self.store.insert_request_log(&log, None).await?;
        Ok(())
    }

    pub async fn list_request_logs(
        &self,
        query: &RequestLogQuery,
    ) -> Result<RequestLogPage, GatewayError> {
        self.request_logging.list_request_logs(query).await
    }

    pub async fn get_request_log_detail(
        &self,
        request_log_id: Uuid,
    ) -> Result<RequestLogDetail, GatewayError> {
        self.request_logging
            .get_request_log_detail(request_log_id)
            .await
    }

    pub async fn refresh_pricing_catalog_if_stale(&self) -> Result<(), GatewayError> {
        self.pricing_catalog.refresh_if_stale().await
    }

    pub async fn dispatch_pending_budget_alert_deliveries(
        &self,
        limit: u32,
    ) -> Result<usize, GatewayError> {
        self.budget_alerts.dispatch_pending_deliveries(limit).await
    }

    pub async fn evaluate_budget_alert_after_user_budget_upsert(
        &self,
        budget: &UserBudgetRecord,
        current_spend: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        self.budget_alerts
            .evaluate_after_user_budget_upsert(budget, current_spend, occurred_at)
            .await
    }

    pub async fn evaluate_budget_alert_after_team_budget_upsert(
        &self,
        budget: &TeamBudgetRecord,
        current_spend: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        self.budget_alerts
            .evaluate_after_team_budget_upsert(budget, current_spend, occurred_at)
            .await
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

    pub async fn enforce_pre_provider_budget(
        &self,
        auth: &AuthenticatedApiKey,
        request_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        self.budget_guard
            .enforce_pre_provider_budget(auth, request_id, occurred_at)
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
    ) -> Result<RecordedChatUsage, GatewayError> {
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
        } else if let Err(error) = self.budget_alerts.evaluate_after_usage(auth, &record).await {
            warn!(
                request_id = %request_id,
                ownership_scope_key = %record.ownership_scope_key,
                error = %error,
                "budget alert evaluation failed after usage ledger insert"
            );
        }

        Ok(RecordedChatUsage {
            disposition,
            pricing_status: record.pricing_status,
            prompt_tokens: record.prompt_tokens,
            completion_tokens: record.completion_tokens,
            total_tokens: record.total_tokens,
            cost_usd: money_to_f64(record.computed_cost_usd),
        })
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

fn money_to_f64(value: Money4) -> Option<f64> {
    if value == Money4::ZERO {
        None
    } else {
        Some(value.as_scaled_i64() as f64 / Money4::SCALE as f64)
    }
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, ApiKeyRepository, AuthenticatedApiKey, BudgetRepository, GatewayModel,
        ModelRepository, ModelRoute, Money4, PricingCatalogRepository, ProviderCapabilities,
        ProviderConnection, ProviderRepository, RequestLogDetail, RequestLogPage,
        RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestLogRepository,
        RoutePlanner, StoreError, StoreHealth,
    };
    use serde_json::{Map, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::GatewayService;

    #[derive(Default)]
    struct TestRepo {
        model: Option<GatewayModel>,
        routes: Vec<ModelRoute>,
        providers: HashMap<String, ProviderConnection>,
        provider_lookups: AtomicUsize,
    }

    #[async_trait]
    impl ApiKeyRepository for TestRepo {
        async fn get_api_key_by_public_id(
            &self,
            _public_id: &str,
        ) -> Result<Option<gateway_core::ApiKeyRecord>, StoreError> {
            unreachable!("not used in resolve_request test")
        }

        async fn touch_api_key_last_used(&self, _api_key_id: Uuid) -> Result<(), StoreError> {
            unreachable!("not used in resolve_request test")
        }
    }

    #[async_trait]
    impl gateway_core::BudgetAlertRepository for TestRepo {}

    #[async_trait]
    impl BudgetRepository for TestRepo {
        async fn get_active_budget_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<gateway_core::UserBudgetRecord>, StoreError> {
            Ok(None)
        }

        async fn get_usage_ledger_by_request_and_scope(
            &self,
            _request_id: &str,
            _ownership_scope_key: &str,
        ) -> Result<Option<gateway_core::UsageLedgerRecord>, StoreError> {
            Ok(None)
        }

        async fn sum_usage_cost_for_user_in_window(
            &self,
            _user_id: Uuid,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(Money4::ZERO)
        }

        async fn insert_usage_ledger_if_absent(
            &self,
            _event: &gateway_core::UsageLedgerRecord,
        ) -> Result<bool, StoreError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl ModelRepository for TestRepo {
        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(self.model.clone().into_iter().collect())
        }

        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self
                .model
                .clone()
                .filter(|model| model.model_key == model_key))
        }

        async fn list_models_for_api_key(
            &self,
            _api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(self.model.clone().into_iter().collect())
        }

        async fn list_routes_for_model(
            &self,
            _model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            Ok(self.routes.clone())
        }
    }

    #[async_trait]
    impl gateway_core::IdentityRepository for TestRepo {
        async fn get_user_by_id(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<gateway_core::UserRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_by_id(
            &self,
            _team_id: Uuid,
        ) -> Result<Option<gateway_core::TeamRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_membership_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<gateway_core::TeamMembershipRecord>, StoreError> {
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
    impl PricingCatalogRepository for TestRepo {
        async fn get_pricing_catalog_cache(
            &self,
            _catalog_key: &str,
        ) -> Result<Option<gateway_core::PricingCatalogCacheRecord>, StoreError> {
            Ok(None)
        }

        async fn upsert_pricing_catalog_cache(
            &self,
            _cache: &gateway_core::PricingCatalogCacheRecord,
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

        async fn list_active_model_pricing(
            &self,
        ) -> Result<Vec<gateway_core::ModelPricingRecord>, StoreError> {
            Ok(Vec::new())
        }

        async fn insert_model_pricing(
            &self,
            _record: &gateway_core::ModelPricingRecord,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn close_model_pricing(
            &self,
            _model_pricing_id: Uuid,
            _effective_end_at: OffsetDateTime,
            _updated_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn resolve_model_pricing_at(
            &self,
            _pricing_provider_id: &str,
            _pricing_model_id: &str,
            _occurred_at: OffsetDateTime,
        ) -> Result<Option<gateway_core::ModelPricingRecord>, StoreError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl RequestLogRepository for TestRepo {
        async fn insert_request_log(
            &self,
            _log: &RequestLogRecord,
            _payload: Option<&RequestLogPayloadRecord>,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn list_request_logs(
            &self,
            _query: &RequestLogQuery,
        ) -> Result<RequestLogPage, StoreError> {
            unreachable!("not used in resolve_request test")
        }

        async fn get_request_log_detail(
            &self,
            _request_log_id: Uuid,
        ) -> Result<RequestLogDetail, StoreError> {
            unreachable!("not used in resolve_request test")
        }
    }

    #[async_trait]
    impl ProviderRepository for TestRepo {
        async fn get_provider_by_key(
            &self,
            provider_key: &str,
        ) -> Result<Option<ProviderConnection>, StoreError> {
            self.provider_lookups.fetch_add(1, Ordering::SeqCst);
            Ok(self.providers.get(provider_key).cloned())
        }
    }

    #[async_trait]
    impl StoreHealth for TestRepo {
        async fn ping(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    struct PassthroughPlanner;

    impl RoutePlanner for PassthroughPlanner {
        fn plan_routes(
            &self,
            routes: &[ModelRoute],
        ) -> Result<Vec<ModelRoute>, gateway_core::RouteError> {
            Ok(routes.to_vec())
        }
    }

    fn auth() -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "pk_test".to_string(),
            name: "test".to_string(),
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: None,
            owner_team_id: None,
        }
    }

    fn model() -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: None,
            tags: Vec::new(),
            rank: 10,
        }
    }

    fn route(model_id: Uuid, provider_key: &str) -> ModelRoute {
        ModelRoute {
            id: Uuid::new_v4(),
            model_id,
            provider_key: provider_key.to_string(),
            upstream_model: "gpt-5-mini".to_string(),
            priority: 10,
            weight: 1.0,
            enabled: true,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            capabilities: ProviderCapabilities::all_enabled(),
        }
    }

    fn provider(provider_key: &str) -> ProviderConnection {
        ProviderConnection {
            provider_key: provider_key.to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://openrouter.ai/api/v1",
                "display": {
                    "label": "OpenRouter",
                    "icon_key": "openrouter"
                }
            }),
            secrets: Some(json!({
                "api_key": "sk-live-raw",
                "service_account": {
                    "client_email": "svc@example.com",
                    "private_key": "-----BEGIN PRIVATE KEY-----",
                    "scopes": ["scope-a", "scope-b"]
                }
            })),
        }
    }

    #[tokio::test]
    async fn resolve_request_caches_provider_connections_for_viable_routes() {
        let model = model();
        let repo = Arc::new(TestRepo {
            model: Some(model.clone()),
            routes: vec![route(model.id, "router")],
            providers: HashMap::from([("router".to_string(), provider("router"))]),
            provider_lookups: AtomicUsize::new(0),
        });
        let service = GatewayService::new(repo.clone(), Arc::new(PassthroughPlanner));

        let resolved = service
            .resolve_request(&auth(), "fast")
            .await
            .expect("request should resolve");

        assert_eq!(resolved.routes.len(), 1);
        let provider = resolved
            .provider_connections
            .get("router")
            .expect("provider should be cached");
        assert_eq!(provider.provider_key, "router");
        assert_eq!(
            provider.config["display"]["icon_key"],
            serde_json::Value::String("openrouter".to_string())
        );
        let redacted = provider
            .redacted_secrets
            .as_ref()
            .expect("redacted secrets should be cached");
        assert_eq!(redacted["api_key"], "********");
        assert_eq!(redacted["service_account"]["client_email"], "********");
        assert_eq!(redacted["service_account"]["private_key"], "********");
        assert_eq!(redacted["service_account"]["scopes"][0], "********");
        assert_eq!(redacted["service_account"]["scopes"][1], "********");
        let serialized = redacted.to_string();
        assert!(!serialized.contains("sk-live-raw"));
        assert!(!serialized.contains("svc@example.com"));
        assert!(!serialized.contains("BEGIN PRIVATE KEY"));
        assert_eq!(repo.provider_lookups.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_request_excludes_missing_provider_from_cache_and_routes() {
        let model = model();
        let repo = Arc::new(TestRepo {
            model: Some(model.clone()),
            routes: vec![route(model.id, "missing")],
            providers: HashMap::new(),
            provider_lookups: AtomicUsize::new(0),
        });
        let service = GatewayService::new(repo.clone(), Arc::new(PassthroughPlanner));

        let error = service
            .resolve_request(&auth(), "fast")
            .await
            .expect_err("missing provider should leave no viable routes");

        assert!(matches!(
            error,
            gateway_core::GatewayError::Route(gateway_core::RouteError::NoRoutesAvailable(_))
        ));
        assert_eq!(repo.provider_lookups.load(Ordering::SeqCst), 1);
    }
}
