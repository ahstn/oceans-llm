use super::*;
use crate::shared::{parse_uuid, serialize_json, serialize_optional_json, unix_to_datetime};

const JOB_COLUMNS: &str = "batch_id, idempotency_key, request_hash, api_key_id, user_id, team_id, service_account_id, model_id, model_key, resolved_model_key, route_id, provider_key, upstream_model, endpoint, status, provider_batch_id, request_count, completed_count, failed_count, cost_usd_10000, pricing_status, provider_usage_json, error_json, created_at, submitted_at, completed_at, updated_at, next_poll_at, lease_owner, lease_expires_at, provider_context_json, pricing_snapshot_json";
const ITEM_COLUMNS: &str = "batch_item_id, batch_id, custom_id, status, request_body_json, response_body_json, error_json, provider_request_id, provider_usage_json, cost_usd_10000, completed_at, created_at, updated_at";

fn decode_optional_json(raw: Option<String>) -> Result<Option<serde_json::Value>, StoreError> {
    raw.as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn decode_job(row: &libsql::Row) -> Result<BatchJobRecord, StoreError> {
    let endpoint: String = row.get(13).map_err(to_query_error)?;
    let status: String = row.get(14).map_err(to_query_error)?;
    let pricing_status: String = row.get(20).map_err(to_query_error)?;
    let user_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let team_id: Option<String> = row.get(5).map_err(to_query_error)?;
    let service_account_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let submitted_at: Option<i64> = row.get(24).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.get(25).map_err(to_query_error)?;
    let next_poll_at: Option<i64> = row.get(27).map_err(to_query_error)?;
    let lease_expires_at: Option<i64> = row.get(29).map_err(to_query_error)?;

    Ok(BatchJobRecord {
        batch_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        idempotency_key: row.get(1).map_err(to_query_error)?,
        request_hash: row.get(2).map_err(to_query_error)?,
        api_key_id: parse_uuid(&row.get::<String>(3).map_err(to_query_error)?)?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        service_account_id: service_account_id.as_deref().map(parse_uuid).transpose()?,
        model_id: parse_uuid(&row.get::<String>(7).map_err(to_query_error)?)?,
        model_key: row.get(8).map_err(to_query_error)?,
        resolved_model_key: row.get(9).map_err(to_query_error)?,
        route_id: parse_uuid(&row.get::<String>(10).map_err(to_query_error)?)?,
        provider_key: row.get(11).map_err(to_query_error)?,
        upstream_model: row.get(12).map_err(to_query_error)?,
        endpoint: BatchEndpoint::from_db(&endpoint).ok_or_else(|| {
            StoreError::Serialization(format!("unknown batch endpoint `{endpoint}`"))
        })?,
        status: BatchStatus::from_db(&status)
            .ok_or_else(|| StoreError::Serialization(format!("unknown batch status `{status}`")))?,
        provider_batch_id: row.get(15).map_err(to_query_error)?,
        request_count: row.get(16).map_err(to_query_error)?,
        completed_count: row.get(17).map_err(to_query_error)?,
        failed_count: row.get(18).map_err(to_query_error)?,
        cost_usd: row
            .get::<Option<i64>>(19)
            .map_err(to_query_error)?
            .map(Money4::from_scaled),
        pricing_status: BatchPricingStatus::from_db(&pricing_status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown batch pricing status `{pricing_status}`"))
        })?,
        provider_usage: decode_optional_json(row.get(21).map_err(to_query_error)?)?,
        error: decode_optional_json(row.get(22).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.get(23).map_err(to_query_error)?)?,
        submitted_at: submitted_at.map(unix_to_datetime).transpose()?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        updated_at: unix_to_datetime(row.get(26).map_err(to_query_error)?)?,
        next_poll_at: next_poll_at.map(unix_to_datetime).transpose()?,
        lease_owner: row.get(28).map_err(to_query_error)?,
        lease_expires_at: lease_expires_at.map(unix_to_datetime).transpose()?,
        provider_context: serde_json::from_str(&row.get::<String>(30).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        pricing_snapshot: row
            .get::<Option<String>>(31)
            .map_err(to_query_error)?
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
    })
}

fn decode_item(row: &libsql::Row) -> Result<BatchItemRecord, StoreError> {
    let status: String = row.get(3).map_err(to_query_error)?;
    let request_body: String = row.get(4).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.get(10).map_err(to_query_error)?;
    Ok(BatchItemRecord {
        batch_item_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        batch_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
        custom_id: row.get(2).map_err(to_query_error)?,
        status: BatchItemStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown batch item status `{status}`"))
        })?,
        request_body: serde_json::from_str(&request_body)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        response_body: decode_optional_json(row.get(5).map_err(to_query_error)?)?,
        error: decode_optional_json(row.get(6).map_err(to_query_error)?)?,
        provider_request_id: row.get(7).map_err(to_query_error)?,
        provider_usage: decode_optional_json(row.get(8).map_err(to_query_error)?)?,
        cost_usd: row
            .get::<Option<i64>>(9)
            .map_err(to_query_error)?
            .map(Money4::from_scaled),
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(row.get(11).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(12).map_err(to_query_error)?)?,
    })
}

fn access_parts(scope: BatchAccessScope) -> (&'static str, Option<String>) {
    match scope {
        BatchAccessScope::All => ("all", None),
        BatchAccessScope::ApiKey(id) => ("api_key", Some(id.to_string())),
        BatchAccessScope::User(id) => ("user", Some(id.to_string())),
    }
}

async fn load_job(
    connection: &libsql::Connection,
    batch_id: Uuid,
    scope: BatchAccessScope,
) -> Result<BatchJobRecord, StoreError> {
    let (scope_kind, scope_id) = access_parts(scope);
    let sql = format!(
        "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE batch_id = ?1 AND (?2 = 'all' OR (?2 = 'api_key' AND api_key_id = ?3) OR (?2 = 'user' AND user_id = ?3))"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![batch_id.to_string(), scope_kind, scope_id],
        )
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_job(&row))
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(format!("batch `{batch_id}` was not found")))
}

#[async_trait]
impl BatchRepository for LibsqlStore {
    async fn insert_batch(&self, batch: &NewBatchJob) -> Result<BatchJobRecord, StoreError> {
        let job = &batch.job;
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let result = tx
            .execute(
                "INSERT INTO batch_jobs (batch_id, idempotency_key, request_hash, api_key_id, user_id, team_id, service_account_id, model_id, model_key, resolved_model_key, route_id, provider_key, upstream_model, endpoint, status, provider_batch_id, request_count, completed_count, failed_count, cost_usd_10000, pricing_status, provider_usage_json, error_json, created_at, submitted_at, completed_at, updated_at, next_poll_at, lease_owner, lease_expires_at, provider_context_json, pricing_snapshot_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32)",
                libsql::params![
                    job.batch_id.to_string(), job.idempotency_key.as_str(), job.request_hash.as_str(),
                    job.api_key_id.to_string(), job.user_id.map(|id| id.to_string()),
                    job.team_id.map(|id| id.to_string()), job.service_account_id.map(|id| id.to_string()),
                    job.model_id.to_string(), job.model_key.as_str(), job.resolved_model_key.as_str(),
                    job.route_id.to_string(), job.provider_key.as_str(), job.upstream_model.as_str(),
                    job.endpoint.as_str(), job.status.as_str(), job.provider_batch_id.as_deref(),
                    job.request_count, job.completed_count, job.failed_count,
                    job.cost_usd.map(Money4::as_scaled_i64), job.pricing_status.as_str(),
                    serialize_optional_json(job.provider_usage.as_ref())?, serialize_optional_json(job.error.as_ref())?,
                    job.created_at.unix_timestamp(), job.submitted_at.map(|time| time.unix_timestamp()),
                    job.completed_at.map(|time| time.unix_timestamp()), job.updated_at.unix_timestamp(),
                    job.next_poll_at.map(|time| time.unix_timestamp()), job.lease_owner.as_deref(),
                    job.lease_expires_at.map(|time| time.unix_timestamp()),
                    serialize_json(&job.provider_context)?, serialize_optional_json(job.pricing_snapshot.as_ref())?
                ],
            )
            .await;
        if let Err(error) = result {
            return Err(if error.to_string().contains("UNIQUE") {
                StoreError::Conflict("batch idempotency key already exists".to_string())
            } else {
                to_query_error(error)
            });
        }
        const INSERT_CHUNK_SIZE: usize = 1_000;
        for items in batch.items.chunks(INSERT_CHUNK_SIZE) {
            let placeholders = std::iter::repeat_n("(?, ?, ?, 'pending', ?, ?, ?)", items.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "INSERT INTO batch_items (batch_item_id, batch_id, custom_id, status, request_body_json, created_at, updated_at) VALUES {placeholders}"
            );
            let mut params = Vec::with_capacity(items.len() * 6);
            for item in items {
                params.push(libsql::Value::Text(item.batch_item_id.to_string()));
                params.push(libsql::Value::Text(job.batch_id.to_string()));
                params.push(libsql::Value::Text(item.custom_id.clone()));
                params.push(libsql::Value::Text(serialize_json(&item.request_body)?));
                params.push(libsql::Value::Integer(job.created_at.unix_timestamp()));
                params.push(libsql::Value::Integer(job.created_at.unix_timestamp()));
            }
            tx.execute(&sql, params).await.map_err(to_query_error)?;
        }
        tx.commit().await.map_err(to_query_error)?;
        Ok(job.clone())
    }

    async fn get_batch_by_idempotency_key(
        &self,
        api_key_id: Uuid,
        idempotency_key: &str,
    ) -> Result<Option<BatchJobRecord>, StoreError> {
        let sql = format!(
            "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE api_key_id = ?1 AND idempotency_key = ?2"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![api_key_id.to_string(), idempotency_key],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_job(&row))
            .transpose()
    }

    async fn get_batch(
        &self,
        batch_id: Uuid,
        scope: BatchAccessScope,
    ) -> Result<BatchJobRecord, StoreError> {
        load_job(&self.connection, batch_id, scope).await
    }

    async fn list_batches(
        &self,
        query: &BatchQuery,
        scope: BatchAccessScope,
    ) -> Result<BatchPage, StoreError> {
        let page_size = query.page_size.clamp(1, MAX_BATCH_PAGE_SIZE);
        let page = query.page.max(1);
        let offset = i64::from((page - 1).saturating_mul(page_size));
        let (scope_kind, scope_id) = access_parts(scope);
        let count_params = libsql::params![
            scope_kind,
            scope_id.clone(),
            query.status.map(|value| value.as_str().to_string()),
            query.model_key.as_deref(),
            query.provider_key.as_deref(),
            query.user_id.map(|id| id.to_string()),
            query.service_account_id.map(|id| id.to_string()),
            query.created_at_start.map(|time| time.unix_timestamp()),
            query.created_at_end.map(|time| time.unix_timestamp())
        ];
        let where_clause = "(?1 = 'all' OR (?1 = 'api_key' AND api_key_id = ?2) OR (?1 = 'user' AND user_id = ?2)) AND (?3 IS NULL OR status = ?3) AND (?4 IS NULL OR model_key = ?4) AND (?5 IS NULL OR provider_key = ?5) AND (?6 IS NULL OR user_id = ?6) AND (?7 IS NULL OR service_account_id = ?7) AND (?8 IS NULL OR created_at >= ?8) AND (?9 IS NULL OR created_at < ?9)";
        let count_sql = format!("SELECT COUNT(*) FROM batch_jobs WHERE {where_clause}");
        let mut count_rows = self
            .connection
            .query(&count_sql, count_params)
            .await
            .map_err(to_query_error)?;
        let total: i64 = count_rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::Unexpected("batch count returned no row".to_string()))?
            .get(0)
            .map_err(to_query_error)?;
        let list_sql = format!(
            "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE {where_clause} ORDER BY created_at DESC, batch_id DESC LIMIT ?10 OFFSET ?11"
        );
        let list_params = libsql::params![
            scope_kind,
            scope_id,
            query.status.map(|value| value.as_str().to_string()),
            query.model_key.as_deref(),
            query.provider_key.as_deref(),
            query.user_id.map(|id| id.to_string()),
            query.service_account_id.map(|id| id.to_string()),
            query.created_at_start.map(|time| time.unix_timestamp()),
            query.created_at_end.map(|time| time.unix_timestamp()),
            i64::from(page_size),
            offset
        ];
        let mut rows = self
            .connection
            .query(&list_sql, list_params)
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            items.push(decode_job(&row)?);
        }
        Ok(BatchPage {
            items,
            page,
            page_size,
            total: u64::try_from(total).unwrap_or_default(),
        })
    }

    async fn list_batch_items(
        &self,
        batch_id: Uuid,
        query: &BatchItemQuery,
        scope: BatchAccessScope,
    ) -> Result<BatchItemPage, StoreError> {
        let _ = load_job(&self.connection, batch_id, scope).await?;
        let page_size = query.page_size.clamp(1, MAX_BATCH_RESULT_PAGE_SIZE);
        let page = query.page.max(1);
        let offset = i64::from((page - 1).saturating_mul(page_size));
        let status = query.status.map(|value| value.as_str().to_string());
        let mut count_rows = self.connection.query("SELECT COUNT(*) FROM batch_items WHERE batch_id = ?1 AND (?2 IS NULL OR status = ?2)", libsql::params![batch_id.to_string(), status.clone()]).await.map_err(to_query_error)?;
        let total: i64 = count_rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::Unexpected("batch item count returned no row".to_string()))?
            .get(0)
            .map_err(to_query_error)?;
        let sql = format!(
            "SELECT {ITEM_COLUMNS} FROM batch_items WHERE batch_id = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY custom_id ASC LIMIT ?3 OFFSET ?4"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![batch_id.to_string(), status, i64::from(page_size), offset],
            )
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            items.push(decode_item(&row)?);
        }
        Ok(BatchItemPage {
            items,
            page,
            page_size,
            total: u64::try_from(total).unwrap_or_default(),
        })
    }

    async fn claim_batch_jobs(
        &self,
        worker_id: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<BatchJobRecord>, StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let mut rows = tx.query("SELECT batch_id FROM batch_jobs WHERE status IN ('queued', 'validating', 'in_progress', 'finalizing', 'cancel_requested', 'cancelling') AND (next_poll_at IS NULL OR next_poll_at <= ?1) AND (lease_expires_at IS NULL OR lease_expires_at <= ?1) ORDER BY created_at ASC LIMIT ?2", libsql::params![now.unix_timestamp(), i64::from(limit)]).await.map_err(to_query_error)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            ids.push(row.get::<String>(0).map_err(to_query_error)?);
        }
        drop(rows);
        for id in &ids {
            tx.execute("UPDATE batch_jobs SET status = CASE WHEN status = 'queued' THEN 'submitting' ELSE status END, lease_owner = ?1, lease_expires_at = ?2, updated_at = ?3 WHERE batch_id = ?4 AND (lease_expires_at IS NULL OR lease_expires_at <= ?3)", libsql::params![worker_id, lease_expires_at.unix_timestamp(), now.unix_timestamp(), id.as_str()]).await.map_err(to_query_error)?;
        }
        tx.commit().await.map_err(to_query_error)?;
        let mut claimed = Vec::new();
        for id in ids {
            let sql = format!(
                "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE batch_id = ?1 AND lease_owner = ?2"
            );
            let mut rows = self
                .connection
                .query(&sql, libsql::params![id, worker_id])
                .await
                .map_err(to_query_error)?;
            if let Some(row) = rows.next().await.map_err(to_query_error)? {
                claimed.push(decode_job(&row)?);
            }
        }
        Ok(claimed)
    }

    async fn mark_stale_batch_submissions_unknown(
        &self,
        now: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        self.connection.execute("UPDATE batch_jobs SET status = 'submission_unknown', error_json = '{\"message\":\"submission lease expired before the provider ID was stored; manual reconciliation is required\"}', completed_at = ?1, updated_at = ?1, lease_owner = NULL, lease_expires_at = NULL WHERE status = 'submitting' AND lease_expires_at <= ?1", [now.unix_timestamp()]).await.map_err(to_query_error)
    }

    async fn renew_batch_lease(
        &self,
        batch_id: Uuid,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE batch_jobs SET lease_expires_at=?1,updated_at=?2 WHERE batch_id=?3 AND lease_owner=?4 AND lease_expires_at>?2",
                libsql::params![
                    lease_expires_at.unix_timestamp(),
                    now.unix_timestamp(),
                    batch_id.to_string(),
                    lease_owner
                ],
            )
            .await
            .map_err(to_query_error)?;
        if changed == 0 {
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` lease was lost"
            )));
        }
        Ok(())
    }

    async fn mark_batch_submitted(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        state: &ProviderBatchState,
        next_poll_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let changed = self.connection.execute("UPDATE batch_jobs SET status = ?1, provider_batch_id = ?2, request_count = CASE WHEN ?3 > 0 THEN ?3 ELSE request_count END, completed_count = ?4, failed_count = ?5, cost_usd_10000 = ?6, pricing_status = ?7, provider_usage_json = ?8, error_json = ?9, submitted_at = COALESCE(?10, submitted_at), completed_at = ?11, updated_at = ?12, next_poll_at = ?13, lease_owner = NULL, lease_expires_at = NULL WHERE batch_id = ?14 AND lease_owner = ?15 AND status = 'submitting'", libsql::params![state.status.as_str(), state.provider_batch_id.as_str(), state.request_count, state.completed_count, state.failed_count, state.provider_cost_usd.map(Money4::as_scaled_i64), if state.provider_cost_usd.is_some() { BatchPricingStatus::ProviderReported } else { BatchPricingStatus::Pending }.as_str(), serialize_optional_json(state.provider_usage.as_ref())?, serialize_optional_json(state.error.as_ref())?, state.submitted_at.map(|time| time.unix_timestamp()), state.completed_at.map(|time| time.unix_timestamp()), OffsetDateTime::now_utc().unix_timestamp(), next_poll_at.unix_timestamp(), batch_id.to_string(), worker_id]).await.map_err(to_query_error)?;
        if changed == 0 {
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` lease was lost"
            )));
        }
        Ok(())
    }

    async fn mark_batch_submission_failed(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        status: BatchStatus,
        error: &serde_json::Value,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let changed = self.connection.execute("UPDATE batch_jobs SET status = ?1, error_json = ?2, completed_at = ?3, updated_at = ?3, next_poll_at = NULL, lease_owner = NULL, lease_expires_at = NULL WHERE batch_id = ?4 AND lease_owner = ?5", libsql::params![status.as_str(), serialize_json(error)?, completed_at.unix_timestamp(), batch_id.to_string(), worker_id]).await.map_err(to_query_error)?;
        if changed == 0 {
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` lease was lost"
            )));
        }
        Ok(())
    }

    async fn apply_batch_poll_update(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        update: &BatchPollUpdate,
    ) -> Result<(), StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let state = &update.state;
        let changed = tx.execute("UPDATE batch_jobs SET status = ?1, completed_count = ?2, failed_count = ?3, cost_usd_10000 = ?4, pricing_status = ?5, provider_usage_json = ?6, error_json = ?7, submitted_at = COALESCE(?8, submitted_at), completed_at = ?9, updated_at = ?10, next_poll_at = ?11, lease_owner = NULL, lease_expires_at = NULL WHERE batch_id = ?12 AND lease_owner = ?13", libsql::params![state.status.as_str(), state.completed_count, state.failed_count, state.provider_cost_usd.map(Money4::as_scaled_i64), update.pricing_status.unwrap_or(if state.provider_cost_usd.is_some() { BatchPricingStatus::ProviderReported } else { BatchPricingStatus::Pending }).as_str(), serialize_optional_json(state.provider_usage.as_ref())?, serialize_optional_json(state.error.as_ref())?, state.submitted_at.map(|time| time.unix_timestamp()), state.completed_at.map(|time| time.unix_timestamp()), OffsetDateTime::now_utc().unix_timestamp(), update.next_poll_at.map(|time| time.unix_timestamp()), batch_id.to_string(), worker_id]).await.map_err(to_query_error)?;
        if changed == 0 {
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` lease was lost"
            )));
        }
        for result in &update.results {
            tx.execute("UPDATE batch_items SET status = ?1, response_body_json = ?2, error_json = ?3, provider_request_id = ?4, provider_usage_json = ?5, completed_at = ?6, updated_at = ?7, cost_usd_10000 = ?10 WHERE batch_id = ?8 AND custom_id = ?9", libsql::params![if result.error.is_some() { "failed" } else { "succeeded" }, serialize_optional_json(result.response_body.as_ref())?, serialize_optional_json(result.error.as_ref())?, result.provider_request_id.as_deref(), serialize_optional_json(result.provider_usage.as_ref())?, result.completed_at.map(|time| time.unix_timestamp()), OffsetDateTime::now_utc().unix_timestamp(), batch_id.to_string(), result.custom_id.as_str(), result.cost_usd.map(Money4::as_scaled_i64)]).await.map_err(to_query_error)?;
        }
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }

    async fn release_batch_lease_after_error(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        error: &serde_json::Value,
        next_poll_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let changed = self.connection.execute("UPDATE batch_jobs SET status = CASE WHEN status = 'submitting' THEN 'queued' ELSE status END, error_json = ?1, next_poll_at = ?2, updated_at = ?3, lease_owner = NULL, lease_expires_at = NULL WHERE batch_id = ?4 AND lease_owner = ?5", libsql::params![serialize_json(error)?, next_poll_at.unix_timestamp(), OffsetDateTime::now_utc().unix_timestamp(), batch_id.to_string(), worker_id]).await.map_err(to_query_error)?;
        if changed == 0 {
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` lease was lost"
            )));
        }
        Ok(())
    }

    async fn request_batch_cancel(
        &self,
        batch_id: Uuid,
        scope: BatchAccessScope,
        requested_at: OffsetDateTime,
    ) -> Result<BatchJobRecord, StoreError> {
        let current = load_job(&self.connection, batch_id, scope).await?;
        if current.status.is_terminal() {
            return Ok(current);
        }
        if current.status == BatchStatus::Submitting {
            return Err(StoreError::Conflict(
                "batch submission is in progress; retry cancellation after submission finishes"
                    .to_string(),
            ));
        }
        let (status, completed_at) = if current.status == BatchStatus::Queued {
            ("cancelled", Some(requested_at.unix_timestamp()))
        } else {
            ("cancel_requested", None)
        };
        let changed = self.connection.execute("UPDATE batch_jobs SET status = ?1, completed_at = ?2, next_poll_at = ?3, updated_at = ?3, lease_owner = NULL, lease_expires_at = NULL WHERE batch_id = ?4 AND status = ?5", libsql::params![status, completed_at, requested_at.unix_timestamp(), batch_id.to_string(), current.status.as_str()]).await.map_err(to_query_error)?;
        if changed == 0 {
            let latest = load_job(&self.connection, batch_id, scope).await?;
            if latest.status.is_terminal() {
                return Ok(latest);
            }
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` changed while cancellation was requested"
            )));
        }
        load_job(&self.connection, batch_id, scope).await
    }

    async fn replace_batch_item_requests(
        &self,
        batch_id: Uuid,
        requests: &[gateway_core::ProviderBatchRequestItem],
    ) -> Result<(), StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        for request in requests {
            tx.execute(
                "UPDATE batch_items SET request_body_json = ?1, updated_at = ?2 WHERE batch_id = ?3 AND custom_id = ?4",
                libsql::params![
                    serialize_json(&request.body)?,
                    OffsetDateTime::now_utc().unix_timestamp(),
                    batch_id.to_string(),
                    request.custom_id.clone()
                ],
            )
            .await
            .map_err(to_query_error)?;
        }
        tx.commit().await.map_err(to_query_error)
    }

    async fn get_batch_items_for_worker(
        &self,
        batch_id: Uuid,
    ) -> Result<Vec<BatchItemRecord>, StoreError> {
        let sql = format!(
            "SELECT {ITEM_COLUMNS} FROM batch_items WHERE batch_id = ?1 ORDER BY custom_id ASC"
        );
        let mut rows = self
            .connection
            .query(&sql, [batch_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            items.push(decode_item(&row)?);
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gateway_core::{
        BatchAccessScope, BatchEndpoint, BatchItemQuery, BatchJobRecord, BatchPollUpdate,
        BatchPricingPolicy, BatchPricingSnapshot, BatchPricingStatus, BatchQuery, BatchRepository,
        BatchStatus, BatchTokenRates, Money4, NewBatchItem, NewBatchJob, ProviderBatchResult,
        ProviderBatchState, ProviderRequestContext, RouteCompatibility, StoreError,
    };
    use serde_json::{Map, json};
    use tempfile::tempdir;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use crate::{LibsqlStore, run_migrations};

    #[tokio::test]
    async fn batch_lifecycle_preserves_scope_results_and_cost() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let now = OffsetDateTime::now_utc();
        let user_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        seed_dependencies(&store, now, user_id, api_key_id, model_id, route_id).await;

        let batch_id = Uuid::new_v4();
        let new_job = NewBatchJob {
            job: BatchJobRecord {
                batch_id,
                idempotency_key: "retry-key".to_string(),
                request_hash: "request-hash".to_string(),
                api_key_id,
                user_id: Some(user_id),
                team_id: None,
                service_account_id: None,
                model_id,
                model_key: "analysis".to_string(),
                resolved_model_key: "analysis".to_string(),
                route_id,
                provider_key: "openai".to_string(),
                upstream_model: "gpt-test".to_string(),
                endpoint: BatchEndpoint::ChatCompletions,
                status: BatchStatus::Queued,
                provider_batch_id: None,
                request_count: 1,
                completed_count: 0,
                failed_count: 0,
                cost_usd: None,
                pricing_status: BatchPricingStatus::Pending,
                provider_usage: None,
                error: None,
                created_at: now,
                submitted_at: None,
                completed_at: None,
                updated_at: now,
                next_poll_at: None,
                lease_owner: None,
                lease_expires_at: None,
                provider_context: ProviderRequestContext {
                    request_id: batch_id.to_string(),
                    model_key: "analysis".to_string(),
                    provider_key: "openai".to_string(),
                    upstream_model: "gpt-test".to_string(),
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    request_headers: BTreeMap::new(),
                    compatibility: RouteCompatibility::default(),
                },
                pricing_snapshot: Some(BatchPricingSnapshot {
                    rates: Some(BatchTokenRates {
                        input: Some(Money4::from_scaled(20_000)),
                        output: Some(Money4::from_scaled(80_000)),
                        cache_read: None,
                        cache_write: None,
                    }),
                    policy: BatchPricingPolicy::HalfAllTokenRates,
                }),
            },
            items: vec![NewBatchItem {
                batch_item_id: Uuid::new_v4(),
                custom_id: "row-1".to_string(),
                request_body: json!({"messages": [{"role": "user", "content": "hello"}]}),
            }],
        };
        store.insert_batch(&new_job).await.expect("insert batch");

        let replay = store
            .get_batch_by_idempotency_key(api_key_id, "retry-key")
            .await
            .expect("idempotency lookup")
            .expect("batch exists");
        assert_eq!(replay.batch_id, batch_id);
        assert_eq!(replay.pricing_snapshot, new_job.job.pricing_snapshot);
        assert!(matches!(
            store
                .get_batch(batch_id, BatchAccessScope::ApiKey(Uuid::new_v4()))
                .await,
            Err(StoreError::NotFound(_))
        ));
        let page = store
            .list_batches(
                &BatchQuery {
                    model_key: Some("analysis".to_string()),
                    ..BatchQuery::default()
                },
                BatchAccessScope::User(user_id),
            )
            .await
            .expect("list batches");
        assert_eq!(page.total, 1);

        let claimed = store
            .claim_batch_jobs("worker-1", now, now + Duration::minutes(2), 1)
            .await
            .expect("claim submission");
        assert_eq!(claimed[0].status, BatchStatus::Submitting);
        assert!(matches!(
            store
                .renew_batch_lease(
                    batch_id,
                    "stale-worker",
                    now + Duration::seconds(10),
                    now + Duration::minutes(4),
                )
                .await,
            Err(StoreError::Conflict(_))
        ));
        store
            .renew_batch_lease(
                batch_id,
                "worker-1",
                now + Duration::seconds(10),
                now + Duration::minutes(4),
            )
            .await
            .expect("renew submission lease");
        store
            .release_batch_lease_after_error(
                batch_id,
                "worker-1",
                &json!({"message": "retry"}),
                now + Duration::seconds(20),
            )
            .await
            .expect("release submission lease");
        let released = store
            .get_batch(batch_id, BatchAccessScope::ApiKey(api_key_id))
            .await
            .expect("get released batch");
        assert_eq!(released.status, BatchStatus::Queued);
        let reclaimed = store
            .claim_batch_jobs(
                "worker-2",
                now + Duration::seconds(30),
                now + Duration::minutes(4),
                1,
            )
            .await
            .expect("reclaim submission");
        assert_eq!(reclaimed[0].status, BatchStatus::Submitting);
        store
            .mark_batch_submitted(
                batch_id,
                "worker-2",
                &ProviderBatchState {
                    provider_batch_id: "provider-batch-1".to_string(),
                    status: BatchStatus::InProgress,
                    request_count: 0,
                    completed_count: 0,
                    failed_count: 0,
                    provider_usage: None,
                    provider_cost_usd: None,
                    error: None,
                    submitted_at: Some(now),
                    completed_at: None,
                },
                now + Duration::minutes(1),
            )
            .await
            .expect("mark submitted");
        let submitted = store
            .get_batch(batch_id, BatchAccessScope::ApiKey(api_key_id))
            .await
            .expect("get submitted batch");
        assert_eq!(submitted.request_count, 1);
        let claimed = store
            .claim_batch_jobs(
                "poll-worker",
                now + Duration::minutes(1),
                now + Duration::minutes(3),
                1,
            )
            .await
            .expect("claim provider poll");
        assert_eq!(claimed[0].status, BatchStatus::InProgress);
        let cancellation_time = now + Duration::minutes(1) + Duration::seconds(1);
        let cancellation = store
            .request_batch_cancel(
                batch_id,
                BatchAccessScope::ApiKey(api_key_id),
                cancellation_time,
            )
            .await
            .expect("request cancellation");
        assert_eq!(cancellation.status, BatchStatus::CancelRequested);
        assert_eq!(cancellation.lease_owner, None);

        let completed_update = BatchPollUpdate {
            state: ProviderBatchState {
                provider_batch_id: "provider-batch-1".to_string(),
                status: BatchStatus::Completed,
                request_count: 1,
                completed_count: 1,
                failed_count: 0,
                provider_usage: Some(json!({"input_tokens": 10, "output_tokens": 2})),
                provider_cost_usd: Some(Money4::from_scaled(25)),
                error: None,
                submitted_at: Some(now),
                completed_at: Some(now + Duration::seconds(1)),
            },
            results: vec![ProviderBatchResult {
                custom_id: "row-1".to_string(),
                response_body: Some(json!({"id": "response-1"})),
                error: None,
                provider_request_id: Some("request-1".to_string()),
                provider_usage: Some(json!({"input_tokens": 10, "output_tokens": 2})),
                completed_at: Some(now + Duration::seconds(1)),
                cost_usd: Some(Money4::from_scaled(25)),
            }],
            next_poll_at: None,
            pricing_status: Some(BatchPricingStatus::ProviderReported),
        };
        assert!(matches!(
            store
                .apply_batch_poll_update(batch_id, "poll-worker", &completed_update)
                .await,
            Err(StoreError::Conflict(_))
        ));
        let pending_cancellation = store
            .get_batch(batch_id, BatchAccessScope::ApiKey(api_key_id))
            .await
            .expect("get pending cancellation");
        assert_eq!(pending_cancellation.status, BatchStatus::CancelRequested);

        let claimed = store
            .claim_batch_jobs(
                "worker-3",
                cancellation_time + Duration::seconds(1),
                cancellation_time + Duration::minutes(2),
                1,
            )
            .await
            .expect("claim cancellation");
        assert_eq!(claimed[0].status, BatchStatus::CancelRequested);
        store
            .apply_batch_poll_update(batch_id, "worker-3", &completed_update)
            .await
            .expect("apply result");

        let completed = store
            .get_batch(batch_id, BatchAccessScope::ApiKey(api_key_id))
            .await
            .expect("get completed batch");
        assert_eq!(completed.status, BatchStatus::Completed);
        assert_eq!(completed.cost_usd, Some(Money4::from_scaled(25)));
        let results = store
            .list_batch_items(
                batch_id,
                &BatchItemQuery::default(),
                BatchAccessScope::ApiKey(api_key_id),
            )
            .await
            .expect("list results");
        assert_eq!(results.total, 1);
        assert_eq!(
            results.items[0].response_body,
            Some(json!({"id": "response-1"}))
        );
        assert_eq!(results.items[0].cost_usd, Some(Money4::from_scaled(25)));
    }

    async fn seed_dependencies(
        store: &LibsqlStore,
        now: OffsetDateTime,
        user_id: Uuid,
        api_key_id: Uuid,
        model_id: Uuid,
        route_id: Uuid,
    ) {
        let timestamp = now.unix_timestamp();
        store.connection.execute("INSERT INTO users (user_id,name,email,email_normalized,global_role,auth_mode,status,must_change_password,request_logging_enabled,model_access_mode,created_at,updated_at) VALUES (?1,'User','user@example.com','user@example.com','user','password','active',0,1,'all',?2,?2)", libsql::params![user_id.to_string(), timestamp]).await.expect("insert user");
        store.connection.execute("INSERT INTO api_keys (id,public_id,secret_hash,name,status,owner_kind,owner_user_id,created_at) VALUES (?1,'gwk_test','hash','Test','active','user',?2,?3)", libsql::params![api_key_id.to_string(), user_id.to_string(), timestamp]).await.expect("insert key");
        store.connection.execute("INSERT INTO providers (provider_key,provider_type,config_json,created_at,updated_at) VALUES ('openai','openai_compat','{}',?1,?1)", [timestamp]).await.expect("insert provider");
        store.connection.execute("INSERT INTO gateway_models (id,model_key,tags_json,rank,created_at,updated_at) VALUES (?1,'analysis','[]',100,?2,?2)", libsql::params![model_id.to_string(), timestamp]).await.expect("insert model");
        store.connection.execute("INSERT INTO model_routes (id,model_id,provider_key,upstream_model,priority,weight,enabled,extra_headers_json,extra_body_json,created_at,updated_at) VALUES (?1,?2,'openai','gpt-test',100,1.0,1,'{}','{}',?3,?3)", libsql::params![route_id.to_string(), model_id.to_string(), timestamp]).await.expect("insert route");
    }
}
