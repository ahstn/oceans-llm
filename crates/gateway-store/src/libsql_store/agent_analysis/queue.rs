use super::*;

#[async_trait]
impl AgentAnalysisQueueRepository for LibsqlStore {
    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        let written = self.connection.execute(
            "INSERT INTO agent_analysis_recompute_queue (queue_item_id, agent_session_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) ON CONFLICT(queue_item_id) DO NOTHING",
            libsql::params![item.queue_item_id.to_string(), item.agent_session_id.to_string(), item.reason.as_str(), crate::shared::serialize_json(&item.desired_versions)?, item.status.as_str(), item.lease_owner.as_deref(), item.lease_expires_at.map(OffsetDateTime::unix_timestamp), i64::from(item.attempts), i64::from(item.max_attempts), item.last_error.as_deref(), item.available_at.unix_timestamp(), item.created_at.unix_timestamp(), item.updated_at.unix_timestamp(), item.completed_at.map(OffsetDateTime::unix_timestamp)],
        ).await.map_err(to_query_error)?;
        Ok(written > 0)
    }

    async fn claim_agent_analysis(
        &self,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<Option<AgentAnalysisQueueRecord>, StoreError> {
        let transaction = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        transaction
            .execute(
                "UPDATE agent_analysis_recompute_queue SET status = 'failed', lease_owner = NULL, lease_expires_at = NULL, last_error = 'lease attempts exhausted', completed_at = ?1, updated_at = ?1 WHERE status = 'leased' AND lease_expires_at <= ?1 AND attempts >= max_attempts",
                [now.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
        let mut rows = transaction.query("SELECT queue_item_id FROM agent_analysis_recompute_queue WHERE ((status = 'pending' AND available_at <= ?1) OR (status = 'leased' AND lease_expires_at <= ?1)) AND attempts < max_attempts ORDER BY available_at, created_at LIMIT 1", [now.unix_timestamp()]).await.map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            transaction.commit().await.map_err(to_query_error)?;
            return Ok(None);
        };
        let queue_item_id: String = row.get(0).map_err(to_query_error)?;
        drop(rows);
        let updated = transaction.execute("UPDATE agent_analysis_recompute_queue SET status = 'leased', lease_owner = ?2, lease_expires_at = ?3, attempts = attempts + 1, updated_at = ?1 WHERE queue_item_id = ?4 AND ((status = 'pending' AND available_at <= ?1) OR (status = 'leased' AND lease_expires_at <= ?1))", libsql::params![now.unix_timestamp(), lease_owner, lease_expires_at.unix_timestamp(), queue_item_id.as_str()]).await.map_err(to_query_error)?;
        if updated == 0 {
            transaction.commit().await.map_err(to_query_error)?;
            return Ok(None);
        }
        let sql = format!(
            "SELECT {QUEUE_COLUMNS} FROM agent_analysis_recompute_queue WHERE queue_item_id = ?1"
        );
        let mut claimed_rows = transaction
            .query(&sql, [queue_item_id])
            .await
            .map_err(to_query_error)?;
        let claimed = claimed_rows
            .next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_queue)
            .transpose()?;
        drop(claimed_rows);
        transaction.commit().await.map_err(to_query_error)?;
        Ok(claimed)
    }

    async fn renew_agent_analysis_lease(
        &self,
        queue_item_id: Uuid,
        lease_owner: &str,
        updated_at: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        self.connection
            .execute(
                "UPDATE agent_analysis_recompute_queue SET lease_expires_at = ?4, updated_at = ?3 WHERE queue_item_id = ?1 AND status = 'leased' AND lease_owner = ?2 AND lease_expires_at > ?3",
                libsql::params![
                    queue_item_id.to_string(),
                    lease_owner,
                    updated_at.unix_timestamp(),
                    lease_expires_at.unix_timestamp()
                ],
            )
            .await
            .map(|updated| updated > 0)
            .map_err(to_query_error)
    }

    async fn complete_agent_analysis(
        &self,
        queue_item_id: Uuid,
        lease_owner: &str,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let updated = self.connection.execute("UPDATE agent_analysis_recompute_queue SET status = 'completed', lease_owner = NULL, lease_expires_at = NULL, completed_at = ?3, updated_at = ?3 WHERE queue_item_id = ?1 AND status = 'leased' AND lease_owner = ?2", libsql::params![queue_item_id.to_string(), lease_owner, completed_at.unix_timestamp()]).await.map_err(to_query_error)?;
        if updated == 0 {
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
        let updated = self.connection.execute("UPDATE agent_analysis_recompute_queue SET status = ?3, lease_owner = NULL, lease_expires_at = NULL, last_error = ?4, available_at = COALESCE(?5, available_at), updated_at = ?6, completed_at = CASE WHEN ?5 IS NULL THEN ?6 ELSE NULL END WHERE queue_item_id = ?1 AND status = 'leased' AND lease_owner = ?2", libsql::params![queue_item_id.to_string(), lease_owner, status, error, retry_at.map(OffsetDateTime::unix_timestamp), updated_at.unix_timestamp()]).await.map_err(to_query_error)?;
        if updated == 0 {
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
        let reports = self
            .connection
            .execute(
                "DELETE FROM agent_session_analyses WHERE expires_at < ?1",
                [expires_before.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
        let queue = self.connection.execute("DELETE FROM agent_analysis_recompute_queue WHERE status IN ('completed', 'failed') AND updated_at < ?1", [queue_cutoff.unix_timestamp()]).await.map_err(to_query_error)?;
        Ok(reports.saturating_add(queue))
    }

    async fn purge_agent_analysis_before(
        &self,
        request_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        let request_cutoff_millis = datetime_to_unix_millis(request_cutoff)?;
        let observations = self.connection.execute(
            "DELETE FROM agent_inferred_observation_sets WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < ?1)",
            [request_cutoff_millis],
        ).await.map_err(to_query_error)?;
        let requests = self.connection.execute(
            "DELETE FROM agent_session_requests WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < ?1)",
            [request_cutoff_millis],
        ).await.map_err(to_query_error)?;
        let sessions = self
            .connection
            .execute(
                "DELETE FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < ?1 AND NOT EXISTS (SELECT 1 FROM agent_session_analyses a WHERE a.agent_session_id = agent_sessions.agent_session_id) AND NOT EXISTS (SELECT 1 FROM agent_analysis_recompute_queue q WHERE q.agent_session_id = agent_sessions.agent_session_id)",
                [request_cutoff_millis],
            )
            .await
            .map_err(to_query_error)?;
        let session_sources = self
            .connection
            .execute(
                "DELETE FROM agent_session_sources WHERE last_seen_at < ?1 AND NOT EXISTS (SELECT 1 FROM agent_sessions s WHERE s.agent_session_source_id = agent_session_sources.agent_session_source_id)",
                [request_cutoff.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
        Ok(observations
            .saturating_add(requests)
            .saturating_add(sessions)
            .saturating_add(session_sources))
    }

    async fn delete_agent_analysis_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let reports = self
            .connection
            .execute(
                "DELETE FROM agent_session_analyses WHERE ownership_scope_key = ?1",
                [ownership_scope_key],
            )
            .await
            .map_err(to_query_error)?;
        let sessions = self
            .connection
            .execute(
                "DELETE FROM agent_sessions WHERE ownership_scope_key = ?1",
                [ownership_scope_key],
            )
            .await
            .map_err(to_query_error)?;
        let session_sources = self
            .connection
            .execute(
                "DELETE FROM agent_session_sources WHERE ownership_scope_key = ?1",
                [ownership_scope_key],
            )
            .await
            .map_err(to_query_error)?;
        Ok(reports
            .saturating_add(sessions)
            .saturating_add(session_sources))
    }
}
