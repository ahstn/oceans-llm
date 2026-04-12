use std::collections::HashMap;

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

    async fn list_providers_by_keys(
        &self,
        provider_keys: &[String],
    ) -> Result<HashMap<String, ProviderConnection>, StoreError> {
        if provider_keys.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT provider_key, provider_type, config_json, secrets_json \
             FROM providers WHERE provider_key IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for provider_key in provider_keys {
                separated.push_bind(provider_key);
            }
        }
        builder.push(" ) ORDER BY provider_key ASC");

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;

        let mut providers = HashMap::with_capacity(rows.len());
        for row in &rows {
            let provider = decode_provider_connection(row)?;
            providers.insert(provider.provider_key.clone(), provider);
        }

        Ok(providers)
    }
}
