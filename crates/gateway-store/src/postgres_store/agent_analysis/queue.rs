use super::*;

#[async_trait]
impl AgentAnalysisQueueRepository for PostgresStore {
    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        let prepared = crate::agent_session_jobs::PreparedAnalysis::new(item)?;
        let mut connection = self.pool.acquire().await.map_err(to_query_error)?;
        enqueue_on(&mut connection, &prepared).await
    }

    async fn claim_agent_analysis(
        &self,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<Option<AgentAnalysisQueueRecord>, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query(EXHAUSTED_LEASES_SQL)
            .bind(now.unix_timestamp())
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        let row = sqlx::query(CLAIM_CANDIDATE_SQL)
            .bind(now.unix_timestamp())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(to_query_error)?;
            return Ok(None);
        };
        let queue_item_id: String = row.try_get(0).map_err(to_query_error)?;
        sqlx::query(CLAIM_LEASE_SQL)
            .bind(now.unix_timestamp())
            .bind(lease_owner)
            .bind(lease_expires_at.unix_timestamp())
            .bind(&queue_item_id)
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        let sql = format!(
            "SELECT {QUEUE_COLUMNS} FROM agent_analysis_recompute_queue WHERE queue_item_id = $1"
        );
        let claimed_row = sqlx::query(&sql)
            .bind(queue_item_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        let claimed = decode_queue(&claimed_row)?;
        transaction.commit().await.map_err(to_query_error)?;
        Ok(Some(claimed))
    }

    async fn renew_agent_analysis_lease(
        &self,
        lease: AgentAnalysisLease<'_>,
        updated_at: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        sqlx::query(RENEW_LEASE_SQL)
            .bind(lease.queue_item_id.to_string())
            .bind(lease.owner)
            .bind(updated_at.unix_timestamp())
            .bind(lease_expires_at.unix_timestamp())
            .bind(lease.epoch)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(to_query_error)
    }

    async fn complete_agent_analysis(
        &self,
        lease: AgentAnalysisLease<'_>,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(COMPLETE_LEASE_SQL)
            .bind(lease.queue_item_id.to_string())
            .bind(lease.owner)
            .bind(completed_at.unix_timestamp())
            .bind(lease.epoch)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(
                "leased agent analysis queue item not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn fail_agent_analysis(
        &self,
        lease: AgentAnalysisLease<'_>,
        error: &str,
        retry_at: Option<OffsetDateTime>,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let status = if retry_at.is_some() {
            AgentAnalysisQueueStatus::Pending
        } else {
            AgentAnalysisQueueStatus::Failed
        };
        let result = sqlx::query(FAIL_LEASE_SQL)
            .bind(lease.queue_item_id.to_string())
            .bind(lease.owner)
            .bind(status.as_str())
            .bind(error)
            .bind(retry_at.map(OffsetDateTime::unix_timestamp))
            .bind(updated_at.unix_timestamp())
            .bind(lease.epoch)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(
                "leased agent analysis queue item not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn purge_expired_agent_analysis(
        &self,
        expires_before: OffsetDateTime,
        queue_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let reports = sqlx::query("DELETE FROM agent_session_analyses WHERE expires_at < $1")
            .bind(expires_before.unix_timestamp())
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .rows_affected();
        let queue = sqlx::query("DELETE FROM agent_analysis_recompute_queue WHERE status IN ('completed', 'failed') AND updated_at < $1").bind(queue_cutoff.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
        transaction.commit().await.map_err(to_query_error)?;
        Ok(reports.saturating_add(queue))
    }

    async fn purge_agent_analysis_before(
        &self,
        request_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let request_cutoff_millis = datetime_to_unix_millis(request_cutoff)?;
        sqlx::query("SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < $1 ORDER BY agent_session_id FOR NO KEY UPDATE")
            .bind(request_cutoff_millis).fetch_all(&mut *transaction).await.map_err(to_query_error)?;
        let observations = sqlx::query("DELETE FROM agent_inferred_observation_sets WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < $1)").bind(request_cutoff_millis).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
        let requests = sqlx::query("DELETE FROM agent_session_requests WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < $1)").bind(request_cutoff_millis).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
        sqlx::query("UPDATE agent_sessions SET request_count = 0, next_ordinal = 0, nested_fact_count = 0, input_generation = input_generation + 1 WHERE lifecycle = 'finalized' AND input_watermark_at < $1")
            .bind(request_cutoff_millis).execute(&mut *transaction).await.map_err(to_query_error)?;
        let sessions = sqlx::query("DELETE FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < $1 AND NOT EXISTS (SELECT 1 FROM agent_session_analyses a WHERE a.agent_session_id = agent_sessions.agent_session_id) AND NOT EXISTS (SELECT 1 FROM agent_analysis_recompute_queue q WHERE q.agent_session_id = agent_sessions.agent_session_id)")
            .bind(request_cutoff_millis)
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .rows_affected();
        let session_sources = sqlx::query(
            "DELETE FROM agent_session_sources WHERE last_seen_at < $1 AND NOT EXISTS (SELECT 1 FROM agent_sessions s WHERE s.agent_session_source_id = agent_session_sources.agent_session_source_id)",
        )
        .bind(request_cutoff.unix_timestamp())
        .execute(&mut *transaction)
        .await
        .map_err(to_query_error)?
        .rows_affected();
        transaction.commit().await.map_err(to_query_error)?;
        Ok(observations
            .saturating_add(requests)
            .saturating_add(sessions)
            .saturating_add(session_sources))
    }

    async fn delete_agent_analysis_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let reports =
            sqlx::query("DELETE FROM agent_session_analyses WHERE ownership_scope_key = $1")
                .bind(ownership_scope_key)
                .execute(&mut *transaction)
                .await
                .map_err(to_query_error)?
                .rows_affected();
        let sessions = sqlx::query("DELETE FROM agent_sessions WHERE ownership_scope_key = $1")
            .bind(ownership_scope_key)
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .rows_affected();
        let session_sources =
            sqlx::query("DELETE FROM agent_session_sources WHERE ownership_scope_key = $1")
                .bind(ownership_scope_key)
                .execute(&mut *transaction)
                .await
                .map_err(to_query_error)?
                .rows_affected();
        transaction.commit().await.map_err(to_query_error)?;
        Ok(reports
            .saturating_add(sessions)
            .saturating_add(session_sources))
    }
}

pub(super) async fn enqueue_on(
    connection: &mut sqlx::PgConnection,
    prepared: &crate::agent_session_jobs::PreparedAnalysis<'_>,
) -> Result<bool, StoreError> {
    let item = prepared.record;
    let result = sqlx::query(concat!(
        "INSERT INTO agent_analysis_recompute_queue AS q ",
        "(queue_item_id, agent_session_id, reason, desired_versions_json, status, attempts, max_attempts, available_at, created_at, updated_at, request_key, requested_generation) ",
        "SELECT $1, $2, $3, $4, 'pending', 0, $5, $6, $7, $8, $1, input_generation FROM agent_sessions WHERE agent_session_id = $2 ",
        "ON CONFLICT (agent_session_id) DO UPDATE SET ",
        "reason = EXCLUDED.reason, desired_versions_json = EXCLUDED.desired_versions_json, request_key = EXCLUDED.request_key, ",
        "requested_generation = GREATEST(q.requested_generation, EXCLUDED.requested_generation), request_revision = q.request_revision + 1, ",
        "status = CASE WHEN q.status = 'leased' THEN 'leased' ELSE 'pending' END, ",
        "attempts = CASE WHEN q.status = 'leased' THEN q.attempts ELSE 0 END, max_attempts = EXCLUDED.max_attempts, ",
        "available_at = LEAST(q.available_at, EXCLUDED.available_at), updated_at = GREATEST(q.updated_at, EXCLUDED.updated_at), completed_at = NULL, last_error = NULL ",
        "WHERE q.request_key <> EXCLUDED.request_key OR q.desired_versions_json <> EXCLUDED.desired_versions_json OR q.requested_generation < EXCLUDED.requested_generation"
    ))
        .bind(&prepared.id).bind(&prepared.session_id).bind(&item.reason).bind(&prepared.versions)
        .bind(prepared.max_attempts).bind(item.available_at.unix_timestamp())
        .bind(item.created_at.unix_timestamp()).bind(item.updated_at.unix_timestamp())
        .execute(&mut *connection).await.map_err(to_query_error)?;
    if result.rows_affected() == 0
        && sqlx::query("SELECT 1 FROM agent_sessions WHERE agent_session_id = $1")
            .bind(&prepared.session_id)
            .fetch_optional(connection)
            .await
            .map_err(to_query_error)?
            .is_none()
    {
        return Err(StoreError::NotFound("agent session not found".to_string()));
    }
    Ok(result.rows_affected() > 0)
}

pub(super) async fn wake_on(
    connection: &mut sqlx::PgConnection,
    session_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    sqlx::query(concat!(
        "UPDATE agent_analysis_recompute_queue q SET requested_generation = s.input_generation, request_revision = q.request_revision + 1, ",
        "status = CASE WHEN q.status = 'leased' THEN 'leased' ELSE 'pending' END, ",
        "attempts = CASE WHEN q.status = 'leased' THEN q.attempts ELSE 0 END, ",
        "available_at = LEAST(q.available_at, $2), updated_at = GREATEST(q.updated_at, $2), completed_at = NULL, last_error = NULL ",
        "FROM agent_sessions s WHERE q.agent_session_id = $1 AND s.agent_session_id = q.agent_session_id AND q.requested_generation < s.input_generation"
    )).bind(session_id).bind(now).execute(connection).await.map_err(to_query_error)?;
    Ok(())
}

const EXHAUSTED_LEASES_SQL: &str = r"
UPDATE agent_analysis_recompute_queue
SET status = 'failed', lease_owner = NULL, lease_expires_at = NULL,
    last_error = 'lease attempts exhausted', completed_at = $1, updated_at = $1
WHERE status = 'leased' AND lease_expires_at <= $1 AND attempts >= max_attempts
  AND request_revision = leased_revision
";

const CLAIM_CANDIDATE_SQL: &str = r"
SELECT queue_item_id FROM agent_analysis_recompute_queue
WHERE ((status = 'pending' AND available_at <= $1) OR (status = 'leased' AND lease_expires_at <= $1))
  AND (attempts < max_attempts OR request_revision > leased_revision)
ORDER BY available_at, created_at, queue_item_id LIMIT 1 FOR UPDATE SKIP LOCKED
";

const CLAIM_LEASE_SQL: &str = r"
UPDATE agent_analysis_recompute_queue
SET status = 'leased', lease_owner = $2, lease_expires_at = $3,
    attempts = CASE WHEN request_revision > leased_revision THEN 1 ELSE attempts + 1 END,
    leased_generation = requested_generation, leased_revision = request_revision,
    lease_epoch = lease_epoch + 1, updated_at = $1
WHERE queue_item_id = $4
";

const RENEW_LEASE_SQL: &str = r"
UPDATE agent_analysis_recompute_queue SET lease_expires_at = $4, updated_at = $3
WHERE queue_item_id = $1 AND status = 'leased' AND lease_owner = $2
  AND lease_expires_at > $3 AND lease_epoch = $5
";

const COMPLETE_LEASE_SQL: &str = r"
UPDATE agent_analysis_recompute_queue
SET status = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN 'pending' ELSE 'completed' END,
    attempts = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN 0 ELSE attempts END,
    lease_owner = NULL, lease_expires_at = NULL,
    completed_at = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN NULL ELSE $3 END,
    available_at = $3, updated_at = $3, last_error = NULL
WHERE queue_item_id = $1 AND status = 'leased' AND lease_owner = $2 AND lease_epoch = $4
";

const FAIL_LEASE_SQL: &str = r"
UPDATE agent_analysis_recompute_queue
SET status = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN 'pending' ELSE $3 END,
    attempts = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN 0 ELSE attempts END,
    lease_owner = NULL, lease_expires_at = NULL,
    last_error = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN NULL ELSE $4 END,
    available_at = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        THEN $6 ELSE COALESCE($5, available_at) END,
    updated_at = $6,
    completed_at = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation
        OR $5 IS NOT NULL THEN NULL ELSE $6 END
WHERE queue_item_id = $1 AND status = 'leased' AND lease_owner = $2 AND lease_epoch = $7
";
