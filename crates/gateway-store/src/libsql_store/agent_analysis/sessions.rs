use super::*;

#[async_trait]
impl AgentSessionTraceRepository for LibsqlStore {
    async fn upsert_agent_session_source(
        &self,
        session: &AgentSessionSourceRecord,
    ) -> Result<AgentSessionSourceRecord, StoreError> {
        self.connection.execute(
            "INSERT INTO agent_session_sources (agent_session_source_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) ON CONFLICT DO NOTHING",
            libsql::params![session.agent_session_source_id.to_string(), session.ownership_scope_key.as_str(), session.api_key_id.to_string(), session.user_id.map(|value| value.to_string()), session.team_id.map(|value| value.to_string()), session.service_account_id.map(|value| value.to_string()), session.actor_user_id.map(|value| value.to_string()), session.normalized_session_id.as_str(), session.adapter_namespace.as_str(), session.adapter_version.as_str(), session.source_provenance.as_str(), session.harness_key.as_str(), session.harness_label.as_str(), session.first_seen_at.unix_timestamp(), session.last_seen_at.unix_timestamp(), session.created_at.unix_timestamp(), session.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
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
        self.connection.execute(
            "UPDATE agent_session_sources SET api_key_id = ?2, adapter_version = ?3, source_provenance = ?4, first_seen_at = MIN(first_seen_at, ?5), last_seen_at = MAX(last_seen_at, ?6), updated_at = MAX(updated_at, ?7) WHERE agent_session_source_id = ?1",
            libsql::params![session.agent_session_source_id.to_string(), session.api_key_id.to_string(), session.adapter_version.as_str(), session.source_provenance.as_str(), session.first_seen_at.unix_timestamp(), session.last_seen_at.unix_timestamp(), session.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
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
            "SELECT {SESSION_SOURCE_COLUMNS} FROM agent_session_sources WHERE agent_session_source_id = ?1"
        );
        let mut rows = self
            .connection
            .query(&sql, [agent_session_source_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
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
            "SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE ownership_scope_key = ?1 AND lifecycle = 'open' AND boundary_group_key = ?4 AND ((?2 IS NULL AND agent_session_source_id IS NULL AND harness_key = ?3) OR agent_session_source_id = ?2) ORDER BY started_at DESC LIMIT 1"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    ownership_scope_key,
                    agent_session_source_id.map(|value| value.to_string()),
                    harness_key,
                    boundary_group_key,
                ],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_session)
            .transpose()
    }

    async fn insert_agent_session_if_absent(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<bool, StoreError> {
        let written = self.connection.execute(
            "INSERT INTO agent_sessions (agent_session_id, agent_session_source_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23) ON CONFLICT DO NOTHING",
            libsql::params![session.agent_session_id.to_string(), session.agent_session_source_id.map(|value| value.to_string()), session.ownership_scope_key.as_str(), session.api_key_id.to_string(), session.user_id.map(|value| value.to_string()), session.team_id.map(|value| value.to_string()), session.service_account_id.map(|value| value.to_string()), session.actor_user_id.map(|value| value.to_string()), session.requested_model_key.as_str(), session.operation.as_str(), session.caller_class.as_str(), crate::shared::serialize_json(&session.request_tags)?, session.harness_key.as_str(), session.boundary_group_key.as_str(), session.boundary_policy_version.as_str(), enum_name(session.lifecycle)?, enum_name(session.boundary_confidence)?, session.started_at.unix_timestamp(), session.ended_at.map(OffsetDateTime::unix_timestamp), datetime_to_unix_millis(session.input_watermark_at)?, session.finalized_reason.as_deref(), session.created_at.unix_timestamp(), session.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        if written > 0 {
            return Ok(true);
        }
        let Some(existing) = self.query_session_by_id(session.agent_session_id).await? else {
            return Ok(false);
        };
        if agent_session_identity_matches(&existing, session) {
            self.connection
                .execute(
                    "UPDATE agent_sessions SET api_key_id = ?2 WHERE agent_session_id = ?1",
                    libsql::params![
                        session.agent_session_id.to_string(),
                        session.api_key_id.to_string()
                    ],
                )
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
    ) -> Result<bool, StoreError> {
        let updated = self.connection.execute(
            "UPDATE agent_sessions SET lifecycle = 'finalized', boundary_confidence = ?2, ended_at = ?3, input_watermark_at = MAX(input_watermark_at, ?4), finalized_reason = ?5, updated_at = MAX(updated_at, ?6), input_generation = input_generation + 1 WHERE agent_session_id = ?1 AND lifecycle = 'open' AND input_watermark_at = ?7",
            libsql::params![session.agent_session_id.to_string(), enum_name(session.boundary_confidence)?, session.ended_at.map(OffsetDateTime::unix_timestamp), datetime_to_unix_millis(session.input_watermark_at)?, session.finalized_reason.as_deref(), session.updated_at.unix_timestamp(), datetime_to_unix_millis(expected_input_watermark_at)?],
        ).await.map_err(to_query_error)?;
        Ok(updated > 0)
    }

    async fn append_agent_session_input(
        &self,
        input: AgentSessionInput<'_>,
    ) -> Result<AgentSessionInputAppendResult, StoreError> {
        self.append_session_input(input).await
    }
    async fn list_agent_session_request_attempts(
        &self,
        agent_session_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT a.request_attempt_id, a.request_log_id, a.request_id, a.attempt_number,
                       a.route_id, a.provider_key, a.upstream_model, a.status, a.status_code,
                       a.error_code, a.error_detail, a.error_detail_truncated, a.retryable,
                       a.terminal, a.produced_final_response, a.stream, a.started_at,
                       a.completed_at, a.latency_ms, a.metadata_json
                FROM request_log_attempts a
                JOIN agent_session_requests r ON r.request_log_id = a.request_log_id
                WHERE r.agent_session_id = ?1
                ORDER BY r.ordinal, a.attempt_number
                LIMIT ?2
                "#,
                libsql::params![agent_session_id.to_string(), i64::from(limit)],
            )
            .await
            .map_err(to_query_error)?;
        let mut attempts = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            attempts.push(super::request_logs::decode_request_attempt_row(&row)?);
        }
        Ok(attempts)
    }

    async fn load_agent_observation_sets(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                "SELECT observation_set_id FROM agent_inferred_observation_sets WHERE agent_session_id = ?1 ORDER BY source_watermark_at, created_at, observation_set_id LIMIT 1001",
                [agent_session_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            ids.push(parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?);
        }
        let mut sets = Vec::with_capacity(ids.len());
        for id in ids {
            let set = self.query_observation_set_by_id(id).await?.ok_or_else(|| {
                StoreError::Unexpected("agent observation set disappeared".to_string())
            })?;
            sets.push(set);
        }
        Ok(sets)
    }

    async fn link_request_log_to_agent_session(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError> {
        let updated = self.connection.execute(
            "UPDATE request_logs SET agent_session_source_id = ?2, agent_session_id = ?3, agent_analysis_source = ?4, agent_analysis_coverage_json = ?5 WHERE request_log_id = ?1",
            libsql::params![link.request_log_id.to_string(), link.agent_session_source_id.map(|value| value.to_string()), link.agent_session_id.to_string(), link.analysis_source.as_str(), crate::shared::serialize_json(&link.coverage)?],
        ).await.map_err(to_query_error)?;
        if updated == 0 {
            return Err(StoreError::NotFound("request log not found".to_string()));
        }
        Ok(())
    }

    async fn load_agent_session_trace(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Option<AgentSessionTraceRecord>, StoreError> {
        let session_sql =
            format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE agent_session_id = ?1");
        let mut session_rows = self
            .connection
            .query(&session_sql, [agent_session_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let Some(session_row) = session_rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };
        let session = decode_session(&session_row)?;
        let session_source = if let Some(session_source_id) = session.agent_session_source_id {
            let sql = format!(
                "SELECT {SESSION_SOURCE_COLUMNS} FROM agent_session_sources WHERE agent_session_source_id = ?1"
            );
            let mut rows = self
                .connection
                .query(&sql, [session_source_id.to_string()])
                .await
                .map_err(to_query_error)?;
            rows.next()
                .await
                .map_err(to_query_error)?
                .as_ref()
                .map(decode_session_source)
                .transpose()?
        } else {
            None
        };
        let request_sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM agent_session_requests WHERE agent_session_id = ?1 ORDER BY occurred_at, ordinal LIMIT 1001"
        );
        let mut request_rows = self
            .connection
            .query(&request_sql, [agent_session_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let mut requests = Vec::new();
        while let Some(row) = request_rows.next().await.map_err(to_query_error)? {
            requests.push(decode_request_link(&row)?);
        }
        let latest_observation_set = self.load_observation_set(agent_session_id).await?;
        let analysis_sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE agent_session_id = ?1 AND stale = 0 ORDER BY analyzed_at DESC LIMIT 1"
        );
        let mut analysis_rows = self
            .connection
            .query(&analysis_sql, [agent_session_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let latest_analysis = analysis_rows
            .next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_analysis)
            .transpose()?;
        Ok(Some(AgentSessionTraceRecord {
            session,
            session_source,
            requests,
            latest_observation_set,
            latest_analysis,
        }))
    }
}
