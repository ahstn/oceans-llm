use std::path::PathBuf;

use async_trait::async_trait;
use gateway_core::{
    ApiKeyRepository, AuthMode, BudgetAlertRepository, BudgetRepository, GlobalRole,
    IdentityRepository, IdentityUserRecord, MembershipRole, ModelRepository, OidcProviderRecord,
    PasswordInvitationRecord, PricingCatalogRepository, ProviderRepository, RequestLogRepository,
    SeedApiKey, SeedModel, SeedProvider, StoreError, StoreHealth, TeamMembershipRecord, TeamRecord,
    UserOidcAuthRecord, UserPasswordAuthRecord, UserRecord, UserSessionRecord, UserStatus,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{LibsqlStore, PostgresStore};

#[derive(Debug, Clone)]
pub enum StoreConnectionOptions {
    Libsql { path: PathBuf },
    Postgres { url: String, max_connections: u32 },
}

impl StoreConnectionOptions {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Libsql { .. } => "libsql",
            Self::Postgres { .. } => "postgres",
        }
    }
}

#[async_trait]
pub trait GatewayStore:
    ApiKeyRepository
    + ModelRepository
    + ProviderRepository
    + IdentityRepository
    + BudgetRepository
    + BudgetAlertRepository
    + RequestLogRepository
    + PricingCatalogRepository
    + StoreHealth
    + Send
    + Sync
{
    async fn has_platform_admin(&self) -> Result<bool, StoreError>;
    async fn upsert_bootstrap_admin_user(
        &self,
        name: &str,
        email: &str,
        must_change_password: bool,
    ) -> Result<UserRecord, StoreError>;
    async fn list_api_keys(&self) -> Result<Vec<gateway_core::ApiKeyRecord>, StoreError>;
    async fn get_api_key_by_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<gateway_core::ApiKeyRecord>, StoreError>;
    async fn create_api_key(
        &self,
        api_key: &gateway_core::NewApiKeyRecord,
    ) -> Result<gateway_core::ApiKeyRecord, StoreError>;
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
    async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError>;
    async fn get_identity_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<IdentityUserRecord>, StoreError>;
    async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError>;
    async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError>;
    async fn list_enabled_oidc_providers(&self) -> Result<Vec<OidcProviderRecord>, StoreError>;
    async fn get_enabled_oidc_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<OidcProviderRecord>, StoreError>;
    async fn get_user_by_email_normalized(
        &self,
        email_normalized: &str,
    ) -> Result<Option<UserRecord>, StoreError>;
    async fn get_team_by_key(&self, team_key: &str) -> Result<Option<TeamRecord>, StoreError>;
    async fn create_team(&self, team_key: &str, team_name: &str) -> Result<TeamRecord, StoreError>;
    async fn update_team_name(
        &self,
        team_id: Uuid,
        team_name: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn create_identity_user(
        &self,
        name: &str,
        email: &str,
        email_normalized: &str,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        status: UserStatus,
    ) -> Result<UserRecord, StoreError>;
    async fn update_identity_user(
        &self,
        user_id: Uuid,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn deactivate_identity_user(
        &self,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
    ) -> Result<(), StoreError>;
    async fn remove_team_membership(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StoreError>;
    async fn transfer_team_membership(
        &self,
        user_id: Uuid,
        from_team_id: Uuid,
        to_team_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn get_user_password_auth(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserPasswordAuthRecord>, StoreError>;
    async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError>;
    async fn update_team_membership_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn find_active_password_invitation_for_user(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError>;
    async fn revoke_password_invitations_for_user(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn create_password_invitation(
        &self,
        invitation_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<PasswordInvitationRecord, StoreError>;
    async fn get_password_invitation(
        &self,
        invitation_id: Uuid,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError>;
    async fn mark_password_invitation_consumed(
        &self,
        invitation_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn store_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn update_user_status(
        &self,
        user_id: Uuid,
        status: UserStatus,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn update_user_must_change_password(
        &self,
        user_id: Uuid,
        must_change_password: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn create_user_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<UserSessionRecord, StoreError>;
    async fn get_user_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<UserSessionRecord>, StoreError>;
    async fn touch_user_session(
        &self,
        session_id: Uuid,
        last_seen_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn revoke_user_sessions(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError>;
    async fn get_user_oidc_auth_by_user(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError>;
    async fn create_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        subject: &str,
        email_claim: Option<&str>,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn set_user_oidc_link(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError>;
    async fn clear_user_oidc_link(&self, user_id: Uuid) -> Result<(), StoreError>;
    async fn delete_user_password_auth(&self, user_id: Uuid) -> Result<(), StoreError>;
    async fn delete_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<(), StoreError>;
    async fn find_invited_oidc_user(
        &self,
        email_normalized: &str,
        oidc_provider_id: &str,
    ) -> Result<Option<UserRecord>, StoreError>;
    async fn seed_from_inputs(
        &self,
        providers: &[SeedProvider],
        models: &[SeedModel],
        api_keys: &[SeedApiKey],
    ) -> Result<(), StoreError>;
}

#[derive(Clone)]
pub enum AnyStore {
    Libsql(LibsqlStore),
    Postgres(PostgresStore),
}

impl AnyStore {
    pub async fn connect(options: &StoreConnectionOptions) -> anyhow::Result<Self> {
        match options {
            StoreConnectionOptions::Libsql { path } => {
                let path = path.to_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "libsql database path must be valid utf-8: {}",
                        path.display()
                    )
                })?;
                Ok(Self::Libsql(LibsqlStore::new_local(path).await?))
            }
            StoreConnectionOptions::Postgres {
                url,
                max_connections,
            } => Ok(Self::Postgres(
                PostgresStore::connect(url, *max_connections).await?,
            )),
        }
    }
}

macro_rules! dispatch_store {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            Self::Libsql(store) => store.$method($($arg),*).await,
            Self::Postgres(store) => store.$method($($arg),*).await,
        }
    };
}

#[async_trait]
impl StoreHealth for AnyStore {
    async fn ping(&self) -> Result<(), StoreError> {
        dispatch_store!(self, ping())
    }
}

#[async_trait]
impl ApiKeyRepository for AnyStore {
    async fn get_api_key_by_public_id(
        &self,
        public_id: &str,
    ) -> Result<Option<gateway_core::ApiKeyRecord>, StoreError> {
        dispatch_store!(self, get_api_key_by_public_id(public_id))
    }

    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> Result<(), StoreError> {
        dispatch_store!(self, touch_api_key_last_used(api_key_id))
    }
}

#[async_trait]
impl ModelRepository for AnyStore {
    async fn list_models(&self) -> Result<Vec<gateway_core::GatewayModel>, StoreError> {
        dispatch_store!(self, list_models())
    }

    async fn get_model_by_key(
        &self,
        model_key: &str,
    ) -> Result<Option<gateway_core::GatewayModel>, StoreError> {
        dispatch_store!(self, get_model_by_key(model_key))
    }

    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<gateway_core::GatewayModel>, StoreError> {
        dispatch_store!(self, list_models_for_api_key(api_key_id))
    }

    async fn list_routes_for_model(
        &self,
        model_id: Uuid,
    ) -> Result<Vec<gateway_core::ModelRoute>, StoreError> {
        dispatch_store!(self, list_routes_for_model(model_id))
    }
}

#[async_trait]
impl ProviderRepository for AnyStore {
    async fn get_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<gateway_core::ProviderConnection>, StoreError> {
        dispatch_store!(self, get_provider_by_key(provider_key))
    }
}

#[async_trait]
impl IdentityRepository for AnyStore {
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
        dispatch_store!(self, get_user_by_id(user_id))
    }

    async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
        dispatch_store!(self, get_team_by_id(team_id))
    }

    async fn get_team_membership_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<TeamMembershipRecord>, StoreError> {
        dispatch_store!(self, get_team_membership_for_user(user_id))
    }

    async fn list_allowed_model_keys_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        dispatch_store!(self, list_allowed_model_keys_for_user(user_id))
    }

    async fn list_allowed_model_keys_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        dispatch_store!(self, list_allowed_model_keys_for_team(team_id))
    }

    async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        dispatch_store!(self, list_team_memberships(team_id))
    }
}

#[async_trait]
impl BudgetRepository for AnyStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<gateway_core::UserBudgetRecord>, StoreError> {
        dispatch_store!(self, get_active_budget_for_user(user_id))
    }

    async fn get_active_budget_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Option<gateway_core::TeamBudgetRecord>, StoreError> {
        dispatch_store!(self, get_active_budget_for_team(team_id))
    }

    async fn upsert_active_budget_for_user(
        &self,
        user_id: Uuid,
        cadence: gateway_core::BudgetCadence,
        amount_usd: gateway_core::Money4,
        hard_limit: bool,
        timezone: &str,
        updated_at: OffsetDateTime,
    ) -> Result<gateway_core::UserBudgetRecord, StoreError> {
        dispatch_store!(
            self,
            upsert_active_budget_for_user(
                user_id, cadence, amount_usd, hard_limit, timezone, updated_at
            )
        )
    }

    async fn deactivate_active_budget_for_user(
        &self,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        dispatch_store!(self, deactivate_active_budget_for_user(user_id, updated_at))
    }

    async fn upsert_active_budget_for_team(
        &self,
        team_id: Uuid,
        cadence: gateway_core::BudgetCadence,
        amount_usd: gateway_core::Money4,
        hard_limit: bool,
        timezone: &str,
        updated_at: OffsetDateTime,
    ) -> Result<gateway_core::TeamBudgetRecord, StoreError> {
        dispatch_store!(
            self,
            upsert_active_budget_for_team(
                team_id, cadence, amount_usd, hard_limit, timezone, updated_at
            )
        )
    }

    async fn deactivate_active_budget_for_team(
        &self,
        team_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        dispatch_store!(self, deactivate_active_budget_for_team(team_id, updated_at))
    }

    async fn get_usage_ledger_by_request_and_scope(
        &self,
        request_id: &str,
        ownership_scope_key: &str,
    ) -> Result<Option<gateway_core::UsageLedgerRecord>, StoreError> {
        dispatch_store!(
            self,
            get_usage_ledger_by_request_and_scope(request_id, ownership_scope_key)
        )
    }

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<gateway_core::Money4, StoreError> {
        dispatch_store!(
            self,
            sum_usage_cost_for_user_in_window(user_id, window_start, window_end)
        )
    }

    async fn sum_usage_cost_for_team_in_window(
        &self,
        team_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<gateway_core::Money4, StoreError> {
        dispatch_store!(
            self,
            sum_usage_cost_for_team_in_window(team_id, window_start, window_end)
        )
    }

    async fn list_usage_daily_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<gateway_core::ApiKeyOwnerKind>,
    ) -> Result<Vec<gateway_core::SpendDailyAggregateRecord>, StoreError> {
        dispatch_store!(
            self,
            list_usage_daily_aggregates(window_start, window_end, owner_kind)
        )
    }

    async fn list_usage_owner_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<gateway_core::ApiKeyOwnerKind>,
    ) -> Result<Vec<gateway_core::SpendOwnerAggregateRecord>, StoreError> {
        dispatch_store!(
            self,
            list_usage_owner_aggregates(window_start, window_end, owner_kind)
        )
    }

    async fn list_usage_model_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<gateway_core::ApiKeyOwnerKind>,
    ) -> Result<Vec<gateway_core::SpendModelAggregateRecord>, StoreError> {
        dispatch_store!(
            self,
            list_usage_model_aggregates(window_start, window_end, owner_kind)
        )
    }

    async fn insert_usage_ledger_if_absent(
        &self,
        event: &gateway_core::UsageLedgerRecord,
    ) -> Result<bool, StoreError> {
        dispatch_store!(self, insert_usage_ledger_if_absent(event))
    }
}

#[async_trait]
impl BudgetAlertRepository for AnyStore {
    async fn create_budget_alert_with_deliveries(
        &self,
        alert: &gateway_core::BudgetAlertRecord,
        deliveries: &[gateway_core::BudgetAlertDeliveryRecord],
    ) -> Result<bool, StoreError> {
        dispatch_store!(self, create_budget_alert_with_deliveries(alert, deliveries))
    }

    async fn list_budget_alert_history(
        &self,
        query: &gateway_core::BudgetAlertHistoryQuery,
    ) -> Result<gateway_core::BudgetAlertHistoryPage, StoreError> {
        dispatch_store!(self, list_budget_alert_history(query))
    }

    async fn claim_pending_budget_alert_delivery_tasks(
        &self,
        limit: u32,
        claimed_at: OffsetDateTime,
    ) -> Result<Vec<gateway_core::BudgetAlertDispatchTask>, StoreError> {
        dispatch_store!(
            self,
            claim_pending_budget_alert_delivery_tasks(limit, claimed_at)
        )
    }

    async fn mark_budget_alert_delivery_sent(
        &self,
        delivery_id: Uuid,
        provider_message_id: Option<&str>,
        sent_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            mark_budget_alert_delivery_sent(delivery_id, provider_message_id, sent_at)
        )
    }

    async fn mark_budget_alert_delivery_failed(
        &self,
        delivery_id: Uuid,
        failure_reason: &str,
        failed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            mark_budget_alert_delivery_failed(delivery_id, failure_reason, failed_at)
        )
    }
}

#[async_trait]
impl RequestLogRepository for AnyStore {
    async fn insert_request_log(
        &self,
        log: &gateway_core::RequestLogRecord,
        payload: Option<&gateway_core::RequestLogPayloadRecord>,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, insert_request_log(log, payload))
    }

    async fn list_request_logs(
        &self,
        query: &gateway_core::RequestLogQuery,
    ) -> Result<gateway_core::RequestLogPage, StoreError> {
        dispatch_store!(self, list_request_logs(query))
    }

    async fn get_request_log_detail(
        &self,
        request_log_id: Uuid,
    ) -> Result<gateway_core::RequestLogDetail, StoreError> {
        dispatch_store!(self, get_request_log_detail(request_log_id))
    }
}

#[async_trait]
impl PricingCatalogRepository for AnyStore {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<gateway_core::PricingCatalogCacheRecord>, StoreError> {
        dispatch_store!(self, get_pricing_catalog_cache(catalog_key))
    }

    async fn upsert_pricing_catalog_cache(
        &self,
        cache: &gateway_core::PricingCatalogCacheRecord,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, upsert_pricing_catalog_cache(cache))
    }

    async fn touch_pricing_catalog_cache_fetched_at(
        &self,
        catalog_key: &str,
        fetched_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            touch_pricing_catalog_cache_fetched_at(catalog_key, fetched_at)
        )
    }

    async fn list_active_model_pricing(
        &self,
    ) -> Result<Vec<gateway_core::ModelPricingRecord>, StoreError> {
        dispatch_store!(self, list_active_model_pricing())
    }

    async fn insert_model_pricing(
        &self,
        record: &gateway_core::ModelPricingRecord,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, insert_model_pricing(record))
    }

    async fn close_model_pricing(
        &self,
        model_pricing_id: Uuid,
        effective_end_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            close_model_pricing(model_pricing_id, effective_end_at, updated_at)
        )
    }

    async fn resolve_model_pricing_at(
        &self,
        pricing_provider_id: &str,
        pricing_model_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<Option<gateway_core::ModelPricingRecord>, StoreError> {
        dispatch_store!(
            self,
            resolve_model_pricing_at(pricing_provider_id, pricing_model_id, occurred_at)
        )
    }
}

#[async_trait]
impl GatewayStore for AnyStore {
    async fn has_platform_admin(&self) -> Result<bool, StoreError> {
        dispatch_store!(self, has_platform_admin())
    }

    async fn upsert_bootstrap_admin_user(
        &self,
        name: &str,
        email: &str,
        must_change_password: bool,
    ) -> Result<UserRecord, StoreError> {
        dispatch_store!(
            self,
            upsert_bootstrap_admin_user(name, email, must_change_password)
        )
    }

    async fn list_api_keys(&self) -> Result<Vec<gateway_core::ApiKeyRecord>, StoreError> {
        match self {
            Self::Libsql(store) => GatewayStore::list_api_keys(store).await,
            Self::Postgres(store) => GatewayStore::list_api_keys(store).await,
        }
    }

    async fn get_api_key_by_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<gateway_core::ApiKeyRecord>, StoreError> {
        match self {
            Self::Libsql(store) => GatewayStore::get_api_key_by_id(store, api_key_id).await,
            Self::Postgres(store) => GatewayStore::get_api_key_by_id(store, api_key_id).await,
        }
    }

    async fn create_api_key(
        &self,
        api_key: &gateway_core::NewApiKeyRecord,
    ) -> Result<gateway_core::ApiKeyRecord, StoreError> {
        match self {
            Self::Libsql(store) => GatewayStore::create_api_key(store, api_key).await,
            Self::Postgres(store) => GatewayStore::create_api_key(store, api_key).await,
        }
    }

    async fn replace_api_key_model_grants(
        &self,
        api_key_id: Uuid,
        model_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => {
                GatewayStore::replace_api_key_model_grants(store, api_key_id, model_ids).await
            }
            Self::Postgres(store) => {
                GatewayStore::replace_api_key_model_grants(store, api_key_id, model_ids).await
            }
        }
    }

    async fn revoke_api_key(
        &self,
        api_key_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => GatewayStore::revoke_api_key(store, api_key_id, revoked_at).await,
            Self::Postgres(store) => {
                GatewayStore::revoke_api_key(store, api_key_id, revoked_at).await
            }
        }
    }

    async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
        dispatch_store!(self, list_identity_users())
    }

    async fn get_identity_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<IdentityUserRecord>, StoreError> {
        dispatch_store!(self, get_identity_user(user_id))
    }

    async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        dispatch_store!(self, list_active_teams())
    }

    async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        dispatch_store!(self, list_teams())
    }

    async fn list_enabled_oidc_providers(&self) -> Result<Vec<OidcProviderRecord>, StoreError> {
        dispatch_store!(self, list_enabled_oidc_providers())
    }

    async fn get_enabled_oidc_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<OidcProviderRecord>, StoreError> {
        dispatch_store!(self, get_enabled_oidc_provider_by_key(provider_key))
    }

    async fn get_user_by_email_normalized(
        &self,
        email_normalized: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        dispatch_store!(self, get_user_by_email_normalized(email_normalized))
    }

    async fn get_team_by_key(&self, team_key: &str) -> Result<Option<TeamRecord>, StoreError> {
        dispatch_store!(self, get_team_by_key(team_key))
    }

    async fn create_team(&self, team_key: &str, team_name: &str) -> Result<TeamRecord, StoreError> {
        dispatch_store!(self, create_team(team_key, team_name))
    }

    async fn update_team_name(
        &self,
        team_id: Uuid,
        team_name: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, update_team_name(team_id, team_name, updated_at))
    }

    async fn create_identity_user(
        &self,
        name: &str,
        email: &str,
        email_normalized: &str,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        status: UserStatus,
    ) -> Result<UserRecord, StoreError> {
        dispatch_store!(
            self,
            create_identity_user(
                name,
                email,
                email_normalized,
                global_role,
                auth_mode,
                status
            )
        )
    }

    async fn update_identity_user(
        &self,
        user_id: Uuid,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            update_identity_user(user_id, global_role, auth_mode, updated_at)
        )
    }

    async fn deactivate_identity_user(
        &self,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, deactivate_identity_user(user_id, updated_at))
    }

    async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, assign_team_membership(user_id, team_id, role))
    }

    async fn remove_team_membership(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        dispatch_store!(self, remove_team_membership(team_id, user_id))
    }

    async fn transfer_team_membership(
        &self,
        user_id: Uuid,
        from_team_id: Uuid,
        to_team_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            transfer_team_membership(user_id, from_team_id, to_team_id, role, updated_at)
        )
    }

    async fn get_user_password_auth(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserPasswordAuthRecord>, StoreError> {
        dispatch_store!(self, get_user_password_auth(user_id))
    }

    async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        dispatch_store!(self, list_team_memberships(team_id))
    }

    async fn update_team_membership_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            update_team_membership_role(team_id, user_id, role, updated_at)
        )
    }

    async fn find_active_password_invitation_for_user(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        dispatch_store!(self, find_active_password_invitation_for_user(user_id, now))
    }

    async fn revoke_password_invitations_for_user(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            revoke_password_invitations_for_user(user_id, revoked_at)
        )
    }

    async fn create_password_invitation(
        &self,
        invitation_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<PasswordInvitationRecord, StoreError> {
        dispatch_store!(
            self,
            create_password_invitation(invitation_id, user_id, token_hash, expires_at, created_at)
        )
    }

    async fn get_password_invitation(
        &self,
        invitation_id: Uuid,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        dispatch_store!(self, get_password_invitation(invitation_id))
    }

    async fn mark_password_invitation_consumed(
        &self,
        invitation_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            mark_password_invitation_consumed(invitation_id, consumed_at)
        )
    }

    async fn store_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            store_user_password(user_id, password_hash, updated_at)
        )
    }

    async fn update_user_status(
        &self,
        user_id: Uuid,
        status: UserStatus,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, update_user_status(user_id, status, updated_at))
    }

    async fn update_user_must_change_password(
        &self,
        user_id: Uuid,
        must_change_password: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            update_user_must_change_password(user_id, must_change_password, updated_at)
        )
    }

    async fn create_user_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<UserSessionRecord, StoreError> {
        dispatch_store!(
            self,
            create_user_session(session_id, user_id, token_hash, expires_at, created_at)
        )
    }

    async fn get_user_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<UserSessionRecord>, StoreError> {
        dispatch_store!(self, get_user_session(session_id))
    }

    async fn touch_user_session(
        &self,
        session_id: Uuid,
        last_seen_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, touch_user_session(session_id, last_seen_at))
    }

    async fn revoke_user_sessions(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, revoke_user_sessions(user_id, revoked_at))
    }

    async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        dispatch_store!(self, get_user_oidc_auth(oidc_provider_id, subject))
    }

    async fn get_user_oidc_auth_by_user(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        dispatch_store!(self, get_user_oidc_auth_by_user(user_id, oidc_provider_id))
    }

    async fn create_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        subject: &str,
        email_claim: Option<&str>,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            create_user_oidc_auth(user_id, oidc_provider_id, subject, email_claim, created_at)
        )
    }

    async fn set_user_oidc_link(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            set_user_oidc_link(user_id, oidc_provider_id, created_at)
        )
    }

    async fn clear_user_oidc_link(&self, user_id: Uuid) -> Result<(), StoreError> {
        dispatch_store!(self, clear_user_oidc_link(user_id))
    }

    async fn delete_user_password_auth(&self, user_id: Uuid) -> Result<(), StoreError> {
        dispatch_store!(self, delete_user_password_auth(user_id))
    }

    async fn delete_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, delete_user_oidc_auth(user_id, oidc_provider_id))
    }

    async fn find_invited_oidc_user(
        &self,
        email_normalized: &str,
        oidc_provider_id: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        dispatch_store!(
            self,
            find_invited_oidc_user(email_normalized, oidc_provider_id)
        )
    }

    async fn seed_from_inputs(
        &self,
        providers: &[SeedProvider],
        models: &[SeedModel],
        api_keys: &[SeedApiKey],
    ) -> Result<(), StoreError> {
        dispatch_store!(self, seed_from_inputs(providers, models, api_keys))
    }
}
