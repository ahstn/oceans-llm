use super::*;
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};

fn normalize_query(query: &RequestLogQuery) -> (i64, i64) {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let offset = i64::from(page.saturating_sub(1) * page_size);
    (i64::from(page), offset)
}

fn decode_request_log_row(row: &libsql::Row) -> Result<RequestLogRecord, StoreError> {
    let request_log_id: String = row.get(0).map_err(to_query_error)?;
    let api_key_id: String = row.get(2).map_err(to_query_error)?;
    let user_id: Option<String> = row.get(3).map_err(to_query_error)?;
    let team_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let has_payload: i64 = row.get(13).map_err(to_query_error)?;
    let request_payload_truncated: i64 = row.get(14).map_err(to_query_error)?;
    let response_payload_truncated: i64 = row.get(15).map_err(to_query_error)?;
    let metadata_json: String = row.get(16).map_err(to_query_error)?;
    let occurred_at: i64 = row.get(17).map_err(to_query_error)?;

    Ok(RequestLogRecord {
        request_log_id: parse_uuid(&request_log_id)?,
        request_id: row.get(1).map_err(to_query_error)?,
        api_key_id: parse_uuid(&api_key_id)?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        model_key: row.get(5).map_err(to_query_error)?,
        resolved_model_key: row.get(6).map_err(to_query_error)?,
        provider_key: row.get(7).map_err(to_query_error)?,
        status_code: row.get(8).map_err(to_query_error)?,
        latency_ms: row.get(9).map_err(to_query_error)?,
        prompt_tokens: row.get(10).map_err(to_query_error)?,
        completion_tokens: row.get(11).map_err(to_query_error)?,
        total_tokens: row.get(12).map_err(to_query_error)?,
        error_code: row.get(18).map_err(to_query_error)?,
        has_payload: has_payload == 1,
        request_payload_truncated: request_payload_truncated == 1,
        response_payload_truncated: response_payload_truncated == 1,
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}

#[async_trait]
impl RequestLogRepository for LibsqlStore {
    async fn insert_request_log(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
    ) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&log.metadata)?;

        self.connection
            .execute(
                r#"
                INSERT INTO request_logs (
                    request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                    resolved_model_key, provider_key, status_code, latency_ms, prompt_tokens,
                    completion_tokens, total_tokens, has_payload, request_payload_truncated,
                    response_payload_truncated, error_code, metadata_json, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
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
                    if log.has_payload { 1_i64 } else { 0_i64 },
                    if log.request_payload_truncated { 1_i64 } else { 0_i64 },
                    if log.response_payload_truncated { 1_i64 } else { 0_i64 },
                    log.error_code.as_deref(),
                    metadata_json,
                    log.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        if let Some(payload) = payload {
            let request_json = serialize_json(&payload.request_json)?;
            let response_json = serialize_json(&payload.response_json)?;
            self.connection
                .execute(
                    r#"
                    INSERT INTO request_log_payloads (request_log_id, request_json, response_json)
                    VALUES (?1, ?2, ?3)
                    "#,
                    libsql::params![
                        payload.request_log_id.to_string(),
                        request_json,
                        response_json
                    ],
                )
                .await
                .map_err(|error| StoreError::Query(error.to_string()))?;
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

        let mut count_rows = self
            .connection
            .query(
                r#"
                SELECT COUNT(*)
                FROM request_logs
                WHERE (?1 IS NULL OR request_id = ?1)
                  AND (?2 IS NULL OR model_key = ?2)
                  AND (?3 IS NULL OR provider_key = ?3)
                  AND (?4 IS NULL OR status_code = ?4)
                  AND (?5 IS NULL OR user_id = ?5)
                  AND (?6 IS NULL OR team_id = ?6)
                "#,
                libsql::params![
                    request_id,
                    model_key,
                    provider_key,
                    query.status_code,
                    user_id.clone(),
                    team_id.clone()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        let count_row = count_rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
            .ok_or_else(|| StoreError::Query("request log count row missing".to_string()))?;
        let total: i64 = count_row.get(0).map_err(to_query_error)?;

        let mut rows = self
            .connection
            .query(
                r#"
                SELECT request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                       resolved_model_key, provider_key, status_code, latency_ms, prompt_tokens,
                       completion_tokens, total_tokens, has_payload, request_payload_truncated,
                       response_payload_truncated, metadata_json, occurred_at, error_code
                FROM request_logs
                WHERE (?1 IS NULL OR request_id = ?1)
                  AND (?2 IS NULL OR model_key = ?2)
                  AND (?3 IS NULL OR provider_key = ?3)
                  AND (?4 IS NULL OR status_code = ?4)
                  AND (?5 IS NULL OR user_id = ?5)
                  AND (?6 IS NULL OR team_id = ?6)
                ORDER BY occurred_at DESC, request_log_id DESC
                LIMIT ?7 OFFSET ?8
                "#,
                libsql::params![
                    request_id,
                    model_key,
                    provider_key,
                    query.status_code,
                    user_id,
                    team_id,
                    page_size,
                    offset
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut items = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            items.push(decode_request_log_row(&row)?);
        }

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
        let mut rows = self
            .connection
            .query(
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
                WHERE rl.request_log_id = ?1
                "#,
                [request_log_id.to_string()],
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

        let log = decode_request_log_row(&row)?;
        let request_json: Option<String> = row.get(19).map_err(to_query_error)?;
        let response_json: Option<String> = row.get(20).map_err(to_query_error)?;
        let payload = match (request_json, response_json) {
            (Some(request_json), Some(response_json)) => Some(RequestLogPayloadRecord {
                request_log_id,
                request_json: serde_json::from_str(&request_json)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                response_json: serde_json::from_str(&response_json)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
            }),
            _ => None,
        };

        Ok(Some(RequestLogDetail { log, payload }))
    }
}
