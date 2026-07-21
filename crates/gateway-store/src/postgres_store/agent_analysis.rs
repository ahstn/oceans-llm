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
use sqlx::Row;

fn enum_name<T: Serialize>(value: T) -> Result<String, StoreError> {
    serde_json::to_value(value)
        .map_err(|error| StoreError::Serialization(error.to_string()))?
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

fn decode_session(row: &PgRow) -> Result<AgentSessionRecord, StoreError> {
    Ok(AgentSessionRecord {
        agent_session_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        ownership_scope_key: row.try_get(1).map_err(to_query_error)?,
        api_key_id: parse_uuid(&row.try_get::<String, _>(2).map_err(to_query_error)?)?,
        user_id: parse_optional_uuid(row.try_get(3).map_err(to_query_error)?)?,
        team_id: parse_optional_uuid(row.try_get(4).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.try_get(5).map_err(to_query_error)?)?,
        actor_user_id: parse_optional_uuid(row.try_get(6).map_err(to_query_error)?)?,
        normalized_session_id: row.try_get(7).map_err(to_query_error)?,
        adapter_namespace: row.try_get(8).map_err(to_query_error)?,
        adapter_version: row.try_get(9).map_err(to_query_error)?,
        source_provenance: row.try_get(10).map_err(to_query_error)?,
        harness_key: row.try_get(11).map_err(to_query_error)?,
        harness_label: row.try_get(12).map_err(to_query_error)?,
        first_seen_at: unix_to_datetime(row.try_get(13).map_err(to_query_error)?)?,
        last_seen_at: unix_to_datetime(row.try_get(14).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.try_get(15).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(16).map_err(to_query_error)?)?,
    })
}

fn decode_task(row: &PgRow) -> Result<AgentTaskWindowRecord, StoreError> {
    let request_tags_json: String = row.try_get(11).map_err(to_query_error)?;
    let lifecycle: String = row.try_get(15).map_err(to_query_error)?;
    let confidence: String = row.try_get(16).map_err(to_query_error)?;
    let ended_at: Option<i64> = row.try_get(18).map_err(to_query_error)?;
    Ok(AgentTaskWindowRecord {
        agent_task_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        agent_session_id: parse_optional_uuid(row.try_get(1).map_err(to_query_error)?)?,
        ownership_scope_key: row.try_get(2).map_err(to_query_error)?,
        api_key_id: parse_uuid(&row.try_get::<String, _>(3).map_err(to_query_error)?)?,
        user_id: parse_optional_uuid(row.try_get(4).map_err(to_query_error)?)?,
        team_id: parse_optional_uuid(row.try_get(5).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.try_get(6).map_err(to_query_error)?)?,
        actor_user_id: parse_optional_uuid(row.try_get(7).map_err(to_query_error)?)?,
        requested_model_key: row.try_get(8).map_err(to_query_error)?,
        operation: row.try_get(9).map_err(to_query_error)?,
        caller_class: row.try_get(10).map_err(to_query_error)?,
        request_tags: serde_json::from_str(&request_tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        harness_key: row.try_get(12).map_err(to_query_error)?,
        boundary_group_key: row.try_get(13).map_err(to_query_error)?,
        boundary_policy_version: row.try_get(14).map_err(to_query_error)?,
        lifecycle: parse_lifecycle(&lifecycle)?,
        boundary_confidence: parse_confidence(&confidence)?,
        started_at: unix_to_datetime(row.try_get(17).map_err(to_query_error)?)?,
        ended_at: ended_at.map(unix_to_datetime).transpose()?,
        input_watermark_at: unix_to_datetime(row.try_get(19).map_err(to_query_error)?)?,
        finalized_reason: row.try_get(20).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.try_get(21).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(22).map_err(to_query_error)?)?,
    })
}

fn decode_request_link(row: &PgRow) -> Result<AgentTaskRequestLinkRecord, StoreError> {
    let confidence: String = row.try_get(8).map_err(to_query_error)?;
    let limitations_json: String = row.try_get(9).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.try_get(11).map_err(to_query_error)?;
    Ok(AgentTaskRequestLinkRecord {
        agent_task_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        request_id: row.try_get(1).map_err(to_query_error)?,
        request_log_id: parse_optional_uuid(row.try_get(2).map_err(to_query_error)?)?,
        usage_event_id: parse_optional_uuid(row.try_get(3).map_err(to_query_error)?)?,
        ordinal: row.try_get(4).map_err(to_query_error)?,
        execution_id: row.try_get(5).map_err(to_query_error)?,
        parent_execution_id: row.try_get(6).map_err(to_query_error)?,
        normalized_session_id: row.try_get(7).map_err(to_query_error)?,
        correlation_confidence: parse_confidence(&confidence)?,
        limitation_codes: serde_json::from_str(&limitations_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(row.try_get(10).map_err(to_query_error)?)?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
        terminal_success: row.try_get(12).map_err(to_query_error)?,
    })
}

fn decode_observation(row: &PgRow) -> Result<InferredObservation, StoreError> {
    let kind: String = row.try_get(1).map_err(to_query_error)?;
    let evidence: String = row.try_get(3).map_err(to_query_error)?;
    let facts_json: String = row.try_get(5).map_err(to_query_error)?;
    let limitations_json: String = row.try_get(6).map_err(to_query_error)?;
    Ok(InferredObservation {
        observation_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        kind: serde_json::from_value(serde_json::Value::String(kind))
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        source_request_id: row.try_get(2).map_err(to_query_error)?,
        parser_version: row.try_get(7).map_err(to_query_error)?,
        evidence: serde_json::from_value(serde_json::Value::String(evidence))
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(row.try_get(4).map_err(to_query_error)?)?,
        facts: serde_json::from_str(&facts_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        limitations: serde_json::from_str(&limitations_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
    })
}

fn decode_analysis(row: &PgRow) -> Result<AgentTaskAnalysisRecord, StoreError> {
    let report_json: String = row.try_get(14).map_err(to_query_error)?;
    let superseded_by: Option<String> = row.try_get(16).map_err(to_query_error)?;
    Ok(AgentTaskAnalysisRecord {
        analysis_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        agent_task_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        boundary_policy_version: row.try_get(3).map_err(to_query_error)?,
        input_watermark_at: unix_to_datetime(row.try_get(4).map_err(to_query_error)?)?,
        observation_set_id: parse_uuid(&row.try_get::<String, _>(5).map_err(to_query_error)?)?,
        observation_parser_version: row.try_get(6).map_err(to_query_error)?,
        pricing_policy_version: row.try_get(9).map_err(to_query_error)?,
        cohort_version: row.try_get(10).map_err(to_query_error)?,
        cohort_fallback_level: u8::try_from(row.try_get::<i32, _>(11).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        cohort_sample_size: u64::try_from(row.try_get::<i64, _>(12).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        cohort_snapshot_digest: row.try_get(13).map_err(to_query_error)?,
        analyzed_at: unix_to_datetime(row.try_get(15).map_err(to_query_error)?)?,
        report: serde_json::from_str(&report_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        stale: row.try_get::<i32, _>(17).map_err(to_query_error)? == 1,
        superseded_by_analysis_id: superseded_by.as_deref().map(parse_uuid).transpose()?,
        expires_at: unix_to_datetime(row.try_get(18).map_err(to_query_error)?)?,
        ownership_scope_key: row.try_get(19).map_err(to_query_error)?,
        user_id: parse_optional_uuid(row.try_get(20).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.try_get(21).map_err(to_query_error)?)?,
    })
}

fn decode_queue(row: &PgRow) -> Result<AgentAnalysisQueueRecord, StoreError> {
    let desired_versions_json: String = row.try_get(3).map_err(to_query_error)?;
    let status: String = row.try_get(4).map_err(to_query_error)?;
    let lease_expires_at: Option<i64> = row.try_get(6).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.try_get(13).map_err(to_query_error)?;
    Ok(AgentAnalysisQueueRecord {
        queue_item_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        agent_task_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        reason: row.try_get(2).map_err(to_query_error)?,
        desired_versions: serde_json::from_str::<AgentAnalysisDesiredVersions>(
            &desired_versions_json,
        )
        .map_err(|error| StoreError::Serialization(error.to_string()))?,
        status: AgentAnalysisQueueStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown agent analysis queue status `{status}`"))
        })?,
        lease_owner: row.try_get(5).map_err(to_query_error)?,
        lease_expires_at: lease_expires_at.map(unix_to_datetime).transpose()?,
        attempts: u32::try_from(row.try_get::<i32, _>(7).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        max_attempts: u32::try_from(row.try_get::<i32, _>(8).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        last_error: row.try_get(9).map_err(to_query_error)?,
        available_at: unix_to_datetime(row.try_get(10).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.try_get(11).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(12).map_err(to_query_error)?)?,
        completed_at: completed_at.map(unix_to_datetime).transpose()?,
    })
}

const SESSION_COLUMNS: &str = "agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at";
const TASK_COLUMNS: &str = "agent_task_id, agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at";
const REQUEST_COLUMNS: &str = "agent_task_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success";
const ANALYSIS_COLUMNS: &str = "analysis_id, agent_task_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, report_json, analyzed_at, superseded_by_analysis_id, stale, expires_at, ownership_scope_key, user_id, service_account_id";
const QUEUE_COLUMNS: &str = "queue_item_id, agent_task_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at";

impl PostgresStore {
    async fn query_session_by_natural_key(
        &self,
        ownership_scope_key: &str,
        adapter_namespace: &str,
        normalized_session_id: &str,
    ) -> Result<Option<AgentSessionRecord>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE ownership_scope_key = $1 AND adapter_namespace = $2 AND normalized_session_hash = $3"
        );
        sqlx::query(&sql)
            .bind(ownership_scope_key)
            .bind(adapter_namespace)
            .bind(normalized_session_id)
            .fetch_optional(&self.pool)
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
        let sql = format!("SELECT {TASK_COLUMNS} FROM agent_task_windows WHERE agent_task_id = $1");
        sqlx::query(&sql)
            .bind(agent_task_id.to_string())
            .fetch_optional(&self.pool)
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
        let row = sqlx::query("SELECT observation_set_id, agent_task_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE agent_task_id = $1 ORDER BY source_watermark_at DESC, created_at DESC LIMIT 1")
            .bind(agent_task_id.to_string()).fetch_optional(&self.pool).await.map_err(to_query_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let observation_set_id = parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?;
        let parser_version: String = row.try_get(2).map_err(to_query_error)?;
        let coverage_json: String = row.try_get(4).map_err(to_query_error)?;
        let observation_rows = sqlx::query("SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at, o.facts_json, o.limitation_codes_json, s.parser_version FROM agent_inferred_observations o JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id WHERE o.agent_task_id = $1 ORDER BY o.occurred_at, o.observation_id")
            .bind(agent_task_id.to_string()).fetch_all(&self.pool).await.map_err(to_query_error)?;
        let mut observations = Vec::with_capacity(observation_rows.len());
        for observation_row in observation_rows {
            observations.push(decode_observation(&observation_row)?);
        }
        Ok(Some(AgentObservationSetRecord {
            observation_set_id,
            agent_task_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
            parser_version,
            source_watermark_at: unix_to_datetime(row.try_get(3).map_err(to_query_error)?)?,
            coverage: serde_json::from_str(&coverage_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            created_at: unix_to_datetime(row.try_get(5).map_err(to_query_error)?)?,
            observations,
        }))
    }
    async fn query_observation_set_by_id(
        &self,
        observation_set_id: Uuid,
    ) -> Result<Option<AgentObservationSetRecord>, StoreError> {
        let row = sqlx::query("SELECT observation_set_id, agent_task_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE observation_set_id = $1")
            .bind(observation_set_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let coverage_json: String = row.try_get(4).map_err(to_query_error)?;
        let observation_rows = sqlx::query("SELECT o.observation_id, o.kind, o.source_request_id, o.evidence, o.occurred_at, o.facts_json, o.limitation_codes_json, s.parser_version FROM agent_inferred_observations o JOIN agent_inferred_observation_sets s ON s.observation_set_id = o.observation_set_id WHERE o.observation_set_id = $1 ORDER BY o.occurred_at, o.observation_id")
            .bind(observation_set_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let mut observations = Vec::with_capacity(observation_rows.len());
        for observation_row in observation_rows {
            observations.push(decode_observation(&observation_row)?);
        }
        Ok(Some(AgentObservationSetRecord {
            observation_set_id,
            agent_task_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
            parser_version: row.try_get(2).map_err(to_query_error)?,
            source_watermark_at: unix_to_datetime(row.try_get(3).map_err(to_query_error)?)?,
            coverage: serde_json::from_str(&coverage_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            created_at: unix_to_datetime(row.try_get(5).map_err(to_query_error)?)?,
            observations,
        }))
    }
}

#[async_trait]
impl AgentSessionAnalysisRepository for PostgresStore {
    async fn upsert_agent_session(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<AgentSessionRecord, StoreError> {
        sqlx::query("INSERT INTO agent_sessions (agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) ON CONFLICT DO NOTHING")
            .bind(session.agent_session_id.to_string()).bind(&session.ownership_scope_key).bind(session.api_key_id.to_string()).bind(session.user_id.map(|value| value.to_string())).bind(session.team_id.map(|value| value.to_string())).bind(session.service_account_id.map(|value| value.to_string())).bind(session.actor_user_id.map(|value| value.to_string())).bind(&session.normalized_session_id).bind(&session.adapter_namespace).bind(&session.adapter_version).bind(&session.source_provenance).bind(&session.harness_key).bind(&session.harness_label).bind(session.first_seen_at.unix_timestamp()).bind(session.last_seen_at.unix_timestamp()).bind(session.created_at.unix_timestamp()).bind(session.updated_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
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
        sqlx::query("UPDATE agent_sessions SET first_seen_at = LEAST(first_seen_at, $2), last_seen_at = GREATEST(last_seen_at, $3), updated_at = GREATEST(updated_at, $4) WHERE agent_session_id = $1")
            .bind(session.agent_session_id.to_string())
            .bind(session.first_seen_at.unix_timestamp())
            .bind(session.last_seen_at.unix_timestamp())
            .bind(session.updated_at.unix_timestamp())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
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
            format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE agent_session_id = $1");
        sqlx::query(&sql)
            .bind(agent_session_id.to_string())
            .fetch_optional(&self.pool)
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
            "SELECT {TASK_COLUMNS} FROM agent_task_windows WHERE ownership_scope_key = $1 AND lifecycle = 'open' AND boundary_group_key = $4 AND (($2 IS NULL AND agent_session_id IS NULL AND harness_key = $3) OR agent_session_id = $2) ORDER BY started_at DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(ownership_scope_key)
            .bind(agent_session_id.map(|value| value.to_string()))
            .bind(harness_key)
            .bind(boundary_group_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_task).transpose()
    }

    async fn insert_agent_task_if_absent(
        &self,
        task: &AgentTaskWindowRecord,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "INSERT INTO agent_task_windows (agent_task_id, agent_session_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23) ON CONFLICT DO NOTHING",
        )
        .bind(task.agent_task_id.to_string())
        .bind(task.agent_session_id.map(|value| value.to_string()))
        .bind(&task.ownership_scope_key)
        .bind(task.api_key_id.to_string())
        .bind(task.user_id.map(|value| value.to_string()))
        .bind(task.team_id.map(|value| value.to_string()))
        .bind(task.service_account_id.map(|value| value.to_string()))
        .bind(task.actor_user_id.map(|value| value.to_string()))
        .bind(&task.requested_model_key)
        .bind(&task.operation)
        .bind(&task.caller_class)
        .bind(crate::shared::serialize_json(&task.request_tags)?)
        .bind(&task.harness_key)
        .bind(&task.boundary_group_key)
        .bind(&task.boundary_policy_version)
        .bind(enum_name(task.lifecycle)?)
        .bind(enum_name(task.boundary_confidence)?)
        .bind(task.started_at.unix_timestamp())
        .bind(task.ended_at.map(OffsetDateTime::unix_timestamp))
        .bind(task.input_watermark_at.unix_timestamp())
        .bind(task.finalized_reason.as_deref())
        .bind(task.created_at.unix_timestamp())
        .bind(task.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        if result.rows_affected() > 0 {
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
        let result = sqlx::query("UPDATE agent_task_windows SET lifecycle = $2, boundary_confidence = $3, ended_at = $4, input_watermark_at = GREATEST(input_watermark_at, $5), finalized_reason = $6, updated_at = GREATEST(updated_at, $7) WHERE agent_task_id = $1")
            .bind(task.agent_task_id.to_string()).bind(enum_name(task.lifecycle)?).bind(enum_name(task.boundary_confidence)?).bind(task.ended_at.map(OffsetDateTime::unix_timestamp)).bind(task.input_watermark_at.unix_timestamp()).bind(task.finalized_reason.as_deref()).bind(task.updated_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
        if result.rows_affected() == 0 {
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
        let result = sqlx::query("UPDATE agent_task_windows SET lifecycle = $2, boundary_confidence = $3, ended_at = $4, input_watermark_at = $5, finalized_reason = $6, updated_at = $7 WHERE agent_task_id = $1 AND lifecycle = 'open' AND input_watermark_at = $8")
            .bind(task.agent_task_id.to_string()).bind(enum_name(task.lifecycle)?).bind(enum_name(task.boundary_confidence)?).bind(task.ended_at.map(OffsetDateTime::unix_timestamp)).bind(task.input_watermark_at.unix_timestamp()).bind(task.finalized_reason.as_deref()).bind(task.updated_at.unix_timestamp()).bind(expected_input_watermark_at.unix_timestamp()).execute(&self.pool).await.map_err(to_query_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn append_agent_task_request(
        &self,
        link: &AgentTaskRequestLinkRecord,
    ) -> Result<bool, StoreError> {
        let activity_at = link.completed_at.unwrap_or(link.occurred_at);
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query(
            "SELECT agent_task_id FROM agent_task_windows WHERE agent_task_id = $1 FOR UPDATE",
        )
        .bind(link.agent_task_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(to_query_error)?;
        let result = sqlx::query("INSERT INTO agent_task_window_requests (agent_task_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success) VALUES ($1, $2, $3, $4, (SELECT COALESCE(MAX(ordinal) + 1, 0) FROM agent_task_window_requests WHERE agent_task_id = $1), $5, $6, $7, $8, $9, $10, $11, $12) ON CONFLICT(agent_task_id, request_id) DO NOTHING")
            .bind(link.agent_task_id.to_string()).bind(&link.request_id).bind(link.request_log_id.map(|value| value.to_string())).bind(link.usage_event_id.map(|value| value.to_string())).bind(link.execution_id.as_deref()).bind(link.parent_execution_id.as_deref()).bind(link.normalized_session_id.as_deref()).bind(enum_name(link.correlation_confidence)?).bind(crate::shared::serialize_json(&link.limitation_codes)?).bind(link.occurred_at.unix_timestamp()).bind(link.completed_at.map(OffsetDateTime::unix_timestamp)).bind(link.terminal_success).execute(&mut *transaction).await.map_err(to_query_error)?;
        let inserted = result.rows_affected() > 0;
        if inserted {
            sqlx::query("UPDATE agent_task_windows SET input_watermark_at = GREATEST(input_watermark_at, $2), updated_at = GREATEST(updated_at, $2) WHERE agent_task_id = $1").bind(link.agent_task_id.to_string()).bind(activity_at.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?;
        } else {
            let sql = format!(
                "SELECT {REQUEST_COLUMNS} FROM agent_task_window_requests WHERE agent_task_id = $1 AND request_id = $2"
            );
            let existing = sqlx::query(&sql)
                .bind(link.agent_task_id.to_string())
                .bind(&link.request_id)
                .fetch_optional(&mut *transaction)
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
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let result = sqlx::query("INSERT INTO agent_inferred_observation_sets (observation_set_id, agent_task_id, parser_version, source_watermark_at, coverage_json, created_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT(observation_set_id) DO NOTHING")
            .bind(set.observation_set_id.to_string()).bind(set.agent_task_id.to_string()).bind(&set.parser_version).bind(set.source_watermark_at.unix_timestamp()).bind(crate::shared::serialize_json(&set.coverage)?).bind(set.created_at.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            for observation in &set.observations {
                let observation_result = sqlx::query("INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_task_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT(observation_id) DO NOTHING")
                    .bind(observation.observation_id.to_string()).bind(set.observation_set_id.to_string()).bind(set.agent_task_id.to_string()).bind(enum_name(observation.kind)?).bind(&observation.source_request_id).bind(enum_name(observation.evidence)?).bind(observation.occurred_at.unix_timestamp()).bind(crate::shared::serialize_json(&observation.facts)?).bind(crate::shared::serialize_json(&observation.limitations)?).execute(&mut *transaction).await.map_err(to_query_error)?;
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

    async fn link_request_log_to_agent_task(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE request_logs SET agent_session_id = $2, agent_task_id = $3, agent_analysis_source = $4, agent_analysis_coverage_json = $5 WHERE request_log_id = $1",
        )
        .bind(link.request_log_id.to_string())
        .bind(link.agent_session_id.map(|value| value.to_string()))
        .bind(link.agent_task_id.to_string())
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

    async fn load_agent_task_trace(
        &self,
        agent_task_id: Uuid,
    ) -> Result<Option<AgentTaskTraceRecord>, StoreError> {
        let task_sql =
            format!("SELECT {TASK_COLUMNS} FROM agent_task_windows WHERE agent_task_id = $1");
        let task_row = sqlx::query(&task_sql)
            .bind(agent_task_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        let Some(task_row) = task_row else {
            return Ok(None);
        };
        let task = decode_task(&task_row)?;
        let session = if let Some(session_id) = task.agent_session_id {
            let sql =
                format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE agent_session_id = $1");
            sqlx::query(&sql)
                .bind(session_id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(to_query_error)?
                .as_ref()
                .map(decode_session)
                .transpose()?
        } else {
            None
        };
        let request_sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM agent_task_window_requests WHERE agent_task_id = $1 ORDER BY occurred_at, ordinal"
        );
        let request_rows = sqlx::query(&request_sql)
            .bind(agent_task_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let requests = request_rows
            .iter()
            .map(decode_request_link)
            .collect::<Result<Vec<_>, _>>()?;
        let latest_observation_set = self.load_observation_set(agent_task_id).await?;
        let analysis_sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_task_analyses WHERE agent_task_id = $1 AND stale = 0 ORDER BY analyzed_at DESC LIMIT 1"
        );
        let latest_analysis = sqlx::query(&analysis_sql)
            .bind(agent_task_id.to_string())
            .fetch_optional(&self.pool)
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
        let result = sqlx::query("INSERT INTO agent_task_analyses (analysis_id, agent_task_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, analyzed_at, report_json, stale, superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id) SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22 WHERE EXISTS (SELECT 1 FROM agent_task_windows WHERE agent_task_id = $2 AND input_watermark_at = $5) ON CONFLICT DO NOTHING")
            .bind(analysis.analysis_id.to_string()).bind(analysis.agent_task_id.to_string()).bind(&analysis.report.report_schema_version).bind(&analysis.boundary_policy_version).bind(analysis.input_watermark_at.unix_timestamp()).bind(analysis.observation_set_id.to_string()).bind(&analysis.observation_parser_version).bind(&analysis.report.analyzer_version).bind(&analysis.report.score_policy_version).bind(&analysis.pricing_policy_version).bind(&analysis.cohort_version).bind(i32::from(analysis.cohort_fallback_level)).bind(i64::try_from(analysis.cohort_sample_size).map_err(|error| StoreError::Serialization(error.to_string()))?).bind(&analysis.cohort_snapshot_digest).bind(analysis.analyzed_at.unix_timestamp()).bind(crate::shared::serialize_json(&analysis.report)?).bind(i32::from(analysis.stale)).bind(analysis.superseded_by_analysis_id.map(|value| value.to_string())).bind(analysis.expires_at.unix_timestamp()).bind(&analysis.ownership_scope_key).bind(analysis.user_id.map(|value| value.to_string())).bind(analysis.service_account_id.map(|value| value.to_string())).execute(&self.pool).await.map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            return Ok(true);
        }
        let sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_task_analyses WHERE agent_task_id = $1 AND report_schema_version = $2 AND boundary_policy_version = $3 AND input_watermark_at = $4 AND observation_set_id = $5 AND observation_parser_version = $6 AND analyzer_version = $7 AND score_policy_version = $8 AND pricing_policy_version = $9 AND cohort_version = $10 AND cohort_fallback_level = $11 AND cohort_sample_size = $12 AND cohort_snapshot_digest = $13"
        );
        let existing = sqlx::query(&sql)
            .bind(analysis.agent_task_id.to_string())
            .bind(&analysis.report.report_schema_version)
            .bind(&analysis.boundary_policy_version)
            .bind(analysis.input_watermark_at.unix_timestamp())
            .bind(analysis.observation_set_id.to_string())
            .bind(&analysis.observation_parser_version)
            .bind(&analysis.report.analyzer_version)
            .bind(&analysis.report.score_policy_version)
            .bind(&analysis.pricing_policy_version)
            .bind(&analysis.cohort_version)
            .bind(i32::from(analysis.cohort_fallback_level))
            .bind(
                i64::try_from(analysis.cohort_sample_size)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
            )
            .bind(&analysis.cohort_snapshot_digest)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_analysis)
            .transpose()?;
        if let Some(existing) = existing {
            return if agent_task_analysis_matches(&existing, analysis) {
                Ok(false)
            } else {
                Err(StoreError::Conflict(format!(
                    "agent task analysis for task `{}` conflicts with the existing record",
                    analysis.agent_task_id
                )))
            };
        }
        if sqlx::query("SELECT 1 FROM agent_task_analyses WHERE analysis_id = $1")
            .bind(analysis.analysis_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .is_some()
        {
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
        let where_sql = "($1::text IS NULL OR t.ownership_scope_key = $1) AND ($2::text IS NULL OR t.user_id = $2) AND ($3::text IS NULL OR EXISTS (SELECT 1 FROM team_memberships tm WHERE tm.user_id = t.user_id AND tm.team_id = $3) OR EXISTS (SELECT 1 FROM service_accounts sa WHERE sa.service_account_id = t.service_account_id AND sa.team_id = $3)) AND ($4::text IS NULL OR t.service_account_id = $4) AND ($5::text IS NULL OR t.harness_key = $5) AND ($6::text IS NULL OR t.lifecycle = $6) AND ($7::bigint IS NULL OR t.started_at >= $7) AND ($8::bigint IS NULL OR t.started_at < $8) AND ($9::text IS NULL OR (latest_analysis.report_json::jsonb ->> 'confidence') = $9) AND ($10::text IS NULL OR t.agent_session_id = $10) AND ($11::text IS NULL OR t.requested_model_key = $11) AND ($12::text IS NULL OR t.operation = $12) AND ($13::text IS NULL OR t.caller_class = $13) AND ($14::text IS NULL OR (latest_analysis.report_json::jsonb ->> 'gateway_outcome') = $14) AND ($15::text IS NULL OR (latest_analysis.report_json::jsonb ->> 'maturity') = $15) AND ($16::smallint IS NULL OR (latest_analysis.report_json::jsonb #>> '{coverage,overall_percent}')::smallint >= $16) AND ($17::text IS NULL OR s.normalized_session_hash = $17) AND (($18::text IS NULL AND $19::text IS NULL) OR ($18::text IS NOT NULL AND EXISTS (SELECT 1 FROM jsonb_each_text(t.request_tags_json::jsonb) AS tag(key, value) WHERE tag.key = $18 AND ($19::text IS NULL OR tag.value = $19))))";
        let count_sql = format!("SELECT COUNT(*) FROM {from_sql} WHERE {where_sql}");
        let count_row = sqlx::query(&count_sql)
            .bind(query.ownership_scope_key.as_deref())
            .bind(query.user_id.map(|value| value.to_string()))
            .bind(query.team_id.map(|value| value.to_string()))
            .bind(query.service_account_id.map(|value| value.to_string()))
            .bind(query.harness_key.as_deref())
            .bind(lifecycle.as_deref())
            .bind(query.started_after.map(OffsetDateTime::unix_timestamp))
            .bind(query.started_before.map(OffsetDateTime::unix_timestamp))
            .bind(score_confidence.as_deref())
            .bind(query.agent_session_id.map(|value| value.to_string()))
            .bind(query.requested_model_key.as_deref())
            .bind(query.operation.as_deref())
            .bind(query.caller_class.as_deref())
            .bind(gateway_outcome.as_deref())
            .bind(score_maturity.as_deref())
            .bind(query.minimum_coverage_percent.map(i16::from))
            .bind(query.normalized_session_id.as_deref())
            .bind(query.request_tag_key.as_deref())
            .bind(query.request_tag_value.as_deref())
            .fetch_one(&self.pool)
            .await
            .map_err(to_query_error)?;
        let total = u64::try_from(count_row.try_get::<i64, _>(0).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?;
        let list_sql = format!(
            "SELECT t.agent_task_id FROM {from_sql} WHERE {where_sql} ORDER BY t.started_at DESC, t.agent_task_id LIMIT $20 OFFSET $21"
        );
        let rows = sqlx::query(&list_sql)
            .bind(query.ownership_scope_key.as_deref())
            .bind(query.user_id.map(|value| value.to_string()))
            .bind(query.team_id.map(|value| value.to_string()))
            .bind(query.service_account_id.map(|value| value.to_string()))
            .bind(query.harness_key.as_deref())
            .bind(lifecycle.as_deref())
            .bind(query.started_after.map(OffsetDateTime::unix_timestamp))
            .bind(query.started_before.map(OffsetDateTime::unix_timestamp))
            .bind(score_confidence.as_deref())
            .bind(query.agent_session_id.map(|value| value.to_string()))
            .bind(query.requested_model_key.as_deref())
            .bind(query.operation.as_deref())
            .bind(query.caller_class.as_deref())
            .bind(gateway_outcome.as_deref())
            .bind(score_maturity.as_deref())
            .bind(query.minimum_coverage_percent.map(i16::from))
            .bind(query.normalized_session_id.as_deref())
            .bind(query.request_tag_key.as_deref())
            .bind(query.request_tag_value.as_deref())
            .bind(i64::from(page_size))
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?;
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
        sqlx::query("UPDATE agent_task_analyses SET stale = 1, superseded_by_analysis_id = $2 WHERE agent_task_id = $1 AND stale = 0 AND ($2 IS NULL OR analysis_id <> $2)").bind(agent_task_id.to_string()).bind(superseded_by.map(|value| value.to_string())).execute(&self.pool).await.map(|result| result.rows_affected()).map_err(to_query_error)
    }

    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query("INSERT INTO agent_analysis_recompute_queue (queue_item_id, agent_task_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) ON CONFLICT(queue_item_id) DO NOTHING")
            .bind(item.queue_item_id.to_string()).bind(item.agent_task_id.to_string()).bind(&item.reason).bind(crate::shared::serialize_json(&item.desired_versions)?).bind(item.status.as_str()).bind(item.lease_owner.as_deref()).bind(item.lease_expires_at.map(OffsetDateTime::unix_timestamp)).bind(i32::try_from(item.attempts).map_err(|error| StoreError::Serialization(error.to_string()))?).bind(i32::try_from(item.max_attempts).map_err(|error| StoreError::Serialization(error.to_string()))?).bind(item.last_error.as_deref()).bind(item.available_at.unix_timestamp()).bind(item.created_at.unix_timestamp()).bind(item.updated_at.unix_timestamp()).bind(item.completed_at.map(OffsetDateTime::unix_timestamp)).execute(&self.pool).await.map_err(to_query_error)?;
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
        let row = sqlx::query("SELECT queue_item_id FROM agent_analysis_recompute_queue WHERE ((status = 'pending' AND available_at <= $1) OR (status = 'leased' AND lease_expires_at <= $1)) AND attempts < max_attempts ORDER BY available_at, created_at FOR UPDATE SKIP LOCKED LIMIT 1").bind(now.unix_timestamp()).fetch_optional(&mut *transaction).await.map_err(to_query_error)?;
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
        let reports = sqlx::query("DELETE FROM agent_task_analyses WHERE expires_at < $1")
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
        let observations = sqlx::query("DELETE FROM agent_inferred_observation_sets WHERE agent_task_id IN (SELECT agent_task_id FROM agent_task_windows WHERE lifecycle = 'finalized' AND input_watermark_at < $1)").bind(request_cutoff.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
        let requests = sqlx::query("DELETE FROM agent_task_window_requests WHERE agent_task_id IN (SELECT agent_task_id FROM agent_task_windows WHERE lifecycle = 'finalized' AND input_watermark_at < $1)").bind(request_cutoff.unix_timestamp()).execute(&mut *transaction).await.map_err(to_query_error)?.rows_affected();
        let tasks = sqlx::query("DELETE FROM agent_task_windows WHERE lifecycle = 'finalized' AND input_watermark_at < $1 AND NOT EXISTS (SELECT 1 FROM agent_task_analyses a WHERE a.agent_task_id = agent_task_windows.agent_task_id) AND NOT EXISTS (SELECT 1 FROM agent_analysis_recompute_queue q WHERE q.agent_task_id = agent_task_windows.agent_task_id)")
            .bind(request_cutoff.unix_timestamp())
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .rows_affected();
        let sessions = sqlx::query(
            "DELETE FROM agent_sessions WHERE last_seen_at < $1 AND NOT EXISTS (SELECT 1 FROM agent_task_windows t WHERE t.agent_session_id = agent_sessions.agent_session_id)",
        )
        .bind(request_cutoff.unix_timestamp())
        .execute(&mut *transaction)
        .await
        .map_err(to_query_error)?
        .rows_affected();
        transaction.commit().await.map_err(to_query_error)?;
        Ok(observations
            .saturating_add(requests)
            .saturating_add(tasks)
            .saturating_add(sessions))
    }

    async fn delete_agent_analysis_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let reports = sqlx::query("DELETE FROM agent_task_analyses WHERE ownership_scope_key = $1")
            .bind(ownership_scope_key)
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .rows_affected();
        let tasks = sqlx::query("DELETE FROM agent_task_windows WHERE ownership_scope_key = $1")
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
        transaction.commit().await.map_err(to_query_error)?;
        Ok(reports.saturating_add(tasks).saturating_add(sessions))
    }
}
