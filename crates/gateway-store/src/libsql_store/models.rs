use std::collections::HashMap;

use super::*;

#[async_trait]
impl ModelRepository for LibsqlStore {
    async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT gm.id, gm.model_key, alias_target.model_key, gm.description, gm.tags_json, gm.rank
                FROM gateway_models gm
                LEFT JOIN gateway_models alias_target ON alias_target.id = gm.alias_target_model_id
                ORDER BY gm.rank ASC, gm.model_key ASC
                "#,
                (),
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut models = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            models.push(decode_gateway_model(&row)?);
        }

        Ok(models)
    }

    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT gm.id, gm.model_key, alias_target.model_key, gm.description, gm.tags_json, gm.rank
                FROM gateway_models gm
                LEFT JOIN gateway_models alias_target ON alias_target.id = gm.alias_target_model_id
                WHERE gm.model_key = ?1
                LIMIT 1
                "#,
                [model_key],
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

        decode_gateway_model(&row).map(Some)
    }

    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT gm.id, gm.model_key, alias_target.model_key, gm.description, gm.tags_json, gm.rank
                FROM gateway_models gm
                LEFT JOIN gateway_models alias_target ON alias_target.id = gm.alias_target_model_id
                INNER JOIN api_key_model_grants grants ON grants.model_id = gm.id
                WHERE grants.api_key_id = ?1
                ORDER BY gm.rank ASC, gm.model_key ASC
                "#,
                [api_key_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut models = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            models.push(decode_gateway_model(&row)?);
        }

        Ok(models)
    }

    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled,
                       extra_headers_json, extra_body_json, capabilities_json
                FROM model_routes
                WHERE model_id = ?1
                ORDER BY priority ASC
                "#,
                [model_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut routes = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            routes.push(decode_model_route(&row)?);
        }

        Ok(routes)
    }

    async fn list_routes_for_models(
        &self,
        model_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ModelRoute>>, StoreError> {
        if model_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = (0..model_ids.len())
            .map(|index| format!("?{}", index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled, \
             extra_headers_json, extra_body_json, capabilities_json \
             FROM model_routes WHERE model_id IN ({placeholders}) ORDER BY model_id ASC, priority ASC"
        );
        let params = model_ids
            .iter()
            .map(|model_id| libsql::Value::Text(model_id.to_string()))
            .collect::<Vec<_>>();

        let mut rows = self
            .connection
            .query(&query, params)
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut routes_by_model = HashMap::with_capacity(model_ids.len());
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            let route = decode_model_route(&row)?;
            routes_by_model
                .entry(route.model_id)
                .or_insert_with(Vec::new)
                .push(route);
        }

        Ok(routes_by_model)
    }
}
