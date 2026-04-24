use std::collections::BTreeMap;

use super::*;
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};
use gateway_core::{RequestTag, RequestTags};

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
    let metadata_json: String = row.try_get(19).map_err(to_query_error)?;
    let occurred_at: i64 = row.try_get(20).map_err(to_query_error)?;

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
        error_code: row.try_get(21).map_err(to_query_error)?,
        has_payload: has_payload == 1,
        request_payload_truncated: request_payload_truncated == 1,
        response_payload_truncated: response_payload_truncated == 1,
        request_tags: RequestTags {
            service: row.try_get(16).map_err(to_query_error)?,
            component: row.try_get(17).map_err(to_query_error)?,
            env: row.try_get(18).map_err(to_query_error)?,
            bespoke: Vec::new(),
        },
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}


fn decode_request_attempt_row(row: &PgRow) -> Result<RequestAttemptRecord, StoreError> {
    let request_attempt_id: String = row.try_get(0).map_err(to_query_error)?;
    let request_log_id: String = row.try_get(1).map_err(to_query_error)?;
    let route_id: String = row.try_get(4).map_err(to_query_error)?;
    let status: String = row.try_get(7).map_err(to_query_error)?;
    let error_detail_truncated: i64 = row.try_get(11).map_err(to_query_error)?;
    let retryable: i64 = row.try_get(12).map_err(to_query_error)?;
    let terminal: i64 = row.try_get(13).map_err(to_query_error)?;
    let produced_final_response: i64 = row.try_get(14).map_err(to_query_error)?;
    let stream: i64 = row.try_get(15).map_err(to_query_error)?;
    let started_at: i64 = row.try_get(16).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.try_get(17).map_err(to_query_error)?;
    let metadata_json: String = row.try_get(19).map_err(to_query_error)?;

    Ok(RequestAttemptRecord {
        request_attempt_id: parse_uuid(&request_attempt_id)?,
        request_log_id: parse_uuid(&request_log_id)?,
        request_id: row.try_get(2).map_err(to_query_error)?,
        attempt_number: row.try_get(3).map_err(to_query_error)?,
        route_id: parse_uuid(&route_id)?,
        provider_key: row.try_get(5).map_err(to_query_error)?,
        upstream_model: row.try_get(6).map_err(to_query_error)?,
        status: RequestAttemptStatus::from_db(&status)
            .ok_or_else(|| StoreError::Serialization(format!("invalid attempt status `{status}`")))?,
        status_code: row.try_get(8).map_err(to_query_error)?,
        error_code: row.try_get(9).map_err(to_query_error)?,
        error_detail: row.try_get(10).map_err(to_query_error)?,
        error_detail_truncated: error_detail_truncated == 1,
        retryable: retryable == 1,
        terminal: terminal == 1,
        produced_final_response: produced_final_response == 1,
        stream: stream == 1,
        started_at: unix_to_datetime(started_at)?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        latency_ms: row.try_get(18).map_err(to_query_error)?,
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
    })
}

async fn insert_request_log_attempts(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    attempts: &[RequestAttemptRecord],
) -> Result<(), StoreError> {
    for attempt in attempts {
        let metadata_json = serialize_json(&attempt.metadata)?;
        sqlx::query(
            r#"
            INSERT INTO request_log_attempts (
                request_attempt_id, request_log_id, request_id, attempt_number, route_id,
                provider_key, upstream_model, status, status_code, error_code, error_detail,
                error_detail_truncated, retryable, terminal, produced_final_response, stream,
                started_at, completed_at, latency_ms, metadata_json
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
            "#,
        )
        .bind(attempt.request_attempt_id.to_string())
        .bind(attempt.request_log_id.to_string())
        .bind(attempt.request_id.as_str())
        .bind(attempt.attempt_number)
        .bind(attempt.route_id.to_string())
        .bind(attempt.provider_key.as_str())
        .bind(attempt.upstream_model.as_str())
        .bind(attempt.status.as_str())
        .bind(attempt.status_code)
        .bind(attempt.error_code.as_deref())
        .bind(attempt.error_detail.as_deref())
        .bind(if attempt.error_detail_truncated { 1_i64 } else { 0_i64 })
        .bind(if attempt.retryable { 1_i64 } else { 0_i64 })
        .bind(if attempt.terminal { 1_i64 } else { 0_i64 })
        .bind(if attempt.produced_final_response { 1_i64 } else { 0_i64 })
        .bind(if attempt.stream { 1_i64 } else { 0_i64 })
        .bind(attempt.started_at.unix_timestamp())
        .bind(attempt.completed_at.map(|value| value.unix_timestamp()))
        .bind(attempt.latency_ms)
        .bind(metadata_json)
        .execute(&mut **tx)
        .await
        .map_err(to_query_error)?;
    }

    Ok(())
}

async fn load_bespoke_tags_for_logs(
    pool: &sqlx::PgPool,
    request_log_ids: &[Uuid],
) -> Result<BTreeMap<Uuid, Vec<RequestTag>>, StoreError> {
    if request_log_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT request_log_id, tag_key, tag_value FROM request_log_tags WHERE request_log_id IN (",
    );
    {
        let mut separated = builder.separated(", ");
        for request_log_id in request_log_ids {
            separated.push_bind(request_log_id.to_string());
        }
    }
    builder.push(") ORDER BY request_log_id ASC, tag_key ASC");

    let rows = builder
        .build()
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;

    let mut tags = BTreeMap::<Uuid, Vec<RequestTag>>::new();
    for row in rows {
        let request_log_id: String = row.try_get(0).map_err(to_query_error)?;
        let request_log_id = parse_uuid(&request_log_id)?;
        let tag_key: String = row.try_get(1).map_err(to_query_error)?;
        let tag_value: String = row.try_get(2).map_err(to_query_error)?;
        tags.entry(request_log_id).or_default().push(RequestTag {
            key: tag_key,
            value: tag_value,
        });
    }

    Ok(tags)
}

#[async_trait]
impl RequestLogRepository for PostgresStore {
    async fn insert_request_log(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
    ) -> Result<(), StoreError> {
        self.insert_request_log_with_attempts(log, payload, &[]).await
    }

    async fn insert_request_log_with_attempts(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
        attempts: &[RequestAttemptRecord],
    ) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&log.metadata)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                resolved_model_key, provider_key, status_code, latency_ms, prompt_tokens,
                completion_tokens, total_tokens, has_payload, request_payload_truncated,
                response_payload_truncated, caller_service, caller_component, caller_env,
                error_code, metadata_json, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22)
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
        .bind(log.request_tags.service.as_deref())
        .bind(log.request_tags.component.as_deref())
        .bind(log.request_tags.env.as_deref())
        .bind(log.error_code.as_deref())
        .bind(metadata_json)
        .bind(log.occurred_at.unix_timestamp())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        for tag in &log.request_tags.bespoke {
            sqlx::query(
                r#"
                INSERT INTO request_log_tags (request_log_id, tag_key, tag_value)
                VALUES ($1, $2, $3)
                "#,
            )
            .bind(log.request_log_id.to_string())
            .bind(tag.key.as_str())
            .bind(tag.value.as_str())
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
        }

        insert_request_log_attempts(&mut tx, attempts).await?;

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
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
        }

        tx.commit().await.map_err(to_query_error)?;
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
        let service = query.service.as_deref();
        let component = query.component.as_deref();
        let env = query.env.as_deref();
        let tag_key = query.tag_key.as_deref();
        let tag_value = query.tag_value.as_deref();

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
              AND ($7::text IS NULL OR caller_service = $7)
              AND ($8::text IS NULL OR caller_component = $8)
              AND ($9::text IS NULL OR caller_env = $9)
              AND (
                ($10::text IS NULL AND $11::text IS NULL)
                OR EXISTS (
                  SELECT 1
                  FROM request_log_tags
                  WHERE request_log_tags.request_log_id = request_logs.request_log_id
                    AND request_log_tags.tag_key = $10
                    AND request_log_tags.tag_value = $11
                )
              )
            "#,
        )
        .bind(request_id)
        .bind(model_key)
        .bind(provider_key)
        .bind(query.status_code)
        .bind(user_id.clone())
        .bind(team_id.clone())
        .bind(service)
        .bind(component)
        .bind(env)
        .bind(tag_key)
        .bind(tag_value)
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;
        let total: i64 = total_row.try_get(0).map_err(to_query_error)?;

        let mut items = sqlx::query(
            r#"
            SELECT request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                   resolved_model_key, provider_key, status_code, latency_ms, prompt_tokens,
                   completion_tokens, total_tokens, has_payload, request_payload_truncated,
                   response_payload_truncated, caller_service, caller_component, caller_env,
                   metadata_json, occurred_at, error_code
            FROM request_logs
            WHERE ($1::text IS NULL OR request_id = $1)
              AND ($2::text IS NULL OR model_key = $2)
              AND ($3::text IS NULL OR provider_key = $3)
              AND ($4::bigint IS NULL OR status_code = $4)
              AND ($5::text IS NULL OR user_id = $5)
              AND ($6::text IS NULL OR team_id = $6)
              AND ($7::text IS NULL OR caller_service = $7)
              AND ($8::text IS NULL OR caller_component = $8)
              AND ($9::text IS NULL OR caller_env = $9)
              AND (
                ($10::text IS NULL AND $11::text IS NULL)
                OR EXISTS (
                  SELECT 1
                  FROM request_log_tags
                  WHERE request_log_tags.request_log_id = request_logs.request_log_id
                    AND request_log_tags.tag_key = $10
                    AND request_log_tags.tag_value = $11
                )
              )
            ORDER BY occurred_at DESC, request_log_id DESC
            LIMIT $12 OFFSET $13
            "#,
        )
        .bind(request_id)
        .bind(model_key)
        .bind(provider_key)
        .bind(query.status_code)
        .bind(user_id)
        .bind(team_id)
        .bind(service)
        .bind(component)
        .bind(env)
        .bind(tag_key)
        .bind(tag_value)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?
        .iter()
        .map(decode_request_log_row)
        .collect::<Result<Vec<_>, _>>()?;

        let request_log_ids = items
            .iter()
            .map(|item| item.request_log_id)
            .collect::<Vec<_>>();
        let tag_map = load_bespoke_tags_for_logs(&self.pool, &request_log_ids).await?;
        for item in &mut items {
            item.request_tags.bespoke = tag_map
                .get(&item.request_log_id)
                .cloned()
                .unwrap_or_default();
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
    ) -> Result<RequestLogDetail, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT rl.request_log_id, rl.request_id, rl.api_key_id, rl.user_id, rl.team_id,
                   rl.model_key, rl.resolved_model_key, rl.provider_key, rl.status_code,
                   rl.latency_ms, rl.prompt_tokens, rl.completion_tokens, rl.total_tokens,
                   rl.has_payload, rl.request_payload_truncated, rl.response_payload_truncated,
                   rl.caller_service, rl.caller_component, rl.caller_env,
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
            return Err(StoreError::NotFound(format!(
                "request log `{request_log_id}` not found"
            )));
        };

        let mut log = decode_request_log_row(&row)?;
        log.request_tags.bespoke = load_bespoke_tags_for_logs(&self.pool, &[request_log_id])
            .await?
            .remove(&request_log_id)
            .unwrap_or_default();
        let request_json: Option<serde_json::Value> = row.try_get(22).map_err(to_query_error)?;
        let response_json: Option<serde_json::Value> = row.try_get(23).map_err(to_query_error)?;
        let payload = match (request_json, response_json) {
            (Some(request_json), Some(response_json)) => Some(RequestLogPayloadRecord {
                request_log_id,
                request_json,
                response_json,
            }),
            _ => None,
        };

        let attempts = self.list_request_log_attempts(request_log_id).await?;

        Ok(RequestLogDetail { log, payload, attempts })
    }
}

#[async_trait]
impl RequestAttemptRepository for PostgresStore {
    async fn list_request_log_attempts(
        &self,
        request_log_id: Uuid,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT request_attempt_id, request_log_id, request_id, attempt_number, route_id,
                   provider_key, upstream_model, status, status_code, error_code, error_detail,
                   error_detail_truncated, retryable, terminal, produced_final_response, stream,
                   started_at, completed_at, latency_ms, metadata_json
            FROM request_log_attempts
            WHERE request_log_id = $1
            ORDER BY attempt_number ASC
            "#,
        )
        .bind(request_log_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        rows.iter().map(decode_request_attempt_row).collect()
    }
}
