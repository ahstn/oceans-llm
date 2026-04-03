use super::*;

#[async_trait]
impl AdminApiKeyRepository for LibsqlStore {
    async fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, public_id, secret_hash, name, status,
                       owner_kind, owner_user_id, owner_team_id,
                       created_at, last_used_at, revoked_at
                FROM api_keys
                ORDER BY created_at DESC, public_id ASC
                "#,
                (),
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut api_keys = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            api_keys.push(decode_api_key(&row)?);
        }

        Ok(api_keys)
    }

    async fn get_api_key_by_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<ApiKeyRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, public_id, secret_hash, name, status,
                       owner_kind, owner_user_id, owner_team_id,
                       created_at, last_used_at, revoked_at
                FROM api_keys
                WHERE id = ?1
                LIMIT 1
                "#,
                [api_key_id.to_string()],
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

    async fn create_api_key(&self, api_key: &NewApiKeyRecord) -> Result<ApiKeyRecord, StoreError> {
        let api_key_id = api_key_uuid(&api_key.public_id);
        self.connection
            .execute(
                r#"
                INSERT INTO api_keys (
                    id, public_id, secret_hash, name, status,
                    owner_kind, owner_user_id, owner_team_id,
                    created_at, last_used_at, revoked_at
                ) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7, ?8, NULL, NULL)
                "#,
                libsql::params![
                    api_key_id.to_string(),
                    api_key.public_id.as_str(),
                    api_key.secret_hash.as_str(),
                    api_key.name.as_str(),
                    api_key.owner_kind.as_str(),
                    api_key.owner_user_id.map(|value| value.to_string()),
                    api_key.owner_team_id.map(|value| value.to_string()),
                    api_key.created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        AdminApiKeyRepository::get_api_key_by_id(self, api_key_id)
            .await?
            .ok_or_else(|| {
                StoreError::NotFound(format!("api key `{api_key_id}` missing after create"))
            })
    }

    async fn replace_api_key_model_grants(
        &self,
        api_key_id: Uuid,
        model_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM api_key_model_grants WHERE api_key_id = ?1",
                [api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        for model_id in model_ids {
            self.connection
                .execute(
                    "INSERT INTO api_key_model_grants (api_key_id, model_id) VALUES (?1, ?2)",
                    libsql::params![api_key_id.to_string(), model_id.to_string()],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;
        }

        Ok(())
    }

    async fn revoke_api_key(
        &self,
        api_key_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let rows_affected = self
            .connection
            .execute(
                r#"
                UPDATE api_keys
                SET status = 'revoked',
                    revoked_at = ?1
                WHERE id = ?2
                  AND revoked_at IS NULL
                "#,
                libsql::params![revoked_at.unix_timestamp(), api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        Ok(rows_affected > 0)
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
