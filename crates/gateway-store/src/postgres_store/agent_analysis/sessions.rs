use super::*;

#[async_trait]
impl AgentSessionTraceRepository for PostgresStore {
    async fn upsert_agent_session_source(
        &self,
        session: &AgentSessionSourceRecord,
    ) -> Result<AgentSessionSourceRecord, StoreError> {
        sqlx::query("INSERT INTO agent_session_sources (agent_session_source_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) ON CONFLICT DO NOTHING")
            .bind(session.agent_session_source_id.to_string()).bind(&session.ownership_scope_key).bind(session.api_key_id.to_string()).bind(session.user_id.map(|value| value.to_string())).bind(session.team_id.map(|value| value.to_string())).bind(session.service_account_id.map(|value| value.to_string())).bind(session.actor_user_id.map(|value| value.to_string())).bind(&session.normalized_session_id).bind(&session.adapter_namespace).bind(&session.adapter_version).bind(&session.source_provenance).bind(&session.harness_key).bind(&session.harness_label).bind(session.first_seen_at.unix_timestamp()).bind(session.last_seen_at.unix_timestamp()).bind(session.created_at.unix_timestamp()).bind(session.updated_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
        let existing = self
            .query_session_source_by_natural_key(
                &session.ownership_scope_key,
                &session.adapter_namespace,
                &session.normalized_session_id,
            )
            .await?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "agent session `{}` conflicts with the existing record",
                    session.agent_session_source_id
                ))
            })?;
        if !agent_session_source_identity_matches(&existing, session) {
            return Err(StoreError::Conflict(format!(
                "agent session `{}` conflicts with the existing record",
                session.agent_session_source_id
            )));
        }
        sqlx::query("UPDATE agent_session_sources SET api_key_id = $2, adapter_version = $3, source_provenance = $4, first_seen_at = LEAST(first_seen_at, $5), last_seen_at = GREATEST(last_seen_at, $6), updated_at = GREATEST(updated_at, $7) WHERE agent_session_source_id = $1")
            .bind(session.agent_session_source_id.to_string())
            .bind(session.api_key_id.to_string())
            .bind(&session.adapter_version)
            .bind(&session.source_provenance)
            .bind(session.first_seen_at.unix_timestamp())
            .bind(session.last_seen_at.unix_timestamp())
            .bind(session.updated_at.unix_timestamp())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        self.query_session_source_by_natural_key(
            &session.ownership_scope_key,
            &session.adapter_namespace,
            &session.normalized_session_id,
        )
        .await?
        .ok_or_else(|| StoreError::Unexpected("upserted agent session was not found".to_string()))
    }

    async fn load_agent_session_source(
        &self,
        agent_session_source_id: Uuid,
    ) -> Result<Option<AgentSessionSourceRecord>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_SOURCE_COLUMNS} FROM agent_session_sources WHERE agent_session_source_id = $1"
        );
        sqlx::query(&sql)
            .bind(agent_session_source_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_session_source)
            .transpose()
    }

    async fn get_open_agent_session(
        &self,
        ownership_scope_key: &str,
        agent_session_source_id: Option<Uuid>,
        harness_key: &str,
        boundary_group_key: &str,
    ) -> Result<Option<AgentSessionRecord>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE ownership_scope_key = $1 AND lifecycle = 'open' AND boundary_group_key = $4 AND (($2 IS NULL AND agent_session_source_id IS NULL AND harness_key = $3) OR agent_session_source_id = $2) ORDER BY started_at DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(ownership_scope_key)
            .bind(agent_session_source_id.map(|value| value.to_string()))
            .bind(harness_key)
            .bind(boundary_group_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_session).transpose()
    }

    async fn insert_agent_session_if_absent(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "INSERT INTO agent_sessions (agent_session_id, agent_session_source_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23) ON CONFLICT DO NOTHING",
        )
        .bind(session.agent_session_id.to_string())
        .bind(session.agent_session_source_id.map(|value| value.to_string()))
        .bind(&session.ownership_scope_key)
        .bind(session.api_key_id.to_string())
        .bind(session.user_id.map(|value| value.to_string()))
        .bind(session.team_id.map(|value| value.to_string()))
        .bind(session.service_account_id.map(|value| value.to_string()))
        .bind(session.actor_user_id.map(|value| value.to_string()))
        .bind(&session.requested_model_key)
        .bind(&session.operation)
        .bind(&session.caller_class)
        .bind(crate::shared::serialize_json(&session.request_tags)?)
        .bind(&session.harness_key)
        .bind(&session.boundary_group_key)
        .bind(&session.boundary_policy_version)
        .bind(enum_name(session.lifecycle)?)
        .bind(enum_name(session.boundary_confidence)?)
        .bind(session.started_at.unix_timestamp())
        .bind(session.ended_at.map(OffsetDateTime::unix_timestamp))
        .bind(datetime_to_unix_millis(session.input_watermark_at)?)
        .bind(session.finalized_reason.as_deref())
        .bind(session.created_at.unix_timestamp())
        .bind(session.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            return Ok(true);
        }
        let Some(existing) = self.query_session_by_id(session.agent_session_id).await? else {
            return Ok(false);
        };
        if agent_session_identity_matches(&existing, session) {
            sqlx::query("UPDATE agent_sessions SET api_key_id = $2 WHERE agent_session_id = $1")
                .bind(session.agent_session_id.to_string())
                .bind(session.api_key_id.to_string())
                .execute(&self.pool)
                .await
                .map_err(to_query_error)?;
            Ok(false)
        } else {
            Err(StoreError::Conflict(format!(
                "agent session `{}` conflicts with the existing record",
                session.agent_session_id
            )))
        }
    }

    async fn finalize_agent_session_if_unchanged(
        &self,
        session: &AgentSessionRecord,
        expected_input_watermark_at: OffsetDateTime,
        analysis: Option<&AgentAnalysisQueueRecord>,
    ) -> Result<bool, StoreError> {
        let session_id = session.agent_session_id.to_string();
        let confidence = enum_name(session.boundary_confidence)?;
        let watermark = datetime_to_unix_millis(session.input_watermark_at)?;
        let expected = datetime_to_unix_millis(expected_input_watermark_at)?;
        let analysis = analysis
            .map(|item| {
                crate::agent_session_jobs::PreparedAnalysis::for_session(
                    item,
                    session.agent_session_id,
                )
            })
            .transpose()?;
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let result = sqlx::query(FINALIZE_SESSION_SQL)
            .bind(&session_id)
            .bind(confidence)
            .bind(session.ended_at.map(OffsetDateTime::unix_timestamp))
            .bind(watermark)
            .bind(session.finalized_reason.as_deref())
            .bind(session.updated_at.unix_timestamp())
            .bind(expected)
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            if let Some(analysis) = analysis {
                super::queue::enqueue_on(&mut transaction, &analysis).await?;
            } else {
                super::queue::wake_on(
                    &mut transaction,
                    &session_id,
                    session.updated_at.unix_timestamp(),
                )
                .await?;
            }
        }
        transaction.commit().await.map_err(to_query_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn append_agent_session_input(
        &self,
        input: AgentSessionInput<'_>,
    ) -> Result<AgentSessionInputAppendResult, StoreError> {
        self.append_session_input(input).await
    }
    async fn list_idle_agent_sessions(
        &self,
        cutoff: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<AgentSessionRecord>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE lifecycle = 'open' AND started_at < $1 AND input_watermark_at < $2 ORDER BY input_watermark_at, agent_session_id LIMIT $3"
        );
        let rows = sqlx::query(&sql)
            .bind(cutoff.unix_timestamp())
            .bind(datetime_to_unix_millis(cutoff)?)
            .bind(i64::from(limit.clamp(1, MAX_AGENT_SESSION_PAGE_SIZE)))
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_session).collect()
    }

    async fn list_agent_session_request_attempts(
        &self,
        agent_session_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT a.request_attempt_id, a.request_log_id, a.request_id, a.attempt_number,
                   a.route_id, a.provider_key, a.upstream_model, a.status, a.status_code,
                   a.error_code, a.error_detail, a.error_detail_truncated, a.retryable,
                   a.terminal, a.produced_final_response, a.stream, a.started_at,
                   a.completed_at, a.latency_ms, a.metadata_json
            FROM request_log_attempts a
            JOIN agent_session_requests r ON r.request_log_id = a.request_log_id
            WHERE r.agent_session_id = $1
            ORDER BY r.ordinal, a.attempt_number
            LIMIT $2
            "#,
        )
        .bind(agent_session_id.to_string())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter()
            .map(super::request_logs::decode_request_attempt_row)
            .collect()
    }

    async fn load_agent_observation_sets(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        self.load_session_observation_sets(agent_session_id).await
    }

    async fn link_request_log_to_agent_session(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE request_logs SET agent_session_source_id = $2, agent_session_id = $3, agent_analysis_source = $4, agent_analysis_coverage_json = $5 WHERE request_log_id = $1",
        )
        .bind(link.request_log_id.to_string())
        .bind(link.agent_session_source_id.map(|value| value.to_string()))
        .bind(link.agent_session_id.to_string())
        .bind(&link.analysis_source)
        .bind(crate::shared::serialize_json(&link.coverage)?)
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound("request log not found".to_string()));
        }
        Ok(())
    }

    async fn load_agent_session_trace(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Option<AgentSessionTraceRecord>, StoreError> {
        let session_sql = format!(
            "SELECT {SESSION_COLUMNS}, input_generation FROM agent_sessions WHERE agent_session_id = $1"
        );
        let session_row = sqlx::query(&session_sql)
            .bind(agent_session_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        let Some(session_row) = session_row else {
            return Ok(None);
        };
        let session = decode_session(&session_row)?;
        let input_generation = session_row.try_get(23).map_err(to_query_error)?;
        let session_source = if let Some(session_source_id) = session.agent_session_source_id {
            let sql = format!(
                "SELECT {SESSION_SOURCE_COLUMNS} FROM agent_session_sources WHERE agent_session_source_id = $1"
            );
            sqlx::query(&sql)
                .bind(session_source_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(to_query_error)?
                .as_ref()
                .map(decode_session_source)
                .transpose()?
        } else {
            None
        };
        let request_sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM agent_session_requests WHERE agent_session_id = $1 ORDER BY occurred_at, ordinal LIMIT 1001"
        );
        let request_rows = sqlx::query(&request_sql)
            .bind(agent_session_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let requests = request_rows
            .iter()
            .map(decode_request_link)
            .collect::<Result<Vec<_>, _>>()?;
        let latest_observation_set = self.load_observation_set(agent_session_id).await?;
        let analysis_sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE agent_session_id = $1 AND stale = 0 AND input_generation = (SELECT input_generation FROM agent_sessions WHERE agent_session_id = $1) ORDER BY analyzed_at DESC, analysis_id DESC LIMIT 1"
        );
        let latest_analysis = sqlx::query(&analysis_sql)
            .bind(agent_session_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_analysis)
            .transpose()?;
        Ok(Some(AgentSessionTraceRecord {
            session,
            input_generation,
            session_source,
            requests,
            latest_observation_set,
            latest_analysis,
        }))
    }
}

const FINALIZE_SESSION_SQL: &str = r"
UPDATE agent_sessions
SET lifecycle = 'finalized', boundary_confidence = $2, ended_at = $3,
    input_watermark_at = GREATEST(input_watermark_at, $4), finalized_reason = $5,
    updated_at = GREATEST(updated_at, $6), input_generation = input_generation + 1
WHERE agent_session_id = $1 AND lifecycle = 'open' AND input_watermark_at = $7
";
