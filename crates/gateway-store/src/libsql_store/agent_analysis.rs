use super::*;
use crate::shared::{
    agent_observation_set_matches, agent_session_analysis_matches, agent_session_identity_matches,
    agent_session_request_matches, agent_session_source_identity_matches, datetime_to_unix_millis,
    parse_uuid, unix_millis_to_datetime, unix_to_datetime,
};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisQueueRecord, AgentAnalysisQueueRepository,
    AgentAnalysisQueueStatus, AgentObservationSetAppendResult, AgentObservationSetRecord,
    AgentRequestLogLinkRecord, AgentSessionAnalysisRecord, AgentSessionListPage,
    AgentSessionListQuery, AgentSessionRecord, AgentSessionReportRepository,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, AgentSessionTraceRecord,
    AgentSessionTraceRepository, Confidence, InferredObservation, MAX_AGENT_SESSION_PAGE_SIZE,
    SessionLifecycleState,
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

fn parse_lifecycle(value: &str) -> Result<SessionLifecycleState, StoreError> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

fn nested_fact_count(set: &AgentObservationSetRecord) -> usize {
    set.observations.iter().fold(0, |total, observation| {
        total
            .saturating_add(observation.facts.supplied_tools.len())
            .saturating_add(observation.facts.supplied_skills.len())
            .saturating_add(observation.facts.file_interactions.len())
    })
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<Uuid>, StoreError> {
    value.as_deref().map(parse_uuid).transpose()
}

fn decode_session_source(row: &libsql::Row) -> Result<AgentSessionSourceRecord, StoreError> {
    Ok(AgentSessionSourceRecord {
        agent_session_source_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
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

fn decode_session(row: &libsql::Row) -> Result<AgentSessionRecord, StoreError> {
    let request_tags_json: String = row.get(11).map_err(to_query_error)?;
    let lifecycle: String = row.get(15).map_err(to_query_error)?;
    let confidence: String = row.get(16).map_err(to_query_error)?;
    let ended_at: Option<i64> = row.get(18).map_err(to_query_error)?;
    Ok(AgentSessionRecord {
        agent_session_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        agent_session_source_id: parse_optional_uuid(row.get(1).map_err(to_query_error)?)?,
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
        input_watermark_at: unix_millis_to_datetime(row.get(19).map_err(to_query_error)?)?,
        finalized_reason: row.get(20).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(21).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(22).map_err(to_query_error)?)?,
    })
}

fn decode_request_link(row: &libsql::Row) -> Result<AgentSessionRequestLinkRecord, StoreError> {
    let confidence: String = row.get(8).map_err(to_query_error)?;
    let limitations_json: String = row.get(9).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.get(11).map_err(to_query_error)?;
    Ok(AgentSessionRequestLinkRecord {
        agent_session_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
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
        occurred_at: unix_millis_to_datetime(row.get(10).map_err(to_query_error)?)?,
        completed_at: completed_at.map(unix_millis_to_datetime).transpose()?,
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

fn decode_analysis(row: &libsql::Row) -> Result<AgentSessionAnalysisRecord, StoreError> {
    let report_json: String = row.get(15).map_err(to_query_error)?;
    let superseded_by: Option<String> = row.get(17).map_err(to_query_error)?;
    Ok(AgentSessionAnalysisRecord {
        analysis_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        agent_session_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
        configuration_version: row.get(23).map_err(to_query_error)?,
        boundary_policy_version: row.get(3).map_err(to_query_error)?,
        input_watermark_at: unix_millis_to_datetime(row.get(4).map_err(to_query_error)?)?,
        observation_set_id: parse_uuid(&row.get::<String>(5).map_err(to_query_error)?)?,
        observation_parser_version: row.get(6).map_err(to_query_error)?,
        pricing_policy_version: row.get(9).map_err(to_query_error)?,
        cohort_version: row.get(10).map_err(to_query_error)?,
        cohort_fallback_level: u8::try_from(row.get::<i64>(11).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        cohort_sample_size: u64::try_from(row.get::<i64>(12).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        cohort_snapshot_digest: row.get(13).map_err(to_query_error)?,
        direct_mcp_snapshot_digest: row.get(14).map_err(to_query_error)?,
        analyzed_at: unix_to_datetime(row.get(16).map_err(to_query_error)?)?,
        report: serde_json::from_str(&report_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        stale: row.get::<i64>(18).map_err(to_query_error)? == 1,
        superseded_by_analysis_id: superseded_by.as_deref().map(parse_uuid).transpose()?,
        expires_at: unix_to_datetime(row.get(19).map_err(to_query_error)?)?,
        ownership_scope_key: row.get(20).map_err(to_query_error)?,
        user_id: parse_optional_uuid(row.get(21).map_err(to_query_error)?)?,
        service_account_id: parse_optional_uuid(row.get(22).map_err(to_query_error)?)?,
    })
}

fn decode_queue(row: &libsql::Row) -> Result<AgentAnalysisQueueRecord, StoreError> {
    let desired_versions_json: String = row.get(3).map_err(to_query_error)?;
    let status: String = row.get(4).map_err(to_query_error)?;
    let lease_expires_at: Option<i64> = row.get(6).map_err(to_query_error)?;
    let completed_at: Option<i64> = row.get(13).map_err(to_query_error)?;
    Ok(AgentAnalysisQueueRecord {
        queue_item_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        agent_session_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
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

const SESSION_SOURCE_COLUMNS: &str = "agent_session_source_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, normalized_session_hash, adapter_namespace, adapter_version, source_provenance, harness_key, harness_label, first_seen_at, last_seen_at, created_at, updated_at";
const SESSION_COLUMNS: &str = "agent_session_id, agent_session_source_id, ownership_scope_key, api_key_id, user_id, team_id, service_account_id, actor_user_id, requested_model_key, operation, caller_class, request_tags_json, harness_key, boundary_group_key, boundary_policy_version, lifecycle, boundary_confidence, started_at, ended_at, input_watermark_at, finalized_reason, created_at, updated_at";
const REQUEST_COLUMNS: &str = "agent_session_id, request_id, request_log_id, usage_event_id, ordinal, execution_id, parent_execution_id, normalized_session_id, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success";
const ANALYSIS_COLUMNS: &str = "analysis_id, agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, direct_mcp_snapshot_digest, report_json, analyzed_at, superseded_by_analysis_id, stale, expires_at, ownership_scope_key, user_id, service_account_id, configuration_version";
const QUEUE_COLUMNS: &str = "queue_item_id, agent_session_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, last_error, available_at, created_at, updated_at, completed_at";

impl LibsqlStore {
    async fn query_session_source_by_natural_key(
        &self,
        ownership_scope_key: &str,
        adapter_namespace: &str,
        normalized_session_id: &str,
    ) -> Result<Option<AgentSessionSourceRecord>, StoreError> {
        let sql = format!(
            "SELECT {SESSION_SOURCE_COLUMNS} FROM agent_session_sources WHERE ownership_scope_key = ?1 AND adapter_namespace = ?2 AND normalized_session_hash = ?3"
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
            .map(decode_session_source)
            .transpose()
    }

    async fn query_session_by_id(
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

    async fn load_observation_set(
        &self,
        agent_session_id: Uuid,
    ) -> Result<Option<AgentObservationSetRecord>, StoreError> {
        let mut rows = self.connection.query(
            "SELECT observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE agent_session_id = ?1 ORDER BY source_watermark_at DESC, created_at DESC LIMIT 1",
            [agent_session_id.to_string()],
        ).await.map_err(to_query_error)?;
        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };
        let observation_set_id = parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?;
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
            agent_session_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
            parser_version: row.get(2).map_err(to_query_error)?,
            source_watermark_at: unix_millis_to_datetime(row.get(3).map_err(to_query_error)?)?,
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
            "SELECT observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at FROM agent_inferred_observation_sets WHERE observation_set_id = ?1",
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
            agent_session_id: parse_uuid(&row.get::<String>(1).map_err(to_query_error)?)?,
            parser_version: row.get(2).map_err(to_query_error)?,
            source_watermark_at: unix_millis_to_datetime(row.get(3).map_err(to_query_error)?)?,
            coverage: serde_json::from_str(&coverage_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            created_at: unix_to_datetime(row.get(5).map_err(to_query_error)?)?,
            observations,
        }))
    }
}

mod queue;
mod reports;
mod sessions;
