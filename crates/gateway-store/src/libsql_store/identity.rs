use super::*;

impl LibsqlStore {
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

        self.get_user_by_id(crate::shared::parse_uuid(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)?)
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

    pub async fn get_identity_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<IdentityUserRecord>, StoreError> {
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
                WHERE users.user_id = ?1
                  AND users.user_id != ?2
                LIMIT 1
                "#,
                libsql::params![user_id.to_string(), SYSTEM_BOOTSTRAP_ADMIN_USER_ID],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_identity_user_record(&row).map(Some)
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
            .ok_or_else(|| StoreError::NotFound("created team missing".to_string()))
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
                SET team_name = ?1,
                    updated_at = ?2
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
        status: UserStatus,
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
                    status.as_str(),
                    now
                ],
            )
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

    pub async fn update_identity_user(
        &self,
        user_id: Uuid,
        global_role: GlobalRole,
        auth_mode: AuthMode,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let mut rows = tx
            .query(
                r#"
                SELECT global_role, status
                FROM users
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Err(StoreError::NotFound("user not found".to_string()));
        };
        let current_role: String = row.get(0).map_err(to_query_error)?;
        let current_status: String = row.get(1).map_err(to_query_error)?;
        if current_role == GlobalRole::PlatformAdmin.as_str()
            && current_status == UserStatus::Active.as_str()
            && global_role != GlobalRole::PlatformAdmin
        {
            let mut rows = tx
                .query(
                    r#"
                    SELECT COUNT(*)
                    FROM users
                    WHERE global_role = 'platform_admin'
                      AND status = 'active'
                      AND user_id != ?1
                      AND user_id != ?2
                    "#,
                    libsql::params![user_id.to_string(), SYSTEM_BOOTSTRAP_ADMIN_USER_ID],
                )
                .await
                .map_err(to_query_error)?;
            let row = rows.next().await.map_err(to_query_error)?.ok_or_else(|| {
                StoreError::Query("active platform admin count missing".to_string())
            })?;
            let count: i64 = row.get(0).map_err(to_query_error)?;
            if count <= 0 {
                return Err(StoreError::Conflict(
                    "the last active platform admin cannot be deactivated or demoted".to_string(),
                ));
            }
        }

        tx.execute(
            r#"
            UPDATE users
            SET global_role = ?1,
                auth_mode = ?2,
                updated_at = ?3
            WHERE user_id = ?4
            "#,
            libsql::params![
                global_role.as_str(),
                auth_mode.as_str(),
                updated_at.unix_timestamp(),
                user_id.to_string(),
            ],
        )
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
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let mut rows = tx
            .query(
                r#"
                SELECT global_role, status
                FROM users
                WHERE user_id = ?1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Err(StoreError::NotFound("user not found".to_string()));
        };
        let current_role: String = row.get(0).map_err(to_query_error)?;
        let current_status: String = row.get(1).map_err(to_query_error)?;
        if current_status == UserStatus::Disabled.as_str() {
            return Err(StoreError::Conflict("user is already disabled".to_string()));
        }
        if current_role == GlobalRole::PlatformAdmin.as_str()
            && current_status == UserStatus::Active.as_str()
        {
            let mut rows = tx
                .query(
                    r#"
                    SELECT COUNT(*)
                    FROM users
                    WHERE global_role = 'platform_admin'
                      AND status = 'active'
                      AND user_id != ?1
                      AND user_id != ?2
                    "#,
                    libsql::params![user_id.to_string(), SYSTEM_BOOTSTRAP_ADMIN_USER_ID],
                )
                .await
                .map_err(to_query_error)?;
            let row = rows.next().await.map_err(to_query_error)?.ok_or_else(|| {
                StoreError::Query("active platform admin count missing".to_string())
            })?;
            let count: i64 = row.get(0).map_err(to_query_error)?;
            if count <= 0 {
                return Err(StoreError::Conflict(
                    "the last active platform admin cannot be deactivated or demoted".to_string(),
                ));
            }
        }

        tx.execute(
            r#"
            UPDATE users
            SET status = ?1,
                updated_at = ?2
            WHERE user_id = ?3
            "#,
            libsql::params![
                UserStatus::Disabled.as_str(),
                updated_at.unix_timestamp(),
                user_id.to_string(),
            ],
        )
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
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT role
                FROM team_memberships
                WHERE team_id = ?1
                  AND user_id = ?2
                LIMIT 1
                "#,
                libsql::params![team_id.to_string(), user_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(false);
        };
        let current_role: String = row.get(0).map_err(to_query_error)?;
        if current_role == MembershipRole::Owner.as_str() {
            return Err(StoreError::Conflict(
                "owner memberships cannot be removed or transferred".to_string(),
            ));
        }

        self.connection
            .execute(
                r#"
                DELETE FROM team_memberships
                WHERE team_id = ?1
                  AND user_id = ?2
                "#,
                libsql::params![team_id.to_string(), user_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(true)
    }

    pub async fn transfer_team_membership(
        &self,
        user_id: Uuid,
        from_team_id: Uuid,
        to_team_id: Uuid,
        role: MembershipRole,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let mut rows = tx
            .query(
                r#"
                SELECT role
                FROM team_memberships
                WHERE user_id = ?1
                  AND team_id = ?2
                LIMIT 1
                "#,
                libsql::params![user_id.to_string(), from_team_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Err(StoreError::NotFound(
                "team membership missing for requested source team".to_string(),
            ));
        };
        let existing_role: String = row.get(0).map_err(to_query_error)?;
        if existing_role == MembershipRole::Owner.as_str() {
            return Err(StoreError::Conflict(
                "owner memberships cannot be removed or transferred".to_string(),
            ));
        }

        tx.execute(
            r#"
            UPDATE team_memberships
            SET team_id = ?1,
                role = ?2,
                updated_at = ?3
            WHERE user_id = ?4
              AND team_id = ?5
            "#,
            libsql::params![
                to_team_id.to_string(),
                role.as_str(),
                updated_at.unix_timestamp(),
                user_id.to_string(),
                from_team_id.to_string(),
            ],
        )
        .await
        .map_err(to_write_error)?;
        tx.commit().await.map_err(to_query_error)?;
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
                SET role = ?1,
                    updated_at = ?2
                WHERE team_id = ?3
                  AND user_id = ?4
                "#,
                libsql::params![
                    role.as_str(),
                    updated_at.unix_timestamp(),
                    team_id.to_string(),
                    user_id.to_string(),
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
                WHERE user_id = ?2
                  AND consumed_at IS NULL
                  AND revoked_at IS NULL
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
                    invitation_id, user_id, token_hash, expires_at, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5)
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
            .ok_or_else(|| StoreError::NotFound("created password invitation missing".to_string()))
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
        status: UserStatus,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE users
                SET status = ?1,
                    updated_at = ?2
                WHERE user_id = ?3
                "#,
                libsql::params![
                    status.as_str(),
                    updated_at.unix_timestamp(),
                    user_id.to_string()
                ],
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
                SET must_change_password = ?1,
                    updated_at = ?2
                WHERE user_id = ?3
                "#,
                libsql::params![
                    if must_change_password { 1_i64 } else { 0_i64 },
                    updated_at.unix_timestamp(),
                    user_id.to_string(),
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
                    session_id, user_id, token_hash, expires_at, created_at, last_seen_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
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
            .ok_or_else(|| StoreError::NotFound("created session missing".to_string()))
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

    pub async fn revoke_user_sessions(
        &self,
        user_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE user_sessions
                SET revoked_at = ?1
                WHERE user_id = ?2
                  AND revoked_at IS NULL
                "#,
                libsql::params![revoked_at.unix_timestamp(), user_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn revoke_user_session(
        &self,
        session_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE user_sessions
                SET revoked_at = ?1
                WHERE session_id = ?2
                  AND revoked_at IS NULL
                "#,
                libsql::params![revoked_at.unix_timestamp(), session_id.to_string()],
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
                WHERE oidc_provider_id = ?1
                  AND subject = ?2
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

    pub async fn get_user_oidc_auth_by_user(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<Option<UserOidcAuthRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_id, oidc_provider_id, subject, email_claim, created_at
                FROM user_oidc_auth
                WHERE user_id = ?1
                  AND oidc_provider_id = ?2
                LIMIT 1
                "#,
                libsql::params![user_id.to_string(), oidc_provider_id],
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
                INSERT INTO user_oidc_auth (
                    user_id, oidc_provider_id, subject, email_claim, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5)
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
                    oidc_provider_id = excluded.oidc_provider_id,
                    created_at = excluded.created_at
                "#,
                libsql::params![
                    user_id.to_string(),
                    oidc_provider_id,
                    created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn clear_user_oidc_link(&self, user_id: Uuid) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM user_oidc_links WHERE user_id = ?1",
                [user_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn delete_user_password_auth(&self, user_id: Uuid) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM user_password_auth WHERE user_id = ?1",
                [user_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    pub async fn delete_user_oidc_auth(
        &self,
        user_id: Uuid,
        oidc_provider_id: &str,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                DELETE FROM user_oidc_auth
                WHERE user_id = ?1
                  AND oidc_provider_id = ?2
                "#,
                libsql::params![user_id.to_string(), oidc_provider_id],
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

    async fn list_team_memberships(
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
                ORDER BY created_at ASC, user_id ASC
                "#,
                [team_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut memberships = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            memberships.push(decode_team_membership_record(&row)?);
        }

        Ok(memberships)
    }
}
