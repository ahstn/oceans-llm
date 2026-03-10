use std::path::PathBuf;

use async_trait::async_trait;
use gateway_core::{
    ApiKeyRepository, AuthMode, BudgetRepository, GlobalRole, IdentityRepository,
    IdentityUserRecord, MembershipRole, ModelRepository, OidcProviderRecord,
    PasswordInvitationRecord, PricingCatalogRepository, ProviderRepository, RequestLogRepository,
    SeedApiKey, SeedModel, SeedProvider, StoreError, StoreHealth, TeamMembershipRecord, TeamRecord,
    UserOidcAuthRecord, UserPasswordAuthRecord, UserRecord, UserSessionRecord,
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
    async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError>;
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
        status: &str,
    ) -> Result<UserRecord, StoreError>;
    async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
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
        status: &str,
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
    async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
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
}

#[async_trait]
impl BudgetRepository for AnyStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<gateway_core::UserBudgetRecord>, StoreError> {
        dispatch_store!(self, get_active_budget_for_user(user_id))
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

    async fn insert_usage_cost_event(
        &self,
        event: &gateway_core::UsageCostEventRecord,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, insert_usage_cost_event(event))
    }
}

#[async_trait]
impl RequestLogRepository for AnyStore {
    async fn insert_request_log(
        &self,
        log: &gateway_core::RequestLogRecord,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, insert_request_log(log))
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

    async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
        dispatch_store!(self, list_identity_users())
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
        status: &str,
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

    async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, assign_team_membership(user_id, team_id, role))
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
        status: &str,
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

    async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        dispatch_store!(self, get_user_oidc_auth(oidc_provider_id, subject))
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
