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

    async fn update_agent_session_window(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<(), StoreError> {
        let updated = self.connection.execute(
            "UPDATE agent_sessions SET lifecycle = ?2, boundary_confidence = ?3, ended_at = ?4, input_watermark_at = MAX(input_watermark_at, ?5), finalized_reason = ?6, updated_at = MAX(updated_at, ?7) WHERE agent_session_id = ?1",
            libsql::params![session.agent_session_id.to_string(), enum_name(session.lifecycle)?, enum_name(session.boundary_confidence)?, session.ended_at.map(OffsetDateTime::unix_timestamp), datetime_to_unix_millis(session.input_watermark_at)?, session.finalized_reason.as_deref(), session.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        if updated == 0 {
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
        let updated = self.connection.execute(
            "UPDATE agent_sessions SET lifecycle = ?2, boundary_confidence = ?3, ended_at = ?4, input_watermark_at = ?5, finalized_reason = ?6, updated_at = ?7 WHERE agent_session_id = ?1 AND lifecycle = 'open' AND input_watermark_at = ?8",
            libsql::params![session.agent_session_id.to_string(), enum_name(session.lifecycle)?, enum_name(session.boundary_confidence)?, session.ended_at.map(OffsetDateTime::unix_timestamp), datetime_to_unix_millis(session.input_watermark_at)?, session.finalized_reason.as_deref(), session.updated_at.unix_timestamp(), datetime_to_unix_millis(expected_input_watermark_at)?],
        ).await.map_err(to_query_error)?;
        Ok(updated > 0)
    }

    async fn append_agent_session_request(
        &self,
        link: &AgentSessionRequestLinkRecord,
    ) -> Result<bool, StoreError> {
        let limitations_json = crate::shared::serialize_json(&link.limitation_codes)?;
        let activity_at = link.completed_at.unwrap_or(link.occurred_at);
        let transaction = self
            .connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_query_error)?;
        let mut lifecycle_rows = transaction
            .query(
                "SELECT lifecycle FROM agent_sessions WHERE agent_session_id = ?1",
                [link.agent_session_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let lifecycle = lifecycle_rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::NotFound("agent session window not found".to_string()))?
            .get::<String>(0)
            .map_err(to_query_error)?;
        if lifecycle != "open" {
            return Err(StoreError::Conflict(format!(
                "agent session `{}` is already finalized",
                link.agent_session_id
            )));
        }
        let mut request_count_rows = transaction
            .query(
                "SELECT COUNT(*), EXISTS(SELECT 1 FROM agent_session_requests WHERE agent_session_id = ?1 AND request_id = ?2) FROM agent_session_requests WHERE agent_session_id = ?1",
                libsql::params![link.agent_session_id.to_string(), link.request_id.as_str()],
            )
            .await
            .map_err(to_query_error)?;
        let request_count_row = request_count_rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| {
                StoreError::Unexpected("request count query returned no row".to_string())
            })?;
        let request_count: i64 = request_count_row.get(0).map_err(to_query_error)?;
        let request_exists: i64 = request_count_row.get(1).map_err(to_query_error)?;
        if request_exists == 0
            && request_count
                >= i64::try_from(gateway_core::MAX_AGENT_SESSION_REQUESTS)
                    .expect("agent session request limit fits in i64")
        {
            return Err(StoreError::Conflict(format!(
                "agent session `{}` reached the request limit",
                link.agent_session_id
            )));
        }
        let written = transaction.execute(
            "INSERT INTO agent_session_requests (agent_session_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success) VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM agent_session_requests WHERE agent_session_id = ?1), ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) ON CONFLICT(agent_session_id, request_id) DO NOTHING",
            libsql::params![link.agent_session_id.to_string(), link.request_id.as_str(), link.request_log_id.map(|value| value.to_string()), link.usage_event_id.map(|value| value.to_string()), link.execution_id.as_deref(), link.parent_execution_id.as_deref(), link.normalized_session_id.as_deref(), enum_name(link.correlation_confidence)?, limitations_json, datetime_to_unix_millis(link.occurred_at)?, link.completed_at.map(datetime_to_unix_millis).transpose()?, link.terminal_success.map(i64::from)],
        ).await.map_err(to_query_error)?;
        let inserted = written > 0;
        if inserted {
            transaction.execute("UPDATE agent_sessions SET started_at = MIN(started_at, ?2), input_watermark_at = MAX(input_watermark_at, ?3), updated_at = MAX(updated_at, ?4) WHERE agent_session_id = ?1", libsql::params![link.agent_session_id.to_string(), link.occurred_at.unix_timestamp(), datetime_to_unix_millis(activity_at)?, activity_at.unix_timestamp()]).await.map_err(to_query_error)?;
        } else {
            let sql = format!(
                "SELECT {REQUEST_COLUMNS} FROM agent_session_requests WHERE agent_session_id = ?1 AND request_id = ?2"
            );
            let mut rows = transaction
                .query(
                    &sql,
                    libsql::params![link.agent_session_id.to_string(), link.request_id.as_str()],
                )
                .await
                .map_err(to_query_error)?;
            let existing = rows
                .next()
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
        let mut rows = self
            .connection
            .query(
                "SELECT COUNT(*) FROM agent_session_requests WHERE agent_session_id = ?1",
                [agent_session_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let count = rows
            .next()
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| {
                StoreError::Unexpected("agent session request count missing".to_string())
            })?
            .get::<i64>(0)
            .map_err(to_query_error)?;
        u64::try_from(count).map_err(|error| StoreError::Serialization(error.to_string()))
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

    async fn append_agent_observation_set(
        &self,
        set: &AgentObservationSetRecord,
    ) -> Result<bool, StoreError> {
        let transaction = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        let coverage_json = crate::shared::serialize_json(&set.coverage)?;
        let written = transaction.execute(
            "INSERT INTO agent_inferred_observation_sets (observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(observation_set_id) DO NOTHING",
            libsql::params![set.observation_set_id.to_string(), set.agent_session_id.to_string(), set.parser_version.as_str(), datetime_to_unix_millis(set.source_watermark_at)?, coverage_json, set.created_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        if written > 0 {
            for observation in &set.observations {
                let observation_written = transaction.execute(
                    "INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_session_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(observation_id) DO NOTHING",
                    libsql::params![observation.observation_id.to_string(), set.observation_set_id.to_string(), set.agent_session_id.to_string(), enum_name(observation.kind)?, observation.source_request_id.as_str(), enum_name(observation.evidence)?, observation.occurred_at.unix_timestamp(), crate::shared::serialize_json(&observation.facts)?, crate::shared::serialize_json(&observation.limitations)?],
                ).await.map_err(to_query_error)?;
                if observation_written == 0 {
                    return Err(StoreError::Conflict(format!(
                        "agent observation `{}` conflicts with the existing record",
                        observation.observation_id
                    )));
                }
            }
        }
        transaction.commit().await.map_err(to_query_error)?;
        if written > 0 {
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
