use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, BudgetAlertRepository, BudgetRecord, BudgetRepository,
    ChatCompletionsRequest, GatewayError, GatewayModel, IdentityRepository,
    McpToolInvocationDetail, McpToolInvocationPage, McpToolInvocationQuery,
    McpToolInvocationRepository, ModelRepository, ModelRoute, Money4, PricingCatalogRepository,
    PricingResolution, PricingUnpricedReason, ProviderRepository, RequestLogDetail, RequestLogPage,
    RequestLogPurgeResult, RequestLogQuery, RequestLogRecord, RequestLogRepository,
    RequestLogRetentionWindow, RequestTags, ResolvedModelPricing, ResponsesRequest, RouteError,
    RoutePlanner, RoutePricingOverride, StoreHealth, UsageLedgerRecord, UsagePricingStatus,
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
    effective_route_metadata::{EffectiveRouteMetadata, resolve_effective_route_metadata},
    mcp_invocation_logging::{McpInvocationLogInput, McpInvocationLogging},
};

#[derive(Debug, Clone)]
pub struct RecordedChatUsage {
    pub pricing_status: UsagePricingStatus,
    pub unpriced_reason: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub uncached_input_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug)]
struct RouteContextOverrideConflict {
    route_id: Uuid,
    provider_key: String,
    upstream_model: String,
    configured_context: i64,
    catalog_context: i64,
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
        self.pricing_catalog.refresh_if_stale_and_sync().await?;
        self.warn_on_route_context_override_conflicts().await
    }

    pub async fn refresh_pricing_catalog_now(&self) -> Result<(), GatewayError> {
        self.pricing_catalog.refresh_now_and_sync().await?;
        self.warn_on_route_context_override_conflicts().await
    }

    pub async fn validate_route_context_overrides(&self) -> Result<(), GatewayError> {
        let Some(conflict) = self
            .route_context_override_conflicts()
            .await?
            .into_iter()
            .next()
        else {
            return Ok(());
        };

        Err(GatewayError::InvalidRequest(format!(
            "route `{}` ({}/{}) context_window_tokens `{}` exceeds catalog context `{}`",
            conflict.route_id,
            conflict.provider_key,
            conflict.upstream_model,
            conflict.configured_context,
            conflict.catalog_context,
        )))
    }

    async fn warn_on_route_context_override_conflicts(&self) -> Result<(), GatewayError> {
        for conflict in self.route_context_override_conflicts().await? {
            warn!(
                route_id = %conflict.route_id,
                provider_key = %conflict.provider_key,
                upstream_model = %conflict.upstream_model,
                configured_context_window_tokens = conflict.configured_context,
                catalog_context_window_tokens = conflict.catalog_context,
                "configured route context exceeds the current catalog limit; using catalog limit"
            );
        }
        Ok(())
    }

    async fn route_context_override_conflicts(
        &self,
    ) -> Result<Vec<RouteContextOverrideConflict>, GatewayError> {
        let mut conflicts = Vec::new();
        for model in self.store.list_models().await? {
            for route in self.store.list_routes_for_model(model.id).await? {
                if !route.enabled || route.weight <= 0.0 {
                    continue;
                }
                let Some(configured) = route.context_window_tokens else {
                    continue;
                };
                let metadata = self
                    .resolve_route_metadata(&route, OffsetDateTime::now_utc())
                    .await?;
                let Some(catalog) = metadata.catalog_limits.context else {
                    continue;
                };
                if configured > catalog {
                    conflicts.push(RouteContextOverrideConflict {
                        route_id: route.id,
                        provider_key: route.provider_key,
                        upstream_model: route.upstream_model,
                        configured_context: configured,
                        catalog_context: catalog,
                    });
                }
            }
        }
        Ok(conflicts)
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
        if let Some(pricing) = &route.pricing_override {
            return Ok(PricingResolution::ConfiguredOverride {
                pricing: pricing.clone(),
            });
        }

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

    pub async fn resolve_route_metadata(
        &self,
        route: &ModelRoute,
        occurred_at: OffsetDateTime,
    ) -> Result<EffectiveRouteMetadata, GatewayError> {
        let provider = self.store.get_provider_by_key(&route.provider_key).await?;
        resolve_effective_route_metadata(self.store.as_ref(), provider.as_ref(), route, occurred_at)
            .await
    }

    pub async fn resolve_route_metadata_with_provider(
        &self,
        route: &ModelRoute,
        provider: Option<&ResolvedProviderConnection>,
        occurred_at: OffsetDateTime,
    ) -> Result<EffectiveRouteMetadata, GatewayError> {
        let provider = provider.map(|provider| gateway_core::ProviderConnection {
            provider_key: provider.provider_key.clone(),
            provider_type: provider.provider_type.clone(),
            config: provider.config.clone(),
            secrets: None,
        });
        resolve_effective_route_metadata(self.store.as_ref(), provider.as_ref(), route, occurred_at)
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
            model_route_id: Some(route.id),
            provider_key: route.provider_key.clone(),
            upstream_model: route.upstream_model.clone(),
            prompt_tokens: usage_summary.prompt_tokens,
            uncached_input_tokens: usage_summary.uncached_input_tokens,
            cache_read_tokens: usage_summary.cache_read_tokens,
            cache_write_tokens: usage_summary.cache_write_tokens,
            completion_tokens: usage_summary.completion_tokens,
            total_tokens: usage_summary.total_tokens,
            provider_usage,
            pricing_status: UsagePricingStatus::UsageMissing,
            unpriced_reason: usage_summary.cache_usage.reason(),
            pricing_row_id: None,
            pricing_provider_id: None,
            pricing_model_id: None,
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            computed_cost_usd: Money4::ZERO,
            occurred_at,
        };

        if usage_summary.has_usage() {
            match self.resolve_route_pricing(route, occurred_at).await? {
                PricingResolution::Exact { pricing } => {
                    apply_exact_pricing(&mut record, &pricing, &usage_summary.cache_usage)?
                }
                PricingResolution::ConfiguredOverride { pricing } => {
                    apply_configured_pricing(&mut record, &pricing, &usage_summary.cache_usage)?
                }
                PricingResolution::Unpriced { reason } => {
                    record.pricing_status = UsagePricingStatus::Unpriced;
                    let pricing_reason = unpriced_reason_string(&reason);
                    record.unpriced_reason = Some(match record.unpriced_reason.take() {
                        Some(normalization_reason) => {
                            format!("{normalization_reason};pricing_unavailable:{pricing_reason}")
                        }
                        None => pricing_reason,
                    });
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
            unpriced_reason: record.unpriced_reason.clone(),
            prompt_tokens: record.prompt_tokens,
            uncached_input_tokens: record.uncached_input_tokens,
            cache_read_tokens: record.cache_read_tokens,
            cache_write_tokens: record.cache_write_tokens,
            completion_tokens: record.completion_tokens,
            total_tokens: record.total_tokens,
            cost_usd: money_to_f64(record.computed_cost_usd),
        })
    }
}

#[derive(Debug, Clone, Default)]
struct UsageSummary {
    prompt_tokens: Option<i64>,
    uncached_input_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cache_usage: CacheUsageNormalization,
}

impl UsageSummary {
    fn has_usage(&self) -> bool {
        self.prompt_tokens.is_some()
            || self.completion_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

fn usage_summary_from_value(value: Option<&Value>) -> Result<UsageSummary, GatewayError> {
    let Some(usage) = value.and_then(Value::as_object) else {
        return Ok(UsageSummary::default());
    };

    let raw_prompt_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64);
    let completion_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64);
    let total_tokens = match usage.get("total_tokens").and_then(Value::as_i64) {
        some @ Some(_) => some,
        None => match (raw_prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt
                .checked_add(completion)
                .ok_or_else(|| GatewayError::Internal("token total overflow".to_string()))
                .map(Some)?,
            _ => None,
        },
    };

    let cache_usage = normalize_cache_tokens(usage, raw_prompt_tokens);
    let prompt_tokens = cache_usage
        .tokens()
        .map_or(raw_prompt_tokens, |tokens| Some(tokens.total_input_tokens));
    let total_tokens = if prompt_tokens != raw_prompt_tokens {
        match (prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt
                .checked_add(completion)
                .ok_or_else(|| GatewayError::Internal("token total overflow".to_string()))
                .map(Some)?,
            _ => total_tokens,
        }
    } else {
        total_tokens
    };
    let (uncached_input_tokens, cache_read_tokens, cache_write_tokens) =
        cache_usage.tokens().map_or((None, None, None), |tokens| {
            (
                Some(tokens.uncached_input_tokens),
                Some(tokens.cache_read_tokens),
                Some(tokens.cache_write_tokens),
            )
        });

    Ok(UsageSummary {
        prompt_tokens,
        uncached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        completion_tokens,
        total_tokens,
        cache_usage,
    })
}

#[derive(Debug, Clone, Default)]
enum CacheUsageNormalization {
    #[default]
    Unavailable,
    Valid(CacheTokenSummary),
    Invalid(String),
    Unsupported(String),
}

impl CacheUsageNormalization {
    fn tokens(&self) -> Option<&CacheTokenSummary> {
        match self {
            Self::Valid(tokens) => Some(tokens),
            Self::Unavailable | Self::Invalid(_) | Self::Unsupported(_) => None,
        }
    }

    fn reason(&self) -> Option<String> {
        match self {
            Self::Invalid(reason) => Some(format!("invalid_cache_token_usage:{reason}")),
            Self::Unsupported(reason) => Some(format!("unsupported_cache_token_usage:{reason}")),
            Self::Unavailable | Self::Valid(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CacheTokenSummary {
    total_input_tokens: i64,
    uncached_input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

fn normalize_cache_tokens(
    usage: &serde_json::Map<String, Value>,
    prompt_tokens: Option<i64>,
) -> CacheUsageNormalization {
    let provider_usage = usage.get("provider_usage").and_then(Value::as_object);
    let details = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .or_else(|| provider_usage.and_then(|raw| raw.get("input_tokens_details")))
        .or_else(|| provider_usage.and_then(|raw| raw.get("prompt_tokens_details")));
    if let Some(details) = details {
        let Some(details) = details.as_object() else {
            return CacheUsageNormalization::Invalid(
                "cache token details must be an object".to_string(),
            );
        };
        return normalize_inclusive_cache_tokens(details, prompt_tokens);
    }

    if let Some(provider_usage) = provider_usage {
        if provider_usage.contains_key("cached_tokens")
            || provider_usage.contains_key("cache_write_tokens")
        {
            return normalize_inclusive_cache_tokens(provider_usage, prompt_tokens);
        }
        if provider_usage.contains_key("cacheReadInputTokens")
            || provider_usage.contains_key("cacheWriteInputTokens")
        {
            if has_mixed_bedrock_ttl_cache_writes(provider_usage) {
                return CacheUsageNormalization::Unsupported(
                    "mixed_ttl_cache_write_classes".to_string(),
                );
            }
            return normalize_exclusive_cache_tokens(
                provider_usage,
                prompt_tokens,
                "cacheReadInputTokens",
                "cacheWriteInputTokens",
            );
        }
        if provider_usage.contains_key("cache_read_input_tokens")
            || provider_usage.contains_key("cache_creation_input_tokens")
        {
            if has_mixed_ttl_cache_writes(provider_usage) {
                return CacheUsageNormalization::Unsupported(
                    "mixed_ttl_cache_write_classes".to_string(),
                );
            }
            return normalize_exclusive_cache_tokens(
                provider_usage,
                prompt_tokens,
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
            );
        }
    }

    CacheUsageNormalization::Unavailable
}

fn normalize_inclusive_cache_tokens(
    fields: &serde_json::Map<String, Value>,
    prompt_tokens: Option<i64>,
) -> CacheUsageNormalization {
    let cache_read_tokens = match cache_token_field(fields, "cached_tokens") {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let cache_write_tokens = match cache_token_field(fields, "cache_write_tokens") {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let Some(prompt_tokens) = prompt_tokens else {
        return CacheUsageNormalization::Invalid(
            "cache token details require total input tokens".to_string(),
        );
    };
    if prompt_tokens < 0 {
        return CacheUsageNormalization::Invalid(
            "input token count cannot be negative".to_string(),
        );
    }
    let Some(cached_input_tokens) = cache_read_tokens.checked_add(cache_write_tokens) else {
        return CacheUsageNormalization::Invalid("cache token total overflow".to_string());
    };
    if cached_input_tokens > prompt_tokens {
        return CacheUsageNormalization::Invalid(
            "cache token buckets exceed total input tokens".to_string(),
        );
    }
    CacheUsageNormalization::Valid(CacheTokenSummary {
        total_input_tokens: prompt_tokens,
        uncached_input_tokens: prompt_tokens - cached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

fn normalize_exclusive_cache_tokens(
    fields: &serde_json::Map<String, Value>,
    uncached_input_tokens: Option<i64>,
    read_field: &str,
    write_field: &str,
) -> CacheUsageNormalization {
    let Some(uncached_input_tokens) = uncached_input_tokens else {
        return CacheUsageNormalization::Invalid(
            "provider cache counters require uncached input tokens".to_string(),
        );
    };
    if uncached_input_tokens < 0 {
        return CacheUsageNormalization::Invalid(
            "input token count cannot be negative".to_string(),
        );
    }
    let cache_read_tokens = match cache_token_field(fields, read_field) {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let cache_write_tokens = match cache_token_field(fields, write_field) {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let Some(total_input_tokens) = uncached_input_tokens
        .checked_add(cache_read_tokens)
        .and_then(|total| total.checked_add(cache_write_tokens))
    else {
        return CacheUsageNormalization::Invalid("cache token total overflow".to_string());
    };
    CacheUsageNormalization::Valid(CacheTokenSummary {
        total_input_tokens,
        uncached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

fn has_mixed_ttl_cache_writes(fields: &serde_json::Map<String, Value>) -> bool {
    let Some(classes) = fields.get("cache_creation").and_then(Value::as_object) else {
        return false;
    };
    classes
        .values()
        .filter_map(Value::as_i64)
        .filter(|tokens| *tokens > 0)
        .count()
        > 1
}

fn has_mixed_bedrock_ttl_cache_writes(fields: &serde_json::Map<String, Value>) -> bool {
    let Some(details) = fields.get("cacheDetails").and_then(Value::as_array) else {
        return false;
    };
    let mut positive_ttls = Vec::new();
    for detail in details.iter().filter_map(Value::as_object) {
        let tokens = detail.get("inputTokens").and_then(Value::as_i64);
        let ttl = detail.get("ttl").and_then(Value::as_str);
        if tokens.is_some_and(|tokens| tokens > 0)
            && let Some(ttl) = ttl
            && !positive_ttls.contains(&ttl)
        {
            positive_ttls.push(ttl);
        }
    }
    positive_ttls.len() > 1
}

fn cache_token_field(details: &serde_json::Map<String, Value>, field: &str) -> Result<i64, String> {
    let Some(value) = details.get(field) else {
        return Ok(0);
    };
    let Some(tokens) = value.as_i64() else {
        return Err(format!("cache token field `{field}` must be an integer"));
    };
    if tokens < 0 {
        return Err(format!("cache token field `{field}` cannot be negative"));
    }
    Ok(tokens)
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
    cache_usage: &CacheUsageNormalization,
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
    record.cache_read_cost_per_million_tokens = pricing.cache_read_cost_per_million_tokens;
    record.cache_write_cost_per_million_tokens = pricing.cache_write_cost_per_million_tokens;

    apply_token_rates(
        record,
        pricing.input_cost_per_million_tokens,
        pricing.output_cost_per_million_tokens,
        cache_usage,
    )
}

fn apply_configured_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &RoutePricingOverride,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    record.pricing_source = Some("configured_override".to_string());
    record.input_cost_per_million_tokens = Some(pricing.input_cost_per_million_tokens);
    record.output_cost_per_million_tokens = Some(pricing.output_cost_per_million_tokens);
    record.cache_read_cost_per_million_tokens = pricing.cache_read_cost_per_million_tokens;
    record.cache_write_cost_per_million_tokens = pricing.cache_write_cost_per_million_tokens;

    apply_token_rates(
        record,
        Some(pricing.input_cost_per_million_tokens),
        Some(pricing.output_cost_per_million_tokens),
        cache_usage,
    )
}

fn apply_token_rates(
    record: &mut UsageLedgerRecord,
    input_rate: Option<Money4>,
    output_rate: Option<Money4>,
    cache_usage: &CacheUsageNormalization,
) -> Result<(), GatewayError> {
    if matches!(cache_usage, CacheUsageNormalization::Unsupported(_)) {
        record.pricing_status = UsagePricingStatus::Unpriced;
        return Ok(());
    }
    if matches!(cache_usage, CacheUsageNormalization::Invalid(_)) {
        if record.prompt_tokens.unwrap_or_default() > 0 && input_rate.is_none() {
            record.pricing_status = UsagePricingStatus::Unpriced;
            return Ok(());
        }
        if record.completion_tokens.unwrap_or_default() > 0 && output_rate.is_none() {
            record.pricing_status = UsagePricingStatus::Unpriced;
            return Ok(());
        }
        record.pricing_status = UsagePricingStatus::LegacyEstimated;
        record.computed_cost_usd = compute_usage_cost(
            record.prompt_tokens,
            input_rate,
            record.completion_tokens,
            output_rate,
        )?;
        return Ok(());
    }
    let input_tokens_requiring_standard_rate = record
        .uncached_input_tokens
        .unwrap_or_else(|| record.prompt_tokens.unwrap_or_default());
    if input_tokens_requiring_standard_rate > 0 && input_rate.is_none() {
        record.pricing_status = UsagePricingStatus::Unpriced;
        record.unpriced_reason = Some("missing_input_rate".to_string());
        return Ok(());
    }
    if record.completion_tokens.unwrap_or_default() > 0 && output_rate.is_none() {
        record.pricing_status = UsagePricingStatus::Unpriced;
        record.unpriced_reason = Some("missing_output_rate".to_string());
        return Ok(());
    }
    if record.cache_read_tokens.unwrap_or_default() > 0
        && record.cache_read_cost_per_million_tokens.is_none()
    {
        record.pricing_status = UsagePricingStatus::Unpriced;
        record.unpriced_reason = Some("missing_cache_read_rate".to_string());
        return Ok(());
    }
    if record.cache_write_tokens.unwrap_or_default() > 0
        && record.cache_write_cost_per_million_tokens.is_none()
    {
        record.pricing_status = UsagePricingStatus::Unpriced;
        record.unpriced_reason = Some("missing_cache_write_rate".to_string());
        return Ok(());
    }

    record.pricing_status = UsagePricingStatus::Priced;
    record.computed_cost_usd = if record.uncached_input_tokens.is_some() {
        compute_cache_aware_usage_cost(record, input_rate, output_rate)?
    } else {
        compute_usage_cost(
            record.prompt_tokens,
            input_rate,
            record.completion_tokens,
            output_rate,
        )?
    };
    Ok(())
}

fn compute_cache_aware_usage_cost(
    record: &UsageLedgerRecord,
    input_rate: Option<Money4>,
    output_rate: Option<Money4>,
) -> Result<Money4, GatewayError> {
    let components = [
        (record.uncached_input_tokens, input_rate),
        (
            record.cache_read_tokens,
            record.cache_read_cost_per_million_tokens,
        ),
        (
            record.cache_write_tokens,
            record.cache_write_cost_per_million_tokens,
        ),
        (record.completion_tokens, output_rate),
    ];
    components
        .into_iter()
        .try_fold(Money4::ZERO, |total, (tokens, rate)| {
            let component = match (tokens, rate) {
                (Some(tokens), Some(rate)) => scaled_cost_for_tokens(tokens, rate)?,
                _ => Money4::ZERO,
            };
            total
                .checked_add(component)
                .ok_or_else(|| GatewayError::Internal("usage cost overflow".to_string()))
        })
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
        BudgetAlertRepository, BudgetCadence, BudgetRecord, BudgetRepository, BudgetScope,
        BudgetSettings, BudgetSource, GatewayError, GatewayModel, IdentityRepository,
        McpToolInvocationDetail, McpToolInvocationPage, McpToolInvocationPayloadRecord,
        McpToolInvocationQuery, McpToolInvocationRecord, McpToolInvocationRepository,
        ModelPricingRecord, ModelPricingSyncChanges, ModelRepository, ModelRoute, Money4,
        PricingCatalogCacheRecord, PricingCatalogRepository, PricingLimits, PricingModalities,
        PricingProvenance, ProviderCapabilities, ProviderConnection, ProviderRepository,
        RequestLogDetail, RequestLogPage, RequestLogPayloadRecord, RequestLogPurgeResult,
        RequestLogQuery, RequestLogRecord, RequestLogRepository, RouteError, RoutePlanner,
        RoutePricingOverride, StoreError, StoreHealth, TeamMembershipRecord, TeamRecord,
        UsageLedgerRecord, UsagePricingStatus, UserRecord,
    };
    use serde_json::{Map, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{CacheUsageNormalization, GatewayService, usage_summary_from_value};

    #[derive(Clone, Default)]
    struct UsageAccountingRepo {
        events: Arc<Mutex<Vec<UsageLedgerRecord>>>,
        models: Vec<GatewayModel>,
        routes: Vec<ModelRoute>,
        budget: Option<BudgetRecord>,
        provider: Option<ProviderConnection>,
        pricing: Option<ModelPricingRecord>,
        pricing_lookup_fails: bool,
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
            scope: &BudgetScope,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            Ok(self
                .budget
                .as_ref()
                .filter(|budget| &budget.scope == scope)
                .cloned())
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
            scope: &BudgetScope,
            window_start: OffsetDateTime,
            window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            let mut total = Money4::ZERO;
            for event in self
                .events
                .lock()
                .expect("events lock")
                .iter()
                .filter(|event| {
                    let in_scope = match scope {
                        BudgetScope::User { user_id } => event.user_id == Some(*user_id),
                        _ => false,
                    };
                    in_scope
                        && event.occurred_at >= window_start
                        && event.occurred_at < window_end
                        && event.pricing_status.counts_toward_spend()
                })
            {
                total = total.checked_add(event.computed_cost_usd).ok_or_else(|| {
                    StoreError::Unexpected("usage accounting test cost overflow".to_string())
                })?;
            }
            Ok(total)
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
            Ok(self.models.clone())
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
            model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            Ok(self
                .routes
                .iter()
                .filter(|route| route.model_id == model_id)
                .cloned()
                .collect())
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

        async fn compare_and_swap_pricing_catalog_cache(
            &self,
            _cache: &PricingCatalogCacheRecord,
            _expected_fetched_at: Option<OffsetDateTime>,
        ) -> Result<bool, StoreError> {
            Ok(true)
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

        async fn apply_model_pricing_sync(
            &self,
            _changes: &ModelPricingSyncChanges,
            _effective_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn resolve_model_pricing_at(
            &self,
            _pricing_provider_id: &str,
            _pricing_model_id: &str,
            _occurred_at: OffsetDateTime,
        ) -> Result<Option<ModelPricingRecord>, StoreError> {
            if self.pricing_lookup_fails {
                return Err(StoreError::Unavailable(
                    "pricing catalog unavailable".to_string(),
                ));
            }
            Ok(self.pricing.clone())
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
            provider_key: &str,
        ) -> Result<Option<ProviderConnection>, StoreError> {
            Ok(self
                .provider
                .as_ref()
                .filter(|provider| provider.provider_key == provider_key)
                .cloned())
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
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            capabilities: ProviderCapabilities::all_enabled(),
            compatibility: Default::default(),
        }
    }

    fn openai_provider(provider_key: &str) -> ProviderConnection {
        ProviderConnection {
            provider_key: provider_key.to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({"pricing_provider_id": "openai"}),
            secrets: None,
        }
    }

    fn pricing_record(context_window_tokens: Option<i64>) -> ModelPricingRecord {
        let now = OffsetDateTime::UNIX_EPOCH;
        ModelPricingRecord {
            model_pricing_id: Uuid::new_v4(),
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: "gpt-5".to_string(),
            display_name: "GPT-5".to_string(),
            input_cost_per_million_tokens: Some(Money4::from_scaled(10_000)),
            output_cost_per_million_tokens: Some(Money4::from_scaled(20_000)),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            input_audio_cost_per_million_tokens: None,
            output_audio_cost_per_million_tokens: None,
            release_date: "2026-01-01".to_string(),
            last_updated: "2026-01-01".to_string(),
            effective_start_at: now,
            effective_end_at: None,
            limits: PricingLimits {
                context: context_window_tokens,
                input: None,
                output: None,
            },
            modalities: PricingModalities {
                input: vec!["text".to_string()],
                output: vec!["text".to_string()],
            },
            provenance: PricingProvenance {
                source: "test".to_string(),
                etag: Some("etag-1".to_string()),
                fetched_at: now,
            },
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn startup_rejects_context_override_above_known_catalog_limit() {
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.upstream_model = "gpt-5".to_string();
        route.context_window_tokens = Some(128_000);
        let provider = openai_provider(&route.provider_key);
        let repo = Arc::new(UsageAccountingRepo {
            models: vec![model],
            routes: vec![route],
            provider: Some(provider),
            pricing: Some(pricing_record(Some(64_000))),
            ..Default::default()
        });
        let service = GatewayService::new(repo, Arc::new(PassThroughPlanner));

        let error = service
            .validate_route_context_overrides()
            .await
            .expect_err("startup validation must reject conflicting cap");

        assert!(
            error
                .to_string()
                .contains("context_window_tokens `128000` exceeds catalog context `64000`"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn startup_context_validation_skips_non_selectable_routes() {
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut disabled_route = vertex_embedding_route(model_id);
        disabled_route.upstream_model = "gpt-5".to_string();
        disabled_route.context_window_tokens = Some(128_000);
        disabled_route.enabled = false;
        let mut zero_weight_route = disabled_route.clone();
        zero_weight_route.id = Uuid::new_v4();
        zero_weight_route.enabled = true;
        zero_weight_route.weight = 0.0;
        let provider = openai_provider(&disabled_route.provider_key);
        let service = GatewayService::new(
            Arc::new(UsageAccountingRepo {
                models: vec![model],
                routes: vec![disabled_route, zero_weight_route],
                provider: Some(provider),
                pricing: Some(pricing_record(Some(64_000))),
                ..Default::default()
            }),
            Arc::new(PassThroughPlanner),
        );

        service
            .validate_route_context_overrides()
            .await
            .expect("disabled and zero-weight routes cannot block startup");
    }

    #[tokio::test]
    async fn configured_pricing_does_not_bypass_context_validation() {
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.upstream_model = "gpt-5".to_string();
        route.context_window_tokens = Some(128_000);
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(10_000),
            output_cost_per_million_tokens: Money4::from_scaled(20_000),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
        });
        let provider = openai_provider(&route.provider_key);
        let service = GatewayService::new(
            Arc::new(UsageAccountingRepo {
                models: vec![model.clone()],
                routes: vec![route.clone()],
                provider: Some(provider.clone()),
                pricing: Some(pricing_record(Some(256_000))),
                ..Default::default()
            }),
            Arc::new(PassThroughPlanner),
        );

        service
            .validate_route_context_overrides()
            .await
            .expect("configured cap below catalog context should be valid");
        assert!(matches!(
            service
                .resolve_route_pricing(&route, OffsetDateTime::now_utc())
                .await
                .expect("configured pricing"),
            gateway_core::PricingResolution::ConfiguredOverride { .. }
        ));

        let conflicting_service = GatewayService::new(
            Arc::new(UsageAccountingRepo {
                models: vec![model],
                routes: vec![route],
                provider: Some(provider),
                pricing: Some(pricing_record(Some(64_000))),
                ..Default::default()
            }),
            Arc::new(PassThroughPlanner),
        );
        let error = conflicting_service
            .validate_route_context_overrides()
            .await
            .expect_err("configured pricing must not bypass an oversized context cap");
        assert!(
            error
                .to_string()
                .contains("context_window_tokens `128000` exceeds catalog context `64000`"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn startup_context_validation_uses_catalog_limits_for_modified_pricing_routes() {
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.upstream_model = "gpt-5".to_string();
        route.context_window_tokens = Some(128_000);
        route
            .extra_body
            .insert("service_tier".to_string(), json!("priority"));
        let provider = openai_provider(&route.provider_key);
        let repo = Arc::new(UsageAccountingRepo {
            models: vec![model],
            routes: vec![route.clone()],
            provider: Some(provider),
            pricing: Some(pricing_record(Some(64_000))),
            ..Default::default()
        });
        let service = GatewayService::new(repo, Arc::new(PassThroughPlanner));

        let error = service
            .validate_route_context_overrides()
            .await
            .expect_err("billing modifiers must not hide catalog context");
        assert!(
            error
                .to_string()
                .contains("context_window_tokens `128000` exceeds catalog context `64000`"),
            "unexpected error: {error}"
        );

        let pricing = service
            .resolve_route_pricing(&route, OffsetDateTime::now_utc())
            .await
            .expect("pricing resolution");
        assert!(matches!(
            pricing,
            gateway_core::PricingResolution::Unpriced {
                reason: gateway_core::PricingUnpricedReason::UnsupportedBillingModifier(_)
            }
        ));

        let metadata = service
            .resolve_route_metadata(&route, OffsetDateTime::now_utc())
            .await
            .expect("effective metadata");
        assert_eq!(metadata.limits.context, Some(64_000));
        assert_eq!(metadata.pricing, None);
        assert_eq!(metadata.pricing_source, None);
    }

    #[tokio::test]
    async fn startup_context_validation_uses_catalog_limits_for_regional_vertex_routes() {
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.upstream_model = "anthropic/claude-sonnet-4-6".to_string();
        route.context_window_tokens = Some(128_000);
        let provider = ProviderConnection {
            provider_key: route.provider_key.clone(),
            provider_type: "gcp_vertex".to_string(),
            config: json!({"location": "us-central1"}),
            secrets: None,
        };
        let repo = Arc::new(UsageAccountingRepo {
            models: vec![model],
            routes: vec![route.clone()],
            provider: Some(provider),
            pricing: Some(pricing_record(Some(64_000))),
            ..Default::default()
        });
        let service = GatewayService::new(repo, Arc::new(PassThroughPlanner));

        let error = service
            .validate_route_context_overrides()
            .await
            .expect_err("regional pricing limits must not hide catalog context");
        assert!(
            error
                .to_string()
                .contains("context_window_tokens `128000` exceeds catalog context `64000`"),
            "unexpected error: {error}"
        );

        let pricing = service
            .resolve_route_pricing(&route, OffsetDateTime::now_utc())
            .await
            .expect("pricing resolution");
        assert!(matches!(
            pricing,
            gateway_core::PricingResolution::Unpriced {
                reason: gateway_core::PricingUnpricedReason::UnsupportedVertexLocation(_)
            }
        ));

        let metadata = service
            .resolve_route_metadata(&route, OffsetDateTime::now_utc())
            .await
            .expect("effective metadata");
        assert_eq!(metadata.limits.context, Some(64_000));
        assert_eq!(metadata.pricing, None);
        assert_eq!(metadata.pricing_source, None);
    }

    #[tokio::test]
    async fn startup_accepts_context_override_when_catalog_context_is_unknown() {
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.upstream_model = "gpt-5".to_string();
        route.context_window_tokens = Some(128_000);
        let provider = openai_provider(&route.provider_key);
        let repo = Arc::new(UsageAccountingRepo {
            models: vec![model],
            routes: vec![route],
            provider: Some(provider),
            pricing: Some(pricing_record(None)),
            ..Default::default()
        });
        let service = GatewayService::new(repo, Arc::new(PassThroughPlanner));

        service
            .validate_route_context_overrides()
            .await
            .expect("unknown catalog context should accept configured cap");
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

    #[tokio::test]
    async fn configured_route_pricing_is_charged_and_snapshotted_when_catalog_is_unavailable() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.upstream_model = "gpt-5".to_string();
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(10_000),
            output_cost_per_million_tokens: Money4::from_scaled(20_000),
            cache_read_cost_per_million_tokens: Some(Money4::from_scaled(1_000)),
            cache_write_cost_per_million_tokens: None,
        });
        let provider = openai_provider(&route.provider_key);
        let mut conflicting_catalog_pricing = pricing_record(Some(256_000));
        conflicting_catalog_pricing.input_cost_per_million_tokens =
            Some(Money4::from_scaled(90_000));
        conflicting_catalog_pricing.output_cost_per_million_tokens =
            Some(Money4::from_scaled(300_000));
        let occurred_at = OffsetDateTime::now_utc();
        let scope = BudgetScope::User {
            user_id: auth.owner_user_id.expect("user owner"),
        };
        let budget = BudgetRecord {
            budget_id: Uuid::new_v4(),
            scope_key: scope.scope_key(),
            scope,
            settings: BudgetSettings {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(20_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            source: BudgetSource::manual(),
            is_active: true,
            created_at: occurred_at,
            updated_at: occurred_at,
        };
        let repo = Arc::new(UsageAccountingRepo {
            budget: Some(budget),
            provider: Some(provider),
            pricing: Some(conflicting_catalog_pricing),
            pricing_lookup_fails: true,
            ..Default::default()
        });
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));

        let recorded = service
            .record_chat_usage(
                &auth,
                &model,
                &route,
                "req_configured_pricing",
                Some(json!({
                    "prompt_tokens": 1_000_000,
                    "completion_tokens": 500_000,
                    "total_tokens": 1_500_000
                })),
                occurred_at,
            )
            .await
            .expect("usage should be recorded");

        assert_eq!(recorded.pricing_status, UsagePricingStatus::Priced);
        assert_eq!(recorded.cost_usd, Some(2.0));

        {
            let events = repo.events.lock().expect("events lock");
            let event = events.first().expect("usage event");
            assert_eq!(event.model_route_id, Some(route.id));
            assert_eq!(event.pricing_source.as_deref(), Some("configured_override"));
            assert_eq!(event.pricing_row_id, None);
            assert_eq!(event.pricing_provider_id, None);
            assert_eq!(event.pricing_model_id, None);
            assert_eq!(event.pricing_source_etag, None);
            assert_eq!(event.pricing_source_fetched_at, None);
            assert_eq!(
                event.input_cost_per_million_tokens,
                Some(Money4::from_scaled(10_000))
            );
            assert_eq!(
                event.output_cost_per_million_tokens,
                Some(Money4::from_scaled(20_000))
            );
            assert_eq!(
                event.cache_read_cost_per_million_tokens,
                Some(Money4::from_scaled(1_000))
            );
            assert_eq!(event.cache_write_cost_per_million_tokens, None);
            assert_eq!(event.computed_cost_usd, Money4::from_scaled(20_000));
        }

        let error = service
            .enforce_pre_provider_budget(
                &auth,
                "req_after_configured_pricing",
                Some(model.id),
                Some(&route.upstream_model),
                occurred_at,
            )
            .await
            .expect_err("configured spend should consume the hard budget");
        assert!(matches!(
            error,
            GatewayError::BudgetExceeded {
                projected_cost_usd,
                limit_usd,
                ..
            } if projected_cost_usd == Money4::from_scaled(20_000)
                && limit_usd == Money4::from_scaled(20_000)
        ));
    }

    #[tokio::test]
    async fn configured_route_pricing_does_not_require_a_catalog_match() {
        let model_id = Uuid::new_v4();
        let mut route = vertex_embedding_route(model_id);
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(12_500),
            output_cost_per_million_tokens: Money4::from_scaled(50_000),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: Some(Money4::from_scaled(15_000)),
        });
        let service = GatewayService::new(
            Arc::new(UsageAccountingRepo::default()),
            Arc::new(PassThroughPlanner),
        );

        let resolution = service
            .resolve_route_pricing(&route, OffsetDateTime::now_utc())
            .await
            .expect("configured pricing should resolve without a catalog row");

        assert!(matches!(
            resolution,
            gateway_core::PricingResolution::ConfiguredOverride { pricing }
                if pricing == route.pricing_override.expect("pricing override")
        ));
    }

    #[tokio::test]
    async fn responses_cache_tokens_are_disjoint_and_priced_by_bucket() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(10_000),
            output_cost_per_million_tokens: Money4::from_scaled(20_000),
            cache_read_cost_per_million_tokens: Some(Money4::from_scaled(1_000)),
            cache_write_cost_per_million_tokens: Some(Money4::from_scaled(12_500)),
        });
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));

        let recorded = service
            .record_chat_usage(
                &auth,
                &model,
                &route,
                "req_responses_cache_usage",
                Some(json!({
                    "input_tokens": 3_618,
                    "output_tokens": 303,
                    "total_tokens": 3_921,
                    "input_tokens_details": {
                        "cached_tokens": 3_340,
                        "cache_write_tokens": 276
                    },
                    "output_tokens_details": {"reasoning_tokens": 95}
                })),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("cache usage should be recorded");

        assert_eq!(recorded.pricing_status, UsagePricingStatus::Priced);
        assert_eq!(recorded.uncached_input_tokens, Some(2));
        assert_eq!(recorded.cache_read_tokens, Some(3_340));
        assert_eq!(recorded.cache_write_tokens, Some(276));
        assert_eq!(recorded.cost_usd, Some(0.0012));

        let events = repo.events.lock().expect("events lock");
        let event = events.first().expect("usage event");
        assert_eq!(
            event.provider_usage["input_tokens_details"]["cached_tokens"],
            3_340
        );
        assert_eq!(event.computed_cost_usd, Money4::from_scaled(12));
    }

    #[test]
    fn responses_cache_tokens_normalize_from_nested_raw_provider_usage() {
        let summary = usage_summary_from_value(Some(&json!({
            "input_tokens": 1_200,
            "output_tokens": 40,
            "provider_usage": {
                "input_tokens_details": {
                    "cached_tokens": 900,
                    "cache_write_tokens": 200
                }
            }
        })))
        .expect("nested raw provider usage should normalize");

        assert_eq!(summary.uncached_input_tokens, Some(100));
        assert_eq!(summary.cache_read_tokens, Some(900));
        assert_eq!(summary.cache_write_tokens, Some(200));
        assert!(matches!(
            summary.cache_usage,
            CacheUsageNormalization::Valid(_)
        ));
    }

    #[test]
    fn responses_cache_tokens_normalize_from_root_raw_provider_usage() {
        let summary = usage_summary_from_value(Some(&json!({
            "input_tokens": 1_200,
            "output_tokens": 40,
            "provider_usage": {
                "cached_tokens": 900,
                "cache_write_tokens": 200
            }
        })))
        .expect("root raw provider usage should normalize");

        assert_eq!(summary.prompt_tokens, Some(1_200));
        assert_eq!(summary.uncached_input_tokens, Some(100));
        assert_eq!(summary.cache_read_tokens, Some(900));
        assert_eq!(summary.cache_write_tokens, Some(200));
    }

    #[test]
    fn bedrock_converse_cache_tokens_add_to_exclusive_input_total() {
        let summary = usage_summary_from_value(Some(&json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "provider_usage": {
                "inputTokens": 100,
                "cacheReadInputTokens": 900,
                "cacheWriteInputTokens": 200
            }
        })))
        .expect("Bedrock cache usage should normalize");

        assert_eq!(summary.prompt_tokens, Some(1_200));
        assert_eq!(summary.total_tokens, Some(1_220));
        assert_eq!(summary.uncached_input_tokens, Some(100));
        assert_eq!(summary.cache_read_tokens, Some(900));
        assert_eq!(summary.cache_write_tokens, Some(200));
    }

    #[test]
    fn mixed_anthropic_ttl_write_classes_remain_unavailable() {
        let summary = usage_summary_from_value(Some(&json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "provider_usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 900,
                "cache_creation_input_tokens": 200,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": 80,
                    "ephemeral_1h_input_tokens": 120
                }
            }
        })))
        .expect("mixed TTL usage should remain recorded");

        assert_eq!(summary.uncached_input_tokens, None);
        assert!(matches!(
            summary.cache_usage,
            CacheUsageNormalization::Unsupported(ref reason)
                if reason == "mixed_ttl_cache_write_classes"
        ));
    }

    #[tokio::test]
    async fn positive_cache_tokens_without_required_rate_are_unpriced() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(10_000),
            output_cost_per_million_tokens: Money4::from_scaled(20_000),
            cache_read_cost_per_million_tokens: Some(Money4::from_scaled(1_000)),
            cache_write_cost_per_million_tokens: None,
        });
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));

        let recorded = service
            .record_chat_usage(
                &auth,
                &model,
                &route,
                "req_missing_cache_write_rate",
                Some(json!({
                    "input_tokens": 2_000,
                    "output_tokens": 10,
                    "input_tokens_details": {
                        "cached_tokens": 1_000,
                        "cache_write_tokens": 500
                    }
                })),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("unpriced cache usage should still be recorded");

        assert_eq!(recorded.pricing_status, UsagePricingStatus::Unpriced);
        let events = repo.events.lock().expect("events lock");
        assert_eq!(
            events[0].unpriced_reason.as_deref(),
            Some("missing_cache_write_rate")
        );
        assert_eq!(events[0].computed_cost_usd, Money4::ZERO);
    }

    #[tokio::test]
    async fn inconsistent_cache_counters_use_legacy_estimate_with_diagnostic() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(10_000),
            output_cost_per_million_tokens: Money4::from_scaled(20_000),
            cache_read_cost_per_million_tokens: Some(Money4::from_scaled(1_000)),
            cache_write_cost_per_million_tokens: Some(Money4::from_scaled(12_500)),
        });
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));

        let recorded = service
            .record_chat_usage(
                &auth,
                &model,
                &route,
                "req_invalid_cache_usage",
                Some(json!({
                    "input_tokens": 100_000,
                    "output_tokens": 10_000,
                    "input_tokens_details": {
                        "cached_tokens": 90_000,
                        "cache_write_tokens": 20_000
                    }
                })),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("invalid cache usage should still be recorded");

        assert_eq!(recorded.pricing_status, UsagePricingStatus::LegacyEstimated);
        let events = repo.events.lock().expect("events lock");
        assert!(
            events[0]
                .unpriced_reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("invalid_cache_token_usage:"))
        );
        assert_eq!(events[0].uncached_input_tokens, None);
        assert_eq!(events[0].computed_cost_usd, Money4::from_scaled(1_200));
    }

    #[tokio::test]
    async fn invalid_cache_counters_preserve_pricing_resolution_failure() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let route = vertex_embedding_route(model_id);
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));

        let recorded = service
            .record_chat_usage(
                &auth,
                &model,
                &route,
                "req_invalid_cache_usage_without_pricing",
                Some(json!({
                    "input_tokens": 100,
                    "output_tokens": 10,
                    "input_tokens_details": {
                        "cached_tokens": 90,
                        "cache_write_tokens": 20
                    }
                })),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("invalid cache usage should still be recorded");

        assert_eq!(recorded.pricing_status, UsagePricingStatus::Unpriced);
        let reason = recorded
            .unpriced_reason
            .as_deref()
            .expect("combined unpriced reason");
        assert!(reason.starts_with("invalid_cache_token_usage:"));
        assert!(reason.contains(";pricing_unavailable:"));
    }
}
