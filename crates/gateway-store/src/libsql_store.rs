use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use gateway_core::{
    ApiKeyOwnerKind, ApiKeyRecord, ApiKeyRepository, AuthMode, BudgetCadence, BudgetRepository,
    GatewayModel, GlobalRole, IdentityRepository, IdentityUserRecord, MembershipRole,
    ModelAccessMode, ModelRepository, ModelRoute, Money4, OidcProviderRecord,
    PasswordInvitationRecord, PricingCatalogCacheRecord, PricingCatalogRepository,
    ProviderConnection, ProviderRepository, RequestLogRecord, RequestLogRepository,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, StoreError, StoreHealth,
    TeamMembershipRecord, TeamRecord, UsageCostEventRecord, UserBudgetRecord, UserOidcAuthRecord,
    UserPasswordAuthRecord, UserRecord, UserSessionRecord,
};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::GatewayStore;

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

    pub(crate) fn connection(&self) -> &libsql::Connection {
        &self.connection
    }

    pub async fn has_platform_admin(&self) -> Result<bool, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT 1
                FROM users
                WHERE global_role = 'platform_admin'
                LIMIT 1
                "#,
                (),
            )
            .await
            .map_err(to_query_error)?;

        Ok(rows.next().await.map_err(to_query_error)?.is_some())
    }

    pub async fn upsert_bootstrap_admin_user(
        &self,
        name: &str,
        email: &str,
        must_change_password: bool,
    ) -> Result<UserRecord, StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let email_normalized = email.trim().to_ascii_lowercase();
        self.connection
            .execute(
                r#"
                INSERT INTO users (
                    user_id, name, email, email_normalized, global_role, auth_mode, status,
                    must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'platform_admin', 'password', 'active', ?5, 1, 'all', ?6, ?6)
                ON CONFLICT(user_id) DO UPDATE SET
                    name = excluded.name,
                    email = excluded.email,
                    email_normalized = excluded.email_normalized,
                    global_role = excluded.global_role,
                    auth_mode = excluded.auth_mode,
                    status = excluded.status,
                    must_change_password = excluded.must_change_password,
                    request_logging_enabled = excluded.request_logging_enabled,
                    model_access_mode = excluded.model_access_mode,
                    updated_at = excluded.updated_at
                "#,
                libsql::params![
                    SYSTEM_BOOTSTRAP_ADMIN_USER_ID,
                    name,
                    email,
                    email_normalized,
                    if must_change_password { 1_i64 } else { 0_i64 },
                    now
                ],
            )
            .await
            .map_err(to_write_error)?;

        self.get_user_by_id(parse_uuid(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)?)
            .await?
            .ok_or_else(|| StoreError::NotFound("bootstrap admin user missing".to_string()))
    }

    pub async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT
                    users.user_id,
                    users.name,
                    users.email,
                    users.email_normalized,
                    users.global_role,
                    users.auth_mode,
                    users.status,
                    users.must_change_password,
                    users.request_logging_enabled,
                    users.model_access_mode,
                    users.created_at,
                    users.updated_at,
                    teams.team_id,
                    teams.team_name,
                    team_memberships.role,
                    user_oidc_links.oidc_provider_id,
                    oidc_providers.provider_key
                FROM users
                LEFT JOIN team_memberships ON team_memberships.user_id = users.user_id
                LEFT JOIN teams ON teams.team_id = team_memberships.team_id
                LEFT JOIN user_oidc_links ON user_oidc_links.user_id = users.user_id
                LEFT JOIN oidc_providers ON oidc_providers.oidc_provider_id = user_oidc_links.oidc_provider_id
                WHERE users.user_id != ?1
                ORDER BY users.created_at DESC, users.email_normalized ASC
                "#,
                [SYSTEM_BOOTSTRAP_ADMIN_USER_ID],
            )
            .await
            .map_err(to_query_error)?;

        let mut users = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            users.push(decode_identity_user_record(&row)?);
        }

        Ok(users)
    }

    pub async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                FROM teams
                WHERE status = 'active'
                ORDER BY team_name ASC
                "#,
                (),
            )
            .await
            .map_err(to_query_error)?;

        let mut teams = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            teams.push(decode_team_record(&row)?);
        }

        Ok(teams)
    }

    pub async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                FROM teams
                ORDER BY team_name ASC
                "#,
                (),
            )
            .await
            .map_err(to_query_error)?;

        let mut teams = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            teams.push(decode_team_record(&row)?);
        }

        Ok(teams)
    }

    pub async fn list_enabled_oidc_providers(&self) -> Result<Vec<OidcProviderRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                       scopes_json, enabled, created_at, updated_at
                FROM oidc_providers
                WHERE enabled = 1
                ORDER BY provider_key ASC
                "#,
                (),
            )
            .await
            .map_err(to_query_error)?;

        let mut providers = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            providers.push(decode_oidc_provider_record(&row)?);
        }

        Ok(providers)
    }

    pub async fn get_enabled_oidc_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<OidcProviderRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                       scopes_json, enabled, created_at, updated_at
                FROM oidc_providers
                WHERE provider_key = ?1 AND enabled = 1
                LIMIT 1
                "#,
                [provider_key],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_oidc_provider_record(&row).map(Some)
    }

    pub async fn get_user_by_email_normalized(
        &self,
        email_normalized: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_id, name, email, email_normalized, global_role, auth_mode, status,
                       must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
                FROM users
                WHERE email_normalized = ?1
                LIMIT 1
                "#,
                [email_normalized],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_user_record(&row).map(Some)
    }

    pub async fn get_team_by_key(&self, team_key: &str) -> Result<Option<TeamRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                FROM teams
                WHERE team_key = ?1
                LIMIT 1
                "#,
                [team_key],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_team_record(&row).map(Some)
    }

    pub async fn create_team(
        &self,
        team_key: &str,
        team_name: &str,
    ) -> Result<TeamRecord, StoreError> {
        let team_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc().unix_timestamp();

        self.connection
            .execute(
                r#"
                INSERT INTO teams (
                    team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 'active', 'all', ?4, ?4)
                "#,
                libsql::params![team_id.to_string(), team_key, team_name, now],
            )
            .await
            .map_err(to_write_error)?;

        self.get_team_by_id(team_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("created team `{team_id}` missing")))
    }

    pub async fn update_team_name(
        &self,
        team_id: Uuid,
        team_name: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE teams
                SET team_name = ?1, updated_at = ?2
                WHERE team_id = ?3
                "#,
                libsql::params![team_name, updated_at.unix_timestamp(), team_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn create_identity_user(
        &self,
        name: &str,
        email: &str,
        email_normalized: &str,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        status: &str,
    ) -> Result<UserRecord, StoreError> {
        let user_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc().unix_timestamp();

        self.connection
            .execute(
                r#"
                INSERT INTO users (
                    user_id, name, email, email_normalized, global_role, auth_mode, status,
                    must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 1, 'all', ?8, ?8)
                "#,
                libsql::params![
                    user_id.to_string(),
                    name,
                    email,
                    email_normalized,
                    global_role.as_str(),
                    auth_mode.as_str(),
                    status,
                    now,
                ],
            )
            .await
            .map_err(to_write_error)?;

        self.get_user_by_id(user_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("created user `{user_id}` missing")))
    }

    pub async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
    ) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        self.connection
            .execute(
                r#"
                INSERT INTO team_memberships (team_id, user_id, role, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?4)
                "#,
                libsql::params![team_id.to_string(), user_id.to_string(), role.as_str(), now],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn get_user_password_auth(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserPasswordAuthRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_id, password_hash, password_updated_at
                FROM user_password_auth
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_user_password_auth_record(&row).map(Some)
    }

    pub async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, user_id, role, created_at, updated_at
                FROM team_memberships
                WHERE team_id = ?1
                ORDER BY created_at ASC
                "#,
                [team_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;

        let mut memberships = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            memberships.push(decode_team_membership_record(&row)?);
        }

        Ok(memberships)
    }

    pub async fn update_team_membership_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE team_memberships
                SET role = ?1, updated_at = ?2
                WHERE team_id = ?3 AND user_id = ?4
                "#,
                libsql::params![
                    role.as_str(),
                    updated_at.unix_timestamp(),
                    team_id.to_string(),
                    user_id.to_string()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn find_active_password_invitation_for_user(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT invitation_id, user_id, token_hash, expires_at, consumed_at, revoked_at, created_at
                FROM password_invitations
                WHERE user_id = ?1
                  AND consumed_at IS NULL
                  AND revoked_at IS NULL
                  AND expires_at > ?2
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                libsql::params![user_id.to_string(), now.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_password_invitation_record(&row).map(Some)
    }

    pub async fn revoke_password_invitations_for_user(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE password_invitations
                SET revoked_at = ?1
                WHERE user_id = ?2 AND consumed_at IS NULL AND revoked_at IS NULL
                "#,
                libsql::params![revoked_at.unix_timestamp(), user_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn create_password_invitation(
        &self,
        invitation_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<PasswordInvitationRecord, StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO password_invitations (
                    invitation_id, user_id, token_hash, expires_at, consumed_at, revoked_at, created_at
                ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5)
                "#,
                libsql::params![
                    invitation_id.to_string(),
                    user_id.to_string(),
                    token_hash,
                    expires_at.unix_timestamp(),
                    created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;

        self.get_password_invitation(invitation_id)
            .await?
            .ok_or_else(|| {
                StoreError::NotFound(format!("password invitation `{invitation_id}` missing"))
            })
    }

    pub async fn get_password_invitation(
        &self,
        invitation_id: Uuid,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT invitation_id, user_id, token_hash, expires_at, consumed_at, revoked_at, created_at
                FROM password_invitations
                WHERE invitation_id = ?1
                LIMIT 1
                "#,
                [invitation_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_password_invitation_record(&row).map(Some)
    }

    pub async fn mark_password_invitation_consumed(
        &self,
        invitation_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE password_invitations
                SET consumed_at = ?1
                WHERE invitation_id = ?2
                "#,
                libsql::params![consumed_at.unix_timestamp(), invitation_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn store_user_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO user_password_auth (user_id, password_hash, password_updated_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(user_id) DO UPDATE SET
                    password_hash = excluded.password_hash,
                    password_updated_at = excluded.password_updated_at
                "#,
                libsql::params![
                    user_id.to_string(),
                    password_hash,
                    updated_at.unix_timestamp()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn update_user_status(
        &self,
        user_id: Uuid,
        status: &str,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE users
                SET status = ?1, updated_at = ?2
                WHERE user_id = ?3
                "#,
                libsql::params![status, updated_at.unix_timestamp(), user_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn update_user_must_change_password(
        &self,
        user_id: Uuid,
        must_change_password: bool,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE users
                SET must_change_password = ?1, updated_at = ?2
                WHERE user_id = ?3
                "#,
                libsql::params![
                    if must_change_password { 1_i64 } else { 0_i64 },
                    updated_at.unix_timestamp(),
                    user_id.to_string()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn create_user_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<UserSessionRecord, StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO user_sessions (
                    session_id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, NULL)
                "#,
                libsql::params![
                    session_id.to_string(),
                    user_id.to_string(),
                    token_hash,
                    expires_at.unix_timestamp(),
                    created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;

        self.get_user_session(session_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("user session `{session_id}` missing")))
    }

    pub async fn get_user_session(
        &self,
        session_id: Uuid,
    ) -> Result<Option<UserSessionRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT session_id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at
                FROM user_sessions
                WHERE session_id = ?1
                LIMIT 1
                "#,
                [session_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_user_session_record(&row).map(Some)
    }

    pub async fn touch_user_session(
        &self,
        session_id: Uuid,
        last_seen_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                "UPDATE user_sessions SET last_seen_at = ?1 WHERE session_id = ?2",
                libsql::params![last_seen_at.unix_timestamp(), session_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_id, oidc_provider_id, subject, email_claim, created_at
                FROM user_oidc_auth
                WHERE oidc_provider_id = ?1 AND subject = ?2
                LIMIT 1
                "#,
                libsql::params![oidc_provider_id, subject],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_user_oidc_auth_record(&row).map(Some)
    }

    pub async fn create_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        subject: &str,
        email_claim: Option<&str>,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO user_oidc_auth (user_id, oidc_provider_id, subject, email_claim, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                libsql::params![
                    user_id.to_string(),
                    oidc_provider_id,
                    subject,
                    email_claim,
                    created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn set_user_oidc_link(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO user_oidc_links (user_id, oidc_provider_id, created_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(user_id) DO UPDATE SET
                    oidc_provider_id = excluded.oidc_provider_id
                "#,
                libsql::params![
                    user_id.to_string(),
                    oidc_provider_id,
                    created_at.unix_timestamp()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn find_invited_oidc_user(
        &self,
        email_normalized: &str,
        oidc_provider_id: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT users.user_id, users.name, users.email, users.email_normalized,
                       users.global_role, users.auth_mode, users.status,
                       users.must_change_password, users.request_logging_enabled, users.model_access_mode,
                       users.created_at, users.updated_at
                FROM users
                INNER JOIN user_oidc_links ON user_oidc_links.user_id = users.user_id
                LEFT JOIN user_oidc_auth ON
                    user_oidc_auth.user_id = users.user_id
                    AND user_oidc_auth.oidc_provider_id = ?2
                WHERE users.email_normalized = ?1
                  AND users.auth_mode = 'oidc'
                  AND users.status = 'invited'
                  AND user_oidc_links.oidc_provider_id = ?2
                  AND user_oidc_auth.user_id IS NULL
                LIMIT 1
                "#,
                libsql::params![email_normalized, oidc_provider_id],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_user_record(&row).map(Some)
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
impl ApiKeyRepository for LibsqlStore {
    async fn get_api_key_by_public_id(
        &self,
        public_id: &str,
    ) -> Result<Option<ApiKeyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, public_id, secret_hash, name, status,
                       owner_kind, owner_user_id, owner_team_id,
                       created_at, last_used_at, revoked_at
                FROM api_keys
                WHERE public_id = ?1
                LIMIT 1
                "#,
                [public_id],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_api_key(&row).map(Some)
    }

    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        self.connection
            .execute(
                "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
                libsql::params![now, api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ModelRepository for LibsqlStore {
    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, model_key, description, tags_json, rank
                FROM gateway_models
                WHERE model_key = ?1
                LIMIT 1
                "#,
                [model_key],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_gateway_model(&row).map(Some)
    }

    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT gm.id, gm.model_key, gm.description, gm.tags_json, gm.rank
                FROM gateway_models gm
                INNER JOIN api_key_model_grants grants ON grants.model_id = gm.id
                WHERE grants.api_key_id = ?1
                ORDER BY gm.rank ASC, gm.model_key ASC
                "#,
                [api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut models = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            models.push(decode_gateway_model(&row)?);
        }

        Ok(models)
    }

    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled,
                       extra_headers_json, extra_body_json
                FROM model_routes
                WHERE model_id = ?1
                ORDER BY priority ASC
                "#,
                [model_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut routes = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            routes.push(decode_model_route(&row)?);
        }

        Ok(routes)
    }
}

#[async_trait]
impl ProviderRepository for LibsqlStore {
    async fn get_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<ProviderConnection>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT provider_key, provider_type, config_json, secrets_json
                FROM providers
                WHERE provider_key = ?1
                LIMIT 1
                "#,
                [provider_key],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_provider_connection(&row).map(Some)
    }
}

#[async_trait]
impl IdentityRepository for LibsqlStore {
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_id, name, email, email_normalized, global_role, auth_mode, status,
                       must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
                FROM users
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_user_record(&row).map(Some)
    }

    async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
                FROM teams
                WHERE team_id = ?1
                LIMIT 1
                "#,
                [team_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_team_record(&row).map(Some)
    }

    async fn get_team_membership_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<TeamMembershipRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_id, user_id, role, created_at, updated_at
                FROM team_memberships
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_team_membership_record(&row).map(Some)
    }

    async fn list_allowed_model_keys_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        list_allowed_model_keys(
            &self.connection,
            r#"
            SELECT gm.model_key
            FROM user_model_allowlist allowlist
            INNER JOIN gateway_models gm ON gm.id = allowlist.model_id
            WHERE allowlist.user_id = ?1
            ORDER BY gm.model_key ASC
            "#,
            user_id.to_string(),
        )
        .await
    }

    async fn list_allowed_model_keys_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        list_allowed_model_keys(
            &self.connection,
            r#"
            SELECT gm.model_key
            FROM team_model_allowlist allowlist
            INNER JOIN gateway_models gm ON gm.id = allowlist.model_id
            WHERE allowlist.team_id = ?1
            ORDER BY gm.model_key ASC
            "#,
            team_id.to_string(),
        )
        .await
    }
}

#[async_trait]
impl BudgetRepository for LibsqlStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserBudgetRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                       is_active, created_at, updated_at
                FROM user_budgets
                WHERE user_id = ?1 AND is_active = 1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_user_budget_record(&row).map(Some)
    }

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT COALESCE(SUM(estimated_cost_10000), 0)
                FROM usage_cost_events
                WHERE user_id = ?1
                  AND occurred_at >= ?2
                  AND occurred_at < ?3
                "#,
                libsql::params![
                    user_id.to_string(),
                    window_start.unix_timestamp(),
                    window_end.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(Money4::ZERO);
        };

        let sum_10000: i64 = row.get(0).map_err(to_query_error)?;
        Ok(Money4::from_scaled(sum_10000))
    }

    async fn insert_usage_cost_event(
        &self,
        event: &UsageCostEventRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO usage_cost_events (
                    usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                    estimated_cost_10000, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                libsql::params![
                    event.usage_event_id.to_string(),
                    event.request_id.as_str(),
                    event.api_key_id.to_string(),
                    event.user_id.map(|value| value.to_string()),
                    event.team_id.map(|value| value.to_string()),
                    event.model_id.map(|value| value.to_string()),
                    event.estimated_cost_usd.as_scaled_i64(),
                    event.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl RequestLogRepository for LibsqlStore {
    async fn insert_request_log(&self, log: &RequestLogRecord) -> Result<(), StoreError> {
        let metadata_json = serde_json::to_string(&log.metadata)
            .map_err(|error| StoreError::Serialization(error.to_string()))?;

        self.connection
            .execute(
                r#"
                INSERT INTO request_logs (
                    request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                    provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                    total_tokens, error_code, metadata_json, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
                libsql::params![
                    log.request_log_id.to_string(),
                    log.request_id.as_str(),
                    log.api_key_id.to_string(),
                    log.user_id.map(|value| value.to_string()),
                    log.team_id.map(|value| value.to_string()),
                    log.model_key.as_str(),
                    log.provider_key.as_str(),
                    log.status_code,
                    log.latency_ms,
                    log.prompt_tokens,
                    log.completion_tokens,
                    log.total_tokens,
                    log.error_code.as_deref(),
                    metadata_json,
                    log.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl PricingCatalogRepository for LibsqlStore {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT catalog_key, source, etag, fetched_at, snapshot_json
                FROM pricing_catalog_cache
                WHERE catalog_key = ?1
                LIMIT 1
                "#,
                [catalog_key],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        let fetched_at: i64 = row.get(3).map_err(to_query_error)?;
        Ok(Some(PricingCatalogCacheRecord {
            catalog_key: row.get(0).map_err(to_query_error)?,
            source: row.get(1).map_err(to_query_error)?,
            etag: row.get(2).map_err(to_query_error)?,
            fetched_at: unix_to_datetime(fetched_at)?,
            snapshot_json: row.get(4).map_err(to_query_error)?,
        }))
    }

    async fn upsert_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO pricing_catalog_cache (
                    catalog_key, source, etag, fetched_at, snapshot_json
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(catalog_key) DO UPDATE SET
                    source = excluded.source,
                    etag = excluded.etag,
                    fetched_at = excluded.fetched_at,
                    snapshot_json = excluded.snapshot_json
                "#,
                libsql::params![
                    cache.catalog_key.as_str(),
                    cache.source.as_str(),
                    cache.etag.as_deref(),
                    cache.fetched_at.unix_timestamp(),
                    cache.snapshot_json.as_str(),
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        Ok(())
    }

    async fn touch_pricing_catalog_cache_fetched_at(
        &self,
        catalog_key: &str,
        fetched_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE pricing_catalog_cache
                SET fetched_at = ?1
                WHERE catalog_key = ?2
                "#,
                libsql::params![fetched_at.unix_timestamp(), catalog_key],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        Ok(())
    }
}

async fn list_allowed_model_keys(
    connection: &libsql::Connection,
    sql: &str,
    owner_id: String,
) -> Result<Vec<String>, StoreError> {
    let mut rows = connection
        .query(sql, [owner_id])
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?;

    let mut model_keys = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?
    {
        let model_key: String = row.get(0).map_err(to_query_error)?;
        model_keys.push(model_key);
    }

    Ok(model_keys)
}

fn decode_api_key(row: &libsql::Row) -> Result<ApiKeyRecord, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let owner_kind: String = row.get(5).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.get(7).map_err(to_query_error)?;
    let created_at: i64 = row.get(8).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.get(9).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(10).map_err(to_query_error)?;

    Ok(ApiKeyRecord {
        id: Uuid::parse_str(&id).map_err(|error| StoreError::Serialization(error.to_string()))?,
        public_id: row.get(1).map_err(to_query_error)?,
        secret_hash: row.get(2).map_err(to_query_error)?,
        name: row.get(3).map_err(to_query_error)?,
        status: row.get(4).map_err(to_query_error)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
        })?,
        owner_user_id: owner_user_id
            .as_deref()
            .map(parse_uuid)
            .transpose()
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        owner_team_id: owner_team_id
            .as_deref()
            .map(parse_uuid)
            .transpose()
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        created_at: unix_to_datetime(created_at)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_gateway_model(row: &libsql::Row) -> Result<GatewayModel, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let tags_json: String = row.get(3).map_err(to_query_error)?;

    Ok(GatewayModel {
        id: parse_uuid(&id)?,
        model_key: row.get(1).map_err(to_query_error)?,
        description: row.get(2).map_err(to_query_error)?,
        tags: serde_json::from_str(&tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        rank: row.get(4).map_err(to_query_error)?,
    })
}

fn decode_model_route(row: &libsql::Row) -> Result<ModelRoute, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let model_id: String = row.get(1).map_err(to_query_error)?;
    let enabled: i64 = row.get(6).map_err(to_query_error)?;
    let extra_headers_json: String = row.get(7).map_err(to_query_error)?;
    let extra_body_json: String = row.get(8).map_err(to_query_error)?;

    Ok(ModelRoute {
        id: parse_uuid(&id)?,
        model_id: parse_uuid(&model_id)?,
        provider_key: row.get(2).map_err(to_query_error)?,
        upstream_model: row.get(3).map_err(to_query_error)?,
        priority: row.get(4).map_err(to_query_error)?,
        weight: row.get(5).map_err(to_query_error)?,
        enabled: enabled == 1,
        extra_headers: json_object_from_str(&extra_headers_json)?,
        extra_body: json_object_from_str(&extra_body_json)?,
    })
}

fn decode_provider_connection(row: &libsql::Row) -> Result<ProviderConnection, StoreError> {
    let config_json: String = row.get(2).map_err(to_query_error)?;
    let secrets_json: Option<String> = row.get(3).map_err(to_query_error)?;

    Ok(ProviderConnection {
        provider_key: row.get(0).map_err(to_query_error)?,
        provider_type: row.get(1).map_err(to_query_error)?,
        config: serde_json::from_str(&config_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        secrets: secrets_json
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| StoreError::Serialization(error.to_string()))
            })
            .transpose()?,
    })
}

fn decode_user_record(row: &libsql::Row) -> Result<UserRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let global_role: String = row.get(4).map_err(to_query_error)?;
    let auth_mode: String = row.get(5).map_err(to_query_error)?;
    let must_change_password: i64 = row.get(7).map_err(to_query_error)?;
    let request_logging_enabled: i64 = row.get(8).map_err(to_query_error)?;
    let model_access_mode: String = row.get(9).map_err(to_query_error)?;
    let created_at: i64 = row.get(10).map_err(to_query_error)?;
    let updated_at: i64 = row.get(11).map_err(to_query_error)?;

    Ok(UserRecord {
        user_id: parse_uuid(&user_id)?,
        name: row.get(1).map_err(to_query_error)?,
        email: row.get(2).map_err(to_query_error)?,
        email_normalized: row.get(3).map_err(to_query_error)?,
        global_role: GlobalRole::from_db(&global_role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown global role `{global_role}`"))
        })?,
        auth_mode: AuthMode::from_db(&auth_mode)
            .ok_or_else(|| StoreError::Serialization(format!("unknown auth mode `{auth_mode}`")))?,
        status: row.get(6).map_err(to_query_error)?,
        must_change_password: must_change_password == 1,
        request_logging_enabled: request_logging_enabled == 1,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_identity_user_record(row: &libsql::Row) -> Result<IdentityUserRecord, StoreError> {
    let team_id: Option<String> = row.get(12).map_err(to_query_error)?;
    let membership_role: Option<String> = row.get(14).map_err(to_query_error)?;
    let oidc_provider_id: Option<String> = row.get(15).map_err(to_query_error)?;
    let membership_role = match membership_role {
        Some(role) => Some(MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?),
        None => None,
    };

    Ok(IdentityUserRecord {
        user: decode_user_record(row)?,
        team_id: team_id
            .as_deref()
            .map(parse_uuid)
            .transpose()
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        team_name: row.get(13).map_err(to_query_error)?,
        membership_role,
        oidc_provider_id,
        oidc_provider_key: row.get(16).map_err(to_query_error)?,
    })
}

fn decode_user_password_auth_record(
    row: &libsql::Row,
) -> Result<UserPasswordAuthRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let password_updated_at: i64 = row.get(2).map_err(to_query_error)?;

    Ok(UserPasswordAuthRecord {
        user_id: parse_uuid(&user_id)?,
        password_hash: row.get(1).map_err(to_query_error)?,
        password_updated_at: unix_to_datetime(password_updated_at)?,
    })
}

fn decode_team_record(row: &libsql::Row) -> Result<TeamRecord, StoreError> {
    let team_id: String = row.get(0).map_err(to_query_error)?;
    let model_access_mode: String = row.get(4).map_err(to_query_error)?;
    let created_at: i64 = row.get(5).map_err(to_query_error)?;
    let updated_at: i64 = row.get(6).map_err(to_query_error)?;

    Ok(TeamRecord {
        team_id: parse_uuid(&team_id)?,
        team_key: row.get(1).map_err(to_query_error)?,
        team_name: row.get(2).map_err(to_query_error)?,
        status: row.get(3).map_err(to_query_error)?,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_team_membership_record(row: &libsql::Row) -> Result<TeamMembershipRecord, StoreError> {
    let team_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let role: String = row.get(2).map_err(to_query_error)?;
    let created_at: i64 = row.get(3).map_err(to_query_error)?;
    let updated_at: i64 = row.get(4).map_err(to_query_error)?;

    Ok(TeamMembershipRecord {
        team_id: parse_uuid(&team_id)?,
        user_id: parse_uuid(&user_id)?,
        role: MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_oidc_provider_record(row: &libsql::Row) -> Result<OidcProviderRecord, StoreError> {
    let scopes_json: String = row.get(5).map_err(to_query_error)?;
    let enabled: i64 = row.get(6).map_err(to_query_error)?;
    let created_at: i64 = row.get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.get(8).map_err(to_query_error)?;

    Ok(OidcProviderRecord {
        oidc_provider_id: row.get(0).map_err(to_query_error)?,
        provider_key: row.get(1).map_err(to_query_error)?,
        provider_type: row.get(2).map_err(to_query_error)?,
        issuer_url: row.get(3).map_err(to_query_error)?,
        client_id: row.get(4).map_err(to_query_error)?,
        scopes: serde_json::from_str(&scopes_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        enabled: enabled == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_password_invitation_record(
    row: &libsql::Row,
) -> Result<PasswordInvitationRecord, StoreError> {
    let invitation_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let expires_at: i64 = row.get(3).map_err(to_query_error)?;
    let consumed_at: Option<i64> = row.get(4).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(5).map_err(to_query_error)?;
    let created_at: i64 = row.get(6).map_err(to_query_error)?;

    Ok(PasswordInvitationRecord {
        invitation_id: parse_uuid(&invitation_id)?,
        user_id: parse_uuid(&user_id)?,
        token_hash: row.get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        consumed_at: consumed_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(created_at)?,
    })
}

fn decode_user_session_record(row: &libsql::Row) -> Result<UserSessionRecord, StoreError> {
    let session_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let expires_at: i64 = row.get(3).map_err(to_query_error)?;
    let created_at: i64 = row.get(4).map_err(to_query_error)?;
    let last_seen_at: i64 = row.get(5).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(6).map_err(to_query_error)?;

    Ok(UserSessionRecord {
        session_id: parse_uuid(&session_id)?,
        user_id: parse_uuid(&user_id)?,
        token_hash: row.get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        created_at: unix_to_datetime(created_at)?,
        last_seen_at: unix_to_datetime(last_seen_at)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_user_oidc_auth_record(row: &libsql::Row) -> Result<UserOidcAuthRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let created_at: i64 = row.get(4).map_err(to_query_error)?;

    Ok(UserOidcAuthRecord {
        user_id: parse_uuid(&user_id)?,
        oidc_provider_id: row.get(1).map_err(to_query_error)?,
        subject: row.get(2).map_err(to_query_error)?,
        email_claim: row.get(3).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
    })
}

fn decode_user_budget_record(row: &libsql::Row) -> Result<UserBudgetRecord, StoreError> {
    let user_budget_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let cadence: String = row.get(2).map_err(to_query_error)?;
    let amount_10000: i64 = row.get(3).map_err(to_query_error)?;
    let hard_limit: i64 = row.get(4).map_err(to_query_error)?;
    let is_active: i64 = row.get(6).map_err(to_query_error)?;
    let created_at: i64 = row.get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.get(8).map_err(to_query_error)?;

    Ok(UserBudgetRecord {
        user_budget_id: parse_uuid(&user_budget_id)?,
        user_id: parse_uuid(&user_id)?,
        cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
            StoreError::Serialization(format!("unknown budget cadence `{cadence}`"))
        })?,
        amount_usd: Money4::from_scaled(amount_10000),
        hard_limit: hard_limit == 1,
        timezone: row.get(5).map_err(to_query_error)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn json_object_from_str(value: &str) -> Result<Map<String, Value>, StoreError> {
    serde_json::from_str(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn unix_to_datetime(ts: i64) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::from_unix_timestamp(ts)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn parse_uuid(raw: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn to_query_error(error: libsql::Error) -> StoreError {
    StoreError::Query(error.to_string())
}

fn to_write_error(error: libsql::Error) -> StoreError {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed")
        || message.contains("CHECK constraint failed")
        || message.contains("FOREIGN KEY constraint failed")
    {
        return StoreError::Conflict(message);
    }

    StoreError::Query(message)
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

    async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        Self::list_active_teams(self).await
    }

    async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        Self::list_teams(self).await
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

    async fn create_identity_user(
        &self,
        name: &str,
        email: &str,
        email_normalized: &str,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        status: &str,
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

    async fn assign_team_membership(
        &self,
        user_id: Uuid,
        team_id: Uuid,
        role: MembershipRole,
    ) -> Result<(), StoreError> {
        Self::assign_team_membership(self, user_id, team_id, role).await
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
        Self::create_password_invitation(self, invitation_id, user_id, token_hash, expires_at, created_at)
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
        status: &str,
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
        Self::update_user_must_change_password(self, user_id, must_change_password, updated_at).await
    }

    async fn create_user_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: &str,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<UserSessionRecord, StoreError> {
        Self::create_user_session(self, session_id, user_id, token_hash, expires_at, created_at).await
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

    async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        Self::get_user_oidc_auth(self, oidc_provider_id, subject).await
    }

    async fn create_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        subject: &str,
        email_claim: Option<&str>,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        Self::create_user_oidc_auth(self, user_id, oidc_provider_id, subject, email_claim, created_at)
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

    async fn find_invited_oidc_user(
        &self,
        email_normalized: &str,
        oidc_provider_id: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        Self::find_invited_oidc_user(self, email_normalized, oidc_provider_id).await
    }

    async fn seed_from_inputs(
        &self,
        providers: &[gateway_core::SeedProvider],
        models: &[gateway_core::SeedModel],
        api_keys: &[gateway_core::SeedApiKey],
    ) -> Result<(), StoreError> {
        self.seed_from_inputs(providers, models, api_keys).await
    }
}
