use super::*;

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
