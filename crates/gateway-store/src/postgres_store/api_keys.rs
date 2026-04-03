use super::*;

#[async_trait]
impl AdminApiKeyRepository for PostgresStore {
    async fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, public_id, secret_hash, name, status,
                   owner_kind, owner_user_id, owner_team_id,
                   created_at, last_used_at, revoked_at
            FROM api_keys
            ORDER BY created_at DESC, public_id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_api_key).collect()
    }

    async fn get_api_key_by_id(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<ApiKeyRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, public_id, secret_hash, name, status,
                   owner_kind, owner_user_id, owner_team_id,
                   created_at, last_used_at, revoked_at
            FROM api_keys
            WHERE id = $1
            LIMIT 1
            "#,
        )
        .bind(api_key_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_api_key).transpose()
    }

    async fn create_api_key(&self, api_key: &NewApiKeyRecord) -> Result<ApiKeyRecord, StoreError> {
        let api_key_id = api_key_uuid(&api_key.public_id);
        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status,
                owner_kind, owner_user_id, owner_team_id,
                created_at, last_used_at, revoked_at
            ) VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, $8, NULL, NULL)
            "#,
        )
        .bind(api_key_id.to_string())
        .bind(api_key.public_id.as_str())
        .bind(api_key.secret_hash.as_str())
        .bind(api_key.name.as_str())
        .bind(api_key.owner_kind.as_str())
        .bind(api_key.owner_user_id.map(|value| value.to_string()))
        .bind(api_key.owner_team_id.map(|value| value.to_string()))
        .bind(api_key.created_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

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
        sqlx::query("DELETE FROM api_key_model_grants WHERE api_key_id = $1")
            .bind(api_key_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;

        for model_id in model_ids {
            sqlx::query("INSERT INTO api_key_model_grants (api_key_id, model_id) VALUES ($1, $2)")
                .bind(api_key_id.to_string())
                .bind(model_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;
        }

        Ok(())
    }

    async fn revoke_api_key(
        &self,
        api_key_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = 'revoked',
                revoked_at = $1
            WHERE id = $2
              AND revoked_at IS NULL
            "#,
        )
        .bind(revoked_at.unix_timestamp())
        .bind(api_key_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

        Ok(result.rows_affected() > 0)
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
