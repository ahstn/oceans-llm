use super::*;
use crate::shared::serialize_json;

#[async_trait]
impl RequestLogRepository for PostgresStore {
    async fn insert_request_log(&self, log: &RequestLogRecord) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&log.metadata)?;

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                resolved_model_key,
                provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                total_tokens, error_code, metadata_json, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            "#,
        )
        .bind(log.request_log_id.to_string())
        .bind(log.request_id.as_str())
        .bind(log.api_key_id.to_string())
        .bind(log.user_id.map(|value| value.to_string()))
        .bind(log.team_id.map(|value| value.to_string()))
        .bind(log.model_key.as_str())
        .bind(log.resolved_model_key.as_str())
        .bind(log.provider_key.as_str())
        .bind(log.status_code)
        .bind(log.latency_ms)
        .bind(log.prompt_tokens)
        .bind(log.completion_tokens)
        .bind(log.total_tokens)
        .bind(log.error_code.as_deref())
        .bind(metadata_json)
        .bind(log.occurred_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}
