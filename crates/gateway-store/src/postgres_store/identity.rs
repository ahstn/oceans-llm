use super::*;

impl PostgresStore {
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

        self.get_user_by_id(crate::shared::parse_uuid(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)?)
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

    pub async fn get_identity_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<IdentityUserRecord>, StoreError> {
        let row = sqlx::query(
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
            WHERE users.user_id = $1
              AND users.user_id != $2
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .bind(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_identity_user_record).transpose()
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
            .ok_or_else(|| StoreError::NotFound("created team missing".to_string()))
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
            SET team_name = $1,
                updated_at = $2
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
        status: UserStatus,
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
        .bind(status.as_str())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;

        self.get_user_by_id(user_id)
            .await?
            .ok_or_else(|| StoreError::NotFound("created user missing".to_string()))
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

    pub async fn update_identity_user(
        &self,
        user_id: Uuid,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let row = sqlx::query(
            r#"
            SELECT global_role, status
            FROM users
            WHERE user_id = $1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?;
        let Some(row) = row else {
            return Err(StoreError::NotFound("user not found".to_string()));
        };
        let current_role: String = row.try_get(0).map_err(to_query_error)?;
        let current_status: String = row.try_get(1).map_err(to_query_error)?;
        if current_role == GlobalRole::PlatformAdmin.as_str()
            && current_status == UserStatus::Active.as_str()
            && global_role != GlobalRole::PlatformAdmin
        {
            let row = sqlx::query(
                r#"
                SELECT COUNT(*)
                FROM users
                WHERE global_role = 'platform_admin'
                  AND status = 'active'
                  AND user_id != $1
                  AND user_id != $2
                "#,
            )
            .bind(user_id.to_string())
            .bind(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
            .fetch_one(&mut *tx)
            .await
            .map_err(to_query_error)?;
            let count: i64 = row.try_get(0).map_err(to_query_error)?;
            if count <= 0 {
                return Err(StoreError::Conflict(
                    "the last active platform admin cannot be deactivated or demoted".to_string(),
                ));
            }
        }

        sqlx::query(
            r#"
            UPDATE users
            SET global_role = $1,
                auth_mode = $2,
                updated_at = $3
            WHERE user_id = $4
            "#,
        )
        .bind(global_role.as_str())
        .bind(auth_mode.as_str())
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_write_error)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }

    pub async fn deactivate_identity_user(
        &self,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let row = sqlx::query(
            r#"
            SELECT global_role, status
            FROM users
            WHERE user_id = $1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?;
        let Some(row) = row else {
            return Err(StoreError::NotFound("user not found".to_string()));
        };
        let current_role: String = row.try_get(0).map_err(to_query_error)?;
        let current_status: String = row.try_get(1).map_err(to_query_error)?;
        if current_status == UserStatus::Disabled.as_str() {
            return Err(StoreError::Conflict("user is already disabled".to_string()));
        }
        if current_role == GlobalRole::PlatformAdmin.as_str()
            && current_status == UserStatus::Active.as_str()
        {
            let row = sqlx::query(
                r#"
                SELECT COUNT(*)
                FROM users
                WHERE global_role = 'platform_admin'
                  AND status = 'active'
                  AND user_id != $1
                  AND user_id != $2
                "#,
            )
            .bind(user_id.to_string())
            .bind(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
            .fetch_one(&mut *tx)
            .await
            .map_err(to_query_error)?;
            let count: i64 = row.try_get(0).map_err(to_query_error)?;
            if count <= 0 {
                return Err(StoreError::Conflict(
                    "the last active platform admin cannot be deactivated or demoted".to_string(),
                ));
            }
        }

        sqlx::query(
            r#"
            UPDATE users
            SET status = $1,
                updated_at = $2
            WHERE user_id = $3
            "#,
        )
        .bind(UserStatus::Disabled.as_str())
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_write_error)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }

    pub async fn remove_team_membership(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT role
            FROM team_memberships
            WHERE team_id = $1
              AND user_id = $2
            LIMIT 1
            "#,
        )
        .bind(team_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let role: String = row.try_get(0).map_err(to_query_error)?;
        if role == MembershipRole::Owner.as_str() {
            return Err(StoreError::Conflict(
                "owner memberships cannot be removed or transferred".to_string(),
            ));
        }

        let result = sqlx::query(
            r#"
            DELETE FROM team_memberships
            WHERE team_id = $1
              AND user_id = $2
            "#,
        )
        .bind(team_id.to_string())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn transfer_team_membership(
        &self,
        user_id: Uuid,
        from_team_id: Uuid,
        to_team_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let exists = sqlx::query(
            r#"
            SELECT role
            FROM team_memberships
            WHERE user_id = $1
              AND team_id = $2
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .bind(from_team_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?;
        let Some(row) = exists else {
            return Err(StoreError::NotFound(
                "team membership missing for requested source team".to_string(),
            ));
        };
        let existing_role: String = row.try_get(0).map_err(to_query_error)?;
        if existing_role == MembershipRole::Owner.as_str() {
            return Err(StoreError::Conflict(
                "owner memberships cannot be removed or transferred".to_string(),
            ));
        }

        sqlx::query(
            r#"
            UPDATE team_memberships
            SET team_id = $1,
                role = $2,
                updated_at = $3
            WHERE user_id = $4
              AND team_id = $5
            "#,
        )
        .bind(to_team_id.to_string())
        .bind(role.as_str())
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .bind(from_team_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_write_error)?;
        tx.commit().await.map_err(to_query_error)?;
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

        row.as_ref()
            .map(decode_user_password_auth_record)
            .transpose()
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
            SET role = $1,
                updated_at = $2
            WHERE team_id = $3
              AND user_id = $4
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

        row.as_ref()
            .map(decode_password_invitation_record)
            .transpose()
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
            WHERE user_id = $2
              AND consumed_at IS NULL
              AND revoked_at IS NULL
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
                invitation_id, user_id, token_hash, expires_at, created_at
            ) VALUES ($1, $2, $3, $4, $5)
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
            .ok_or_else(|| StoreError::NotFound("created password invitation missing".to_string()))
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

        row.as_ref()
            .map(decode_password_invitation_record)
            .transpose()
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
        status: UserStatus,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE users
            SET status = $1,
                updated_at = $2
            WHERE user_id = $3
            "#,
        )
        .bind(status.as_str())
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
            SET must_change_password = $1,
                updated_at = $2
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
                session_id, user_id, token_hash, expires_at, created_at, last_seen_at
            ) VALUES ($1, $2, $3, $4, $5, $5)
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
            .ok_or_else(|| StoreError::NotFound("created session missing".to_string()))
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

    pub async fn revoke_user_sessions(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE user_sessions
            SET revoked_at = $1
            WHERE user_id = $2
              AND revoked_at IS NULL
            "#,
        )
        .bind(revoked_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn revoke_user_session(
        &self,
        session_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE user_sessions
            SET revoked_at = $1
            WHERE session_id = $2
              AND revoked_at IS NULL
            "#,
        )
        .bind(revoked_at.unix_timestamp())
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
            WHERE oidc_provider_id = $1
              AND subject = $2
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

    pub async fn get_user_oidc_auth_by_user(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_id, oidc_provider_id, subject, email_claim, created_at
            FROM user_oidc_auth
            WHERE user_id = $1
              AND oidc_provider_id = $2
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .bind(oidc_provider_id)
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
            INSERT INTO user_oidc_auth (
                user_id, oidc_provider_id, subject, email_claim, created_at
            ) VALUES ($1, $2, $3, $4, $5)
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
                oidc_provider_id = excluded.oidc_provider_id,
                created_at = excluded.created_at
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

    pub async fn clear_user_oidc_link(&self, user_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM user_oidc_links WHERE user_id = $1")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn delete_user_password_auth(&self, user_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM user_password_auth WHERE user_id = $1")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn delete_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            DELETE FROM user_oidc_auth
            WHERE user_id = $1
              AND oidc_provider_id = $2
            "#,
        )
        .bind(user_id.to_string())
        .bind(oidc_provider_id)
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

    async fn list_team_memberships(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT team_id, user_id, role, created_at, updated_at
            FROM team_memberships
            WHERE team_id = $1
            ORDER BY created_at ASC, user_id ASC
            "#,
        )
        .bind(team_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter()
            .map(decode_team_membership_record)
            .collect::<Result<Vec<_>, _>>()
    }
}
