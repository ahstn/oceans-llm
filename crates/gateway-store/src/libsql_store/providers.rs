use super::*;

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
