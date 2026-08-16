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

    async fn update_agent_session_window(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE agent_sessions SET lifecycle = $2, boundary_confidence = $3, ended_at = $4, input_watermark_at = GREATEST(input_watermark_at, $5), finalized_reason = $6, updated_at = GREATEST(updated_at, $7) WHERE agent_session_id = $1")
            .bind(session.agent_session_id.to_string()).bind(enum_name(session.lifecycle)?).bind(enum_name(session.boundary_confidence)?).bind(session.ended_at.map(OffsetDateTime::unix_timestamp)).bind(datetime_to_unix_millis(session.input_watermark_at)?).bind(session.finalized_reason.as_deref()).bind(session.updated_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(
                "agent session window not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn finalize_agent_session_if_unchanged(
        &self,
        session: &AgentSessionRecord,
        expected_input_watermark_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query("UPDATE agent_sessions SET lifecycle = $2, boundary_confidence = $3, ended_at = $4, input_watermark_at = $5, finalized_reason = $6, updated_at = $7 WHERE agent_session_id = $1 AND lifecycle = 'open' AND input_watermark_at = $8")
            .bind(session.agent_session_id.to_string()).bind(enum_name(session.lifecycle)?).bind(enum_name(session.boundary_confidence)?).bind(session.ended_at.map(OffsetDateTime::unix_timestamp)).bind(datetime_to_unix_millis(session.input_watermark_at)?).bind(session.finalized_reason.as_deref()).bind(session.updated_at.unix_timestamp()).bind(datetime_to_unix_millis(expected_input_watermark_at)?).execute(&self.pool).await.map_err(to_query_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn append_agent_session_request(
        &self,
        link: &AgentSessionRequestLinkRecord,
    ) -> Result<bool, StoreError> {
        let activity_at = link.completed_at.unwrap_or(link.occurred_at);
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let lifecycle = sqlx::query_scalar::<_, String>(
            "SELECT lifecycle FROM agent_sessions WHERE agent_session_id = $1 FOR UPDATE",
        )
        .bind(link.agent_session_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(to_query_error)?;
        if lifecycle != "open" {
            return Err(StoreError::Conflict(format!(
                "agent session `{}` is already finalized",
                link.agent_session_id
            )));
        }
        let (request_count, request_exists) = sqlx::query_as::<_, (i64, bool)>(
            "SELECT COUNT(*), EXISTS(SELECT 1 FROM agent_session_requests WHERE agent_session_id = $1 AND request_id = $2) FROM agent_session_requests WHERE agent_session_id = $1",
        )
        .bind(link.agent_session_id.to_string())
        .bind(&link.request_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(to_query_error)?;
        if !request_exists
            && request_count
                >= i64::try_from(gateway_core::MAX_AGENT_SESSION_REQUESTS)
                    .expect("agent session request limit fits in i64")
        {
            return Err(StoreError::Conflict(format!(
                "agent session `{}` reached the request limit",
                link.agent_session_id
            )));
        }
        let result = sqlx::query("INSERT INTO agent_session_requests (agent_session_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success) VALUES ($1, $2, $3, $4, (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM agent_session_requests WHERE agent_session_id = $1), $5, $6, $7, $8, $9, $10, $11, $12) ON CONFLICT(agent_session_id, request_id) DO NOTHING")
            .bind(link.agent_session_id.to_string()).bind(&link.request_id).bind(link.request_log_id.map(|value| value.to_string())).bind(link.usage_event_id.map(|value| value.to_string())).bind(link.execution_id.as_deref()).bind(link.parent_execution_id.as_deref()).bind(link.normalized_session_id.as_deref()).bind(enum_name(link.correlation_confidence)?).bind(crate::shared::serialize_json(&link.limitation_codes)?).bind(datetime_to_unix_millis(link.occurred_at)?).bind(link.completed_at.map(datetime_to_unix_millis).transpose()?).bind(link.terminal_success).execute(&mut *transaction).await.map_err(to_query_error)?;
        let inserted = result.rows_affected() > 0;
        if inserted {
            sqlx::query("UPDATE agent_sessions SET started_at = LEAST(started_at, $2), input_watermark_at = GREATEST(input_watermark_at, $3), updated_at = GREATEST(updated_at, $4) WHERE agent_session_id = $1").bind(link.agent_session_id.to_string()).bind(link.occurred_at.unix_timestamp()).bind(datetime_to_unix_millis(activity_at)?).bind(activity_at.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?;
        } else {
            let sql = format!(
                "SELECT {REQUEST_COLUMNS} FROM agent_session_requests WHERE agent_session_id = $1 AND request_id = $2"
            );
            let existing = sqlx::query(&sql)
                .bind(link.agent_session_id.to_string())
                .bind(&link.request_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(to_query_error)?
                .as_ref()
                .map(decode_request_link)
                .transpose()?
                .ok_or_else(|| {
                    StoreError::Query("agent session request conflict row disappeared".to_string())
                })?;
            if !agent_session_request_matches(&existing, link) {
                return Err(StoreError::Conflict(format!(
                    "agent session request `{}` conflicts with the existing record",
                    link.request_id
                )));
            }
        }
        transaction.commit().await.map_err(to_query_error)?;
        Ok(inserted)
    }

    async fn count_agent_session_requests(
        &self,
        agent_session_id: Uuid,
    ) -> Result<u64, StoreError> {
        let row =
            sqlx::query("SELECT COUNT(*) FROM agent_session_requests WHERE agent_session_id = $1")
                .bind(agent_session_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(to_query_error)?;
        let count = row.try_get::<i64, _>(0).map_err(to_query_error)?;
        u64::try_from(count).map_err(|error| StoreError::Serialization(error.to_string()))
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

    async fn append_agent_observation_set(
        &self,
        set: &AgentObservationSetRecord,
    ) -> Result<bool, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let result = sqlx::query("INSERT INTO agent_inferred_observation_sets (observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT(observation_set_id) DO NOTHING")
            .bind(set.observation_set_id.to_string()).bind(set.agent_session_id.to_string()).bind(&set.parser_version).bind(datetime_to_unix_millis(set.source_watermark_at)?).bind(crate::shared::serialize_json(&set.coverage)?).bind(set.created_at.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            for observation in &set.observations {
                let observation_result = sqlx::query("INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_session_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT(observation_id) DO NOTHING")
                    .bind(observation.observation_id.to_string()).bind(set.observation_set_id.to_string()).bind(set.agent_session_id.to_string()).bind(enum_name(observation.kind)?).bind(&observation.source_request_id).bind(enum_name(observation.evidence)?).bind(observation.occurred_at.unix_timestamp()).bind(crate::shared::serialize_json(&observation.facts)?).bind(crate::shared::serialize_json(&observation.limitations)?).execute(&mut *transaction).await.map_err(to_query_error)?;
                if observation_result.rows_affected() == 0 {
                    return Err(StoreError::Conflict(format!(
                        "agent observation `{}` conflicts with the existing record",
                        observation.observation_id
                    )));
                }
            }
        }
        transaction.commit().await.map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            return Ok(true);
        }
        let existing = self
            .query_observation_set_by_id(set.observation_set_id)
            .await?
            .ok_or_else(|| {
                StoreError::Query("agent observation set conflict row disappeared".to_string())
            })?;
        if agent_observation_set_matches(&existing, set) {
            Ok(false)
        } else {
            Err(StoreError::Conflict(format!(
                "agent observation set `{}` conflicts with the existing record",
                set.observation_set_id
            )))
        }
    }

    async fn load_agent_observation_sets(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT observation_set_id FROM agent_inferred_observation_sets WHERE agent_session_id = $1 ORDER BY source_watermark_at, created_at, observation_set_id LIMIT 1001",
        )
        .bind(agent_session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        let mut sets = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?;
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
        let session_sql =
            format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE agent_session_id = $1");
        let session_row = sqlx::query(&session_sql)
            .bind(agent_session_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        let Some(session_row) = session_row else {
            return Ok(None);
        };
        let session = decode_session(&session_row)?;
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
            "SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE agent_session_id = $1 AND stale = 0 ORDER BY analyzed_at DESC LIMIT 1"
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
            session_source,
            requests,
            latest_observation_set,
            latest_analysis,
        }))
    }
}
