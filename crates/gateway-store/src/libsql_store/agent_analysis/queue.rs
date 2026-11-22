use super::*;

#[async_trait]
impl AgentAnalysisQueueRepository for LibsqlStore {
    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        let prepared = crate::agent_session_jobs::PreparedAnalysis::new(item)?;
        let transaction = self.begin_transaction().await?;
        let inserted = enqueue_on(&transaction, &prepared).await?;
        transaction.commit().await.map_err(to_query_error)?;
        Ok(inserted)
    }

    async fn claim_agent_analysis(
        &self,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<Option<AgentAnalysisQueueRecord>, StoreError> {
        let transaction = self.begin_transaction().await?;
        transaction
            .execute(
                "UPDATE agent_analysis_recompute_queue SET status = 'failed', lease_owner = NULL, lease_expires_at = NULL, last_error = 'lease attempts exhausted', completed_at = ?1, updated_at = ?1 WHERE status = 'leased' AND lease_expires_at <= ?1 AND attempts >= max_attempts AND request_revision = leased_revision",
                [now.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
        let mut rows = transaction.query("SELECT queue_item_id FROM agent_analysis_recompute_queue WHERE ((status = 'pending' AND available_at <= ?1) OR (status = 'leased' AND lease_expires_at <= ?1)) AND (attempts < max_attempts OR request_revision > leased_revision) ORDER BY available_at, created_at, queue_item_id LIMIT 1", [now.unix_timestamp()]).await.map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            transaction.commit().await.map_err(to_query_error)?;
            return Ok(None);
        };
        let queue_item_id: String = row.get(0).map_err(to_query_error)?;
        drop(rows);
        let updated = transaction.execute("UPDATE agent_analysis_recompute_queue SET status = 'leased', lease_owner = ?2, lease_expires_at = ?3, attempts = CASE WHEN request_revision > leased_revision THEN 1 ELSE attempts + 1 END, leased_generation = requested_generation, leased_revision = request_revision, lease_epoch = lease_epoch + 1, updated_at = ?1 WHERE queue_item_id = ?4 AND ((status = 'pending' AND available_at <= ?1) OR (status = 'leased' AND lease_expires_at <= ?1))", libsql::params![now.unix_timestamp(), lease_owner, lease_expires_at.unix_timestamp(), queue_item_id.as_str()]).await.map_err(to_query_error)?;
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
        lease: AgentAnalysisLease<'_>,
        updated_at: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        self.connection
            .execute(
                "UPDATE agent_analysis_recompute_queue SET lease_expires_at = ?4, updated_at = ?3 WHERE queue_item_id = ?1 AND status = 'leased' AND lease_owner = ?2 AND lease_expires_at > ?3 AND lease_epoch = ?5",
                libsql::params![
                    lease.queue_item_id.to_string(),
                    lease.owner,
                    updated_at.unix_timestamp(),
                    lease_expires_at.unix_timestamp(),
                    lease.epoch
                ],
            )
            .await
            .map(|updated| updated > 0)
            .map_err(to_query_error)
    }

    async fn complete_agent_analysis(
        &self,
        lease: AgentAnalysisLease<'_>,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let updated = self.connection.execute("UPDATE agent_analysis_recompute_queue SET status = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN 'pending' ELSE 'completed' END, attempts = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN 0 ELSE attempts END, lease_owner = NULL, lease_expires_at = NULL, completed_at = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN NULL ELSE ?3 END, available_at = ?3, updated_at = ?3, last_error = NULL WHERE queue_item_id = ?1 AND status = 'leased' AND lease_owner = ?2 AND lease_epoch = ?4",
            libsql::params![lease.queue_item_id.to_string(), lease.owner, completed_at.unix_timestamp(), lease.epoch]).await.map_err(to_query_error)?;
        if updated == 0 {
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
        let updated = self.connection.execute("UPDATE agent_analysis_recompute_queue SET status = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN 'pending' ELSE ?3 END, attempts = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN 0 ELSE attempts END, lease_owner = NULL, lease_expires_at = NULL, last_error = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN NULL ELSE ?4 END, available_at = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation THEN ?6 ELSE COALESCE(?5, available_at) END, updated_at = ?6, completed_at = CASE WHEN request_revision > leased_revision OR requested_generation > leased_generation OR ?5 IS NOT NULL THEN NULL ELSE ?6 END WHERE queue_item_id = ?1 AND status = 'leased' AND lease_owner = ?2 AND lease_epoch = ?7",
            libsql::params![lease.queue_item_id.to_string(), lease.owner, status.as_str(), error, retry_at.map(OffsetDateTime::unix_timestamp), updated_at.unix_timestamp(), lease.epoch]).await.map_err(to_query_error)?;
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
        let transaction = self.begin_transaction().await?;
        let observations = transaction.execute(
            "DELETE FROM agent_inferred_observation_sets WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < ?1)",
            [request_cutoff_millis],
        ).await.map_err(to_query_error)?;
        let requests = transaction.execute(
            "DELETE FROM agent_session_requests WHERE agent_session_id IN (SELECT agent_session_id FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < ?1)",
            [request_cutoff_millis],
        ).await.map_err(to_query_error)?;
        transaction.execute(
            "UPDATE agent_sessions SET request_count = 0, next_ordinal = 0, nested_fact_count = 0, input_generation = input_generation + 1 WHERE lifecycle = 'finalized' AND input_watermark_at < ?1",
            [request_cutoff_millis],
        ).await.map_err(to_query_error)?;
        let sessions = transaction.execute(
                "DELETE FROM agent_sessions WHERE lifecycle = 'finalized' AND input_watermark_at < ?1 AND NOT EXISTS (SELECT 1 FROM agent_session_analyses a WHERE a.agent_session_id = agent_sessions.agent_session_id) AND NOT EXISTS (SELECT 1 FROM agent_analysis_recompute_queue q WHERE q.agent_session_id = agent_sessions.agent_session_id)",
                [request_cutoff_millis],
            )
            .await
            .map_err(to_query_error)?;
        let session_sources = transaction.execute(
                "DELETE FROM agent_session_sources WHERE last_seen_at < ?1 AND NOT EXISTS (SELECT 1 FROM agent_sessions s WHERE s.agent_session_source_id = agent_session_sources.agent_session_source_id)",
                [request_cutoff.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
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

pub(super) async fn enqueue_on(
    transaction: &libsql::Transaction,
    prepared: &crate::agent_session_jobs::PreparedAnalysis<'_>,
) -> Result<bool, StoreError> {
    let item = prepared.record;
    let written = transaction.execute(concat!(
        "INSERT INTO agent_analysis_recompute_queue AS q ",
        "(queue_item_id, agent_session_id, reason, desired_versions_json, status, attempts, max_attempts, available_at, created_at, updated_at, request_key, requested_generation) ",
        "SELECT ?1, ?2, ?3, ?4, 'pending', 0, ?5, ?6, ?7, ?8, ?1, input_generation FROM agent_sessions WHERE agent_session_id = ?2 ",
        "ON CONFLICT (agent_session_id) DO UPDATE SET ",
        "reason = excluded.reason, desired_versions_json = excluded.desired_versions_json, request_key = excluded.request_key, ",
        "requested_generation = MAX(q.requested_generation, excluded.requested_generation), request_revision = q.request_revision + 1, ",
        "status = CASE WHEN q.status = 'leased' THEN 'leased' ELSE 'pending' END, ",
        "attempts = CASE WHEN q.status = 'leased' THEN q.attempts ELSE 0 END, max_attempts = excluded.max_attempts, ",
        "available_at = MIN(q.available_at, excluded.available_at), updated_at = MAX(q.updated_at, excluded.updated_at), completed_at = NULL, last_error = NULL ",
        "WHERE q.request_key <> excluded.request_key OR q.desired_versions_json <> excluded.desired_versions_json OR q.requested_generation < excluded.requested_generation"
    ), libsql::params![
        prepared.id.as_str(), prepared.session_id.as_str(), item.reason.as_str(), prepared.versions.as_str(),
        i64::from(prepared.max_attempts), item.available_at.unix_timestamp(), item.created_at.unix_timestamp(), item.updated_at.unix_timestamp(),
    ]).await.map_err(to_query_error)?;
    if written == 0 {
        let mut rows = transaction
            .query(
                "SELECT 1 FROM agent_sessions WHERE agent_session_id = ?1",
                [prepared.session_id.as_str()],
            )
            .await
            .map_err(to_query_error)?;
        if rows.next().await.map_err(to_query_error)?.is_none() {
            return Err(StoreError::NotFound("agent session not found".to_string()));
        }
    }
    Ok(written > 0)
}

pub(super) async fn wake_on(
    transaction: &libsql::Transaction,
    session_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    transaction.execute(concat!(
        "UPDATE agent_analysis_recompute_queue AS q SET requested_generation = s.input_generation, request_revision = q.request_revision + 1, ",
        "status = CASE WHEN q.status = 'leased' THEN 'leased' ELSE 'pending' END, ",
        "attempts = CASE WHEN q.status = 'leased' THEN q.attempts ELSE 0 END, ",
        "available_at = MIN(q.available_at, ?2), updated_at = MAX(q.updated_at, ?2), completed_at = NULL, last_error = NULL ",
        "FROM agent_sessions s WHERE q.agent_session_id = ?1 AND s.agent_session_id = q.agent_session_id AND q.requested_generation < s.input_generation"
    ), libsql::params![session_id, now]).await.map_err(to_query_error)?;
    Ok(())
}
