use super::*;
use crate::shared::serialize_json;

#[async_trait]
impl RequestLogRepository for LibsqlStore {
    async fn insert_request_log(&self, log: &RequestLogRecord) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&log.metadata)?;

        self.connection
            .execute(
                r#"
                INSERT INTO request_logs (
                    request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                    resolved_model_key,
                    provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                    total_tokens, error_code, metadata_json, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                libsql::params![
                    log.request_log_id.to_string(),
                    log.request_id.as_str(),
                    log.api_key_id.to_string(),
                    log.user_id.map(|value| value.to_string()),
                    log.team_id.map(|value| value.to_string()),
                    log.model_key.as_str(),
                    log.resolved_model_key.as_str(),
                    log.provider_key.as_str(),
                    log.status_code,
                    log.latency_ms,
                    log.prompt_tokens,
                    log.completion_tokens,
                    log.total_tokens,
                    log.error_code.as_deref(),
                    metadata_json,
                    log.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}
