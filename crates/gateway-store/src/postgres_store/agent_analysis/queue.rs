use super::*;

#[async_trait]
impl AgentAnalysisQueueRepository for PostgresStore {
    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query("INSERT INTO agent_analysis_recompute_queue (queue_item_id, agent_session_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) ON CONFLICT(queue_item_id) DO NOTHING")
            .bind(item.queue_item_id.to_string()).bind(item.agent_session_id.to_string()).bind(&item.reason).bind(crate::shared::serialize_json(&item.desired_versions)?).bind(item.status.as_str()).bind(item.lease_owner.as_deref()).bind(item.lease_expires_at.map(OffsetDateTime::unix_timestamp)).bind(i32::try_from(item.attempts).map_err(|error| StoreError::Serialization(error.to_string()))?).bind(i32::try_from(item.max_attempts).map_err(|error| StoreError::Serialization(error.to_string()))?).bind(item.last_error.as_deref()).bind(item.available_at.unix_timestamp()).bind(item.created_at.unix_timestamp()).bind(item.updated_at.unix_timestamp()).bind(item.completed_at.map(OffsetDateTime::unix_timestamp)).execute(&self.pool).await.map_err(to_query_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn claim_agent_analysis(
        &self,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<Option<AgentAnalysisQueueRecord>, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query("UPDATE agent_analysis_recompute_queue SET status = 'failed', lease_owner = NULL, lease_expires_at = NULL, last_error = 'lease attempts exhausted', completed_at = $1, updated_at = $1 WHERE status = 'leased' AND lease_expires_at <= $1 AND attempts >= max_attempts")
            .bind(now.unix_timestamp())
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        let row = sqlx::query("SELECT queue_item_id FROM agent_analysis_recompute_queue WHERE ((status = 'pending' AND available_at <= $1) OR (status = 'leased' AND lease_expires_at <= $1)) AND attempts < max_attempts ORDER BY available_at, created_at LIMIT 1 FOR UPDATE SKIP LOCKED").bind(now.unix_timestamp()).fetch_optional(&mut *transaction).await.map_err(to_query_error)?;
        let Some(row) = row else {
            transaction.commit().await.map_err(to_query_error)?;
            return Ok(None);
        };
        let queue_item_id: String = row.try_get(0).map_err(to_query_error)?;
        sqlx::query("UPDATE agent_analysis_recompute_queue SET status = 'leased', lease_owner = $2, lease_expires_at = $3, attempts = attempts + 1, updated_at = $1 WHERE queue_item_id = $4").bind(now.unix_timestamp()).bind(lease_owner).bind(lease_expires_at.unix_timestamp()).bind(&queue_item_id).execute(&mut *transaction).await.map_err(to_query_error)?;
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
        queue_item_id: Uuid,
        lease_owner: &str,
        updated_at: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        sqlx::query(
            "UPDATE agent_analysis_recompute_queue SET lease_expires_at = $4, updated_at = $3 WHERE queue_item_id = $1 AND status = 'leased' AND lease_owner = $2 AND lease_expires_at > $3",
        )
        .bind(queue_item_id.to_string())
        .bind(lease_owner)
        .bind(updated_at.unix_timestamp())
        .bind(lease_expires_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(to_query_error)
    }

    async fn complete_agent_analysis(
        &self,
        queue_item_id: Uuid,
        lease_owner: &str,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE agent_analysis_recompute_queue SET status = 'completed', lease_owner = NULL, lease_expires_at = NULL, completed_at = $3, updated_at = $3 WHERE queue_item_id = $1 AND status = 'leased' AND lease_owner = $2").bind(queue_item_id.to_string()).bind(lease_owner).bind(completed_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(
                "leased agent analysis queue item not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn fail_agent_analysis(
        &self,
        queue_item_id: Uuid,
        lease_owner: &str,
        error: &str,
        retry_at: Option<OffsetDateTime>,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let status = if retry_at.is_some() {
            "pending"
        } else {
            "failed"
        };
        let result = sqlx::query("UPDATE agent_analysis_recompute_queue SET status = $3, lease_owner = NULL, lease_expires_at = NULL, last_error = $4, available_at = COALESCE($5, available_at), updated_at = $6, completed_at = CASE WHEN $5 IS NULL THEN $6 ELSE NULL END WHERE queue_item_id = $1 AND status = 'leased' AND lease_owner = $2").bind(queue_item_id.to_string()).bind(lease_owner).bind(status).bind(error).bind(retry_at.map(OffsetDateTime::unix_timestamp)).bind(updated_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
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
        let observations = sqlx::query("DELETE FROM agent_inferred_observation_sets WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < $1)").bind(request_cutoff_millis).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
        let requests = sqlx::query("DELETE FROM agent_session_requests WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < $1)").bind(request_cutoff_millis).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
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
