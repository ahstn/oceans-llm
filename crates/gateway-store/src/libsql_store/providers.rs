use std::collections::HashMap;

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

    async fn list_providers_by_keys(
        &self,
        provider_keys: &[String],
    ) -> Result<HashMap<String, ProviderConnection>, StoreError> {
        if provider_keys.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = (0..provider_keys.len())
            .map(|index| format!("?{}", index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "SELECT provider_key, provider_type, config_json, secrets_json \
             FROM providers WHERE provider_key IN ({placeholders}) ORDER BY provider_key ASC"
        );
        let params = provider_keys
            .iter()
            .map(|provider_key| libsql::Value::Text(provider_key.clone()))
            .collect::<Vec<_>>();

        let mut rows = self
            .connection
            .query(&query, params)
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut providers = HashMap::with_capacity(provider_keys.len());
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            let provider = decode_provider_connection(&row)?;
            providers.insert(provider.provider_key.clone(), provider);
        }

        Ok(providers)
    }
}
