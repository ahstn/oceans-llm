use agent_session_analysis::AnalysisPolicy;
use std::sync::Arc;
use tokio::sync::Semaphore;

use gateway_core::{
    AgentAnalysisDesiredVersions, AgentSessionAnalysisRepository, AuthenticatedApiKey,
    BudgetAlertRepository, BudgetRecord, BudgetRepository, ChatCompletionsRequest, GatewayError,
    GatewayModel, IdentityRepository, McpToolInvocationDetail, McpToolInvocationPage,
    McpToolInvocationQuery, McpToolInvocationRepository, ModelRepository, ModelRoute, Money4,
    NormalizedUsageAccounting, PricingCatalogRepository, PricingResolution, PricingUnpricedReason,
    ProviderRepository, RequestLogDetail, RequestLogPage, RequestLogPurgeResult, RequestLogQuery,
    RequestLogRecord, RequestLogRepository, RequestLogRetentionWindow, RequestTags,
    ResolvedModelPricing, ResponsesRequest, RouteError, RoutePlanner, RoutePricingOverride,
    StoreHealth, UsageCostAuthority, UsageLedgerRecord, UsagePricingStatus,
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use tracing::warn;
use uuid::Uuid;

use crate::{
    Authenticator, LoggedRequest, ModelAccess, ModelResolver, PricingCatalog, RequestLogContext,
    RequestLogIconMetadata, RequestLogPayloadPolicy, RequestLogging, ResolvedGatewayRequest,
    ResolvedProviderConnection, StreamFailureSummary, StreamLogResultInput,
    StreamResponseCollector,
    agent_analysis::{
        PassiveRequestRecord, REPORT_RETENTION, desired_versions_for_policy,
        finalize_idle_sessions, process_next_analysis, record_prepared_passive_request,
        session_boundary_group_key,
    },
    budget_alerts::{BudgetAlertSender, BudgetAlertService, SinkBudgetAlertSender},
    budget_guard::BudgetGuard,
    budget_scopes::usage_ownership_scope_key,
    effective_route_metadata::{EffectiveRouteMetadata, resolve_effective_route_metadata},
    mcp_invocation_logging::{McpInvocationLogInput, McpInvocationLogging},
    usage_normalization::{NormalizedTokenUsage, normalize_token_usage_best_effort},
};

pub const NORMALIZED_PRICING_POLICY_VERSION: &str = "cache-aware-v1-2026-07-21";
const AGENT_ANALYSIS_MAX_IN_FLIGHT_INGESTIONS: usize = 64;
const AGENT_ANALYSIS_QUEUE_RETENTION: Duration = Duration::days(7);

#[derive(Debug, Clone)]
pub struct RecordedUsage {
    pub pricing_status: UsagePricingStatus,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UsageCostPolicy {
    #[default]
    ShadowLegacy,
    Normalized,
}

#[derive(Debug)]
struct RouteContextOverrideConflict {
    route_id: Uuid,
    provider_key: String,
    upstream_model: String,
    configured_context: i64,
    catalog_context: i64,
}

struct PassiveRequestOutcome<'a> {
    request_log_id: Option<Uuid>,
    response_body: Option<&'a Value>,
    terminal_success: Option<bool>,
    response_payload_truncated: bool,
    completed_at: OffsetDateTime,
}

fn stream_terminal_success(
    collector: &mut StreamResponseCollector,
    explicit_failure: Option<&StreamFailureSummary>,
) -> Option<bool> {
    collector.finish();
    let failure = explicit_failure.or_else(|| collector.failure());
    if failure.is_none() {
        Some(true)
    } else if collector.usage().is_some() {
        None
    } else {
        Some(false)
    }
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
    usage_cost_policy: UsageCostPolicy,
    agent_analysis_enabled: bool,
    agent_analysis_ingestion_limit: Arc<Semaphore>,
    agent_analysis_report_retention: Duration,
    agent_analysis_queue_retention: Duration,
    agent_analysis_policy: AnalysisPolicy,
    agent_analysis_desired_versions: AgentAnalysisDesiredVersions,
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

        let agent_analysis_policy = AnalysisPolicy::default();
        let agent_analysis_desired_versions = desired_versions_for_policy(&agent_analysis_policy);
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
            usage_cost_policy: UsageCostPolicy::default(),
            agent_analysis_enabled: false,
            agent_analysis_ingestion_limit: Arc::new(Semaphore::new(
                AGENT_ANALYSIS_MAX_IN_FLIGHT_INGESTIONS,
            )),
            agent_analysis_report_retention: REPORT_RETENTION,
            agent_analysis_queue_retention: AGENT_ANALYSIS_QUEUE_RETENTION,
            agent_analysis_policy,
            agent_analysis_desired_versions,
        }
    }

    #[must_use]
    pub fn with_usage_cost_policy(mut self, usage_cost_policy: UsageCostPolicy) -> Self {
        self.usage_cost_policy = usage_cost_policy;
        self
    }

    #[must_use]
    pub fn with_agent_analysis_enabled(mut self, enabled: bool) -> Self {
        self.agent_analysis_enabled = enabled;
        self
    }

    #[must_use]
    pub fn with_agent_analysis_retention(
        mut self,
        report_retention: Duration,
        queue_retention: Duration,
    ) -> Self {
        self.agent_analysis_report_retention = report_retention;
        self.agent_analysis_queue_retention = queue_retention;
        self
    }

    #[must_use]
    pub fn with_agent_analysis_policy(mut self, policy: AnalysisPolicy) -> Self {
        self.agent_analysis_desired_versions = desired_versions_for_policy(&policy);
        self.agent_analysis_policy = policy;
        self
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
    ) -> Result<LoggedRequest, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        let completed_at = OffsetDateTime::now_utc();
        let logged = self
            .request_logging
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
            .await?;
        if logged.wrote {
            self.record_passive_request(
                auth,
                context,
                PassiveRequestOutcome {
                    request_log_id: Some(logged.request_log_id),
                    response_body: logged.analysis_response.as_ref(),
                    terminal_success: Some(true),
                    response_payload_truncated: logged.response_payload_truncated,
                    completed_at,
                },
            )
            .await;
        }
        Ok(logged)
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
    ) -> Result<LoggedRequest, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        let completed_at = OffsetDateTime::now_utc();
        let logged = self
            .request_logging
            .log_non_stream_failure(
                auth,
                context,
                provider_key,
                icon_metadata,
                latency_ms,
                gateway_error,
                attempts,
            )
            .await?;
        if logged.wrote {
            self.record_passive_request(
                auth,
                context,
                PassiveRequestOutcome {
                    request_log_id: Some(logged.request_log_id),
                    response_body: None,
                    terminal_success: Some(false),
                    response_payload_truncated: logged.response_payload_truncated,
                    completed_at,
                },
            )
            .await;
        }
        Ok(logged)
    }

    pub async fn log_stream_result(
        &self,
        auth: &AuthenticatedApiKey,
        context: &RequestLogContext,
        stream_result: StreamLogResultInput,
    ) -> Result<LoggedRequest, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        let mut stream_result = stream_result;
        let terminal_success =
            stream_terminal_success(&mut stream_result.collector, stream_result.failure.as_ref());
        let completed_at = OffsetDateTime::now_utc();
        let logged = self
            .request_logging
            .log_stream_result(auth, context, stream_result)
            .await?;
        if logged.wrote {
            self.record_passive_request(
                auth,
                context,
                PassiveRequestOutcome {
                    request_log_id: Some(logged.request_log_id),
                    response_body: logged.analysis_response.as_ref(),
                    terminal_success,
                    response_payload_truncated: logged.response_payload_truncated,
                    completed_at,
                },
            )
            .await;
        }
        Ok(logged)
    }

    async fn record_passive_request(
        &self,
        auth: &AuthenticatedApiKey,
        context: &RequestLogContext,
        outcome: PassiveRequestOutcome<'_>,
    ) where
        S: AgentSessionAnalysisRepository,
    {
        if !self.agent_analysis_enabled {
            return;
        }
        let Ok(permit) = Arc::clone(&self.agent_analysis_ingestion_limit).try_acquire_owned()
        else {
            warn!(
                request_id = context.request_id,
                "passive agent request correlation skipped because the ingestion limit is full"
            );
            return;
        };
        let store = Arc::clone(&self.store);
        let auth = auth.clone();
        let request_id = context.request_id.clone();
        let requested_model_key = context.requested_model_key.clone();
        let request_tags = context.request_tags.clone();
        let request_tags_value = match serde_json::to_value(&request_tags) {
            Ok(value) => value,
            Err(error) => {
                warn!(
                    request_id = context.request_id,
                    error = %error,
                    "passive agent request correlation skipped because request tags were invalid"
                );
                return;
            }
        };
        let operation = context.operation;
        let harness_key = context.agent_harness_key.clone();
        let harness_label = context.agent_harness_label.clone();
        let metadata = context.analysis_metadata.clone();
        let analysis_payload_permitted = context.analysis_payload_permitted;
        let occurred_at = context.started_at;
        let payload_truncated = outcome.response_payload_truncated;
        let response_body = outcome.response_body.cloned();
        let request_log_id = outcome.request_log_id;
        let completed_at = outcome.completed_at;
        let terminal_success = outcome.terminal_success;
        let desired_versions = self.agent_analysis_desired_versions.clone();
        tokio::spawn(async move {
            let boundary_group_key = session_boundary_group_key(&request_tags);
            let input = PassiveRequestRecord {
                auth: &auth,
                request_id: &request_id,
                request_log_id,
                harness_key: &harness_key,
                harness_label: &harness_label,
                metadata: &metadata,
                response_body: analysis_payload_permitted
                    .then_some(response_body.as_ref())
                    .flatten(),
                occurred_at,
                completed_at,
                terminal_success,
                payload_truncated,
                requested_model_key: &requested_model_key,
                operation,
                request_tags: request_tags_value,
                boundary_group_key: &boundary_group_key,
            }
            .prepare();
            if let Err(error) =
                record_prepared_passive_request(store.as_ref(), input, &desired_versions).await
            {
                warn!(
                    request_id,
                    error = %error,
                    "passive agent request correlation failed"
                );
            }
            drop(permit);
        });
    }
    pub async fn finalize_idle_agent_sessions(
        &self,
        now: OffsetDateTime,
    ) -> Result<u64, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        finalize_idle_sessions(
            self.store.as_ref(),
            now,
            &self.agent_analysis_desired_versions,
        )
        .await
    }

    pub async fn process_next_agent_analysis(
        &self,
        lease_owner: &str,
        now: OffsetDateTime,
    ) -> Result<bool, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        process_next_analysis(
            self.store.as_ref(),
            lease_owner,
            now,
            self.agent_analysis_report_retention,
            &self.agent_analysis_policy,
        )
        .await
    }

    pub async fn purge_expired_agent_analysis(
        &self,
        now: OffsetDateTime,
    ) -> Result<u64, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        self.store
            .purge_expired_agent_analysis(now, now - self.agent_analysis_queue_retention)
            .await
            .map_err(Into::into)
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
    ) -> Result<RequestLogPurgeResult, GatewayError>
    where
        S: AgentSessionAnalysisRepository,
    {
        let result = self
            .request_logging
            .purge_request_logs(retention_window, dry_run)
            .await?;
        if !dry_run {
            self.store
                .purge_agent_analysis_before(result.cutoff)
                .await?;
        }
        Ok(result)
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

    pub async fn record_usage(
        &self,
        auth: &AuthenticatedApiKey,
        model: &GatewayModel,
        route: &ModelRoute,
        request_id: &str,
        provider_usage: Option<Value>,
        occurred_at: OffsetDateTime,
    ) -> Result<RecordedUsage, GatewayError> {
        let ownership_scope_key = usage_ownership_scope_key(auth)?;
        let normalization = normalize_token_usage_best_effort(provider_usage.as_ref());
        if let Some(error) = normalization.error.as_ref() {
            warn!(
                request_id,
                provider_key = %route.provider_key,
                error = %error,
                "provider usage normalization failed; preserving raw usage"
            );
        }
        let normalized_usage = &normalization.usage;
        let normalized_accounting =
            normalized_accounting(normalized_usage, normalization.error.as_ref());
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
            prompt_tokens: normalized_usage.legacy_prompt_tokens(),
            completion_tokens: normalized_usage.legacy_completion_tokens(),
            total_tokens: normalized_usage.legacy_total_tokens(),
            provider_usage,
            normalized_usage: Some(normalized_accounting),
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
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            computed_cost_usd: Money4::ZERO,
            occurred_at,
        };

        if normalized_usage.has_usage() {
            match self.resolve_route_pricing(route, occurred_at).await? {
                PricingResolution::Exact { pricing } => {
                    apply_exact_pricing(&mut record, &pricing, self.usage_cost_policy)?
                }
                PricingResolution::ConfiguredOverride { pricing } => {
                    apply_configured_pricing(&mut record, &pricing, self.usage_cost_policy)?
                }
                PricingResolution::Unpriced { reason } => {
                    let reason = unpriced_reason_string(&reason);
                    mark_usage_unpriced(&mut record, &reason, self.usage_cost_policy);
                    warn!(
                        request_id = %request_id,
                        provider_key = %route.provider_key,
                        model_key = %model.model_key,
                        reason = %reason,
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

        Ok(RecordedUsage {
            pricing_status: record.pricing_status,
            prompt_tokens: record.prompt_tokens,
            completion_tokens: record.completion_tokens,
            total_tokens: record.total_tokens,
            cost_usd: money_to_f64(record.computed_cost_usd),
        })
    }
}

fn money_to_f64(value: Money4) -> Option<f64> {
    if value == Money4::ZERO {
        None
    } else {
        Some(value.as_scaled_i64() as f64 / Money4::SCALE as f64)
    }
}

fn normalized_accounting(
    usage: &NormalizedTokenUsage,
    normalization_error: Option<&crate::UsageNormalizationError>,
) -> NormalizedUsageAccounting {
    NormalizedUsageAccounting {
        fresh_input_tokens: usage.fresh_input_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        cache_creation_5m_tokens: usage.cache_creation_5m_tokens,
        cache_creation_30m_tokens: usage.cache_creation_30m_tokens,
        cache_creation_1h_tokens: usage.cache_creation_1h_tokens,
        output_tokens: usage.output_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        provider_total_tokens: usage.provider_total_tokens,
        output_includes_reasoning: usage.semantics.output_includes_reasoning,
        finish_reason: usage.finish_reason.clone(),
        incomplete_reason: usage.incomplete_reason.clone(),
        semantics_version: usage.semantics.version.clone(),
        semantics: json!({
            "token_usage": &usage.semantics,
            "coverage": &usage.coverage,
        }),
        normalization_error: normalization_error.map(ToString::to_string),
        fresh_input_cost_usd: None,
        cache_read_cost_usd: None,
        cache_creation_cost_usd: None,
        output_cost_usd: None,
        reasoning_cost_usd: None,
        uncached_input_cost_usd: None,
        legacy_cost_usd: Money4::ZERO,
        normalized_cost_usd: None,
        normalized_pricing_status: if usage.has_usage() {
            UsagePricingStatus::Unpriced
        } else {
            UsagePricingStatus::UsageMissing
        },
        normalized_unpriced_reason: None,
        pricing_policy_version: NORMALIZED_PRICING_POLICY_VERSION.to_string(),
        authoritative_cost: UsageCostAuthority::Legacy,
        discrepancy_usd: None,
        discrepancy_reason: None,
    }
}

fn mark_usage_unpriced(
    record: &mut UsageLedgerRecord,
    reason: &str,
    usage_cost_policy: UsageCostPolicy,
) {
    record.pricing_status = UsagePricingStatus::Unpriced;
    record.unpriced_reason = Some(reason.to_string());
    if let Some(accounting) = record.normalized_usage.as_mut() {
        accounting.normalized_pricing_status = UsagePricingStatus::Unpriced;
        accounting.normalized_unpriced_reason = Some(reason.to_string());
        accounting.authoritative_cost = fallback_authority(usage_cost_policy);
    }
}

const fn fallback_authority(usage_cost_policy: UsageCostPolicy) -> UsageCostAuthority {
    match usage_cost_policy {
        UsageCostPolicy::ShadowLegacy => UsageCostAuthority::Legacy,
        UsageCostPolicy::Normalized => UsageCostAuthority::LegacyFallback,
    }
}

#[derive(Debug, Clone)]
struct NormalizedBucketCost {
    cost: Option<Money4>,
    error: Option<String>,
}

fn normalized_bucket_cost(
    tokens: Option<i64>,
    rate: Option<Money4>,
    missing_rate_error: &str,
) -> Result<NormalizedBucketCost, GatewayError> {
    let Some(tokens) = tokens else {
        return Ok(NormalizedBucketCost {
            cost: None,
            error: None,
        });
    };
    if tokens == 0 {
        return Ok(NormalizedBucketCost {
            cost: Some(Money4::ZERO),
            error: None,
        });
    }
    let Some(rate) = rate else {
        return Ok(NormalizedBucketCost {
            cost: None,
            error: Some(missing_rate_error.to_string()),
        });
    };
    Ok(NormalizedBucketCost {
        cost: Some(scaled_cost_for_tokens(tokens, rate)?),
        error: None,
    })
}

fn apply_exact_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &ResolvedModelPricing,
    usage_cost_policy: UsageCostPolicy,
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
        pricing.cache_read_cost_per_million_tokens,
        pricing.cache_write_cost_per_million_tokens,
        usage_cost_policy,
    )
}

fn apply_configured_pricing(
    record: &mut UsageLedgerRecord,
    pricing: &RoutePricingOverride,
    usage_cost_policy: UsageCostPolicy,
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
        pricing.cache_read_cost_per_million_tokens,
        pricing.cache_write_cost_per_million_tokens,
        usage_cost_policy,
    )
}

fn populate_uncached_input_cost(
    accounting: &mut NormalizedUsageAccounting,
    input_rate: Option<Money4>,
) -> Result<(), GatewayError> {
    let buckets_non_overlapping =
        accounting.semantics["token_usage"]["input_buckets_non_overlapping"].as_bool()
            == Some(true);
    let (cost, limitation) = if !buckets_non_overlapping {
        (None, Some("input_bucket_overlap_unknown"))
    } else if let (Some(fresh), Some(cache_read), Some(cache_creation)) = (
        accounting.fresh_input_tokens,
        accounting.cache_read_tokens,
        accounting.cache_creation_tokens,
    ) {
        if let Some(rate) = input_rate {
            let tokens = fresh
                .checked_add(cache_read)
                .and_then(|tokens| tokens.checked_add(cache_creation))
                .ok_or_else(|| GatewayError::Internal("usage cost overflow".to_string()))?;
            (Some(scaled_cost_for_tokens(tokens, rate)?), None)
        } else {
            (None, Some("input_rate_unavailable"))
        }
    } else {
        (None, Some("input_bucket_unavailable"))
    };

    accounting.uncached_input_cost_usd = cost;
    accounting.semantics["uncached_input_cost"] = json!({
        "method": "fresh_plus_cache_read_plus_cache_creation_at_normal_input_rate",
        "available": cost.is_some(),
        "limitation": limitation,
    });
    Ok(())
}

fn apply_token_rates(
    record: &mut UsageLedgerRecord,
    input_rate: Option<Money4>,
    output_rate: Option<Money4>,
    cache_read_rate: Option<Money4>,
    cache_write_rate: Option<Money4>,
    usage_cost_policy: UsageCostPolicy,
) -> Result<(), GatewayError> {
    if let Some(accounting) = record.normalized_usage.as_mut() {
        populate_uncached_input_cost(accounting, input_rate)?;
    }
    if record.prompt_tokens.unwrap_or_default() > 0 && input_rate.is_none() {
        mark_usage_unpriced(record, "missing_input_rate", usage_cost_policy);
        return Ok(());
    }
    if record.completion_tokens.unwrap_or_default() > 0 && output_rate.is_none() {
        mark_usage_unpriced(record, "missing_output_rate", usage_cost_policy);
        return Ok(());
    }

    let legacy_cost = compute_usage_cost(
        record.prompt_tokens,
        input_rate,
        record.completion_tokens,
        output_rate,
    )?;
    let Some(accounting) = record.normalized_usage.as_mut() else {
        record.pricing_status = UsagePricingStatus::Priced;
        record.computed_cost_usd = legacy_cost;
        return Ok(());
    };
    accounting.legacy_cost_usd = legacy_cost;

    if accounting.normalization_error.is_some() {
        accounting.normalized_pricing_status = UsagePricingStatus::Unpriced;
        accounting.normalized_unpriced_reason = Some("usage_normalization_failed".to_string());
        accounting.authoritative_cost = fallback_authority(usage_cost_policy);
        record.pricing_status = UsagePricingStatus::Priced;
        record.computed_cost_usd = legacy_cost;
        return Ok(());
    }

    let component_costs = [
        normalized_bucket_cost(
            accounting.fresh_input_tokens,
            input_rate,
            "missing_input_rate",
        )?,
        normalized_bucket_cost(
            accounting.cache_read_tokens,
            cache_read_rate,
            "missing_cache_read_rate",
        )?,
        normalized_bucket_cost(
            accounting.cache_creation_tokens,
            cache_write_rate,
            "missing_cache_write_rate",
        )?,
        normalized_bucket_cost(accounting.output_tokens, output_rate, "missing_output_rate")?,
        normalized_bucket_cost(
            accounting.reasoning_tokens,
            output_rate,
            "missing_reasoning_rate",
        )?,
    ];
    if let Some(reason) = component_costs
        .iter()
        .find_map(|component| component.error.as_deref())
    {
        accounting.normalized_pricing_status = UsagePricingStatus::Unpriced;
        accounting.normalized_unpriced_reason = Some(reason.to_string());
        accounting.authoritative_cost = fallback_authority(usage_cost_policy);
        record.pricing_status = UsagePricingStatus::Priced;
        record.computed_cost_usd = legacy_cost;
        return Ok(());
    }

    accounting.fresh_input_cost_usd = component_costs[0].cost;
    accounting.cache_read_cost_usd = component_costs[1].cost;
    accounting.cache_creation_cost_usd = component_costs[2].cost;
    accounting.output_cost_usd = component_costs[3].cost;
    accounting.reasoning_cost_usd = component_costs[4].cost;
    let output_includes_reasoning =
        accounting.semantics["token_usage"]["output_includes_reasoning"]
            .as_bool()
            .unwrap_or(true);
    let normalized_cost = component_costs
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 4 || !output_includes_reasoning)
        .filter_map(|(_, component)| component.cost)
        .try_fold(Money4::ZERO, |total, cost| total.checked_add(cost))
        .ok_or_else(|| GatewayError::Internal("usage cost overflow".to_string()))?;
    accounting.normalized_cost_usd = Some(normalized_cost);
    accounting.normalized_pricing_status = UsagePricingStatus::Priced;
    accounting.normalized_unpriced_reason = None;
    accounting.authoritative_cost = match usage_cost_policy {
        UsageCostPolicy::ShadowLegacy => UsageCostAuthority::Legacy,
        UsageCostPolicy::Normalized => UsageCostAuthority::Normalized,
    };
    accounting.discrepancy_usd = normalized_cost.checked_sub(legacy_cost);
    accounting.discrepancy_reason =
        (normalized_cost != legacy_cost).then(|| "cache_aware_bucket_pricing".to_string());

    record.pricing_status = UsagePricingStatus::Priced;
    record.computed_cost_usd = match usage_cost_policy {
        UsageCostPolicy::ShadowLegacy => legacy_cost,
        UsageCostPolicy::Normalized => normalized_cost,
    };
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
    use serde_json::{Map, Value, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{GatewayService, UsageCostPolicy, stream_terminal_success};
    use crate::StreamResponseCollector;

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

    #[test]
    fn stream_terminal_success_includes_collector_failures() {
        let mut event_failure = StreamResponseCollector::default();
        event_failure.observe_chunk(
            br#"event: error
data: {"type":"error","error":{"code":"upstream_failed"}}

"#,
        );
        assert_eq!(
            stream_terminal_success(&mut event_failure, None),
            Some(false)
        );

        let mut finish_failure = StreamResponseCollector::default();
        finish_failure.observe_chunk(br#"data: {"incomplete":true}"#);
        assert_eq!(
            stream_terminal_success(&mut finish_failure, None),
            Some(false)
        );
    }

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
            .record_usage(
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
            .record_usage(
                &auth,
                &model,
                &route,
                "req_configured_pricing",
                Some(json!({
                    "prompt_tokens": 1_000_000,
                    "completion_tokens": 500_000,
                    "total_tokens": 1_500_000,
                    "prompt_tokens_details": {"cached_tokens": 100_000}
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
            let accounting = event
                .normalized_usage
                .as_ref()
                .expect("normalized accounting");
            assert_eq!(accounting.fresh_input_tokens, Some(900_000));
            assert_eq!(accounting.cache_read_tokens, Some(100_000));
            assert_eq!(
                accounting.normalized_cost_usd,
                Some(Money4::from_scaled(19_100))
            );
            assert_eq!(
                accounting.authoritative_cost,
                gateway_core::UsageCostAuthority::Legacy
            );
            assert_eq!(accounting.discrepancy_usd, Some(Money4::from_scaled(-900)));
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

        let normalized_repo = Arc::new(UsageAccountingRepo {
            provider: repo.provider.clone(),
            ..Default::default()
        });
        let normalized_service =
            GatewayService::new(normalized_repo.clone(), Arc::new(PassThroughPlanner))
                .with_usage_cost_policy(UsageCostPolicy::Normalized);
        let normalized_recorded = normalized_service
            .record_usage(
                &auth,
                &model,
                &route,
                "req_normalized_pricing",
                Some(json!({
                    "prompt_tokens": 1_000_000,
                    "completion_tokens": 500_000,
                    "total_tokens": 1_500_000,
                    "prompt_tokens_details": {"cached_tokens": 100_000}
                })),
                occurred_at,
            )
            .await
            .expect("normalized usage should be recorded");
        assert_eq!(normalized_recorded.cost_usd, Some(1.91));
        let events = normalized_repo.events.lock().expect("events lock");
        let normalized_event = events.last().expect("normalized usage event");
        assert_eq!(
            normalized_event.computed_cost_usd,
            Money4::from_scaled(19_100)
        );
        assert_eq!(
            normalized_event
                .normalized_usage
                .as_ref()
                .expect("normalized accounting")
                .authoritative_cost,
            gateway_core::UsageCostAuthority::Normalized
        );
    }

    #[tokio::test]
    async fn normalized_pricing_populates_bucket_reasoning_and_uncached_input_costs() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let mut route = vertex_embedding_route(model_id);
        route.pricing_override = Some(RoutePricingOverride {
            input_cost_per_million_tokens: Money4::from_scaled(10_000),
            output_cost_per_million_tokens: Money4::from_scaled(20_000),
            cache_read_cost_per_million_tokens: Some(Money4::from_scaled(1_000)),
            cache_write_cost_per_million_tokens: Some(Money4::from_scaled(15_000)),
        });
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));

        service
            .record_usage(
                &auth,
                &model,
                &route,
                "req_bucket_costs",
                Some(json!({
                    "prompt_tokens": 1_000_000,
                    "completion_tokens": 500_000,
                    "total_tokens": 1_500_000,
                    "prompt_tokens_details": {
                        "cached_tokens": 100_000,
                        "cache_write_tokens": 50_000
                    },
                    "completion_tokens_details": {"reasoning_tokens": 100_000}
                })),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("usage should be priced");

        service
            .record_usage(
                &auth,
                &model,
                &route,
                "req_unavailable_cache_buckets",
                Some(json!({
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                })),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("usage with unavailable cache buckets should be recorded");

        let events = repo.events.lock().expect("events lock");
        let accounting = events[0]
            .normalized_usage
            .as_ref()
            .expect("normalized accounting");
        assert_eq!(
            accounting.fresh_input_cost_usd,
            Some(Money4::from_scaled(8_500))
        );
        assert_eq!(
            accounting.cache_read_cost_usd,
            Some(Money4::from_scaled(100))
        );
        assert_eq!(
            accounting.cache_creation_cost_usd,
            Some(Money4::from_scaled(750))
        );
        assert_eq!(
            accounting.output_cost_usd,
            Some(Money4::from_scaled(10_000))
        );
        assert_eq!(
            accounting.reasoning_cost_usd,
            Some(Money4::from_scaled(2_000))
        );
        assert_eq!(
            accounting.normalized_cost_usd,
            Some(Money4::from_scaled(19_350))
        );
        assert_eq!(
            accounting.uncached_input_cost_usd,
            Some(Money4::from_scaled(10_000))
        );
        assert_eq!(
            accounting.semantics["uncached_input_cost"]["limitation"],
            Value::Null
        );

        let unavailable = events[1]
            .normalized_usage
            .as_ref()
            .expect("normalized accounting");
        assert_eq!(unavailable.cache_read_cost_usd, None);
        assert_eq!(unavailable.cache_creation_cost_usd, None);
        assert_eq!(unavailable.uncached_input_cost_usd, None);
        assert_eq!(
            unavailable.semantics["uncached_input_cost"]["limitation"],
            "input_bucket_unavailable"
        );
    }

    #[tokio::test]
    async fn request_log_and_usage_ledger_share_normalization_policy() {
        let auth = auth();
        let model_id = Uuid::new_v4();
        let model = model(model_id);
        let route = vertex_embedding_route(model_id);
        let repo = Arc::new(UsageAccountingRepo::default());
        let service = GatewayService::new(repo.clone(), Arc::new(PassThroughPlanner));
        let fixtures = [
            json!({
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120,
                "prompt_tokens_details": {
                    "cached_tokens": 40,
                    "cache_write_tokens": "malformed"
                }
            }),
            json!({"prompt_tokens": -1, "completion_tokens": 3}),
            json!({"prompt_tokens": i64::MAX, "completion_tokens": 1}),
            json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 99}),
            json!({}),
        ];

        for (index, fixture) in fixtures.iter().enumerate() {
            service
                .record_usage(
                    &auth,
                    &model,
                    &route,
                    &format!("req_parity_{index}"),
                    Some(fixture.clone()),
                    OffsetDateTime::now_utc(),
                )
                .await
                .expect("fixture should be recorded");
        }

        let events = repo.events.lock().expect("events lock");
        for (event, fixture) in events.iter().zip(fixtures.iter()) {
            let request_summary = crate::request_logging::usage_summary_from_value(Some(fixture));
            let outcome = crate::normalize_token_usage_best_effort(Some(fixture));
            assert_eq!(event.prompt_tokens, request_summary.prompt_tokens);
            assert_eq!(event.completion_tokens, request_summary.completion_tokens);
            assert_eq!(event.total_tokens, request_summary.total_tokens);
            let accounting = event
                .normalized_usage
                .as_ref()
                .expect("normalized accounting");
            assert_eq!(
                accounting.normalization_error,
                outcome.error.as_ref().map(ToString::to_string)
            );
            assert_eq!(
                accounting.semantics["token_usage"],
                serde_json::to_value(&outcome.usage.semantics).expect("serialize semantics")
            );
            assert_eq!(
                accounting.semantics["coverage"],
                serde_json::to_value(&outcome.usage.coverage).expect("serialize coverage")
            );
        }
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
}
