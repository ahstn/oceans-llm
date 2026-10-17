use gateway_core::{
    AgentObservationSetRecord, AgentSessionAnalysisRecord, AgentSessionRecord,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, ModelRoutingPolicy, StoreError,
};
use serde::Serialize;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) fn parse_uuid(raw: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

/// SQL for `count_mcp_tool_grants_outside_team`, with the backend's
/// placeholders for the server and team IDs. Each subject's team follows the
/// same precedence as the service's subject lookup, so both agree on who is
/// inside the team. An API key that is revoked, has no MCP scope, or whose
/// server list leaves this server out holds no access, as at runtime, and nor
/// does a disabled service account or a key it owns.
pub(crate) fn grants_outside_team_sql(server: &str, team: &str) -> String {
    format!(
        r#"
        SELECT COUNT(*) FROM (
            SELECT
                CASE g.subject_kind
                    WHEN 'team' THEN st.team_id
                    WHEN 'user' THEN um.team_id
                    WHEN 'service_account' THEN ssa.team_id
                    WHEN 'api_key' THEN CASE
                        WHEN k.owner_team_id IS NOT NULL THEN k.owner_team_id
                        WHEN k.owner_service_account_id IS NOT NULL THEN ksa.team_id
                        ELSE km.team_id
                    END
                END AS subject_team,
                CASE g.subject_kind
                    WHEN 'team' THEN st.team_id IS NOT NULL
                    WHEN 'user' THEN su.user_id IS NOT NULL
                    WHEN 'service_account' THEN ssa.service_account_id IS NOT NULL AND ssa.status = 'active'
                    WHEN 'api_key' THEN k.id IS NOT NULL AND k.revoked_at IS NULL
                        -- The key may call MCP and this server: NULL lists allow all.
                        AND (k.scopes IS NULL
                            OR (' ' || k.scopes || ' ') LIKE '% mcp:aggregate %'
                            OR (' ' || k.scopes || ' ') LIKE '% mcp:proxy %')
                        AND (k.mcp_server_keys IS NULL
                            OR (' ' || k.mcp_server_keys || ' ')
                                LIKE ('% ' || replace(replace(srv.server_key, '%', '\%'), '_', '\_') || ' %') ESCAPE '\')
                        AND CASE
                        WHEN k.owner_team_id IS NOT NULL THEN TRUE
                        WHEN k.owner_service_account_id IS NOT NULL THEN ksa.service_account_id IS NOT NULL AND ksa.status = 'active'
                        WHEN k.owner_user_id IS NOT NULL THEN ku.user_id IS NOT NULL
                        ELSE TRUE
                    END
                    ELSE FALSE
                END AS subject_exists
            FROM mcp_tool_grants g
            JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id
            JOIN external_mcp_servers srv ON srv.mcp_server_id = t.mcp_server_id
            LEFT JOIN teams st ON g.subject_kind = 'team' AND st.team_id = g.subject_id
            LEFT JOIN users su ON g.subject_kind = 'user' AND su.user_id = g.subject_id
            LEFT JOIN team_memberships um ON g.subject_kind = 'user' AND um.user_id = g.subject_id
            LEFT JOIN service_accounts ssa ON g.subject_kind = 'service_account' AND ssa.service_account_id = g.subject_id
            LEFT JOIN api_keys k ON g.subject_kind = 'api_key' AND k.id = g.subject_id
            LEFT JOIN service_accounts ksa ON ksa.service_account_id = k.owner_service_account_id
            LEFT JOIN users ku ON ku.user_id = k.owner_user_id
            LEFT JOIN team_memberships km ON km.user_id = k.owner_user_id
            WHERE g.target_kind = 'tool' AND g.is_active = 1 AND t.mcp_server_id = {server}
              -- A disabled server exposes nothing, and cannot be enabled again.
              AND srv.status = 'active'
        ) resolved
        WHERE resolved.subject_exists AND (resolved.subject_team IS NULL OR resolved.subject_team <> {team})
        "#
    )
}

/// SQL counting a server's active credentials that may not follow it to a new
/// URL: user-scoped, `secret_ref`, or not the owning team's.
pub(crate) fn foreign_credentials_sql(server: &str, team: &str) -> String {
    format!(
        "SELECT COUNT(*) FROM mcp_upstream_credential_bindings WHERE mcp_server_id = {server} AND revoked_at IS NULL AND (owner_scope_kind = 'user' OR secret_ref IS NOT NULL OR owner_team_id IS NULL OR owner_team_id <> {team})"
    )
}

/// A team admin's retarget refused because `count` such credentials remain.
pub(crate) fn foreign_credentials_conflict(count: i64) -> StoreError {
    StoreError::Conflict(format!(
        "{count} user or secret-reference credential binding(s) would be sent to the new URL; revoke them before changing server_url"
    ))
}

/// SQL for `list_mcp_tool_access_for_api_keys`: `keys` is the backend's
/// predicate on `g.subject_id`, `server` its placeholder for the optional
/// server filter.
pub(crate) fn key_tool_grants_sql(keys: &str, server: &str) -> String {
    format!(
        r#"
        SELECT g.subject_id, t.mcp_tool_id, t.mcp_server_id, s.server_key
        FROM mcp_tool_grants g
        JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id
        JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id
        WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = 'api_key'
          AND g.subject_id {keys} AND t.is_active = 1 AND s.status = 'active'
          AND ({server} IS NULL OR t.mcp_server_id = {server})
        UNION
        SELECT g.subject_id, t.mcp_tool_id, t.mcp_server_id, s.server_key
        FROM mcp_tool_grants g
        JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id
        JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id
        JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id
        JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id
        WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = 'api_key'
          AND g.subject_id {keys} AND ts.status = 'active' AND t.is_active = 1
          AND s.status = 'active' AND ({server} IS NULL OR t.mcp_server_id = {server})
        "#
    )
}

/// SQL for the first active tool set holding one of a server's tools.
pub(crate) fn active_toolset_for_server_sql(server: &str) -> String {
    format!(
        "SELECT ts.toolset_key FROM mcp_toolset_tools tst JOIN mcp_toolsets ts ON ts.toolset_id = tst.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id WHERE t.mcp_server_id = {server} AND ts.status = 'active' ORDER BY ts.toolset_key LIMIT 1"
    )
}

/// A narrowing refused because a tool set holds the server's tools.
pub(crate) fn stranded_toolset(toolset_key: &str) -> StoreError {
    StoreError::Conflict(format!(
        "tool set `{toolset_key}` contains this server's tools; remove them before limiting the server to one team"
    ))
}

/// A narrowing refused because grants are held outside the new owning team.
pub(crate) fn stranded_grants(count: i64) -> StoreError {
    StoreError::Conflict(format!(
        "{count} active grant(s) on this server's tools are held outside the owning team; revoke them before limiting the server to one team"
    ))
}

/// A grant write whose subject changed team since it was authorized.
pub(crate) fn subject_team_changed() -> StoreError {
    StoreError::Conflict(
        "the grant subject moved to another team; reload and try again".to_string(),
    )
}

/// A tool set write that names a tool of a team-only server. Tool sets are
/// granted organisation-wide, so the store refuses it under the same write.
pub(crate) fn team_only_toolset_tool(tool: &str, server_key: &str) -> StoreError {
    StoreError::Conflict(format!(
        "tool `{tool}` belongs to MCP server `{server_key}`, which is visible to its team only; publish the server to the organisation before adding its tools to a tool set"
    ))
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

pub(crate) fn parse_model_routing_policy(
    value: Option<&str>,
) -> Result<Option<ModelRoutingPolicy>, StoreError> {
    value
        .map(|value| {
            let policy: ModelRoutingPolicy = serde_json::from_str(value)
                .map_err(|error| StoreError::Serialization(error.to_string()))?;
            policy.validate().map_err(StoreError::Serialization)?;
            Ok(policy)
        })
        .transpose()
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

/// Whether saving `input` would leave the repository as it is.
pub(crate) fn review_agent_repository_matches(
    existing: &gateway_core::ReviewAgentRepositoryRecord,
    input: &gateway_core::UpdateReviewAgentRepositoryRecord,
) -> bool {
    existing.external_repository_id == input.external_repository_id
        && existing.owner == input.owner
        && existing.name == input.name
        && existing.full_name == input.full_name
        && existing.service_account_id == input.service_account_id
        && existing.status == input.status
        && existing.settings == input.settings
        && existing.settings_json == input.settings_json
}

/// Whether the requested access matches what the key already has, ignoring the
/// order and repeats of the model ids.
pub(crate) fn same_model_access(
    current: &gateway_core::ApiKeyModelAccess,
    model_grant_mode: gateway_core::ApiKeyModelGrantMode,
    model_ids: &[Uuid],
) -> bool {
    let requested: std::collections::BTreeSet<Uuid> = model_ids.iter().copied().collect();
    let current_ids: std::collections::BTreeSet<Uuid> = current.model_ids.iter().copied().collect();
    current.model_grant_mode == model_grant_mode && current_ids == requested
}

/// A revoke can commit after the caller checked the key; checked again under the
/// replacement's lock, a revoked key's access is never rewritten.
pub(crate) fn ensure_active_for_update(status: &str) -> Result<(), StoreError> {
    if status == gateway_core::ApiKeyStatus::Active.as_str() {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "revoked api keys cannot be updated".to_string(),
        ))
    }
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
