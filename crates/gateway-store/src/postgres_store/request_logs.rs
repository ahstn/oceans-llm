use super::*;
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};

fn normalize_query(query: &RequestLogQuery) -> (i64, i64) {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let offset = i64::from(page.saturating_sub(1) * page_size);
    (i64::from(page), offset)
}

fn decode_request_log_row(row: &PgRow) -> Result<RequestLogRecord, StoreError> {
    let request_log_id: String = row.try_get(0).map_err(to_query_error)?;
    let api_key_id: String = row.try_get(2).map_err(to_query_error)?;
    let user_id: Option<String> = row.try_get(3).map_err(to_query_error)?;
    let team_id: Option<String> = row.try_get(4).map_err(to_query_error)?;
    let has_payload: i64 = row.try_get(13).map_err(to_query_error)?;
    let request_payload_truncated: i64 = row.try_get(14).map_err(to_query_error)?;
    let response_payload_truncated: i64 = row.try_get(15).map_err(to_query_error)?;
    let metadata_json: String = row.try_get(16).map_err(to_query_error)?;
    let occurred_at: i64 = row.try_get(17).map_err(to_query_error)?;

    Ok(RequestLogRecord {
        request_log_id: parse_uuid(&request_log_id)?,
        request_id: row.try_get(1).map_err(to_query_error)?,
        api_key_id: parse_uuid(&api_key_id)?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        model_key: row.try_get(5).map_err(to_query_error)?,
        resolved_model_key: row.try_get(6).map_err(to_query_error)?,
        provider_key: row.try_get(7).map_err(to_query_error)?,
        status_code: row.try_get(8).map_err(to_query_error)?,
        latency_ms: row.try_get(9).map_err(to_query_error)?,
        prompt_tokens: row.try_get(10).map_err(to_query_error)?,
        completion_tokens: row.try_get(11).map_err(to_query_error)?,
        total_tokens: row.try_get(12).map_err(to_query_error)?,
        error_code: row.try_get(18).map_err(to_query_error)?,
        has_payload: has_payload == 1,
        request_payload_truncated: request_payload_truncated == 1,
        response_payload_truncated: response_payload_truncated == 1,
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}

#[async_trait]
impl RequestLogRepository for PostgresStore {
    async fn insert_request_log(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
    ) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&log.metadata)?;

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                resolved_model_key, provider_key, status_code, latency_ms, prompt_tokens,
                completion_tokens, total_tokens, has_payload, request_payload_truncated,
                response_payload_truncated, error_code, metadata_json, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
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
        .bind(if log.has_payload { 1_i64 } else { 0_i64 })
        .bind(if log.request_payload_truncated {
            1_i64
        } else {
            0_i64
        })
        .bind(if log.response_payload_truncated {
            1_i64
        } else {
            0_i64
        })
        .bind(log.error_code.as_deref())
        .bind(metadata_json)
        .bind(log.occurred_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

        if let Some(payload) = payload {
            sqlx::query(
                r#"
                INSERT INTO request_log_payloads (request_log_id, request_json, response_json)
                VALUES ($1, $2, $3)
                "#,
            )
            .bind(payload.request_log_id.to_string())
            .bind(&payload.request_json)
            .bind(&payload.response_json)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        }

        Ok(())
    }

    async fn list_request_logs(
        &self,
        query: &RequestLogQuery,
    ) -> Result<RequestLogPage, StoreError> {
        let (page, offset) = normalize_query(query);
        let page_size = i64::from(query.page_size.clamp(1, 200));
        let request_id = query.request_id.as_deref();
        let model_key = query.model_key.as_deref();
        let provider_key = query.provider_key.as_deref();
        let user_id = query.user_id.map(|value| value.to_string());
        let team_id = query.team_id.map(|value| value.to_string());

        let total_row = sqlx::query(
            r#"
            SELECT COUNT(*)
            FROM request_logs
            WHERE ($1::text IS NULL OR request_id = $1)
              AND ($2::text IS NULL OR model_key = $2)
              AND ($3::text IS NULL OR provider_key = $3)
              AND ($4::bigint IS NULL OR status_code = $4)
              AND ($5::text IS NULL OR user_id = $5)
              AND ($6::text IS NULL OR team_id = $6)
            "#,
        )
        .bind(request_id)
        .bind(model_key)
        .bind(provider_key)
        .bind(query.status_code)
        .bind(user_id.clone())
        .bind(team_id.clone())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;
        let total: i64 = total_row.try_get(0).map_err(to_query_error)?;

        let rows = sqlx::query(
            r#"
            SELECT request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                   resolved_model_key, provider_key, status_code, latency_ms, prompt_tokens,
                   completion_tokens, total_tokens, has_payload, request_payload_truncated,
                   response_payload_truncated, metadata_json, occurred_at, error_code
            FROM request_logs
            WHERE ($1::text IS NULL OR request_id = $1)
              AND ($2::text IS NULL OR model_key = $2)
              AND ($3::text IS NULL OR provider_key = $3)
              AND ($4::bigint IS NULL OR status_code = $4)
              AND ($5::text IS NULL OR user_id = $5)
              AND ($6::text IS NULL OR team_id = $6)
            ORDER BY occurred_at DESC, request_log_id DESC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind(request_id)
        .bind(model_key)
        .bind(provider_key)
        .bind(query.status_code)
        .bind(user_id)
        .bind(team_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        let items = rows
            .iter()
            .map(decode_request_log_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RequestLogPage {
            items,
            page: u32::try_from(page).unwrap_or(u32::MAX),
            page_size: u32::try_from(page_size).unwrap_or(u32::MAX),
            total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        })
    }

    async fn get_request_log_detail(
        &self,
        request_log_id: Uuid,
    ) -> Result<Option<RequestLogDetail>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT rl.request_log_id, rl.request_id, rl.api_key_id, rl.user_id, rl.team_id,
                   rl.model_key, rl.resolved_model_key, rl.provider_key, rl.status_code,
                   rl.latency_ms, rl.prompt_tokens, rl.completion_tokens, rl.total_tokens,
                   rl.has_payload, rl.request_payload_truncated, rl.response_payload_truncated,
                   rl.metadata_json, rl.occurred_at, rl.error_code,
                   rlp.request_json, rlp.response_json
            FROM request_logs rl
            LEFT JOIN request_log_payloads rlp
              ON rlp.request_log_id = rl.request_log_id
            WHERE rl.request_log_id = $1
            "#,
        )
        .bind(request_log_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let log = decode_request_log_row(&row)?;
        let request_json: Option<serde_json::Value> = row.try_get(19).map_err(to_query_error)?;
        let response_json: Option<serde_json::Value> = row.try_get(20).map_err(to_query_error)?;
        let payload = match (request_json, response_json) {
            (Some(request_json), Some(response_json)) => Some(RequestLogPayloadRecord {
                request_log_id,
                request_json,
                response_json,
            }),
            _ => None,
        };

        Ok(Some(RequestLogDetail { log, payload }))
    }
}
