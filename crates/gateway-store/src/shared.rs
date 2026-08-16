use gateway_core::{
    AgentObservationSetRecord, AgentSessionAnalysisRecord, AgentSessionRecord,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, StoreError,
};
use serde::Serialize;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) fn parse_uuid(raw: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn unix_to_datetime(ts: i64) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::from_unix_timestamp(ts)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn datetime_to_unix_millis(value: OffsetDateTime) -> Result<i64, StoreError> {
    i64::try_from(value.unix_timestamp_nanos() / 1_000_000)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn unix_millis_to_datetime(ts: i64) -> Result<OffsetDateTime, StoreError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ts) * 1_000_000)
        .map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn json_object_from_str(value: &str) -> Result<Map<String, Value>, StoreError> {
    serde_json::from_str(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn serialize_json<T>(value: &T) -> Result<String, StoreError>
where
    T: ?Sized + Serialize,
{
    serde_json::to_string(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn serialize_optional_json<T>(value: Option<&T>) -> Result<Option<String>, StoreError>
where
    T: ?Sized + Serialize,
{
    value.map(serialize_json).transpose()
}

fn same_timestamp(left: OffsetDateTime, right: OffsetDateTime) -> bool {
    left.unix_timestamp() == right.unix_timestamp()
}

fn same_timestamp_millis(left: OffsetDateTime, right: OffsetDateTime) -> bool {
    left.unix_timestamp_nanos() / 1_000_000 == right.unix_timestamp_nanos() / 1_000_000
}

fn same_optional_timestamp_millis(
    left: Option<OffsetDateTime>,
    right: Option<OffsetDateTime>,
) -> bool {
    left.map(|value| value.unix_timestamp_nanos() / 1_000_000)
        == right.map(|value| value.unix_timestamp_nanos() / 1_000_000)
}

pub(crate) fn agent_session_source_identity_matches(
    stored: &AgentSessionSourceRecord,
    candidate: &AgentSessionSourceRecord,
) -> bool {
    stored.agent_session_source_id == candidate.agent_session_source_id
        && stored.ownership_scope_key == candidate.ownership_scope_key
        && stored.user_id == candidate.user_id
        && stored.team_id == candidate.team_id
        && stored.service_account_id == candidate.service_account_id
        && stored.actor_user_id == candidate.actor_user_id
        && stored.normalized_session_id == candidate.normalized_session_id
        && stored.adapter_namespace == candidate.adapter_namespace
        && stored.harness_key == candidate.harness_key
        && stored.harness_label == candidate.harness_label
}

pub(crate) fn agent_session_identity_matches(
    stored: &AgentSessionRecord,
    candidate: &AgentSessionRecord,
) -> bool {
    stored.agent_session_id == candidate.agent_session_id
        && stored.agent_session_source_id == candidate.agent_session_source_id
        && stored.ownership_scope_key == candidate.ownership_scope_key
        && stored.user_id == candidate.user_id
        && stored.team_id == candidate.team_id
        && stored.service_account_id == candidate.service_account_id
        && stored.actor_user_id == candidate.actor_user_id
        && stored.requested_model_key == candidate.requested_model_key
        && stored.operation == candidate.operation
        && stored.caller_class == candidate.caller_class
        && stored.request_tags == candidate.request_tags
        && stored.harness_key == candidate.harness_key
        && stored.boundary_group_key == candidate.boundary_group_key
        && stored.boundary_policy_version == candidate.boundary_policy_version
        && same_timestamp(stored.started_at, candidate.started_at)
        && same_timestamp(stored.created_at, candidate.created_at)
}

pub(crate) fn agent_observation_set_matches(
    stored: &AgentObservationSetRecord,
    candidate: &AgentObservationSetRecord,
) -> bool {
    stored.observation_set_id == candidate.observation_set_id
        && stored.agent_session_id == candidate.agent_session_id
        && stored.parser_version == candidate.parser_version
        && same_timestamp_millis(stored.source_watermark_at, candidate.source_watermark_at)
        && stored.coverage == candidate.coverage
        && same_timestamp(stored.created_at, candidate.created_at)
        && stored.observations.len() == candidate.observations.len()
        && stored
            .observations
            .iter()
            .zip(&candidate.observations)
            .all(|(stored, candidate)| {
                stored.observation_id == candidate.observation_id
                    && stored.kind == candidate.kind
                    && stored.source_request_id == candidate.source_request_id
                    && stored.parser_version == candidate.parser_version
                    && stored.evidence == candidate.evidence
                    && same_timestamp(stored.occurred_at, candidate.occurred_at)
                    && stored.facts == candidate.facts
                    && stored.limitations == candidate.limitations
            })
}

pub(crate) fn agent_session_analysis_matches(
    stored: &AgentSessionAnalysisRecord,
    candidate: &AgentSessionAnalysisRecord,
) -> bool {
    stored.agent_session_id == candidate.agent_session_id
        && stored.configuration_version == candidate.configuration_version
        && stored.boundary_policy_version == candidate.boundary_policy_version
        && same_timestamp_millis(stored.input_watermark_at, candidate.input_watermark_at)
        && stored.observation_set_id == candidate.observation_set_id
        && stored.observation_parser_version == candidate.observation_parser_version
        && stored.pricing_policy_version == candidate.pricing_policy_version
        && stored.cohort_version == candidate.cohort_version
        && stored.cohort_fallback_level == candidate.cohort_fallback_level
        && stored.cohort_sample_size == candidate.cohort_sample_size
        && stored.cohort_snapshot_digest == candidate.cohort_snapshot_digest
        && stored.direct_mcp_snapshot_digest == candidate.direct_mcp_snapshot_digest
        && stored.report == candidate.report
        && stored.ownership_scope_key == candidate.ownership_scope_key
        && stored.user_id == candidate.user_id
        && stored.service_account_id == candidate.service_account_id
}

pub(crate) fn agent_session_request_matches(
    stored: &AgentSessionRequestLinkRecord,
    candidate: &AgentSessionRequestLinkRecord,
) -> bool {
    let AgentSessionRequestLinkRecord {
        agent_session_id: stored_session_id,
        request_id: stored_request_id,
        request_log_id: stored_request_log_id,
        usage_event_id: stored_usage_event_id,
        ordinal: _,
        execution_id: stored_execution_id,
        parent_execution_id: stored_parent_execution_id,
        normalized_session_id: stored_normalized_session_id,
        correlation_confidence: stored_confidence,
        limitation_codes: stored_limitations,
        occurred_at: stored_occurred_at,
        completed_at: stored_completed_at,
        terminal_success: stored_terminal_success,
    } = stored;
    let AgentSessionRequestLinkRecord {
        agent_session_id: candidate_session_id,
        request_id: candidate_request_id,
        request_log_id: candidate_request_log_id,
        usage_event_id: candidate_usage_event_id,
        ordinal: _,
        execution_id: candidate_execution_id,
        parent_execution_id: candidate_parent_execution_id,
        normalized_session_id: candidate_normalized_session_id,
        correlation_confidence: candidate_confidence,
        limitation_codes: candidate_limitations,
        occurred_at: candidate_occurred_at,
        completed_at: candidate_completed_at,
        terminal_success: candidate_terminal_success,
    } = candidate;

    stored_session_id == candidate_session_id
        && stored_request_id == candidate_request_id
        && stored_request_log_id == candidate_request_log_id
        && stored_usage_event_id == candidate_usage_event_id
        && stored_execution_id == candidate_execution_id
        && stored_parent_execution_id == candidate_parent_execution_id
        && stored_normalized_session_id == candidate_normalized_session_id
        && stored_confidence == candidate_confidence
        && stored_limitations == candidate_limitations
        && same_timestamp_millis(*stored_occurred_at, *candidate_occurred_at)
        && same_optional_timestamp_millis(*stored_completed_at, *candidate_completed_at)
        && stored_terminal_success == candidate_terminal_success
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{json_object_from_str, parse_uuid, serialize_json, serialize_optional_json};

    #[test]
    fn serialize_helpers_round_trip_json_values() {
        let payload = json!({"provider": "openai", "timeout_ms": 120000});
        let encoded = serialize_json(&payload).expect("encode");
        let decoded = json_object_from_str(&encoded).expect("decode");

        assert_eq!(decoded.get("provider"), Some(&json!("openai")));
        assert_eq!(decoded.get("timeout_ms"), Some(&json!(120000)));
    }

    #[test]
    fn serialize_optional_json_handles_none() {
        assert_eq!(
            serialize_optional_json::<serde_json::Value>(None).expect("encode none"),
            None
        );
    }

    #[test]
    fn parse_uuid_rejects_invalid_values() {
        assert!(parse_uuid("not-a-uuid").is_err());
    }
}
