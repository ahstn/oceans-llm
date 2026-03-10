use super::*;

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
