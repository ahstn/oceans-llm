use anyhow::Context;
use async_trait::async_trait;
use gateway_core::{
    ApiKeyOwnerKind, ApiKeyRecord, ApiKeyRepository, AuthMode, BudgetCadence, BudgetRepository,
    GatewayModel, GlobalRole, IdentityRepository, IdentityUserRecord, MembershipRole,
    ModelAccessMode, ModelRepository, ModelRoute, Money4, OidcProviderRecord,
    PasswordInvitationRecord, PricingCatalogCacheRecord, PricingCatalogRepository,
    ProviderConnection, ProviderRepository, RequestLogRecord, RequestLogRepository,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY, StoreError,
    StoreHealth, TeamMembershipRecord, TeamRecord, UsageCostEventRecord, UserBudgetRecord,
    UserOidcAuthRecord, UserPasswordAuthRecord, UserRecord, UserSessionRecord,
};
use serde_json::{Map, Value};
use sqlx::{
    PgPool, Row,
    postgres::{PgPoolOptions, PgRow},
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::seed::{api_key_uuid, model_uuid, route_uuid};

#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub async fn connect(url: &str, max_connections: u32) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.max(1))
            .connect(url)
            .await
            .with_context(|| "failed opening postgres connection pool".to_string())?;

        Ok(Self { pool })
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn has_platform_admin(&self) -> Result<bool, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT 1
            FROM users
            WHERE global_role = 'platform_admin'
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        Ok(row.is_some())
    }

    pub async fn upsert_bootstrap_admin_user(
        &self,
        name: &str,
        email: &str,
        must_change_password: bool,
    ) -> Result<UserRecord, StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let email_normalized = email.trim().to_ascii_lowercase();

        sqlx::query(
            r#"
            INSERT INTO users (
                user_id, name, email, email_normalized, global_role, auth_mode, status,
                must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, 'platform_admin', 'password', 'active', $5, 1, 'all', $6, $6)
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
        )
        .bind(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
        .bind(name)
        .bind(email)
        .bind(email_normalized)
        .bind(if must_change_password { 1_i64 } else { 0_i64 })
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;

        self.get_user_by_id(parse_uuid(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)?)
            .await?
            .ok_or_else(|| StoreError::NotFound("bootstrap admin user missing".to_string()))
    }

    pub async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
        let rows = sqlx::query(
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
            WHERE users.user_id != $1
            ORDER BY users.created_at DESC, users.email_normalized ASC
            "#,
        )
        .bind(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_identity_user_record).collect()
    }

    pub async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            FROM teams
            WHERE status = 'active'
            ORDER BY team_name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_team_record).collect()
    }

    pub async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            FROM teams
            ORDER BY team_name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_team_record).collect()
    }

    pub async fn list_enabled_oidc_providers(&self) -> Result<Vec<OidcProviderRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                   scopes_json, enabled, created_at, updated_at
            FROM oidc_providers
            WHERE enabled = 1
            ORDER BY provider_key ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_oidc_provider_record).collect()
    }

    pub async fn get_enabled_oidc_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<OidcProviderRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                   scopes_json, enabled, created_at, updated_at
            FROM oidc_providers
            WHERE provider_key = $1 AND enabled = 1
            LIMIT 1
            "#,
        )
        .bind(provider_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_oidc_provider_record).transpose()
    }

    pub async fn get_user_by_email_normalized(
        &self,
        email_normalized: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_id, name, email, email_normalized, global_role, auth_mode, status,
                   must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
            FROM users
            WHERE email_normalized = $1
            LIMIT 1
            "#,
        )
        .bind(email_normalized)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_record).transpose()
    }

    pub async fn get_team_by_key(&self, team_key: &str) -> Result<Option<TeamRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            FROM teams
            WHERE team_key = $1
            LIMIT 1
            "#,
        )
        .bind(team_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_team_record).transpose()
    }

    pub async fn create_team(
        &self,
        team_key: &str,
        team_name: &str,
    ) -> Result<TeamRecord, StoreError> {
        let team_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc().unix_timestamp();

        sqlx::query(
            r#"
            INSERT INTO teams (
                team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES ($1, $2, $3, 'active', 'all', $4, $4)
            "#,
        )
        .bind(team_id.to_string())
        .bind(team_key)
        .bind(team_name)
        .bind(now)
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            UPDATE teams
            SET team_name = $1, updated_at = $2
            WHERE team_id = $3
            "#,
        )
        .bind(team_name)
        .bind(updated_at.unix_timestamp())
        .bind(team_id.to_string())
        .execute(&self.pool)
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

        sqlx::query(
            r#"
            INSERT INTO users (
                user_id, name, email, email_normalized, global_role, auth_mode, status,
                must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, 0, 1, 'all', $8, $8)
            "#,
        )
        .bind(user_id.to_string())
        .bind(name)
        .bind(email)
        .bind(email_normalized)
        .bind(global_role.as_str())
        .bind(auth_mode.as_str())
        .bind(status)
        .bind(now)
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            INSERT INTO team_memberships (team_id, user_id, role, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $4)
            "#,
        )
        .bind(team_id.to_string())
        .bind(user_id.to_string())
        .bind(role.as_str())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn get_user_password_auth(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserPasswordAuthRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_id, password_hash, password_updated_at
            FROM user_password_auth
            WHERE user_id = $1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_password_auth_record).transpose()
    }

    pub async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT team_id, user_id, role, created_at, updated_at
            FROM team_memberships
            WHERE team_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(team_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_team_membership_record).collect()
    }

    pub async fn update_team_membership_role(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE team_memberships
            SET role = $1, updated_at = $2
            WHERE team_id = $3 AND user_id = $4
            "#,
        )
        .bind(role.as_str())
        .bind(updated_at.unix_timestamp())
        .bind(team_id.to_string())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn find_active_password_invitation_for_user(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<PasswordInvitationRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT invitation_id, user_id, token_hash, expires_at, consumed_at, revoked_at, created_at
            FROM password_invitations
            WHERE user_id = $1
              AND consumed_at IS NULL
              AND revoked_at IS NULL
              AND expires_at > $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .bind(now.unix_timestamp())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_password_invitation_record).transpose()
    }

    pub async fn revoke_password_invitations_for_user(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE password_invitations
            SET revoked_at = $1
            WHERE user_id = $2 AND consumed_at IS NULL AND revoked_at IS NULL
            "#,
        )
        .bind(revoked_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            INSERT INTO password_invitations (
                invitation_id, user_id, token_hash, expires_at, consumed_at, revoked_at, created_at
            ) VALUES ($1, $2, $3, $4, NULL, NULL, $5)
            "#,
        )
        .bind(invitation_id.to_string())
        .bind(user_id.to_string())
        .bind(token_hash)
        .bind(expires_at.unix_timestamp())
        .bind(created_at.unix_timestamp())
        .execute(&self.pool)
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
        let row = sqlx::query(
            r#"
            SELECT invitation_id, user_id, token_hash, expires_at, consumed_at, revoked_at, created_at
            FROM password_invitations
            WHERE invitation_id = $1
            LIMIT 1
            "#,
        )
        .bind(invitation_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_password_invitation_record).transpose()
    }

    pub async fn mark_password_invitation_consumed(
        &self,
        invitation_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE password_invitations
            SET consumed_at = $1
            WHERE invitation_id = $2
            "#,
        )
        .bind(consumed_at.unix_timestamp())
        .bind(invitation_id.to_string())
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            INSERT INTO user_password_auth (user_id, password_hash, password_updated_at)
            VALUES ($1, $2, $3)
            ON CONFLICT(user_id) DO UPDATE SET
                password_hash = excluded.password_hash,
                password_updated_at = excluded.password_updated_at
            "#,
        )
        .bind(user_id.to_string())
        .bind(password_hash)
        .bind(updated_at.unix_timestamp())
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            UPDATE users
            SET status = $1, updated_at = $2
            WHERE user_id = $3
            "#,
        )
        .bind(status)
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            UPDATE users
            SET must_change_password = $1, updated_at = $2
            WHERE user_id = $3
            "#,
        )
        .bind(if must_change_password { 1_i64 } else { 0_i64 })
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            INSERT INTO user_sessions (
                session_id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at
            ) VALUES ($1, $2, $3, $4, $5, $5, NULL)
            "#,
        )
        .bind(session_id.to_string())
        .bind(user_id.to_string())
        .bind(token_hash)
        .bind(expires_at.unix_timestamp())
        .bind(created_at.unix_timestamp())
        .execute(&self.pool)
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
        let row = sqlx::query(
            r#"
            SELECT session_id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at
            FROM user_sessions
            WHERE session_id = $1
            LIMIT 1
            "#,
        )
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_session_record).transpose()
    }

    pub async fn touch_user_session(
        &self,
        session_id: Uuid,
        last_seen_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE user_sessions SET last_seen_at = $1 WHERE session_id = $2")
            .bind(last_seen_at.unix_timestamp())
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn get_user_oidc_auth(
        &self,
        oidc_provider_id: &str,
        subject: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_id, oidc_provider_id, subject, email_claim, created_at
            FROM user_oidc_auth
            WHERE oidc_provider_id = $1 AND subject = $2
            LIMIT 1
            "#,
        )
        .bind(oidc_provider_id)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_oidc_auth_record).transpose()
    }

    pub async fn create_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
        subject: &str,
        email_claim: Option<&str>,
        created_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO user_oidc_auth (user_id, oidc_provider_id, subject, email_claim, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(user_id.to_string())
        .bind(oidc_provider_id)
        .bind(subject)
        .bind(email_claim)
        .bind(created_at.unix_timestamp())
        .execute(&self.pool)
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
        sqlx::query(
            r#"
            INSERT INTO user_oidc_links (user_id, oidc_provider_id, created_at)
            VALUES ($1, $2, $3)
            ON CONFLICT(user_id) DO UPDATE SET
                oidc_provider_id = excluded.oidc_provider_id
            "#,
        )
        .bind(user_id.to_string())
        .bind(oidc_provider_id)
        .bind(created_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn find_invited_oidc_user(
        &self,
        email_normalized: &str,
        oidc_provider_id: &str,
    ) -> Result<Option<UserRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT users.user_id, users.name, users.email, users.email_normalized,
                   users.global_role, users.auth_mode, users.status,
                   users.must_change_password, users.request_logging_enabled, users.model_access_mode,
                   users.created_at, users.updated_at
            FROM users
            INNER JOIN user_oidc_links ON user_oidc_links.user_id = users.user_id
            LEFT JOIN user_oidc_auth ON
                user_oidc_auth.user_id = users.user_id
                AND user_oidc_auth.oidc_provider_id = $2
            WHERE users.email_normalized = $1
              AND users.auth_mode = 'oidc'
              AND users.status = 'invited'
              AND user_oidc_links.oidc_provider_id = $2
              AND user_oidc_auth.user_id IS NULL
            LIMIT 1
            "#,
        )
        .bind(email_normalized)
        .bind(oidc_provider_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_record).transpose()
    }

    pub async fn seed_from_inputs(
        &self,
        providers: &[gateway_core::SeedProvider],
        models: &[gateway_core::SeedModel],
        api_keys: &[gateway_core::SeedApiKey],
    ) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();

        sqlx::query(
            r#"
            INSERT INTO teams (
                team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES ($1, $2, 'System Legacy', 'active', 'all', $3, $3)
            ON CONFLICT(team_id) DO NOTHING
            "#,
        )
        .bind(SYSTEM_LEGACY_TEAM_ID)
        .bind(SYSTEM_LEGACY_TEAM_KEY)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

        for provider in providers {
            let config_json = serde_json::to_string(&provider.config)
                .map_err(|error| StoreError::Serialization(error.to_string()))?;
            let secrets_json = provider
                .secrets
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| StoreError::Serialization(error.to_string()))?;

            sqlx::query(
                r#"
                INSERT INTO providers (
                    provider_key, provider_type, config_json, secrets_json, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $5)
                ON CONFLICT(provider_key) DO UPDATE SET
                    provider_type = excluded.provider_type,
                    config_json = excluded.config_json,
                    secrets_json = excluded.secrets_json,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(provider.provider_key.as_str())
            .bind(provider.provider_type.as_str())
            .bind(config_json)
            .bind(secrets_json)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        }

        let mut model_ids = std::collections::HashMap::new();
        for model in models {
            let model_id = model_uuid(&model.model_key);
            model_ids.insert(model.model_key.clone(), model_id);
            let tags_json = serde_json::to_string(&model.tags)
                .map_err(|error| StoreError::Serialization(error.to_string()))?;

            sqlx::query(
                r#"
                INSERT INTO gateway_models (
                    id, model_key, description, tags_json, rank, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, $6)
                ON CONFLICT(model_key) DO UPDATE SET
                    description = excluded.description,
                    tags_json = excluded.tags_json,
                    rank = excluded.rank,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(model_id.to_string())
            .bind(model.model_key.as_str())
            .bind(model.description.clone())
            .bind(tags_json)
            .bind(model.rank)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;

            sqlx::query("DELETE FROM model_routes WHERE model_id = $1")
                .bind(model_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;

            for (route_index, route) in model.routes.iter().enumerate() {
                let route_id = route_uuid(
                    &model.model_key,
                    &route.provider_key,
                    &route.upstream_model,
                    route.priority,
                    route_index,
                );
                let extra_headers_json = serde_json::to_string(&route.extra_headers)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?;
                let extra_body_json = serde_json::to_string(&route.extra_body)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?;

                sqlx::query(
                    r#"
                    INSERT INTO model_routes (
                        id, model_id, provider_key, upstream_model, priority, weight, enabled,
                        extra_headers_json, extra_body_json, created_at, updated_at
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
                    ON CONFLICT(id) DO UPDATE SET
                        weight = excluded.weight,
                        enabled = excluded.enabled,
                        extra_headers_json = excluded.extra_headers_json,
                        extra_body_json = excluded.extra_body_json,
                        updated_at = excluded.updated_at
                    "#,
                )
                .bind(route_id.to_string())
                .bind(model_id.to_string())
                .bind(route.provider_key.as_str())
                .bind(route.upstream_model.as_str())
                .bind(route.priority)
                .bind(route.weight)
                .bind(if route.enabled { 1_i64 } else { 0_i64 })
                .bind(extra_headers_json)
                .bind(extra_body_json)
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;
            }
        }

        for api_key in api_keys {
            let key_id = api_key_uuid(&api_key.public_id);

            sqlx::query(
                r#"
                INSERT INTO api_keys (
                    id, public_id, secret_hash, name, status,
                    owner_kind, owner_user_id, owner_team_id, created_at
                ) VALUES ($1, $2, $3, $4, 'active', 'team', NULL, $5, $6)
                ON CONFLICT(public_id) DO UPDATE SET
                    secret_hash = excluded.secret_hash,
                    name = excluded.name,
                    owner_kind = excluded.owner_kind,
                    owner_user_id = excluded.owner_user_id,
                    owner_team_id = excluded.owner_team_id
                "#,
            )
            .bind(key_id.to_string())
            .bind(api_key.public_id.as_str())
            .bind(api_key.secret_hash.as_str())
            .bind(api_key.name.as_str())
            .bind(SYSTEM_LEGACY_TEAM_ID)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;

            sqlx::query("DELETE FROM api_key_model_grants WHERE api_key_id = $1")
                .bind(key_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;

            for model_key in &api_key.allowed_models {
                let model_id = model_ids.get(model_key).ok_or_else(|| {
                    StoreError::NotFound(format!(
                        "seed api key `{}` references unknown model `{model_key}`",
                        api_key.public_id
                    ))
                })?;

                sqlx::query(
                    r#"
                    INSERT INTO api_key_model_grants (api_key_id, model_id)
                    VALUES ($1, $2)
                    ON CONFLICT(api_key_id, model_id) DO NOTHING
                    "#,
                )
                .bind(key_id.to_string())
                .bind(model_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl StoreHealth for PostgresStore {
    async fn ping(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ApiKeyRepository for PostgresStore {
    async fn get_api_key_by_public_id(
        &self,
        public_id: &str,
    ) -> Result<Option<ApiKeyRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, public_id, secret_hash, name, status,
                   owner_kind, owner_user_id, owner_team_id,
                   created_at, last_used_at, revoked_at
            FROM api_keys
            WHERE public_id = $1
            LIMIT 1
            "#,
        )
        .bind(public_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_api_key).transpose()
    }

    async fn touch_api_key_last_used(&self, api_key_id: Uuid) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE id = $2")
            .bind(now)
            .bind(api_key_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        Ok(())
    }
}

#[async_trait]
impl ModelRepository for PostgresStore {
    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, model_key, description, tags_json, rank
            FROM gateway_models
            WHERE model_key = $1
            LIMIT 1
            "#,
        )
        .bind(model_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_gateway_model).transpose()
    }

    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT gm.id, gm.model_key, gm.description, gm.tags_json, gm.rank
            FROM gateway_models gm
            INNER JOIN api_key_model_grants grants ON grants.model_id = gm.id
            WHERE grants.api_key_id = $1
            ORDER BY gm.rank ASC, gm.model_key ASC
            "#,
        )
        .bind(api_key_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_gateway_model).collect()
    }

    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled,
                   extra_headers_json, extra_body_json
            FROM model_routes
            WHERE model_id = $1
            ORDER BY priority ASC
            "#,
        )
        .bind(model_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_model_route).collect()
    }
}

#[async_trait]
impl ProviderRepository for PostgresStore {
    async fn get_provider_by_key(
        &self,
        provider_key: &str,
    ) -> Result<Option<ProviderConnection>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT provider_key, provider_type, config_json, secrets_json
            FROM providers
            WHERE provider_key = $1
            LIMIT 1
            "#,
        )
        .bind(provider_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_provider_connection).transpose()
    }
}

#[async_trait]
impl IdentityRepository for PostgresStore {
    async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_id, name, email, email_normalized, global_role, auth_mode, status,
                   must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
            FROM users
            WHERE user_id = $1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_record).transpose()
    }

    async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            FROM teams
            WHERE team_id = $1
            LIMIT 1
            "#,
        )
        .bind(team_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_team_record).transpose()
    }

    async fn get_team_membership_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<TeamMembershipRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT team_id, user_id, role, created_at, updated_at
            FROM team_memberships
            WHERE user_id = $1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_team_membership_record).transpose()
    }

    async fn list_allowed_model_keys_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        list_allowed_model_keys(
            &self.pool,
            r#"
            SELECT gm.model_key
            FROM user_model_allowlist allowlist
            INNER JOIN gateway_models gm ON gm.id = allowlist.model_id
            WHERE allowlist.user_id = $1
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
            &self.pool,
            r#"
            SELECT gm.model_key
            FROM team_model_allowlist allowlist
            INNER JOIN gateway_models gm ON gm.id = allowlist.model_id
            WHERE allowlist.team_id = $1
            ORDER BY gm.model_key ASC
            "#,
            team_id.to_string(),
        )
        .await
    }
}

#[async_trait]
impl BudgetRepository for PostgresStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserBudgetRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                   is_active, created_at, updated_at
            FROM user_budgets
            WHERE user_id = $1 AND is_active = 1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_budget_record).transpose()
    }

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(estimated_cost_10000), 0)
            FROM usage_cost_events
            WHERE user_id = $1
              AND occurred_at >= $2
              AND occurred_at < $3
            "#,
        )
        .bind(user_id.to_string())
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;

        Ok(Money4::from_scaled(
            row.try_get::<i64, _>(0).map_err(to_query_error)?,
        ))
    }

    async fn insert_usage_cost_event(
        &self,
        event: &UsageCostEventRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO usage_cost_events (
                usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                estimated_cost_10000, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event.usage_event_id.to_string())
        .bind(event.request_id.as_str())
        .bind(event.api_key_id.to_string())
        .bind(event.user_id.map(|value| value.to_string()))
        .bind(event.team_id.map(|value| value.to_string()))
        .bind(event.model_id.map(|value| value.to_string()))
        .bind(event.estimated_cost_usd.as_scaled_i64())
        .bind(event.occurred_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}

#[async_trait]
impl RequestLogRepository for PostgresStore {
    async fn insert_request_log(&self, log: &RequestLogRecord) -> Result<(), StoreError> {
        let metadata_json = serde_json::to_string(&log.metadata)
            .map_err(|error| StoreError::Serialization(error.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                total_tokens, error_code, metadata_json, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
        )
        .bind(log.request_log_id.to_string())
        .bind(log.request_id.as_str())
        .bind(log.api_key_id.to_string())
        .bind(log.user_id.map(|value| value.to_string()))
        .bind(log.team_id.map(|value| value.to_string()))
        .bind(log.model_key.as_str())
        .bind(log.provider_key.as_str())
        .bind(log.status_code)
        .bind(log.latency_ms)
        .bind(log.prompt_tokens)
        .bind(log.completion_tokens)
        .bind(log.total_tokens)
        .bind(log.error_code.as_deref())
        .bind(metadata_json)
        .bind(log.occurred_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}

#[async_trait]
impl PricingCatalogRepository for PostgresStore {
    async fn get_pricing_catalog_cache(
        &self,
        catalog_key: &str,
    ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT catalog_key, source, etag, fetched_at, snapshot_json
            FROM pricing_catalog_cache
            WHERE catalog_key = $1
            LIMIT 1
            "#,
        )
        .bind(catalog_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_pricing_catalog_cache_record).transpose()
    }

    async fn upsert_pricing_catalog_cache(
        &self,
        cache: &PricingCatalogCacheRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO pricing_catalog_cache (
                catalog_key, source, etag, fetched_at, snapshot_json
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT(catalog_key) DO UPDATE SET
                source = excluded.source,
                etag = excluded.etag,
                fetched_at = excluded.fetched_at,
                snapshot_json = excluded.snapshot_json
            "#,
        )
        .bind(cache.catalog_key.as_str())
        .bind(cache.source.as_str())
        .bind(cache.etag.as_deref())
        .bind(cache.fetched_at.unix_timestamp())
        .bind(cache.snapshot_json.as_str())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }

    async fn touch_pricing_catalog_cache_fetched_at(
        &self,
        catalog_key: &str,
        fetched_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE pricing_catalog_cache
            SET fetched_at = $1
            WHERE catalog_key = $2
            "#,
        )
        .bind(fetched_at.unix_timestamp())
        .bind(catalog_key)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}

async fn list_allowed_model_keys(
    pool: &PgPool,
    sql: &str,
    owner_id: String,
) -> Result<Vec<String>, StoreError> {
    let rows = sqlx::query(sql)
        .bind(owner_id)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;

    rows.iter()
        .map(|row| row.try_get::<String, _>(0).map_err(to_query_error))
        .collect()
}

fn decode_api_key(row: &PgRow) -> Result<ApiKeyRecord, StoreError> {
    let owner_kind: String = row.try_get(5).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.try_get(6).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.try_get(7).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(8).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.try_get(9).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(10).map_err(to_query_error)?;

    Ok(ApiKeyRecord {
        id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        public_id: row.try_get(1).map_err(to_query_error)?,
        secret_hash: row.try_get(2).map_err(to_query_error)?,
        name: row.try_get(3).map_err(to_query_error)?,
        status: row.try_get(4).map_err(to_query_error)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
        })?,
        owner_user_id: owner_user_id.as_deref().map(parse_uuid).transpose()?,
        owner_team_id: owner_team_id.as_deref().map(parse_uuid).transpose()?,
        created_at: unix_to_datetime(created_at)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_gateway_model(row: &PgRow) -> Result<GatewayModel, StoreError> {
    let tags_json: String = row.try_get(3).map_err(to_query_error)?;
    Ok(GatewayModel {
        id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        model_key: row.try_get(1).map_err(to_query_error)?,
        description: row.try_get(2).map_err(to_query_error)?,
        tags: serde_json::from_str(&tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        rank: row.try_get(4).map_err(to_query_error)?,
    })
}

fn decode_model_route(row: &PgRow) -> Result<ModelRoute, StoreError> {
    let enabled: i64 = row.try_get(6).map_err(to_query_error)?;
    let extra_headers_json: String = row.try_get(7).map_err(to_query_error)?;
    let extra_body_json: String = row.try_get(8).map_err(to_query_error)?;

    Ok(ModelRoute {
        id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        model_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        provider_key: row.try_get(2).map_err(to_query_error)?,
        upstream_model: row.try_get(3).map_err(to_query_error)?,
        priority: row.try_get(4).map_err(to_query_error)?,
        weight: row.try_get(5).map_err(to_query_error)?,
        enabled: enabled == 1,
        extra_headers: json_object_from_str(&extra_headers_json)?,
        extra_body: json_object_from_str(&extra_body_json)?,
    })
}

fn decode_provider_connection(row: &PgRow) -> Result<ProviderConnection, StoreError> {
    let config_json: String = row.try_get(2).map_err(to_query_error)?;
    let secrets_json: Option<String> = row.try_get(3).map_err(to_query_error)?;

    Ok(ProviderConnection {
        provider_key: row.try_get(0).map_err(to_query_error)?,
        provider_type: row.try_get(1).map_err(to_query_error)?,
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

fn decode_user_record(row: &PgRow) -> Result<UserRecord, StoreError> {
    let global_role: String = row.try_get(4).map_err(to_query_error)?;
    let auth_mode: String = row.try_get(5).map_err(to_query_error)?;
    let must_change_password: i64 = row.try_get(7).map_err(to_query_error)?;
    let request_logging_enabled: i64 = row.try_get(8).map_err(to_query_error)?;
    let model_access_mode: String = row.try_get(9).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(10).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(11).map_err(to_query_error)?;

    Ok(UserRecord {
        user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        name: row.try_get(1).map_err(to_query_error)?,
        email: row.try_get(2).map_err(to_query_error)?,
        email_normalized: row.try_get(3).map_err(to_query_error)?,
        global_role: GlobalRole::from_db(&global_role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown global role `{global_role}`"))
        })?,
        auth_mode: AuthMode::from_db(&auth_mode)
            .ok_or_else(|| StoreError::Serialization(format!("unknown auth mode `{auth_mode}`")))?,
        status: row.try_get(6).map_err(to_query_error)?,
        must_change_password: must_change_password == 1,
        request_logging_enabled: request_logging_enabled == 1,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_identity_user_record(row: &PgRow) -> Result<IdentityUserRecord, StoreError> {
    let team_id: Option<String> = row.try_get(12).map_err(to_query_error)?;
    let membership_role_raw: Option<String> = row.try_get(14).map_err(to_query_error)?;
    let membership_role = membership_role_raw
        .as_deref()
        .map(|role| {
            MembershipRole::from_db(role).ok_or_else(|| {
                StoreError::Serialization(format!("unknown membership role `{role}`"))
            })
        })
        .transpose()?;

    Ok(IdentityUserRecord {
        user: decode_user_record(row)?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        team_name: row.try_get(13).map_err(to_query_error)?,
        membership_role,
        oidc_provider_id: row.try_get(15).map_err(to_query_error)?,
        oidc_provider_key: row.try_get(16).map_err(to_query_error)?,
    })
}

fn decode_user_password_auth_record(row: &PgRow) -> Result<UserPasswordAuthRecord, StoreError> {
    let password_updated_at: i64 = row.try_get(2).map_err(to_query_error)?;
    Ok(UserPasswordAuthRecord {
        user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        password_hash: row.try_get(1).map_err(to_query_error)?,
        password_updated_at: unix_to_datetime(password_updated_at)?,
    })
}

fn decode_team_record(row: &PgRow) -> Result<TeamRecord, StoreError> {
    let model_access_mode: String = row.try_get(4).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(5).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(6).map_err(to_query_error)?;

    Ok(TeamRecord {
        team_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        team_key: row.try_get(1).map_err(to_query_error)?,
        team_name: row.try_get(2).map_err(to_query_error)?,
        status: row.try_get(3).map_err(to_query_error)?,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_team_membership_record(row: &PgRow) -> Result<TeamMembershipRecord, StoreError> {
    let role: String = row.try_get(2).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(3).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(4).map_err(to_query_error)?;

    Ok(TeamMembershipRecord {
        team_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        role: MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_oidc_provider_record(row: &PgRow) -> Result<OidcProviderRecord, StoreError> {
    let scopes_json: String = row.try_get(5).map_err(to_query_error)?;
    let enabled: i64 = row.try_get(6).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(8).map_err(to_query_error)?;

    Ok(OidcProviderRecord {
        oidc_provider_id: row.try_get(0).map_err(to_query_error)?,
        provider_key: row.try_get(1).map_err(to_query_error)?,
        provider_type: row.try_get(2).map_err(to_query_error)?,
        issuer_url: row.try_get(3).map_err(to_query_error)?,
        client_id: row.try_get(4).map_err(to_query_error)?,
        scopes: serde_json::from_str(&scopes_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        enabled: enabled == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_password_invitation_record(row: &PgRow) -> Result<PasswordInvitationRecord, StoreError> {
    let expires_at: i64 = row.try_get(3).map_err(to_query_error)?;
    let consumed_at: Option<i64> = row.try_get(4).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(5).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(6).map_err(to_query_error)?;

    Ok(PasswordInvitationRecord {
        invitation_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        token_hash: row.try_get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        consumed_at: consumed_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(created_at)?,
    })
}

fn decode_user_session_record(row: &PgRow) -> Result<UserSessionRecord, StoreError> {
    let expires_at: i64 = row.try_get(3).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(4).map_err(to_query_error)?;
    let last_seen_at: i64 = row.try_get(5).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(6).map_err(to_query_error)?;

    Ok(UserSessionRecord {
        session_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        token_hash: row.try_get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        created_at: unix_to_datetime(created_at)?,
        last_seen_at: unix_to_datetime(last_seen_at)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_user_oidc_auth_record(row: &PgRow) -> Result<UserOidcAuthRecord, StoreError> {
    let created_at: i64 = row.try_get(4).map_err(to_query_error)?;
    Ok(UserOidcAuthRecord {
        user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        oidc_provider_id: row.try_get(1).map_err(to_query_error)?,
        subject: row.try_get(2).map_err(to_query_error)?,
        email_claim: row.try_get(3).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
    })
}

fn decode_user_budget_record(row: &PgRow) -> Result<UserBudgetRecord, StoreError> {
    let cadence: String = row.try_get(2).map_err(to_query_error)?;
    let amount_10000: i64 = row.try_get(3).map_err(to_query_error)?;
    let hard_limit: i64 = row.try_get(4).map_err(to_query_error)?;
    let is_active: i64 = row.try_get(6).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(8).map_err(to_query_error)?;

    Ok(UserBudgetRecord {
        user_budget_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
            StoreError::Serialization(format!("unknown budget cadence `{cadence}`"))
        })?,
        amount_usd: Money4::from_scaled(amount_10000),
        hard_limit: hard_limit == 1,
        timezone: row.try_get(5).map_err(to_query_error)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_pricing_catalog_cache_record(row: &PgRow) -> Result<PricingCatalogCacheRecord, StoreError> {
    let fetched_at: i64 = row.try_get(3).map_err(to_query_error)?;
    Ok(PricingCatalogCacheRecord {
        catalog_key: row.try_get(0).map_err(to_query_error)?,
        source: row.try_get(1).map_err(to_query_error)?,
        etag: row.try_get(2).map_err(to_query_error)?,
        fetched_at: unix_to_datetime(fetched_at)?,
        snapshot_json: row.try_get(4).map_err(to_query_error)?,
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

fn to_query_error(error: sqlx::Error) -> StoreError {
    let message = error.to_string();
    if let sqlx::Error::Database(db) = &error
        && matches!(db.code().as_deref(), Some("23505" | "23503" | "23514"))
    {
        return StoreError::Conflict(message);
    }

    StoreError::Query(message)
}

fn to_write_error(error: sqlx::Error) -> StoreError {
    to_query_error(error)
}
