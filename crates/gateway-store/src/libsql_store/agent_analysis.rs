use super::*;
use crate::shared::{
    agent_observation_set_matches, agent_session_identity_matches, agent_task_analysis_matches,
    agent_task_identity_matches, agent_task_request_matches, parse_uuid, unix_to_datetime,
};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisQueueRecord, AgentAnalysisQueueStatus,
    AgentObservationSetRecord, AgentRequestLogLinkRecord, AgentSessionAnalysisRepository,
    AgentSessionRecord, AgentTaskAnalysisRecord, AgentTaskListPage, AgentTaskListQuery,
    AgentTaskRequestLinkRecord, AgentTaskTraceRecord, AgentTaskWindowRecord, Confidence,
    InferredObservation, MAX_AGENT_TASK_PAGE_SIZE, TaskLifecycleState,
};
use serde::Serialize;

fn enum_name<T: Serialize>(value: T) -> Result<String, StoreError> {
    let value = serde_json::to_value(value)
        .map_err(|error| StoreError::Serialization(error.to_string()))?;
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| StoreError::Serialization("enum did not serialize as a string".to_string()))
}

fn parse_confidence(value: &str) -> Result<Confidence, StoreError> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn parse_lifecycle(value: &str) -> Result<TaskLifecycleState, StoreError> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<Uuid>, StoreError> {
    value.as_deref().map(parse_uuid).transpose()
}

fn decode_session(row: &libsql::Row) -> Result<AgentSessionRecord, StoreError> {
    Ok(AgentSessionRecord {
        agent_session_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        ownership_scope_key: row.get(1).map_err(to_query_error)?,
        api_key_id: parse_uuid(&row.get::<String>(2).map_err(to_query_error)?)?,
        user_id: parse_optional_uuid(row.get(3).map_err(to_query_error)?)?,
        team_id: parse_optional_uuid(row.get(4).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.get(5).map_err(to_query_error)?)?,
        actor_user_id: parse_optional_uuid(row.get(6).map_err(to_query_error)?)?,
        normalized_session_id: row.get(7).map_err(to_query_error)?,
        adapter_namespace: row.get(8).map_err(to_query_error)?,
        adapter_version: row.get(9).map_err(to_query_error)?,
        source_provenance: row.get(10).map_err(to_query_error)?,
        harness_key: row.get(11).map_err(to_query_error)?,
        harness_label: row.get(12).map_err(to_query_error)?,
        first_seen_at: unix_to_datetime(row.get(13).map_err(to_query_error)?)?,
        last_seen_at: unix_to_datetime(row.get(14).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.get(15).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(16).map_err(to_query_error)?)?,
    })
}

fn decode_task(row: &libsql::Row) -> Result<AgentTaskWindowRecord, StoreError> {
    let request_tags_json: String = row.get(11).map_err(to_query_error)?;
    let lifecycle: String = row.get(15).map_err(to_query_error)?;
    let confidence: String = row.get(16).map_err(to_query_error)?;
    let ended_at: Option<i64> = row.get(18).map_err(to_query_error)?;
    Ok(AgentTaskWindowRecord {
        agent_task_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        agent_session_id: parse_optional_uuid(row.get(1).map_err(to_query_error)?)?,
        ownership_scope_key: row.get(2).map_err(to_query_error)?,
        api_key_id: parse_uuid(&row.get::<String>(3).map_err(to_query_error)?)?,
        user_id: parse_optional_uuid(row.get(4).map_err(to_query_error)?)?,
        team_id: parse_optional_uuid(row.get(5).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.get(6).map_err(to_query_error)?)?,
        actor_user_id: parse_optional_uuid(row.get(7).map_err(to_query_error)?)?,
        requested_model_key: row.get(8).map_err(to_query_error)?,
        operation: row.get(9).map_err(to_query_error)?,
        caller_class: row.get(10).map_err(to_query_error)?,
        request_tags: serde_json::from_str(&request_tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        harness_key: row.get(12).map_err(to_query_error)?,
        boundary_group_key: row.get(13).map_err(to_query_error)?,
        boundary_policy_version: row.get(14).map_err(to_query_error)?,
        lifecycle: parse_lifecycle(&lifecycle)?,
        boundary_confidence: parse_confidence(&confidence)?,
        started_at: unix_to_datetime(row.get(17).map_err(to_query_error)?)?,
        ended_at: ended_at.map(unix_to_datetime).transpose()?,
        input_watermark_at: unix_to_datetime(row.get(19).map_err(to_query_error)?)?,
        finalized_reason: row.get(20).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(21).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(22).map_err(to_query_error)?)?,
    })
}

fn decode_request_link(row: &libsql::Row) -> Result<AgentTaskRequestLinkRecord, StoreError> {
    let confidence: String = row.get(8).map_err(to_query_error)?;
    let limitations_json: String = row.get(9).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.get(11).map_err(to_query_error)?;
    Ok(AgentTaskRequestLinkRecord {
        agent_task_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        request_id: row.get(1).map_err(to_query_error)?,
        request_log_id: parse_optional_uuid(row.get(2).map_err(to_query_error)?)?,
        usage_event_id: parse_optional_uuid(row.get(3).map_err(to_query_error)?)?,
        ordinal: row.get(4).map_err(to_query_error)?,
        execution_id: row.get(5).map_err(to_query_error)?,
        parent_execution_id: row.get(6).map_err(to_query_error)?,
        normalized_session_id: row.get(7).map_err(to_query_error)?,
        correlation_confidence: parse_confidence(&confidence)?,
        limitation_codes: serde_json::from_str(&limitations_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(row.get(10).map_err(to_query_error)?)?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        terminal_success: row
            .get::<Option<i64>>(12)
            .map_err(to_query_error)?
            .map(|value| value != 0),
    })
}

fn decode_observation(row: &libsql::Row) -> Result<InferredObservation, StoreError> {
    let kind: String = row.get(1).map_err(to_query_error)?;
    let evidence: String = row.get(3).map_err(to_query_error)?;
    let facts_json: String = row.get(5).map_err(to_query_error)?;
    let limitations_json: String = row.get(6).map_err(to_query_error)?;
    Ok(InferredObservation {
        observation_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        kind: serde_json::from_value(serde_json::Value::String(kind))
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        source_request_id: row.get(2).map_err(to_query_error)?,
        parser_version: row.get(7).map_err(to_query_error)?,
        evidence: serde_json::from_value(serde_json::Value::String(evidence))
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(row.get(4).map_err(to_query_error)?)?,
        facts: serde_json::from_str(&facts_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        limitations: serde_json::from_str(&limitations_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
    })
}

fn decode_analysis(row: &libsql::Row) -> Result<AgentTaskAnalysisRecord, StoreError> {
    let report_json: String = row.get(14).map_err(to_query_error)?;
    let superseded_by: Option<String> = row.get(16).map_err(to_query_error)?;
    Ok(AgentTaskAnalysisRecord {
        analysis_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        agent_task_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
        boundary_policy_version: row.get(3).map_err(to_query_error)?,
        input_watermark_at: unix_to_datetime(row.get(4).map_err(to_query_error)?)?,
        observation_set_id: parse_uuid(&row.get::<String>(5).map_err(to_query_error)?)?,
        observation_parser_version: row.get(6).map_err(to_query_error)?,
        pricing_policy_version: row.get(9).map_err(to_query_error)?,
        cohort_version: row.get(10).map_err(to_query_error)?,
        cohort_fallback_level: u8::try_from(row.get::<i64>(11).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        cohort_sample_size: u64::try_from(row.get::<i64>(12).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        cohort_snapshot_digest: row.get(13).map_err(to_query_error)?,
        analyzed_at: unix_to_datetime(row.get(15).map_err(to_query_error)?)?,
        report: serde_json::from_str(&report_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        stale: row.get::<i64>(17).map_err(to_query_error)? == 1,
        superseded_by_analysis_id: superseded_by.as_deref().map(parse_uuid).transpose()?,
        expires_at: unix_to_datetime(row.get(18).map_err(to_query_error)?)?,
        ownership_scope_key: row.get(19).map_err(to_query_error)?,
        user_id: parse_optional_uuid(row.get(20).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.get(21).map_err(to_query_error)?)?,
    })
}

fn decode_queue(row: &libsql::Row) -> Result<AgentAnalysisQueueRecord, StoreError> {
    let desired_versions_json: String = row.get(3).map_err(to_query_error)?;
    let status: String = row.get(4).map_err(to_query_error)?;
    let lease_expires_at: Option<i64> = row.get(6).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.get(13).map_err(to_query_error)?;
    Ok(AgentAnalysisQueueRecord {
        queue_item_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        agent_task_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
        reason: row.get(2).map_err(to_query_error)?,
        desired_versions: serde_json::from_str::<AgentAnalysisDesiredVersions>(
            &desired_versions_json,
        )
        .map_err(|error| StoreError::Serialization(error.to_string()))?,
        status: AgentAnalysisQueueStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown agent analysis queue status `{status}`"))
        })?,
        lease_owner: row.get(5).map_err(to_query_error)?,
        lease_expires_at: lease_expires_at.map(unix_to_datetime).transpose()?,
        attempts: u32::try_from(row.get::<i64>(7).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        max_attempts: u32::try_from(row.get::<i64>(8).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        last_error: row.get(9).map_err(to_query_error)?,
        available_at: unix_to_datetime(row.get(10).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.get(11).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(12).map_err(to_query_error)?)?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
    })
}

const SESSION_COLUMNS: &str = "agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at";
const TASK_COLUMNS: &str = "agent_task_id, agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at";
const REQUEST_COLUMNS: &str = "agent_task_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success";
const ANALYSIS_COLUMNS: &str = "analysis_id, agent_task_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, report_json, analyzed_at, superseded_by_analysis_id, stale, expires_at, ownership_scope_key, user_id, service_account_id";
const QUEUE_COLUMNS: &str = "queue_item_id, agent_task_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at";

impl LibsqlStore {
    async fn query_session_by_natural_key(
        &self,
        ownership_scope_key: &str,
        adapter_namespace: &str,
        normalized_session_id: &str,
    ) -> Result<Option<AgentSessionRecord>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE ownership_scope_key = ?1 AND adapter_namespace = ?2 AND normalized_session_hash = ?3"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    ownership_scope_key,
                    adapter_namespace,
                    normalized_session_id
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

    async fn query_task_by_id(
        &self,
        agent_task_id: Uuid,
    ) -> Result<Option<AgentTaskWindowRecord>, StoreError> {
        let sql = format!("SELECT {TASK_COLUMNS} FROM agent_task_windows WHERE agent_task_id = ?1");
        let mut rows = self
            .connection
            .query(&sql, [agent_task_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_task)
            .transpose()
    }

    async fn load_observation_set(
        &self,
        agent_task_id: Uuid,
    ) -> Result<Option<AgentObservationSetRecord>, StoreError> {
        let mut rows = self.connection.query(
            "SELECT observation_set_id, agent_task_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE agent_task_id = ?1 ORDER BY source_watermark_at DESC, created_at DESC LIMIT 1",
            [agent_task_id.to_string()],
        ).await.map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };
        let observation_set_id = parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?;
        let coverage_json: String = row.get(4).map_err(to_query_error)?;
        let mut observation_rows = self.connection.query(
            "SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at, o.facts_json, o.limitation_codes_json, s.parser_version FROM agent_inferred_observations o JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id WHERE o.agent_task_id = ?1 ORDER BY o.occurred_at, o.observation_id",
            [agent_task_id.to_string()],
        ).await.map_err(to_query_error)?;
        let mut observations = Vec::new();
        while let Some(observation_row) = observation_rows.next().await.map_err(to_query_error)? {
            observations.push(decode_observation(&observation_row)?);
        }
        Ok(Some(AgentObservationSetRecord {
            observation_set_id,
            agent_task_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
            parser_version: row.get(2).map_err(to_query_error)?,
            source_watermark_at: unix_to_datetime(row.get(3).map_err(to_query_error)?)?,
            coverage: serde_json::from_str(&coverage_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            created_at: unix_to_datetime(row.get(5).map_err(to_query_error)?)?,
            observations,
        }))
    }
    async fn query_observation_set_by_id(
        &self,
        observation_set_id: Uuid,
    ) -> Result<Option<AgentObservationSetRecord>, StoreError> {
        let mut rows = self.connection.query(
            "SELECT observation_set_id, agent_task_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE observation_set_id = ?1",
            [observation_set_id.to_string()],
        ).await.map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };
        let coverage_json: String = row.get(4).map_err(to_query_error)?;
        let mut observation_rows = self.connection.query(
            "SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at, o.facts_json, o.limitation_codes_json, s.parser_version FROM agent_inferred_observations o JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id WHERE o.observation_set_id = ?1 ORDER BY o.occurred_at, o.observation_id",
            [observation_set_id.to_string()],
        ).await.map_err(to_query_error)?;
        let mut observations = Vec::new();
        while let Some(observation_row) = observation_rows.next().await.map_err(to_query_error)? {
            observations.push(decode_observation(&observation_row)?);
        }
        Ok(Some(AgentObservationSetRecord {
            observation_set_id,
            agent_task_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
            parser_version: row.get(2).map_err(to_query_error)?,
            source_watermark_at: unix_to_datetime(row.get(3).map_err(to_query_error)?)?,
            coverage: serde_json::from_str(&coverage_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            created_at: unix_to_datetime(row.get(5).map_err(to_query_error)?)?,
            observations,
        }))
    }
}

#[async_trait]
impl AgentSessionAnalysisRepository for LibsqlStore {
    async fn upsert_agent_session(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<AgentSessionRecord, StoreError> {
        self.connection.execute(
            "INSERT INTO agent_sessions (agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) ON CONFLICT DO NOTHING",
            libsql::params![session.agent_session_id.to_string(), session.ownership_scope_key.as_str(), session.api_key_id.to_string(), session.user_id.map(|value| value.to_string()), session.team_id.map(|value| value.to_string()), session.service_account_id.map(|value| value.to_string()), session.actor_user_id.map(|value| value.to_string()), session.normalized_session_id.as_str(), session.adapter_namespace.as_str(), session.adapter_version.as_str(), session.source_provenance.as_str(), session.harness_key.as_str(), session.harness_label.as_str(), session.first_seen_at.unix_timestamp(), session.last_seen_at.unix_timestamp(), session.created_at.unix_timestamp(), session.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        let existing = self
            .query_session_by_natural_key(
                &session.ownership_scope_key,
                &session.adapter_namespace,
                &session.normalized_session_id,
            )
            .await?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "agent session `{}` conflicts with the existing record",
                    session.agent_session_id
                ))
            })?;
        if !agent_session_identity_matches(&existing, session) {
            return Err(StoreError::Conflict(format!(
                "agent session `{}` conflicts with the existing record",
                session.agent_session_id
            )));
        }
        self.connection.execute(
            "UPDATE agent_sessions SET first_seen_at = MIN(first_seen_at, ?2), last_seen_at = MAX(last_seen_at, ?3), updated_at = MAX(updated_at, ?4) WHERE agent_session_id = ?1",
            libsql::params![session.agent_session_id.to_string(), session.first_seen_at.unix_timestamp(), session.last_seen_at.unix_timestamp(), session.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        self.query_session_by_natural_key(
            &session.ownership_scope_key,
            &session.adapter_namespace,
            &session.normalized_session_id,
        )
        .await?
        .ok_or_else(|| StoreError::Unexpected("upserted agent session was not found".to_string()))
    }

    async fn load_agent_session(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Option<AgentSessionRecord>, StoreError> {
        let sql =
            format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE agent_session_id = ?1");
        let mut rows = self
            .connection
            .query(&sql, [agent_session_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_session)
            .transpose()
    }

    async fn get_open_agent_task(
        &self,
        ownership_scope_key: &str,
        agent_session_id: Option<Uuid>,
        harness_key: &str,
        boundary_group_key: &str,
    ) -> Result<Option<AgentTaskWindowRecord>, StoreError> {
        let sql = format!(
            "SELECT {TASK_COLUMNS} FROM agent_task_windows WHERE ownership_scope_key = ?1 AND lifecycle = 'open' AND boundary_group_key = ?4 AND ((?2 IS NULL AND agent_session_id IS NULL AND harness_key = ?3) OR agent_session_id = ?2) ORDER BY started_at DESC LIMIT 1"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    ownership_scope_key,
                    agent_session_id.map(|value| value.to_string()),
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
            .map(decode_task)
            .transpose()
    }

    async fn insert_agent_task_if_absent(
        &self,
        task: &AgentTaskWindowRecord,
    ) -> Result<bool, StoreError> {
        let written = self.connection.execute(
            "INSERT INTO agent_task_windows (agent_task_id, agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23) ON CONFLICT DO NOTHING",
            libsql::params![task.agent_task_id.to_string(), task.agent_session_id.map(|value| value.to_string()), task.ownership_scope_key.as_str(), task.api_key_id.to_string(), task.user_id.map(|value| value.to_string()), task.team_id.map(|value| value.to_string()), task.service_account_id.map(|value| value.to_string()), task.actor_user_id.map(|value| value.to_string()), task.requested_model_key.as_str(), task.operation.as_str(), task.caller_class.as_str(), crate::shared::serialize_json(&task.request_tags)?, task.harness_key.as_str(), task.boundary_group_key.as_str(), task.boundary_policy_version.as_str(), enum_name(task.lifecycle)?, enum_name(task.boundary_confidence)?, task.started_at.unix_timestamp(), task.ended_at.map(OffsetDateTime::unix_timestamp), task.input_watermark_at.unix_timestamp(), task.finalized_reason.as_deref(), task.created_at.unix_timestamp(), task.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        if written > 0 {
            return Ok(true);
        }
        let Some(existing) = self.query_task_by_id(task.agent_task_id).await? else {
            return Ok(false);
        };
        if agent_task_identity_matches(&existing, task) {
            Ok(false)
        } else {
            Err(StoreError::Conflict(format!(
                "agent task `{}` conflicts with the existing record",
                task.agent_task_id
            )))
        }
    }

    async fn update_agent_task_window(
        &self,
        task: &AgentTaskWindowRecord,
    ) -> Result<(), StoreError> {
        let updated = self.connection.execute(
            "UPDATE agent_task_windows SET lifecycle = ?2, boundary_confidence = ?3, ended_at = ?4, input_watermark_at = MAX(input_watermark_at, ?5), finalized_reason = ?6, updated_at = MAX(updated_at, ?7) WHERE agent_task_id = ?1",
            libsql::params![task.agent_task_id.to_string(), enum_name(task.lifecycle)?, enum_name(task.boundary_confidence)?, task.ended_at.map(OffsetDateTime::unix_timestamp), task.input_watermark_at.unix_timestamp(), task.finalized_reason.as_deref(), task.updated_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        if updated == 0 {
            return Err(StoreError::NotFound(
                "agent task window not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn finalize_agent_task_if_unchanged(
        &self,
        task: &AgentTaskWindowRecord,
        expected_input_watermark_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let updated = self.connection.execute(
            "UPDATE agent_task_windows SET lifecycle = ?2, boundary_confidence = ?3, ended_at = ?4, input_watermark_at = ?5, finalized_reason = ?6, updated_at = ?7 WHERE agent_task_id = ?1 AND lifecycle = 'open' AND input_watermark_at = ?8",
            libsql::params![task.agent_task_id.to_string(), enum_name(task.lifecycle)?, enum_name(task.boundary_confidence)?, task.ended_at.map(OffsetDateTime::unix_timestamp), task.input_watermark_at.unix_timestamp(), task.finalized_reason.as_deref(), task.updated_at.unix_timestamp(), expected_input_watermark_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        Ok(updated > 0)
    }

    async fn append_agent_task_request(
        &self,
        link: &AgentTaskRequestLinkRecord,
    ) -> Result<bool, StoreError> {
        let limitations_json = crate::shared::serialize_json(&link.limitation_codes)?;
        let activity_at = link.completed_at.unwrap_or(link.occurred_at);
        let transaction = self
            .connection
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(to_query_error)?;
        let written = transaction.execute(
            "INSERT INTO agent_task_window_requests (agent_task_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success) VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM agent_task_window_requests WHERE agent_task_id = ?1), ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) ON CONFLICT(agent_task_id, request_id) DO NOTHING",
            libsql::params![link.agent_task_id.to_string(), link.request_id.as_str(), link.request_log_id.map(|value| value.to_string()), link.usage_event_id.map(|value| value.to_string()), link.execution_id.as_deref(), link.parent_execution_id.as_deref(), link.normalized_session_id.as_deref(), enum_name(link.correlation_confidence)?, limitations_json, link.occurred_at.unix_timestamp(), link.completed_at.map(OffsetDateTime::unix_timestamp), link.terminal_success.map(i64::from)],
        ).await.map_err(to_query_error)?;
        let inserted = written > 0;
        if inserted {
            transaction.execute("UPDATE agent_task_windows SET input_watermark_at = MAX(input_watermark_at, ?2), updated_at = MAX(updated_at, ?2) WHERE agent_task_id = ?1", libsql::params![link.agent_task_id.to_string(), activity_at.unix_timestamp()]).await.map_err(to_query_error)?;
        } else {
            let sql = format!(
                "SELECT {REQUEST_COLUMNS} FROM agent_task_window_requests WHERE agent_task_id = ?1 AND request_id = ?2"
            );
            let mut rows = transaction
                .query(
                    &sql,
                    libsql::params![link.agent_task_id.to_string(), link.request_id.as_str()],
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
                    StoreError::Query("agent task request conflict row disappeared".to_string())
                })?;
            if !agent_task_request_matches(&existing, link) {
                return Err(StoreError::Conflict(format!(
                    "agent task request `{}` conflicts with the existing record",
                    link.request_id
                )));
            }
        }
        transaction.commit().await.map_err(to_query_error)?;
        Ok(inserted)
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
            "INSERT INTO agent_inferred_observation_sets (observation_set_id, agent_task_id, parser_version, source_watermark_at, coverage_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(observation_set_id) DO NOTHING",
            libsql::params![set.observation_set_id.to_string(), set.agent_task_id.to_string(), set.parser_version.as_str(), set.source_watermark_at.unix_timestamp(), coverage_json, set.created_at.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        if written > 0 {
            for observation in &set.observations {
                let observation_written = transaction.execute(
                    "INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_task_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(observation_id) DO NOTHING",
                    libsql::params![observation.observation_id.to_string(), set.observation_set_id.to_string(), set.agent_task_id.to_string(), enum_name(observation.kind)?, observation.source_request_id.as_str(), enum_name(observation.evidence)?, observation.occurred_at.unix_timestamp(), crate::shared::serialize_json(&observation.facts)?, crate::shared::serialize_json(&observation.limitations)?],
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

    async fn link_request_log_to_agent_task(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError> {
        let updated = self.connection.execute(
            "UPDATE request_logs SET agent_session_id = ?2, agent_task_id = ?3, agent_analysis_source = ?4, agent_analysis_coverage_json = ?5 WHERE request_log_id = ?1",
            libsql::params![link.request_log_id.to_string(), link.agent_session_id.map(|value| value.to_string()), link.agent_task_id.to_string(), link.analysis_source.as_str(), crate::shared::serialize_json(&link.coverage)?],
        ).await.map_err(to_query_error)?;
        if updated == 0 {
            return Err(StoreError::NotFound("request log not found".to_string()));
        }
        Ok(())
    }

    async fn load_agent_task_trace(
        &self,
        agent_task_id: Uuid,
    ) -> Result<Option<AgentTaskTraceRecord>, StoreError> {
        let task_sql =
            format!("SELECT {TASK_COLUMNS} FROM agent_task_windows WHERE agent_task_id = ?1");
        let mut task_rows = self
            .connection
            .query(&task_sql, [agent_task_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let Some(task_row) = task_rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };
        let task = decode_task(&task_row)?;
        let session = if let Some(session_id) = task.agent_session_id {
            let sql =
                format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE agent_session_id = ?1");
            let mut rows = self
                .connection
                .query(&sql, [session_id.to_string()])
                .await
                .map_err(to_query_error)?;
            rows.next()
                .await
                .map_err(to_query_error)?
                .as_ref()
                .map(decode_session)
                .transpose()?
        } else {
            None
        };
        let request_sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM agent_task_window_requests WHERE agent_task_id = ?1 ORDER BY occurred_at, ordinal"
        );
        let mut request_rows = self
            .connection
            .query(&request_sql, [agent_task_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let mut requests = Vec::new();
        while let Some(row) = request_rows.next().await.map_err(to_query_error)? {
            requests.push(decode_request_link(&row)?);
        }
        let latest_observation_set = self.load_observation_set(agent_task_id).await?;
        let analysis_sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_task_analyses WHERE agent_task_id = ?1 AND stale = 0 ORDER BY analyzed_at DESC LIMIT 1"
        );
        let mut analysis_rows = self
            .connection
            .query(&analysis_sql, [agent_task_id.to_string()])
            .await
            .map_err(to_query_error)?;
        let latest_analysis = analysis_rows
            .next()
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_analysis)
            .transpose()?;
        Ok(Some(AgentTaskTraceRecord {
            task,
            session,
            requests,
            latest_observation_set,
            latest_analysis,
        }))
    }

    async fn append_agent_task_analysis(
        &self,
        analysis: &AgentTaskAnalysisRecord,
    ) -> Result<bool, StoreError> {
        let written = self.connection.execute(
            "INSERT INTO agent_task_analyses (analysis_id, agent_task_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, analyzed_at, report_json, stale, superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id) SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22 WHERE EXISTS (SELECT 1 FROM agent_task_windows WHERE agent_task_id = ?2 AND input_watermark_at = ?5) ON CONFLICT DO NOTHING",
            libsql::params![analysis.analysis_id.to_string(), analysis.agent_task_id.to_string(), analysis.report.report_schema_version.as_str(), analysis.boundary_policy_version.as_str(), analysis.input_watermark_at.unix_timestamp(), analysis.observation_set_id.to_string(), analysis.observation_parser_version.as_str(), analysis.report.analyzer_version.as_str(), analysis.report.score_policy_version.as_str(), analysis.pricing_policy_version.as_str(), analysis.cohort_version.as_str(), i64::from(analysis.cohort_fallback_level), i64::try_from(analysis.cohort_sample_size).map_err(|error| StoreError::Serialization(error.to_string()))?, analysis.cohort_snapshot_digest.as_str(), analysis.analyzed_at.unix_timestamp(), crate::shared::serialize_json(&analysis.report)?, i64::from(analysis.stale), analysis.superseded_by_analysis_id.map(|value| value.to_string()), analysis.expires_at.unix_timestamp(), analysis.ownership_scope_key.as_str(), analysis.user_id.map(|value| value.to_string()), analysis.service_account_id.map(|value| value.to_string())],
        ).await.map_err(to_query_error)?;
        if written > 0 {
            return Ok(true);
        }
        let sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_task_analyses WHERE agent_task_id = ?1 AND report_schema_version = ?2 AND boundary_policy_version = ?3 AND input_watermark_at = ?4 AND observation_set_id = ?5 AND observation_parser_version = ?6 AND analyzer_version = ?7 AND score_policy_version = ?8 AND pricing_policy_version = ?9 AND cohort_version = ?10 AND cohort_fallback_level = ?11 AND cohort_sample_size = ?12 AND cohort_snapshot_digest = ?13"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    analysis.agent_task_id.to_string(),
                    analysis.report.report_schema_version.as_str(),
                    analysis.boundary_policy_version.as_str(),
                    analysis.input_watermark_at.unix_timestamp(),
                    analysis.observation_set_id.to_string(),
                    analysis.observation_parser_version.as_str(),
                    analysis.report.analyzer_version.as_str(),
                    analysis.report.score_policy_version.as_str(),
                    analysis.pricing_policy_version.as_str(),
                    analysis.cohort_version.as_str(),
                    i64::from(analysis.cohort_fallback_level),
                    i64::try_from(analysis.cohort_sample_size)
                        .map_err(|error| StoreError::Serialization(error.to_string()))?,
                    analysis.cohort_snapshot_digest.as_str()
                ],
            )
            .await
            .map_err(to_query_error)?;
        if let Some(row) = rows.next().await.map_err(to_query_error)? {
            let existing = decode_analysis(&row)?;
            return if agent_task_analysis_matches(&existing, analysis) {
                Ok(false)
            } else {
                Err(StoreError::Conflict(format!(
                    "agent task analysis for task `{}` conflicts with the existing record",
                    analysis.agent_task_id
                )))
            };
        }
        let mut id_rows = self
            .connection
            .query(
                "SELECT 1 FROM agent_task_analyses WHERE analysis_id = ?1",
                [analysis.analysis_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        if id_rows.next().await.map_err(to_query_error)?.is_some() {
            return Err(StoreError::Conflict(format!(
                "agent task analysis `{}` conflicts with the existing record",
                analysis.analysis_id
            )));
        }
        Ok(false)
    }

    async fn list_agent_tasks(
        &self,
        query: &AgentTaskListQuery,
    ) -> Result<AgentTaskListPage, StoreError> {
        let page = query.page.max(1);
        let page_size = query.page_size.clamp(1, MAX_AGENT_TASK_PAGE_SIZE);
        let offset = i64::from(page.saturating_sub(1).saturating_mul(page_size));
        let lifecycle = query.lifecycle.map(enum_name).transpose()?;
        let score_confidence = query.score_confidence.map(enum_name).transpose()?;
        let gateway_outcome = query.gateway_outcome.map(enum_name).transpose()?;
        let score_maturity = query.score_maturity.map(enum_name).transpose()?;
        let from_sql = "agent_task_windows t LEFT JOIN agent_sessions s ON s.agent_session_id = t.agent_session_id LEFT JOIN agent_task_analyses latest_analysis ON latest_analysis.analysis_id = (SELECT a.analysis_id FROM agent_task_analyses a WHERE a.agent_task_id = t.agent_task_id AND a.stale = 0 ORDER BY a.analyzed_at DESC, a.analysis_id DESC LIMIT 1)";
        let where_sql = "(?1 IS NULL OR t.ownership_scope_key = ?1) AND (?2 IS NULL OR t.user_id = ?2) AND (?3 IS NULL OR EXISTS (SELECT 1 FROM team_memberships tm WHERE tm.user_id = t.user_id AND tm.team_id = ?3) OR EXISTS (SELECT 1 FROM service_accounts sa WHERE sa.service_account_id = t.service_account_id AND sa.team_id = ?3)) AND (?4 IS NULL OR t.service_account_id = ?4) AND (?5 IS NULL OR t.harness_key = ?5) AND (?6 IS NULL OR t.lifecycle = ?6) AND (?7 IS NULL OR t.started_at >= ?7) AND (?8 IS NULL OR t.started_at < ?8) AND (?9 IS NULL OR json_extract(latest_analysis.report_json, '$.confidence') = ?9) AND (?10 IS NULL OR t.agent_session_id = ?10) AND (?11 IS NULL OR t.requested_model_key = ?11) AND (?12 IS NULL OR t.operation = ?12) AND (?13 IS NULL OR t.caller_class = ?13) AND (?14 IS NULL OR json_extract(latest_analysis.report_json, '$.gateway_outcome') = ?14) AND (?15 IS NULL OR json_extract(latest_analysis.report_json, '$.maturity') = ?15) AND (?16 IS NULL OR CAST(json_extract(latest_analysis.report_json, '$.coverage.overall_percent') AS INTEGER) >= ?16) AND (?17 IS NULL OR s.normalized_session_hash = ?17) AND ((?18 IS NULL AND ?19 IS NULL) OR (?18 IS NOT NULL AND EXISTS (SELECT 1 FROM json_each(t.request_tags_json) tag WHERE tag.key = ?18 AND (?19 IS NULL OR CAST(tag.value AS TEXT) = ?19))))";
        let count_sql = format!("SELECT COUNT(*) FROM {from_sql} WHERE {where_sql}");
        let mut count_rows = self
            .connection
            .query(
                &count_sql,
                libsql::params![
                    query.ownership_scope_key.as_deref(),
                    query.user_id.map(|value| value.to_string()),
                    query.team_id.map(|value| value.to_string()),
                    query.service_account_id.map(|value| value.to_string()),
                    query.harness_key.as_deref(),
                    lifecycle.as_deref(),
                    query.started_after.map(OffsetDateTime::unix_timestamp),
                    query.started_before.map(OffsetDateTime::unix_timestamp),
                    score_confidence.as_deref(),
                    query.agent_session_id.map(|value| value.to_string()),
                    query.requested_model_key.as_deref(),
                    query.operation.as_deref(),
                    query.caller_class.as_deref(),
                    gateway_outcome.as_deref(),
                    score_maturity.as_deref(),
                    query.minimum_coverage_percent.map(i64::from),
                    query.normalized_session_id.as_deref(),
                    query.request_tag_key.as_deref(),
                    query.request_tag_value.as_deref(),
                ],
            )
            .await
            .map_err(to_query_error)?;
        let total = u64::try_from(
            count_rows
                .next()
                .await
                .map_err(to_query_error)?
                .ok_or_else(|| StoreError::Unexpected("agent task count missing".to_string()))?
                .get::<i64>(0)
                .map_err(to_query_error)?,
        )
        .map_err(|error| StoreError::Serialization(error.to_string()))?;
        let list_sql = format!(
            "SELECT t.agent_task_id FROM {from_sql} WHERE {where_sql} ORDER BY t.started_at DESC, t.agent_task_id LIMIT ?20 OFFSET ?21"
        );
        let mut rows = self
            .connection
            .query(
                &list_sql,
                libsql::params![
                    query.ownership_scope_key.as_deref(),
                    query.user_id.map(|value| value.to_string()),
                    query.team_id.map(|value| value.to_string()),
                    query.service_account_id.map(|value| value.to_string()),
                    query.harness_key.as_deref(),
                    lifecycle.as_deref(),
                    query.started_after.map(OffsetDateTime::unix_timestamp),
                    query.started_before.map(OffsetDateTime::unix_timestamp),
                    score_confidence.as_deref(),
                    query.agent_session_id.map(|value| value.to_string()),
                    query.requested_model_key.as_deref(),
                    query.operation.as_deref(),
                    query.caller_class.as_deref(),
                    gateway_outcome.as_deref(),
                    score_maturity.as_deref(),
                    query.minimum_coverage_percent.map(i64::from),
                    query.normalized_session_id.as_deref(),
                    query.request_tag_key.as_deref(),
                    query.request_tag_value.as_deref(),
                    i64::from(page_size),
                    offset,
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            ids.push(parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?);
        }
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(trace) = self.load_agent_task_trace(id).await? {
                items.push(trace);
            }
        }
        Ok(AgentTaskListPage {
            items,
            page,
            page_size,
            total,
        })
    }

    async fn mark_agent_task_analyses_stale(
        &self,
        agent_task_id: Uuid,
        superseded_by: Option<Uuid>,
    ) -> Result<u64, StoreError> {
        self.connection.execute("UPDATE agent_task_analyses SET stale = 1, superseded_by_analysis_id = ?2 WHERE agent_task_id = ?1 AND stale = 0 AND (?2 IS NULL OR analysis_id <> ?2)", libsql::params![agent_task_id.to_string(), superseded_by.map(|value| value.to_string())]).await.map_err(to_query_error)
    }

    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        let written = self.connection.execute(
            "INSERT INTO agent_analysis_recompute_queue (queue_item_id, agent_task_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) ON CONFLICT(queue_item_id) DO NOTHING",
            libsql::params![item.queue_item_id.to_string(), item.agent_task_id.to_string(), item.reason.as_str(), crate::shared::serialize_json(&item.desired_versions)?, item.status.as_str(), item.lease_owner.as_deref(), item.lease_expires_at.map(OffsetDateTime::unix_timestamp), i64::from(item.attempts), i64::from(item.max_attempts), item.last_error.as_deref(), item.available_at.unix_timestamp(), item.created_at.unix_timestamp(), item.updated_at.unix_timestamp(), item.completed_at.map(OffsetDateTime::unix_timestamp)],
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
                "DELETE FROM agent_task_analyses WHERE expires_at < ?1",
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
        let observations = self.connection.execute(
            "DELETE FROM agent_inferred_observation_sets WHERE agent_task_id IN (SELECT agent_task_id FROM agent_task_windows WHERE lifecycle = 'finalized' AND input_watermark_at < ?1)",
            [request_cutoff.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        let requests = self.connection.execute(
            "DELETE FROM agent_task_window_requests WHERE agent_task_id IN (SELECT agent_task_id FROM agent_task_windows WHERE lifecycle = 'finalized' AND input_watermark_at < ?1)",
            [request_cutoff.unix_timestamp()],
        ).await.map_err(to_query_error)?;
        let tasks = self
            .connection
            .execute(
                "DELETE FROM agent_task_windows WHERE lifecycle = 'finalized' AND input_watermark_at < ?1 AND NOT EXISTS (SELECT 1 FROM agent_task_analyses a WHERE a.agent_task_id = agent_task_windows.agent_task_id) AND NOT EXISTS (SELECT 1 FROM agent_analysis_recompute_queue q WHERE q.agent_task_id = agent_task_windows.agent_task_id)",
                [request_cutoff.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
        let sessions = self
            .connection
            .execute(
                "DELETE FROM agent_sessions WHERE last_seen_at < ?1 AND NOT EXISTS (SELECT 1 FROM agent_task_windows t WHERE t.agent_session_id = agent_sessions.agent_session_id)",
                [request_cutoff.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;
        Ok(observations
            .saturating_add(requests)
            .saturating_add(tasks)
            .saturating_add(sessions))
    }

    async fn delete_agent_analysis_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let reports = self
            .connection
            .execute(
                "DELETE FROM agent_task_analyses WHERE ownership_scope_key = ?1",
                [ownership_scope_key],
            )
            .await
            .map_err(to_query_error)?;
        let tasks = self
            .connection
            .execute(
                "DELETE FROM agent_task_windows WHERE ownership_scope_key = ?1",
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
        Ok(reports.saturating_add(tasks).saturating_add(sessions))
    }
}
