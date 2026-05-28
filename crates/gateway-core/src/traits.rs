use std::{collections::HashMap, pin::Pin, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    budgets::{BudgetRecord, BudgetScope, BudgetScopeKind, BudgetSettings},
    domain::{
        ApiKeyRecord, BudgetAlertDeliveryRecord, BudgetAlertDispatchTask, BudgetAlertHistoryPage,
        BudgetAlertHistoryQuery, BudgetAlertRecord, ExternalMcpDiscoveryRunRecord,
        ExternalMcpServerRecord, ExternalMcpToolRecord, FocusExportAggregateRecord,
        FocusExportDiagnosticsRecord, GatewayModel, HarnessUsageBucketRecord,
        HarnessUsageLeaderRecord, McpToolInvocationDetail, McpToolInvocationPage,
        McpToolInvocationPayloadRecord, McpToolInvocationQuery, McpToolInvocationRecord,
        ModelPricingRecord, ModelRoute, Money4, NewApiKeyRecord, NewExternalMcpServerRecord,
        PricingCatalogCacheRecord, ProviderCapabilities, ProviderConnection,
        ProviderRequestContext, RequestAttemptRecord, RequestLogDetail, RequestLogPage,
        RequestLogPayloadRecord, RequestLogPurgeResult, RequestLogQuery, RequestLogRecord,
        ServiceAccountRecord, SpendDailyAggregateRecord, SpendModelAggregateRecord,
        SpendOwnerAggregateRecord, TeamMembershipRecord, TeamRecord, UpdateExternalMcpServerRecord,
        UpsertExternalMcpToolRecord, UsageLeaderboardBucketRecord, UsageLeaderboardUserRecord,
        UsageLedgerRecord, UserRecord,
    },
    error::{ProviderError, RouteError, StoreError},
    protocol::core::{ChatRequest, EmbeddingsRequest, ResponsesRequest},
};

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn get_api_key_by_public_id(
        &self,
        public_id: &str,
    ) -> Result<Option<ApiKeyRecord>, StoreError>;

    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> Result<(), StoreError>;

    async fn get_service_account_by_id(
        &self,
        service_account_id: Uuid,
    ) -> Result<Option<ServiceAccountRecord>, StoreError> {
        let _ = service_account_id;
        Err(StoreError::Unexpected(
            "service accounts are not implemented for this repository".to_string(),
        ))
    }
}

#[async_trait]
pub trait AdminApiKeyRepository: Send + Sync {
    async fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>, StoreError>;

    async fn get_api_key_by_id(&self, api_key_id: Uuid)
    -> Result<Option<ApiKeyRecord>, StoreError>;

    async fn create_api_key(&self, api_key: &NewApiKeyRecord) -> Result<ApiKeyRecord, StoreError>;

    async fn replace_api_key_model_grants(
        &self,
        api_key_id: Uuid,
        model_ids: &[Uuid],
    ) -> Result<(), StoreError>;

    async fn revoke_api_key(
        &self,
        api_key_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;
}

#[async_trait]
pub trait ModelRepository: Send + Sync {
    async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError>;
    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError>;
    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError>;
    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError>;

    async fn list_routes_for_models(
        &self,
        model_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ModelRoute>>, StoreError> {
        let mut routes_by_model = HashMap::with_capacity(model_ids.len());
        for model_id in model_ids {
            routes_by_model.insert(*model_id, self.list_routes_for_model(*model_id).await?);
        }
        Ok(routes_by_model)
    }
}

#[async_trait]
pub trait ProviderRepository: Send + Sync {
    async fn get_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<ProviderConnection>, StoreError>;

    async fn list_providers_by_keys(
        &self,
        provider_keys: &[String],
    ) -> Result<HashMap<String, ProviderConnection>, StoreError> {
        let mut providers = HashMap::with_capacity(provider_keys.len());
        for provider_key in provider_keys {
            if let Some(provider) = self.get_provider_by_key(provider_key).await? {
                providers.insert(provider_key.clone(), provider);
            }
        }
        Ok(providers)
    }
}

#[async_trait]
pub trait IdentityRepository: Send + Sync {
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError>;
    async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError>;
    async fn get_service_account_by_id(
        &self,
        service_account_id: Uuid,
    ) -> Result<Option<ServiceAccountRecord>, StoreError> {
        let _ = service_account_id;
        Err(StoreError::Unexpected(
            "service accounts are not implemented for this repository".to_string(),
        ))
    }
    async fn get_team_membership_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<TeamMembershipRecord>, StoreError>;
    async fn list_allowed_model_keys_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<String>, StoreError>;
    async fn list_allowed_model_keys_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<String>, StoreError>;
    async fn list_allowed_model_keys_for_service_account(
        &self,
        service_account_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        let _ = service_account_id;
        Err(StoreError::Unexpected(
            "service account model allowlists are not implemented for this repository".to_string(),
        ))
    }
    async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        let _ = team_id;
        Err(StoreError::Unexpected(
            "list_team_memberships is not implemented for this repository".to_string(),
        ))
    }
}

#[async_trait]
pub trait AdminIdentityRepository: Send + Sync {
    async fn list_identity_users(&self) -> Result<Vec<crate::IdentityUserRecord>, StoreError>;
    async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError>;
    async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError>;
    async fn list_active_service_accounts(&self) -> Result<Vec<ServiceAccountRecord>, StoreError> {
        Err(StoreError::Unexpected(
            "list_active_service_accounts is not implemented for this repository".to_string(),
        ))
    }
    async fn list_service_accounts(&self) -> Result<Vec<ServiceAccountRecord>, StoreError> {
        Err(StoreError::Unexpected(
            "list_service_accounts is not implemented for this repository".to_string(),
        ))
    }
}

#[async_trait]
pub trait BudgetRepository: Send + Sync {
    async fn get_active_budget_by_scope(
        &self,
        scope: &BudgetScope,
    ) -> Result<Option<BudgetRecord>, StoreError>;
    async fn list_active_budgets(
        &self,
        scope_kind: Option<BudgetScopeKind>,
    ) -> Result<Vec<BudgetRecord>, StoreError> {
        let _ = scope_kind;
        Err(StoreError::Unexpected(
            "list_active_budgets is not implemented for this repository".to_string(),
        ))
    }
    async fn upsert_active_budget(
        &self,
        scope: &BudgetScope,
        settings: &BudgetSettings,
        updated_at: OffsetDateTime,
    ) -> Result<BudgetRecord, StoreError>;
    async fn deactivate_active_budget(
        &self,
        scope: &BudgetScope,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;
    async fn get_usage_ledger_by_request_and_scope(
        &self,
        request_id: &str,
        ownership_scope_key: &str,
    ) -> Result<Option<UsageLedgerRecord>, StoreError>;
    async fn sum_usage_cost_for_budget_scope_in_window(
        &self,
        scope: &BudgetScope,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError>;
    async fn count_active_api_keys_for_service_account(
        &self,
        service_account_id: Uuid,
    ) -> Result<u64, StoreError> {
        let _ = service_account_id;
        Err(StoreError::Unexpected(
            "count_active_api_keys_for_service_account is not implemented for this repository"
                .to_string(),
        ))
    }
    async fn list_usage_daily_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<crate::ApiKeyOwnerKind>,
    ) -> Result<Vec<SpendDailyAggregateRecord>, StoreError> {
        let _ = (window_start, window_end, owner_kind);
        Err(StoreError::Unexpected(
            "list_usage_daily_aggregates is not implemented for this repository".to_string(),
        ))
    }
    async fn list_usage_owner_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<crate::ApiKeyOwnerKind>,
    ) -> Result<Vec<SpendOwnerAggregateRecord>, StoreError> {
        let _ = (window_start, window_end, owner_kind);
        Err(StoreError::Unexpected(
            "list_usage_owner_aggregates is not implemented for this repository".to_string(),
        ))
    }
    async fn list_usage_model_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<crate::ApiKeyOwnerKind>,
    ) -> Result<Vec<SpendModelAggregateRecord>, StoreError> {
        let _ = (window_start, window_end, owner_kind);
        Err(StoreError::Unexpected(
            "list_usage_model_aggregates is not implemented for this repository".to_string(),
        ))
    }
    async fn list_focus_export_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<crate::ApiKeyOwnerKind>,
        owner_user_id: Option<Uuid>,
    ) -> Result<Vec<FocusExportAggregateRecord>, StoreError> {
        let _ = (window_start, window_end, owner_kind, owner_user_id);
        Err(StoreError::Unexpected(
            "list_focus_export_aggregates is not implemented for this repository".to_string(),
        ))
    }
    async fn get_focus_export_diagnostics(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<crate::ApiKeyOwnerKind>,
        owner_user_id: Option<Uuid>,
    ) -> Result<FocusExportDiagnosticsRecord, StoreError> {
        let _ = (window_start, window_end, owner_kind, owner_user_id);
        Err(StoreError::Unexpected(
            "get_focus_export_diagnostics is not implemented for this repository".to_string(),
        ))
    }
    async fn list_usage_user_leaderboard(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<UsageLeaderboardUserRecord>, StoreError> {
        let _ = (window_start, window_end, limit);
        Err(StoreError::Unexpected(
            "list_usage_user_leaderboard is not implemented for this repository".to_string(),
        ))
    }
    async fn list_usage_user_bucket_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        bucket_hours: u8,
        user_ids: &[Uuid],
    ) -> Result<Vec<UsageLeaderboardBucketRecord>, StoreError> {
        let _ = (window_start, window_end, bucket_hours, user_ids);
        Err(StoreError::Unexpected(
            "list_usage_user_bucket_aggregates is not implemented for this repository".to_string(),
        ))
    }
    async fn insert_usage_ledger_if_absent(
        &self,
        event: &UsageLedgerRecord,
    ) -> Result<bool, StoreError>;
}

#[async_trait]
pub trait BudgetAlertRepository: Send + Sync {
    async fn create_budget_alert_with_deliveries(
        &self,
        alert: &BudgetAlertRecord,
        deliveries: &[BudgetAlertDeliveryRecord],
    ) -> Result<bool, StoreError> {
        let _ = (alert, deliveries);
        Err(StoreError::Unexpected(
            "create_budget_alert_with_deliveries is not implemented for this repository"
                .to_string(),
        ))
    }

    async fn list_budget_alert_history(
        &self,
        query: &BudgetAlertHistoryQuery,
    ) -> Result<BudgetAlertHistoryPage, StoreError> {
        let _ = query;
        Err(StoreError::Unexpected(
            "list_budget_alert_history is not implemented for this repository".to_string(),
        ))
    }

    async fn claim_pending_budget_alert_delivery_tasks(
        &self,
        limit: u32,
        claimed_at: OffsetDateTime,
    ) -> Result<Vec<BudgetAlertDispatchTask>, StoreError> {
        let _ = (limit, claimed_at);
        Err(StoreError::Unexpected(
            "claim_pending_budget_alert_delivery_tasks is not implemented for this repository"
                .to_string(),
        ))
    }

    async fn mark_budget_alert_delivery_sent(
        &self,
        delivery_id: Uuid,
        provider_message_id: Option<&str>,
        sent_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let _ = (delivery_id, provider_message_id, sent_at);
        Err(StoreError::Unexpected(
            "mark_budget_alert_delivery_sent is not implemented for this repository".to_string(),
        ))
    }

    async fn mark_budget_alert_delivery_failed(
        &self,
        delivery_id: Uuid,
        failure_reason: &str,
        failed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let _ = (delivery_id, failure_reason, failed_at);
        Err(StoreError::Unexpected(
            "mark_budget_alert_delivery_failed is not implemented for this repository".to_string(),
        ))
    }
}

#[async_trait]
pub trait RequestLogRepository: Send + Sync {
    async fn insert_request_log(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
    ) -> Result<(), StoreError>;

    async fn insert_request_log_with_attempts(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
        attempts: &[RequestAttemptRecord],
    ) -> Result<(), StoreError> {
        let _ = attempts;
        self.insert_request_log(log, payload).await
    }
    async fn list_request_logs(
        &self,
        query: &RequestLogQuery,
    ) -> Result<RequestLogPage, StoreError>;
    async fn get_request_log_detail(
        &self,
        request_log_id: Uuid,
    ) -> Result<RequestLogDetail, StoreError>;

    async fn list_harness_usage_leaders(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<HarnessUsageLeaderRecord>, StoreError> {
        let _ = (window_start, window_end, limit);
        Err(StoreError::Unexpected(
            "list_harness_usage_leaders is not implemented for this repository".to_string(),
        ))
    }

    async fn list_harness_usage_bucket_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        bucket_hours: u8,
        agent_harness_keys: &[String],
    ) -> Result<Vec<HarnessUsageBucketRecord>, StoreError> {
        let _ = (window_start, window_end, bucket_hours, agent_harness_keys);
        Err(StoreError::Unexpected(
            "list_harness_usage_bucket_aggregates is not implemented for this repository"
                .to_string(),
        ))
    }

    async fn purge_request_logs_older_than(
        &self,
        cutoff: OffsetDateTime,
        dry_run: bool,
    ) -> Result<RequestLogPurgeResult, StoreError>;
}

#[async_trait]
pub trait RequestAttemptRepository: Send + Sync {
    async fn list_request_log_attempts(
        &self,
        request_log_id: Uuid,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError>;
}

#[async_trait]
pub trait McpToolInvocationRepository: Send + Sync {
    async fn insert_mcp_tool_invocation(
        &self,
        invocation: &McpToolInvocationRecord,
        payload: Option<&McpToolInvocationPayloadRecord>,
    ) -> Result<(), StoreError>;

    async fn list_mcp_tool_invocations(
        &self,
        query: &McpToolInvocationQuery,
    ) -> Result<McpToolInvocationPage, StoreError>;

    async fn get_mcp_tool_invocation_detail(
        &self,
        mcp_tool_invocation_id: Uuid,
    ) -> Result<McpToolInvocationDetail, StoreError>;
}

#[async_trait]
pub trait McpRegistryRepository: Send + Sync {
    async fn list_external_mcp_servers(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError>;

    async fn get_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError>;

    async fn get_external_mcp_server_by_key(
        &self,
        server_key: &str,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError>;

    async fn create_external_mcp_server(
        &self,
        input: &NewExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError>;

    async fn update_external_mcp_server(
        &self,
        input: &UpdateExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError>;

    async fn disable_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<ExternalMcpServerRecord, StoreError>;

    async fn list_external_mcp_tools(
        &self,
        mcp_server_id: Uuid,
        include_inactive: bool,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError>;

    async fn record_external_mcp_discovery_success(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
        tools: &[UpsertExternalMcpToolRecord],
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError>;

    async fn record_external_mcp_discovery_failure(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
    ) -> Result<(), StoreError>;
}

#[async_trait]
pub trait PricingCatalogRepository: Send + Sync {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError>;

    async fn upsert_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
    ) -> Result<(), StoreError>;

    async fn touch_pricing_catalog_cache_fetched_at(
        &self,
        catalog_key: &str,
        fetched_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError>;
    async fn insert_model_pricing(&self, record: &ModelPricingRecord) -> Result<(), StoreError>;
    async fn close_model_pricing(
        &self,
        model_pricing_id: Uuid,
        effective_end_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn resolve_model_pricing_at(
        &self,
        pricing_provider_id: &str,
        pricing_model_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<Option<ModelPricingRecord>, StoreError>;
}

#[async_trait]
pub trait StoreHealth: Send + Sync {
    async fn ping(&self) -> Result<(), StoreError>;
}

pub trait RoutePlanner: Send + Sync {
    fn plan_routes(&self, routes: &[ModelRoute]) -> Result<Vec<ModelRoute>, RouteError>;
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

#[async_trait]
pub trait ProviderClient: Send + Sync {
    fn provider_key(&self) -> &str;
    fn provider_type(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;

    async fn chat_completions(
        &self,
        request: &ChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn chat_completions_stream(
        &self,
        request: &ChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError>;

    async fn embeddings(
        &self,
        request: &EmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn responses(
        &self,
        request: &ResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn responses_stream(
        &self,
        request: &ResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError>;
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ProviderClient>>,
}

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn ProviderClient>) {
        self.providers
            .insert(provider.provider_key().to_string(), provider);
    }

    #[must_use]
    pub fn get(&self, provider_key: &str) -> Option<Arc<dyn ProviderClient>> {
        self.providers.get(provider_key).cloned()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
