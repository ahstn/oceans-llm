use super::*;
use crate::shared::{parse_uuid, serialize_json, serialize_optional_json, unix_to_datetime};

const JOB_COLUMNS: &str = "batch_id, idempotency_key, request_hash, api_key_id, user_id, team_id, service_account_id, model_id, model_key, resolved_model_key, route_id, provider_key, upstream_model, endpoint, status, provider_batch_id, request_count, completed_count, failed_count, cost_usd_10000, pricing_status, provider_usage_json, error_json, created_at, submitted_at, completed_at, updated_at, next_poll_at, lease_owner, lease_expires_at, provider_context_json, pricing_snapshot_json";
const CLAIMED_JOB_COLUMNS: &str = "b.batch_id, b.idempotency_key, b.request_hash, b.api_key_id, b.user_id, b.team_id, b.service_account_id, b.model_id, b.model_key, b.resolved_model_key, b.route_id, b.provider_key, b.upstream_model, b.endpoint, b.status, b.provider_batch_id, b.request_count, b.completed_count, b.failed_count, b.cost_usd_10000, b.pricing_status, b.provider_usage_json, b.error_json, b.created_at, b.submitted_at, b.completed_at, b.updated_at, b.next_poll_at, b.lease_owner, b.lease_expires_at, b.provider_context_json, b.pricing_snapshot_json";
const ITEM_COLUMNS: &str = "batch_item_id, batch_id, custom_id, status, request_body_json, response_body_json, error_json, provider_request_id, provider_usage_json, cost_usd_10000, completed_at, created_at, updated_at";

fn optional_json(raw: Option<String>) -> Result<Option<serde_json::Value>, StoreError> {
    raw.as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

struct SerializedItemUpdate {
    status: &'static str,
    response_body: Option<String>,
    error: Option<String>,
    provider_request_id: Option<String>,
    provider_usage: Option<String>,
    completed_at: Option<i64>,
    updated_at: i64,
    batch_id: String,
    custom_id: String,
    cost_usd_10000: Option<i64>,
}

fn decode_job(row: &PgRow) -> Result<BatchJobRecord, StoreError> {
    let endpoint: String = row.try_get("endpoint").map_err(to_query_error)?;
    let status: String = row.try_get("status").map_err(to_query_error)?;
    let pricing: String = row.try_get("pricing_status").map_err(to_query_error)?;
    let user_id: Option<String> = row.try_get("user_id").map_err(to_query_error)?;
    let team_id: Option<String> = row.try_get("team_id").map_err(to_query_error)?;
    let service_account_id: Option<String> =
        row.try_get("service_account_id").map_err(to_query_error)?;
    let submitted_at: Option<i64> = row.try_get("submitted_at").map_err(to_query_error)?;
    let completed_at: Option<i64> = row.try_get("completed_at").map_err(to_query_error)?;
    let next_poll_at: Option<i64> = row.try_get("next_poll_at").map_err(to_query_error)?;
    let lease_expires_at: Option<i64> = row.try_get("lease_expires_at").map_err(to_query_error)?;
    Ok(BatchJobRecord {
        batch_id: parse_uuid(
            &row.try_get::<String, _>("batch_id")
                .map_err(to_query_error)?,
        )?,
        idempotency_key: row.try_get("idempotency_key").map_err(to_query_error)?,
        request_hash: row.try_get("request_hash").map_err(to_query_error)?,
        api_key_id: parse_uuid(
            &row.try_get::<String, _>("api_key_id")
                .map_err(to_query_error)?,
        )?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        service_account_id: service_account_id.as_deref().map(parse_uuid).transpose()?,
        model_id: parse_uuid(
            &row.try_get::<String, _>("model_id")
                .map_err(to_query_error)?,
        )?,
        model_key: row.try_get("model_key").map_err(to_query_error)?,
        resolved_model_key: row.try_get("resolved_model_key").map_err(to_query_error)?,
        route_id: parse_uuid(
            &row.try_get::<String, _>("route_id")
                .map_err(to_query_error)?,
        )?,
        provider_key: row.try_get("provider_key").map_err(to_query_error)?,
        upstream_model: row.try_get("upstream_model").map_err(to_query_error)?,
        endpoint: BatchEndpoint::from_db(&endpoint).ok_or_else(|| {
            StoreError::Serialization(format!("unknown batch endpoint `{endpoint}`"))
        })?,
        status: BatchStatus::from_db(&status)
            .ok_or_else(|| StoreError::Serialization(format!("unknown batch status `{status}`")))?,
        provider_batch_id: row.try_get("provider_batch_id").map_err(to_query_error)?,
        request_count: row.try_get("request_count").map_err(to_query_error)?,
        completed_count: row.try_get("completed_count").map_err(to_query_error)?,
        failed_count: row.try_get("failed_count").map_err(to_query_error)?,
        cost_usd: row
            .try_get::<Option<i64>, _>("cost_usd_10000")
            .map_err(to_query_error)?
            .map(Money4::from_scaled),
        pricing_status: BatchPricingStatus::from_db(&pricing).ok_or_else(|| {
            StoreError::Serialization(format!("unknown batch pricing status `{pricing}`"))
        })?,
        provider_usage: optional_json(row.try_get("provider_usage_json").map_err(to_query_error)?)?,
        error: optional_json(row.try_get("error_json").map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.try_get("created_at").map_err(to_query_error)?)?,
        submitted_at: submitted_at.map(unix_to_datetime).transpose()?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        updated_at: unix_to_datetime(row.try_get("updated_at").map_err(to_query_error)?)?,
        next_poll_at: next_poll_at.map(unix_to_datetime).transpose()?,
        lease_owner: row.try_get("lease_owner").map_err(to_query_error)?,
        lease_expires_at: lease_expires_at.map(unix_to_datetime).transpose()?,
        provider_context: serde_json::from_str(
            &row.try_get::<String, _>("provider_context_json")
                .map_err(to_query_error)?,
        )
        .map_err(|error| StoreError::Serialization(error.to_string()))?,
        pricing_snapshot: optional_json(
            row.try_get("pricing_snapshot_json")
                .map_err(to_query_error)?,
        )?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| StoreError::Serialization(error.to_string()))?,
    })
}

fn decode_item(row: &PgRow) -> Result<BatchItemRecord, StoreError> {
    let status: String = row.try_get("status").map_err(to_query_error)?;
    let request: String = row.try_get("request_body_json").map_err(to_query_error)?;
    let completed_at: Option<i64> = row.try_get("completed_at").map_err(to_query_error)?;
    Ok(BatchItemRecord {
        batch_item_id: parse_uuid(
            &row.try_get::<String, _>("batch_item_id")
                .map_err(to_query_error)?,
        )?,
        batch_id: parse_uuid(
            &row.try_get::<String, _>("batch_id")
                .map_err(to_query_error)?,
        )?,
        custom_id: row.try_get("custom_id").map_err(to_query_error)?,
        status: BatchItemStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown batch item status `{status}`"))
        })?,
        request_body: serde_json::from_str(&request)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        response_body: optional_json(row.try_get("response_body_json").map_err(to_query_error)?)?,
        error: optional_json(row.try_get("error_json").map_err(to_query_error)?)?,
        provider_request_id: row.try_get("provider_request_id").map_err(to_query_error)?,
        provider_usage: optional_json(row.try_get("provider_usage_json").map_err(to_query_error)?)?,
        cost_usd: row
            .try_get::<Option<i64>, _>("cost_usd_10000")
            .map_err(to_query_error)?
            .map(Money4::from_scaled),
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(row.try_get("created_at").map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get("updated_at").map_err(to_query_error)?)?,
    })
}

fn scope_parts(scope: BatchAccessScope) -> (&'static str, Option<String>) {
    match scope {
        BatchAccessScope::All => ("all", None),
        BatchAccessScope::ApiKey(id) => ("api_key", Some(id.to_string())),
        BatchAccessScope::User(id) => ("user", Some(id.to_string())),
    }
}

async fn load_job(
    pool: &PgPool,
    batch_id: Uuid,
    scope: BatchAccessScope,
) -> Result<BatchJobRecord, StoreError> {
    let (scope_kind, scope_id) = scope_parts(scope);
    let sql = format!(
        "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE batch_id = $1 AND ($2 = 'all' OR ($2 = 'api_key' AND api_key_id = $3) OR ($2 = 'user' AND user_id = $3))"
    );
    sqlx::query(&sql)
        .bind(batch_id.to_string())
        .bind(scope_kind)
        .bind(scope_id)
        .fetch_optional(pool)
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode_job)
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(format!("batch `{batch_id}` was not found")))
}

#[async_trait]
impl BatchRepository for PostgresStore {
    async fn insert_batch(&self, batch: &NewBatchJob) -> Result<BatchJobRecord, StoreError> {
        let job = &batch.job;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let result = sqlx::query("INSERT INTO batch_jobs (batch_id, idempotency_key, request_hash, api_key_id, user_id, team_id, service_account_id, model_id, model_key, resolved_model_key, route_id, provider_key, upstream_model, endpoint, status, provider_batch_id, request_count, completed_count, failed_count, cost_usd_10000, pricing_status, provider_usage_json, error_json, created_at, submitted_at, completed_at, updated_at, next_poll_at, lease_owner, lease_expires_at, provider_context_json, pricing_snapshot_json) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32)")
            .bind(job.batch_id.to_string()).bind(&job.idempotency_key).bind(&job.request_hash).bind(job.api_key_id.to_string())
            .bind(job.user_id.map(|id| id.to_string())).bind(job.team_id.map(|id| id.to_string())).bind(job.service_account_id.map(|id| id.to_string()))
            .bind(job.model_id.to_string()).bind(&job.model_key).bind(&job.resolved_model_key).bind(job.route_id.to_string()).bind(&job.provider_key).bind(&job.upstream_model)
            .bind(job.endpoint.as_str()).bind(job.status.as_str()).bind(&job.provider_batch_id).bind(job.request_count).bind(job.completed_count).bind(job.failed_count)
            .bind(job.cost_usd.map(Money4::as_scaled_i64)).bind(job.pricing_status.as_str()).bind(serialize_optional_json(job.provider_usage.as_ref())?).bind(serialize_optional_json(job.error.as_ref())?)
            .bind(job.created_at.unix_timestamp()).bind(job.submitted_at.map(|time| time.unix_timestamp())).bind(job.completed_at.map(|time| time.unix_timestamp()))
            .bind(job.updated_at.unix_timestamp()).bind(job.next_poll_at.map(|time| time.unix_timestamp())).bind(&job.lease_owner).bind(job.lease_expires_at.map(|time| time.unix_timestamp())).bind(serialize_json(&job.provider_context)?).bind(serialize_optional_json(job.pricing_snapshot.as_ref())?)
            .execute(&mut *tx).await;
        if let Err(error) = result {
            return Err(
                if error
                    .as_database_error()
                    .and_then(|db| db.code())
                    .as_deref()
                    == Some("23505")
                {
                    StoreError::Conflict("batch idempotency key already exists".to_string())
                } else {
                    to_query_error(error)
                },
            );
        }
        const INSERT_CHUNK_SIZE: usize = 5_000;
        for items in batch.items.chunks(INSERT_CHUNK_SIZE) {
            let rows = items
                .iter()
                .map(|item| {
                    Ok((
                        item.batch_item_id.to_string(),
                        job.batch_id.to_string(),
                        item.custom_id.as_str(),
                        serialize_json(&item.request_body)?,
                        job.created_at.unix_timestamp(),
                    ))
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "INSERT INTO batch_items (batch_item_id, batch_id, custom_id, status, request_body_json, created_at, updated_at) ",
            );
            builder.push_values(rows, |mut values, row| {
                values
                    .push_bind(row.0)
                    .push_bind(row.1)
                    .push_bind(row.2)
                    .push("'pending'")
                    .push_bind(row.3)
                    .push_bind(row.4)
                    .push_bind(row.4);
            });
            builder
                .build()
                .execute(&mut *tx)
                .await
                .map_err(to_query_error)?;
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
            "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE api_key_id = $1 AND idempotency_key = $2"
        );
        sqlx::query(&sql)
            .bind(api_key_id.to_string())
            .bind(idempotency_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_job)
            .transpose()
    }

    async fn get_batch(
        &self,
        batch_id: Uuid,
        scope: BatchAccessScope,
    ) -> Result<BatchJobRecord, StoreError> {
        load_job(&self.pool, batch_id, scope).await
    }

    async fn list_batches(
        &self,
        query: &BatchQuery,
        scope: BatchAccessScope,
    ) -> Result<BatchPage, StoreError> {
        let page_size = query.page_size.clamp(1, MAX_BATCH_PAGE_SIZE);
        let page = query.page.max(1);
        let offset = i64::from((page - 1).saturating_mul(page_size));
        let (scope_kind, scope_id) = scope_parts(scope);
        let where_sql = "($1 = 'all' OR ($1 = 'api_key' AND api_key_id = $2) OR ($1 = 'user' AND user_id = $2)) AND ($3 IS NULL OR status = $3) AND ($4 IS NULL OR model_key = $4) AND ($5 IS NULL OR provider_key = $5) AND ($6 IS NULL OR user_id = $6) AND ($7 IS NULL OR service_account_id = $7) AND ($8 IS NULL OR created_at >= $8) AND ($9 IS NULL OR created_at < $9)";
        let status = query.status.map(|value| value.as_str().to_string());
        let user_id = query.user_id.map(|id| id.to_string());
        let service_account_id = query.service_account_id.map(|id| id.to_string());
        let created_at_start = query.created_at_start.map(|time| time.unix_timestamp());
        let created_at_end = query.created_at_end.map(|time| time.unix_timestamp());
        let count_sql = format!("SELECT COUNT(*) FROM batch_jobs WHERE {where_sql}");
        let total: i64 = sqlx::query(&count_sql)
            .bind(scope_kind)
            .bind(scope_id.clone())
            .bind(status.clone())
            .bind(query.model_key.as_deref())
            .bind(query.provider_key.as_deref())
            .bind(user_id.clone())
            .bind(service_account_id.clone())
            .bind(created_at_start)
            .bind(created_at_end)
            .fetch_one(&self.pool)
            .await
            .map_err(to_query_error)?
            .try_get(0)
            .map_err(to_query_error)?;
        let list_sql = format!(
            "SELECT {JOB_COLUMNS} FROM batch_jobs WHERE {where_sql} ORDER BY created_at DESC, batch_id DESC LIMIT $10 OFFSET $11"
        );
        let rows = sqlx::query(&list_sql)
            .bind(scope_kind)
            .bind(scope_id)
            .bind(status)
            .bind(query.model_key.as_deref())
            .bind(query.provider_key.as_deref())
            .bind(user_id)
            .bind(service_account_id)
            .bind(created_at_start)
            .bind(created_at_end)
            .bind(i64::from(page_size))
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        Ok(BatchPage {
            items: rows.iter().map(decode_job).collect::<Result<_, _>>()?,
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
        let _ = load_job(&self.pool, batch_id, scope).await?;
        let page_size = query.page_size.clamp(1, MAX_BATCH_RESULT_PAGE_SIZE);
        let page = query.page.max(1);
        let offset = i64::from((page - 1).saturating_mul(page_size));
        let status = query.status.map(|s| s.as_str().to_string());
        let total: i64 = sqlx::query(
            "SELECT COUNT(*) FROM batch_items WHERE batch_id = $1 AND ($2 IS NULL OR status = $2)",
        )
        .bind(batch_id.to_string())
        .bind(status.clone())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?
        .try_get(0)
        .map_err(to_query_error)?;
        let sql = format!(
            "SELECT {ITEM_COLUMNS} FROM batch_items WHERE batch_id = $1 AND ($2 IS NULL OR status = $2) ORDER BY custom_id ASC LIMIT $3 OFFSET $4"
        );
        let rows = sqlx::query(&sql)
            .bind(batch_id.to_string())
            .bind(status)
            .bind(i64::from(page_size))
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        Ok(BatchItemPage {
            items: rows.iter().map(decode_item).collect::<Result<_, _>>()?,
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
        let sql = format!(
            "WITH selected AS (SELECT batch_id FROM batch_jobs WHERE status IN ('queued','validating','in_progress','finalizing','cancel_requested','cancelling') AND (next_poll_at IS NULL OR next_poll_at <= $1) AND (lease_expires_at IS NULL OR lease_expires_at <= $1) ORDER BY created_at ASC FOR UPDATE SKIP LOCKED LIMIT $2) UPDATE batch_jobs b SET status = CASE WHEN b.status = 'queued' THEN 'submitting' ELSE b.status END, lease_owner = $3, lease_expires_at = $4, updated_at = $1 FROM selected WHERE b.batch_id = selected.batch_id RETURNING {CLAIMED_JOB_COLUMNS}"
        );
        let rows = sqlx::query(&sql)
            .bind(now.unix_timestamp())
            .bind(i64::from(limit))
            .bind(worker_id)
            .bind(lease_expires_at.unix_timestamp())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_job).collect()
    }

    async fn mark_stale_batch_submissions_unknown(
        &self,
        now: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        Ok(sqlx::query("UPDATE batch_jobs SET status='submission_unknown', error_json='{\"message\":\"submission lease expired before the provider ID was stored; manual reconciliation is required\"}', completed_at=$1, updated_at=$1, lease_owner=NULL, lease_expires_at=NULL WHERE status='submitting' AND lease_expires_at <= $1").bind(now.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?.rows_affected())
    }

    async fn renew_batch_lease(
        &self,
        batch_id: Uuid,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let changed = sqlx::query(
            "UPDATE batch_jobs SET lease_expires_at=$1,updated_at=$2 WHERE batch_id=$3 AND lease_owner=$4 AND lease_expires_at>$2",
        )
        .bind(lease_expires_at.unix_timestamp())
        .bind(now.unix_timestamp())
        .bind(batch_id.to_string())
        .bind(lease_owner)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?
        .rows_affected();
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
        let changed = sqlx::query("UPDATE batch_jobs SET status=$1, provider_batch_id=$2, request_count=CASE WHEN $3 > 0 THEN $3 ELSE request_count END, completed_count=$4, failed_count=$5, cost_usd_10000=$6, pricing_status=$7, provider_usage_json=$8, error_json=$9, submitted_at=COALESCE($10,submitted_at), completed_at=$11, updated_at=$12, next_poll_at=$13, lease_owner=NULL, lease_expires_at=NULL WHERE batch_id=$14 AND lease_owner=$15 AND status='submitting'").bind(state.status.as_str()).bind(&state.provider_batch_id).bind(state.request_count).bind(state.completed_count).bind(state.failed_count).bind(state.provider_cost_usd.map(Money4::as_scaled_i64)).bind(if state.provider_cost_usd.is_some(){BatchPricingStatus::ProviderReported}else{BatchPricingStatus::Pending}.as_str()).bind(serialize_optional_json(state.provider_usage.as_ref())?).bind(serialize_optional_json(state.error.as_ref())?).bind(state.submitted_at.map(|t|t.unix_timestamp())).bind(state.completed_at.map(|t|t.unix_timestamp())).bind(OffsetDateTime::now_utc().unix_timestamp()).bind(next_poll_at.unix_timestamp()).bind(batch_id.to_string()).bind(worker_id).execute(&self.pool).await.map_err(to_query_error)?.rows_affected();
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
        let changed = sqlx::query("UPDATE batch_jobs SET status=$1,error_json=$2,completed_at=$3,updated_at=$3,next_poll_at=NULL,lease_owner=NULL,lease_expires_at=NULL WHERE batch_id=$4 AND lease_owner=$5").bind(status.as_str()).bind(serialize_json(error)?).bind(completed_at.unix_timestamp()).bind(batch_id.to_string()).bind(worker_id).execute(&self.pool).await.map_err(to_query_error)?.rows_affected();
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
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let state = &update.state;
        let changed = sqlx::query("UPDATE batch_jobs SET status=$1,completed_count=$2,failed_count=$3,cost_usd_10000=$4,pricing_status=$5,provider_usage_json=$6,error_json=$7,submitted_at=COALESCE($8,submitted_at),completed_at=$9,updated_at=$10,next_poll_at=$11,lease_owner=NULL,lease_expires_at=NULL WHERE batch_id=$12 AND lease_owner=$13").bind(state.status.as_str()).bind(state.completed_count).bind(state.failed_count).bind(state.provider_cost_usd.map(Money4::as_scaled_i64)).bind(update.pricing_status.unwrap_or(if state.provider_cost_usd.is_some(){BatchPricingStatus::ProviderReported}else{BatchPricingStatus::Pending}).as_str()).bind(serialize_optional_json(state.provider_usage.as_ref())?).bind(serialize_optional_json(state.error.as_ref())?).bind(state.submitted_at.map(|t|t.unix_timestamp())).bind(state.completed_at.map(|t|t.unix_timestamp())).bind(OffsetDateTime::now_utc().unix_timestamp()).bind(update.next_poll_at.map(|t|t.unix_timestamp())).bind(batch_id.to_string()).bind(worker_id).execute(&mut *tx).await.map_err(to_query_error)?.rows_affected();
        if changed == 0 {
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` lease was lost"
            )));
        }
        const UPDATE_CHUNK_SIZE: usize = 3_000;
        for results in update.results.chunks(UPDATE_CHUNK_SIZE) {
            let updated_at = OffsetDateTime::now_utc().unix_timestamp();
            let rows = results
                .iter()
                .map(|result| {
                    Ok(SerializedItemUpdate {
                        status: if result.error.is_some() {
                            "failed"
                        } else {
                            "succeeded"
                        },
                        response_body: serialize_optional_json(result.response_body.as_ref())?,
                        error: serialize_optional_json(result.error.as_ref())?,
                        provider_request_id: result.provider_request_id.clone(),
                        provider_usage: serialize_optional_json(result.provider_usage.as_ref())?,
                        completed_at: result.completed_at.map(|time| time.unix_timestamp()),
                        updated_at,
                        batch_id: batch_id.to_string(),
                        custom_id: result.custom_id.clone(),
                        cost_usd_10000: result.cost_usd.map(Money4::as_scaled_i64),
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "UPDATE batch_items AS item SET status=updates.status,response_body_json=updates.response_body_json,error_json=updates.error_json,provider_request_id=updates.provider_request_id,provider_usage_json=updates.provider_usage_json,completed_at=updates.completed_at,updated_at=updates.updated_at,cost_usd_10000=updates.cost_usd_10000 FROM (",
            );
            builder.push_values(rows, |mut values, row| {
                values
                    .push_bind(row.status)
                    .push_bind(row.response_body)
                    .push_bind(row.error)
                    .push_bind(row.provider_request_id)
                    .push_bind(row.provider_usage)
                    .push_bind(row.completed_at)
                    .push_bind(row.updated_at)
                    .push_bind(row.batch_id)
                    .push_bind(row.custom_id)
                    .push_bind(row.cost_usd_10000);
            });
            builder.push(
                ") AS updates(status,response_body_json,error_json,provider_request_id,provider_usage_json,completed_at,updated_at,batch_id,custom_id,cost_usd_10000) WHERE item.batch_id=updates.batch_id AND item.custom_id=updates.custom_id",
            );
            builder
                .build()
                .execute(&mut *tx)
                .await
                .map_err(to_query_error)?;
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
        let changed = sqlx::query("UPDATE batch_jobs SET status=CASE WHEN status='submitting' THEN 'queued' ELSE status END,error_json=$1,next_poll_at=$2,updated_at=$3,lease_owner=NULL,lease_expires_at=NULL WHERE batch_id=$4 AND lease_owner=$5").bind(serialize_json(error)?).bind(next_poll_at.unix_timestamp()).bind(OffsetDateTime::now_utc().unix_timestamp()).bind(batch_id.to_string()).bind(worker_id).execute(&self.pool).await.map_err(to_query_error)?.rows_affected();
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
        let current = load_job(&self.pool, batch_id, scope).await?;
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
        let changed = sqlx::query("UPDATE batch_jobs SET status=$1,completed_at=$2,next_poll_at=$3,updated_at=$3,lease_owner=NULL,lease_expires_at=NULL WHERE batch_id=$4 AND status=$5").bind(status).bind(completed_at).bind(requested_at.unix_timestamp()).bind(batch_id.to_string()).bind(current.status.as_str()).execute(&self.pool).await.map_err(to_query_error)?.rows_affected();
        if changed == 0 {
            let latest = load_job(&self.pool, batch_id, scope).await?;
            if latest.status.is_terminal() {
                return Ok(latest);
            }
            return Err(StoreError::Conflict(format!(
                "batch `{batch_id}` changed while cancellation was requested"
            )));
        }
        load_job(&self.pool, batch_id, scope).await
    }

    async fn replace_batch_item_requests(
        &self,
        batch_id: Uuid,
        requests: &[gateway_core::ProviderBatchRequestItem],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        for request in requests {
            sqlx::query(
                "UPDATE batch_items SET request_body_json=$1,updated_at=$2 WHERE batch_id=$3 AND custom_id=$4",
            )
            .bind(serialize_json(&request.body)?)
            .bind(OffsetDateTime::now_utc().unix_timestamp())
            .bind(batch_id.to_string())
            .bind(&request.custom_id)
            .execute(&mut *tx)
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
            "SELECT {ITEM_COLUMNS} FROM batch_items WHERE batch_id=$1 ORDER BY custom_id ASC"
        );
        let rows = sqlx::query(&sql)
            .bind(batch_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_item).collect()
    }
}
