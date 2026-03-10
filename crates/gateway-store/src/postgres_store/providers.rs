use super::*;

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
