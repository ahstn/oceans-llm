use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, BudgetAlertRepository, BudgetRecord, BudgetRepository,
    ChatCompletionsRequest, GatewayError, GatewayModel, IdentityRepository,
    McpToolInvocationDetail, McpToolInvocationPage, McpToolInvocationQuery,
    McpToolInvocationRepository, ModelRepository, ModelRoute, Money4, PricingCatalogRepository,
    PricingResolution, PricingUnpricedReason, ProviderRepository, RequestLogDetail, RequestLogPage,
    RequestLogPurgeResult, RequestLogQuery, RequestLogRecord, RequestLogRepository,
    RequestLogRetentionWindow, RequestTags, ResolvedModelPricing, ResponsesRequest, RouteError,
    RoutePlanner, StoreHealth, UsageLedgerRecord, UsagePricingStatus,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::{
    Authenticator, LoggedRequest, ModelAccess, ModelResolver, PricingCatalog, RequestLogContext,
    RequestLogIconMetadata, RequestLogPayloadPolicy, RequestLogging, ResolvedGatewayRequest,
    ResolvedProviderConnection, StreamLogResultInput, StreamResponseCollector,
    budget_alerts::{BudgetAlertSender, BudgetAlertService, SinkBudgetAlertSender},
    budget_guard::BudgetGuard,
    budget_scopes::usage_ownership_scope_key,
    mcp_invocation_logging::{McpInvocationLogInput, McpInvocationLogging},
};

#[derive(Debug, Clone)]
pub struct RecordedChatUsage {
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
    mcp_invocation_logging: McpInvocationLogging<S>,
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
        + McpToolInvocationRepository
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
            RequestLogging::new_with_payload_policy(store.clone(), payload_policy.clone());
        let mcp_invocation_logging = McpInvocationLogging::new_with_payload_policy(
            store.clone(),
            crate::McpInvocationPayloadPolicy::from_request_log_policy(payload_policy),
        );

        Self {
            store,
            authenticator,
            budget_alerts,
            budget_guard,
            model_access,
            model_resolver,
            pricing_catalog,
            request_logging,
            mcp_invocation_logging,
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

    pub async fn authenticate_bearer_token(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedApiKey, GatewayError> {
        self.authenticator
            .authenticate_bearer_token(bearer_token)
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
    ) -> RequestLogContext {
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
    pub fn begin_responses_request_log(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &ResponsesRequest,
        request_headers: &std::collections::BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext {
        self.request_logging.begin_responses_request(
            request_id,
            requested_model_key,
            resolved_model_key,
            request,
            request_headers,
            request_tags,
        )
    }

    #[must_use]
    pub fn begin_embeddings_request_log(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &gateway_core::EmbeddingsRequest,
        request_headers: &std::collections::BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext {
        self.request_logging.begin_embeddings_request(
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

    #[allow(clippy::too_many_arguments)]
    pub async fn log_non_stream_success(
        &self,
        auth: &AuthenticatedApiKey,
        context: &RequestLogContext,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        latency_ms: i64,
        invoked_tool_count: i64,
        response_body: &Value,
        attempts: Vec<gateway_core::RequestAttemptRecord>,
    ) -> Result<LoggedRequest, GatewayError> {
        self.request_logging
            .log_non_stream_success(
                auth,
                context,
                provider_key,
                icon_metadata,
                latency_ms,
                invoked_tool_count,
                response_body,
                attempts,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_non_stream_failure(
        &self,
        auth: &AuthenticatedApiKey,
        context: &RequestLogContext,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        latency_ms: i64,
        gateway_error: &GatewayError,
        attempts: Vec<gateway_core::RequestAttemptRecord>,
    ) -> Result<LoggedRequest, GatewayError> {
        self.request_logging
            .log_non_stream_failure(
                auth,
                context,
                provider_key,
                icon_metadata,
                latency_ms,
                gateway_error,
                attempts,
            )
            .await
    }

    pub async fn log_stream_result(
        &self,
        auth: &AuthenticatedApiKey,
        context: &RequestLogContext,
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

    pub async fn log_mcp_tool_invocation(
        &self,
        auth: &AuthenticatedApiKey,
        input: McpInvocationLogInput,
    ) -> Result<crate::LoggedMcpToolInvocation, GatewayError> {
        self.mcp_invocation_logging
            .log_invocation(auth, input)
            .await
    }

    pub async fn list_mcp_tool_invocations(
        &self,
        query: &McpToolInvocationQuery,
    ) -> Result<McpToolInvocationPage, GatewayError> {
        self.mcp_invocation_logging.list_invocations(query).await
    }

    pub async fn get_mcp_tool_invocation_detail(
        &self,
        mcp_tool_invocation_id: Uuid,
    ) -> Result<McpToolInvocationDetail, GatewayError> {
        self.mcp_invocation_logging
            .get_invocation_detail(mcp_tool_invocation_id)
            .await
    }

    pub async fn purge_request_logs(
        &self,
        retention_window: RequestLogRetentionWindow,
        dry_run: bool,
    ) -> Result<RequestLogPurgeResult, GatewayError> {
        self.request_logging
            .purge_request_logs(retention_window, dry_run)
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

    pub async fn evaluate_budget_alert_after_budget_upsert(
        &self,
        budget: &BudgetRecord,
        current_spend: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        self.budget_alerts
            .evaluate_after_budget_upsert(budget, current_spend, occurred_at)
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
        model_id: Option<Uuid>,
        upstream_model: Option<&str>,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        self.budget_guard
            .enforce_pre_provider_budget(auth, request_id, model_id, upstream_model, occurred_at)
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
        let ownership_scope_key = usage_ownership_scope_key(auth)?;
        let usage_summary = usage_summary_from_value(provider_usage.as_ref())?;
        let provider_usage = provider_usage.unwrap_or_else(|| json!({}));

        let mut record = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key,
            api_key_id: auth.id,
            user_id: auth.owner_user_id,
            team_id: auth.owner_team_id,
            service_account_id: auth.owner_service_account_id,
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

        self.budget_guard
            .enforce_and_record_usage(auth, &record)
            .await?;
        if let Err(error) = self.budget_alerts.evaluate_after_usage(auth, &record).await {
            warn!(
                request_id = %request_id,
                ownership_scope_key = %record.ownership_scope_key,
                error = %error,
                "budget alert evaluation failed after usage ledger insert"
            );
        }

        Ok(RecordedChatUsage {
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

    let prompt_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64);
    let completion_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64);
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
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, ApiKeyRecord, ApiKeyRepository, AuthenticatedApiKey,
        BudgetAlertRepository, BudgetRecord, BudgetRepository, BudgetScope, BudgetSettings,
        BudgetSource, GatewayModel, IdentityRepository, McpToolInvocationDetail,
        McpToolInvocationPage, McpToolInvocationPayloadRecord, McpToolInvocationQuery,
        McpToolInvocationRecord, McpToolInvocationRepository, ModelRepository, ModelRoute, Money4,
        PricingCatalogCacheRecord, PricingCatalogRepository, ProviderCapabilities,
        ProviderConnection, ProviderRepository, RequestLogDetail, RequestLogPage,
        RequestLogPayloadRecord, RequestLogPurgeResult, RequestLogQuery, RequestLogRecord,
        RequestLogRepository, RouteError, RoutePlanner, StoreError, StoreHealth,
        TeamMembershipRecord, TeamRecord, UsageLedgerRecord, UsagePricingStatus, UserRecord,
    };
    use serde_json::{Map, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::GatewayService;

    #[derive(Clone, Default)]
    struct UsageAccountingRepo {
        events: Arc<Mutex<Vec<UsageLedgerRecord>>>,
    }

    struct PassThroughPlanner;

    impl RoutePlanner for PassThroughPlanner {
        fn plan_routes(&self, routes: &[ModelRoute]) -> Result<Vec<ModelRoute>, RouteError> {
            Ok(routes.to_vec())
        }
    }

    #[async_trait]
    impl ApiKeyRepository for UsageAccountingRepo {
        async fn get_api_key_by_public_id(
            &self,
            _public_id: &str,
        ) -> Result<Option<ApiKeyRecord>, StoreError> {
            Ok(None)
        }

        async fn touch_api_key_last_used(&self, _api_key_id: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BudgetAlertRepository for UsageAccountingRepo {}

    #[async_trait]
    impl BudgetRepository for UsageAccountingRepo {
        async fn get_active_budget_by_scope(
            &self,
            _scope: &BudgetScope,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            Ok(None)
        }

        async fn get_latest_budget_by_scope(
            &self,
            _scope: &BudgetScope,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            Ok(None)
        }

        async fn upsert_active_budget(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _updated_at: OffsetDateTime,
        ) -> Result<BudgetRecord, StoreError> {
            Err(StoreError::Unexpected(
                "upsert_active_budget is not used by usage accounting tests".to_string(),
            ))
        }

        async fn upsert_active_budget_with_source(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _source: &BudgetSource,
            _updated_at: OffsetDateTime,
        ) -> Result<BudgetRecord, StoreError> {
            Err(StoreError::Unexpected(
                "upsert_active_budget_with_source is not used by usage accounting tests"
                    .to_string(),
            ))
        }

        async fn upsert_active_budget_with_source_guard(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _source: &BudgetSource,
            _expected_current_source: Option<&BudgetSource>,
            _updated_at: OffsetDateTime,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            Ok(None)
        }

        async fn deactivate_active_budget(
            &self,
            _scope: &BudgetScope,
            _updated_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            Ok(false)
        }

        async fn deactivate_active_budget_by_source(
            &self,
            _scope: &BudgetScope,
            _source: &BudgetSource,
            _updated_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            Ok(false)
        }

        async fn get_usage_ledger_by_request_and_scope(
            &self,
            request_id: &str,
            ownership_scope_key: &str,
        ) -> Result<Option<UsageLedgerRecord>, StoreError> {
            Ok(self
                .events
                .lock()
                .expect("events lock")
                .iter()
                .find(|event| {
                    event.request_id == request_id
                        && event.ownership_scope_key == ownership_scope_key
                })
                .cloned())
        }

        async fn sum_usage_cost_for_budget_scope_in_window(
            &self,
            _scope: &BudgetScope,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(Money4::ZERO)
        }

        async fn insert_usage_ledger_if_absent(
            &self,
            event: &UsageLedgerRecord,
        ) -> Result<bool, StoreError> {
            let mut events = self.events.lock().expect("events lock");
            if events.iter().any(|existing| {
                existing.request_id == event.request_id
                    && existing.ownership_scope_key == event.ownership_scope_key
            }) {
                return Ok(false);
            }
            events.push(event.clone());
            Ok(true)
        }
    }

    #[async_trait]
    impl ModelRepository for UsageAccountingRepo {
        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(Vec::new())
        }

        async fn get_model_by_key(
            &self,
            _model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(None)
        }

        async fn list_models_for_api_key(
            &self,
            _api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_model_allowlists_for_models(
            &self,
            _model_ids: &[Uuid],
        ) -> Result<std::collections::HashMap<Uuid, gateway_core::ModelAllowlistPolicy>, StoreError>
        {
            Ok(std::collections::HashMap::new())
        }

        async fn list_routes_for_model(
            &self,
            _model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl IdentityRepository for UsageAccountingRepo {
        async fn get_user_by_id(&self, _user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_by_id(&self, _team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(None)
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
    impl PricingCatalogRepository for UsageAccountingRepo {
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
    impl RequestLogRepository for UsageAccountingRepo {
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
            Ok(RequestLogPage {
                items: Vec::new(),
                page: 1,
                page_size: 50,
                total: 0,
            })
        }

        async fn get_request_log_detail(
            &self,
            _request_log_id: Uuid,
        ) -> Result<RequestLogDetail, StoreError> {
            Err(StoreError::NotFound("request log not found".to_string()))
        }

        async fn purge_request_logs_older_than(
            &self,
            cutoff: OffsetDateTime,
            dry_run: bool,
        ) -> Result<RequestLogPurgeResult, StoreError> {
            Ok(RequestLogPurgeResult {
                cutoff,
                dry_run,
                matched_count: 0,
                deleted_count: 0,
            })
        }
    }

    #[async_trait]
    impl McpToolInvocationRepository for UsageAccountingRepo {
        async fn insert_mcp_tool_invocation(
            &self,
            _invocation: &McpToolInvocationRecord,
            _payload: Option<&McpToolInvocationPayloadRecord>,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn list_mcp_tool_invocations(
            &self,
            _query: &McpToolInvocationQuery,
        ) -> Result<McpToolInvocationPage, StoreError> {
            Ok(McpToolInvocationPage {
                items: Vec::new(),
                page: 1,
                page_size: 50,
                total: 0,
            })
        }

        async fn get_mcp_tool_invocation_detail(
            &self,
            _mcp_tool_invocation_id: Uuid,
        ) -> Result<McpToolInvocationDetail, StoreError> {
            Err(StoreError::NotFound("mcp invocation not found".to_string()))
        }
    }

    #[async_trait]
    impl ProviderRepository for UsageAccountingRepo {
        async fn get_provider_by_key(
            &self,
            _provider_key: &str,
        ) -> Result<Option<ProviderConnection>, StoreError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl StoreHealth for UsageAccountingRepo {
        async fn ping(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    fn auth() -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(Uuid::new_v4()),
            owner_team_id: None,
            owner_service_account_id: None,
        }
    }

    fn model(model_id: Uuid) -> GatewayModel {
        GatewayModel {
            id: model_id,
            model_key: "embeddings".to_string(),
            alias_target_model_key: None,
            description: None,
            tags: Vec::new(),
            rank: 0,
        }
    }

    fn vertex_embedding_route(model_id: Uuid) -> ModelRoute {
        ModelRoute {
            id: Uuid::new_v4(),
            model_id,
            provider_key: "vertex-prod".to_string(),
            upstream_model: "google/gemini-embedding-001".to_string(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            capabilities: ProviderCapabilities::all_enabled(),
            compatibility: Default::default(),
        }
    }

    #[tokio::test]
    async fn record_chat_usage_keeps_embedding_usage_missing_without_token_counts() {
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let route = vertex_embedding_route(model_id);

        let recorded = service
            .record_chat_usage(
                &auth,
                &model,
                &route,
                "req_missing_embedding_usage",
                Some(json!({"statistics": {"billable_character_count": 999}})),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("usage should be recorded");

        assert_eq!(recorded.pricing_status, UsagePricingStatus::UsageMissing);
        assert_eq!(recorded.prompt_tokens, None);
        assert_eq!(recorded.completion_tokens, None);
        assert_eq!(recorded.total_tokens, None);
        assert_eq!(recorded.cost_usd, None);

        let events = repo.events.lock().expect("events lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].pricing_status, UsagePricingStatus::UsageMissing);
        assert_eq!(events[0].prompt_tokens, None);
        assert_eq!(events[0].completion_tokens, None);
        assert_eq!(events[0].total_tokens, None);
        assert_eq!(
            events[0].provider_usage["statistics"]["billable_character_count"],
            999
        );
    }
}
