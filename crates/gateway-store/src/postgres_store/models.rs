use super::*;

#[async_trait]
impl ModelRepository for PostgresStore {
    async fn get_model_by_key(&self, model_key: &str) -> Result<Option<GatewayModel>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, model_key, description, tags_json, rank
            FROM gateway_models
            WHERE model_key = $1
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
            SELECT gm.id, gm.model_key, gm.description, gm.tags_json, gm.rank
            FROM gateway_models gm
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
                   extra_headers_json, extra_body_json
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
}
