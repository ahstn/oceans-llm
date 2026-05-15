mod api_keys;
mod budget_alerts;
mod budgets;
mod identity;
mod mcp_tool_invocations;
mod models;
mod pricing_catalog;
mod providers;
mod request_logs;
mod seed;
mod support;

use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use gateway_core::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyOwnerKind, ApiKeyRecord,
    ApiKeyRepository, ApiKeyStatus, AuthMode, BudgetAlertChannel, BudgetAlertDeliveryRecord,
    BudgetAlertDeliveryStatus, BudgetAlertDispatchTask, BudgetAlertHistoryPage,
    BudgetAlertHistoryQuery, BudgetAlertHistoryRecord, BudgetAlertRecord, BudgetAlertRepository,
    BudgetCadence, BudgetRepository, CONFIG_SEED_SERVICE_ACCOUNT_ID,
    CONFIG_SEED_SERVICE_ACCOUNT_KEY, CONFIG_SEED_TEAM_ID, CONFIG_SEED_TEAM_KEY, GatewayModel,
    GlobalRole, IdentityRepository, IdentityUserRecord, MAX_MCP_TOOL_INVOCATION_PAGE_SIZE,
    McpToolInvocationDetail, McpToolInvocationPage, McpToolInvocationPayloadRecord,
    McpToolInvocationQuery, McpToolInvocationRecord, McpToolInvocationRepository,
    McpToolInvocationStatus, McpToolPolicyResult, MembershipRole, ModelAccessMode,
    ModelPricingRecord, ModelRepository, ModelRoute, Money4, NewApiKeyRecord, OidcJitMembership,
    OidcJitPolicy, OidcLoginStateRecord, OidcProviderRecord, PasswordInvitationRecord,
    PricingCatalogCacheRecord, PricingCatalogRepository, PricingLimits, PricingModalities,
    PricingProvenance, ProviderConnection, ProviderRepository, RequestAttemptRecord,
    RequestAttemptRepository, RequestAttemptStatus, RequestLogDetail, RequestLogPage,
    RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestLogRepository, RequestTag,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, ServiceAccountBudgetRecord, ServiceAccountRecord,
    ServiceAccountStatus, SpendDailyAggregateRecord, SpendModelAggregateRecord,
    SpendOwnerAggregateRecord, StoreError, StoreHealth, TeamBudgetRecord, TeamMembershipRecord,
    TeamRecord, UsageLeaderboardBucketRecord, UsageLeaderboardUserRecord, UsageLedgerRecord,
    UsagePricingStatus, UserBudgetRecord, UserOidcAuthRecord, UserPasswordAuthRecord, UserRecord,
    UserSessionRecord, UserStatus,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    GatewayStore,
    seed::{api_key_uuid, model_uuid, route_uuid},
};

use support::*;

#[derive(Clone)]
pub struct LibsqlStore {
    connection: Arc<libsql::Connection>,
}

impl LibsqlStore {
    pub async fn new_local(path: &str) -> anyhow::Result<Self> {
        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .with_context(|| format!("failed building local libsql database at `{path}`"))?;
        let connection = db.connect().context("failed opening libsql connection")?;

        Ok(Self {
            connection: Arc::new(connection),
        })
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &libsql::Connection {
        &self.connection
    }
}

#[async_trait]
impl StoreHealth for LibsqlStore {
    async fn ping(&self) -> Result<(), StoreError> {
        let mut rows = self
            .connection
            .query("SELECT 1", ())
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        let _ = rows
            .next()
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl GatewayStore for LibsqlStore {
    async fn has_platform_admin(&self) -> Result<bool, StoreError> {
        Self::has_platform_admin(self).await
    }

    async fn upsert_bootstrap_admin_user(
        &self,
        name: &str,
        email: &str,
        must_change_password: bool,
    ) -> Result<UserRecord, StoreError> {
        Self::upsert_bootstrap_admin_user(self, name, email, must_change_password).await
    }

    async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
        Self::list_identity_users(self).await
    }

    async fn get_identity_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<IdentityUserRecord>, StoreError> {
        Self::get_identity_user(self, user_id).await
    }

    async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        Self::list_active_teams(self).await
    }

    async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        Self::list_teams(self).await
    }

    async fn list_active_service_accounts(&self) -> Result<Vec<ServiceAccountRecord>, StoreError> {
        Self::list_active_service_accounts(self).await
    }

    async fn list_service_accounts(&self) -> Result<Vec<ServiceAccountRecord>, StoreError> {
        Self::list_service_accounts(self).await
    }

    async fn create_service_account(
        &self,
        team_id: Uuid,
        service_account_key: &str,
        service_account_name: &str,
        created_at: OffsetDateTime,
    ) -> Result<ServiceAccountRecord, StoreError> {
        Self::create_service_account(
            self,
            team_id,
            service_account_key,
            service_account_name,
            created_at,
        )
        .await
    }

    async fn update_service_account_name(
        &self,
        service_account_id: Uuid,
        service_account_name: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_service_account_name(
            self,
            service_account_id,
            service_account_name,
            updated_at,
        )
        .await
    }

    async fn disable_service_account(
        &self,
        service_account_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        Self::disable_service_account(self, service_account_id, disabled_at).await
    }

    async fn list_enabled_oidc_providers(&self) -> Result<Vec<OidcProviderRecord>, StoreError> {
        Self::list_enabled_oidc_providers(self).await
    }

    async fn get_enabled_oidc_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<OidcProviderRecord>, StoreError> {
        Self::get_enabled_oidc_provider_by_key(self, provider_key).await
    }

    async fn create_oidc_login_state(
        &self,
        state: &gateway_core::OidcLoginStateRecord,
    ) -> Result<(), StoreError> {
        Self::create_oidc_login_state(self, state).await
    }

    async fn consume_oidc_login_state(
        &self,
        state_hash: &str,
        consumed_at: OffsetDateTime,
    ) -> Result<Option<gateway_core::OidcLoginStateRecord>, StoreError> {
        Self::consume_oidc_login_state(self, state_hash, consumed_at).await
    }

    async fn get_user_by_email_normalized(
        &self,
        email_normalized: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        Self::get_user_by_email_normalized(self, email_normalized).await
    }

    async fn get_team_by_key(&self, team_key: &str) -> Result<Option<TeamRecord>, StoreError> {
        Self::get_team_by_key(self, team_key).await
    }

    async fn create_team(&self, team_key: &str, team_name: &str) -> Result<TeamRecord, StoreError> {
        Self::create_team(self, team_key, team_name).await
    }

    async fn update_team_name(
        &self,
        team_id: Uuid,
        team_name: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_team_name(self, team_id, team_name, updated_at).await
    }

    async fn update_team_tags(
        &self,
        team_id: Uuid,
        tags: &[RequestTag],
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_team_tags(self, team_id, tags, updated_at).await
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
        Self::create_identity_user(
            self,
            name,
            email,
            email_normalized,
            global_role,
            auth_mode,
            status,
        )
        .await
    }

    async fn update_identity_user(
        &self,
        user_id: Uuid,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_identity_user(self, user_id, global_role, auth_mode, updated_at).await
    }

    async fn update_user_tags(
        &self,
        user_id: Uuid,
        tags: &[RequestTag],
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_user_tags(self, user_id, tags, updated_at).await
    }

    async fn deactivate_identity_user(
        &self,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::deactivate_identity_user(self, user_id, updated_at).await
    }

    async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
    ) -> Result<(), StoreError> {
        Self::assign_team_membership(self, user_id, team_id, role).await
    }

    async fn remove_team_membership(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        Self::remove_team_membership(self, team_id, user_id).await
    }

    async fn transfer_team_membership(
        &self,
        user_id: Uuid,
        from_team_id: Uuid,
        to_team_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::transfer_team_membership(self, user_id, from_team_id, to_team_id, role, updated_at)
            .await
    }

    async fn get_user_password_auth(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserPasswordAuthRecord>, StoreError> {
        Self::get_user_password_auth(self, user_id).await
    }

    async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        Self::list_team_memberships(self, team_id).await
    }

    async fn update_team_membership_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_team_membership_role(self, team_id, user_id, role, updated_at).await
    }

    async fn find_active_password_invitation_for_user(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        Self::find_active_password_invitation_for_user(self, user_id, now).await
    }

    async fn revoke_password_invitations_for_user(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::revoke_password_invitations_for_user(self, user_id, revoked_at).await
    }

    async fn create_password_invitation(
        &self,
        invitation_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<PasswordInvitationRecord, StoreError> {
        Self::create_password_invitation(
            self,
            invitation_id,
            user_id,
            token_hash,
            expires_at,
            created_at,
        )
        .await
    }

    async fn get_password_invitation(
        &self,
        invitation_id: Uuid,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        Self::get_password_invitation(self, invitation_id).await
    }

    async fn mark_password_invitation_consumed(
        &self,
        invitation_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::mark_password_invitation_consumed(self, invitation_id, consumed_at).await
    }

    async fn store_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::store_user_password(self, user_id, password_hash, updated_at).await
    }

    async fn update_user_status(
        &self,
        user_id: Uuid,
        status: UserStatus,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_user_status(self, user_id, status, updated_at).await
    }

    async fn update_user_must_change_password(
        &self,
        user_id: Uuid,
        must_change_password: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::update_user_must_change_password(self, user_id, must_change_password, updated_at)
            .await
    }

    async fn create_user_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<UserSessionRecord, StoreError> {
        Self::create_user_session(
            self, session_id, user_id, token_hash, expires_at, created_at,
        )
        .await
    }

    async fn get_user_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<UserSessionRecord>, StoreError> {
        Self::get_user_session(self, session_id).await
    }

    async fn touch_user_session(
        &self,
        session_id: Uuid,
        last_seen_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::touch_user_session(self, session_id, last_seen_at).await
    }

    async fn revoke_user_sessions(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::revoke_user_sessions(self, user_id, revoked_at).await
    }

    async fn revoke_user_session(
        &self,
        session_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::revoke_user_session(self, session_id, revoked_at).await
    }

    async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        Self::get_user_oidc_auth(self, oidc_provider_id, subject).await
    }

    async fn get_user_oidc_auth_by_user(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        Self::get_user_oidc_auth_by_user(self, user_id, oidc_provider_id).await
    }

    async fn create_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        subject: &str,
        email_claim: Option<&str>,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::create_user_oidc_auth(
            self,
            user_id,
            oidc_provider_id,
            subject,
            email_claim,
            created_at,
        )
        .await
    }

    async fn set_user_oidc_link(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::set_user_oidc_link(self, user_id, oidc_provider_id, created_at).await
    }

    async fn clear_user_oidc_link(&self, user_id: Uuid) -> Result<(), StoreError> {
        Self::clear_user_oidc_link(self, user_id).await
    }

    async fn delete_user_password_auth(&self, user_id: Uuid) -> Result<(), StoreError> {
        Self::delete_user_password_auth(self, user_id).await
    }

    async fn delete_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<(), StoreError> {
        Self::delete_user_oidc_auth(self, user_id, oidc_provider_id).await
    }

    async fn find_invited_oidc_user(
        &self,
        email_normalized: &str,
        oidc_provider_id: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        Self::find_invited_oidc_user(self, email_normalized, oidc_provider_id).await
    }

    async fn seed_update_identity_user_profile(
        &self,
        user_id: Uuid,
        name: &str,
        email: &str,
        email_normalized: &str,
        request_logging_enabled: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::seed_update_identity_user_profile(
            self,
            user_id,
            name,
            email,
            email_normalized,
            request_logging_enabled,
            updated_at,
        )
        .await
    }

    async fn seed_from_inputs(
        &self,
        providers: &[gateway_core::SeedProvider],
        models: &[gateway_core::SeedModel],
        api_keys: &[gateway_core::SeedApiKey],
        oidc_providers: &[gateway_core::SeedOidcProvider],
        teams: &[gateway_core::SeedTeam],
        users: &[gateway_core::SeedUser],
    ) -> Result<(), StoreError> {
        self.seed_from_inputs(providers, models, api_keys, oidc_providers, teams, users)
            .await
    }
}

#[async_trait]
impl AdminIdentityRepository for LibsqlStore {
    async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
        Self::list_identity_users(self).await
    }

    async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        Self::list_active_teams(self).await
    }

    async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        Self::list_teams(self).await
    }

    async fn list_active_service_accounts(&self) -> Result<Vec<ServiceAccountRecord>, StoreError> {
        Self::list_active_service_accounts(self).await
    }

    async fn list_service_accounts(&self) -> Result<Vec<ServiceAccountRecord>, StoreError> {
        Self::list_service_accounts(self).await
    }
}
