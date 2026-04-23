use std::collections::HashMap;

use super::*;

#[async_trait]
impl ModelRepository for PostgresStore {
    async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT gm.id, gm.model_key, alias_target.model_key, gm.description, gm.tags_json, gm.rank
            FROM gateway_models gm
            LEFT JOIN gateway_models alias_target ON alias_target.id = gm.alias_target_model_id
            ORDER BY gm.rank ASC, gm.model_key ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_gateway_model).collect()
    }

    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT gm.id, gm.model_key, alias_target.model_key, gm.description, gm.tags_json, gm.rank
            FROM gateway_models gm
            LEFT JOIN gateway_models alias_target ON alias_target.id = gm.alias_target_model_id
            WHERE gm.model_key = $1
            LIMIT 1
            "#,
        )
        .bind(model_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_gateway_model).transpose()
    }

    async fn list_models_for_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<GatewayModel>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT gm.id, gm.model_key, alias_target.model_key, gm.description, gm.tags_json, gm.rank
            FROM gateway_models gm
            LEFT JOIN gateway_models alias_target ON alias_target.id = gm.alias_target_model_id
            INNER JOIN api_key_model_grants grants ON grants.model_id = gm.id
            WHERE grants.api_key_id = $1
            ORDER BY gm.rank ASC, gm.model_key ASC
            "#,
        )
        .bind(api_key_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_gateway_model).collect()
    }

    async fn list_routes_for_model(&self, model_id: Uuid) -> Result<Vec<ModelRoute>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled,
                   extra_headers_json, extra_body_json, capabilities_json, compatibility_json
            FROM model_routes
            WHERE model_id = $1
            ORDER BY priority ASC
            "#,
        )
        .bind(model_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_model_route).collect()
    }

    async fn list_routes_for_models(
        &self,
        model_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ModelRoute>>, StoreError> {
        if model_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id, model_id, provider_key, upstream_model, priority, weight, enabled, \
             extra_headers_json, extra_body_json, capabilities_json, compatibility_json \
             FROM model_routes WHERE model_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for model_id in model_ids {
                separated.push_bind(model_id.to_string());
            }
        }
        builder.push(" ) ORDER BY model_id ASC, priority ASC");

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;

        let mut routes_by_model = HashMap::with_capacity(model_ids.len());
        for row in &rows {
            let route = decode_model_route(row)?;
            routes_by_model
                .entry(route.model_id)
                .or_insert_with(Vec::new)
                .push(route);
        }

        Ok(routes_by_model)
    }
}
