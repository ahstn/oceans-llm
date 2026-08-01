use std::collections::BTreeMap;

use agent_session_analysis::{
    ActivityInterval, AnalysisPolicy, CohortReference, OBSERVATION_PARSER_VERSION,
    SESSION_BOUNDARY_POLICY_VERSION, SessionRequestFact, SessionTrace, SessionUsageFact,
    TraceEvidence,
};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisQueueRecord, AgentAnalysisQueueStatus,
    AgentObservationSetRecord, AgentRequestLogLinkRecord, AgentSessionAnalysisRecord,
    AgentSessionAnalysisRepository, AgentSessionListQuery, AgentSessionRecord,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, AgentSessionTraceRecord,
    AuthenticatedApiKey, BoundedObservationFacts, BoundedToolDefinitionFact, BudgetRepository,
    Confidence, EvidenceQuality, GatewayError, GatewayOutcomeState, IdentityRepository,
    InferredObservation, InferredObservationKind, LimitationCode,
    MAX_MCP_TOOL_INVOCATION_PAGE_SIZE, McpToolInvocationQuery, McpToolInvocationRepository,
    RequestTags, SessionLifecycleState, UsageLedgerRecord,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

const MAX_SUPPLIED_TOOL_FACTS: usize = 256;
const MAX_TOOL_NAME_CHARS: usize = 128;
use uuid::Uuid;

use crate::redaction::REDACTED_VALUE;
use crate::{NORMALIZED_PRICING_POLICY_VERSION, budget_scopes::usage_ownership_scope_key};

pub const COHORT_VERSION: &str = "successful-boundary-group-v2";
pub const SESSION_IDLE_GAP: Duration = Duration::minutes(30);
pub const REPORT_RETENTION: Duration = Duration::days(90);
const SESSION_SOURCE_ID_NAMESPACE: Uuid = Uuid::from_u128(0xc3fc5f3b_56a6_4d1f_99fe_f8ba6d1cc9e1);
const SESSION_ID_NAMESPACE: Uuid = Uuid::from_u128(0x1674a48a_0679_4983_848a_9f6fb626e40d);
const OBSERVATION_SET_ID_NAMESPACE: Uuid = Uuid::from_u128(0x373f2ed6_0734_4af4_bfb3_ea4ad2b890a4);
const OBSERVATION_ID_NAMESPACE: Uuid = Uuid::from_u128(0xbdfc8775_f822_425d_8d0f_9a553961fc58);
const ANALYSIS_ID_NAMESPACE: Uuid = Uuid::from_u128(0x6e390d51_3f14_4cee_9b85_c0b238fe99a2);
const QUEUE_ID_NAMESPACE: Uuid = Uuid::from_u128(0x08d4bc47_3379_4137_843c_3e63be6d500d);
const MAX_EXTERNAL_IDENTIFIER_BYTES: usize = 256;
const MAX_TURN_METADATA_BYTES: usize = 4_096;
const MAX_INFERRED_TOOL_CALLS: usize = 128;
const MAX_TOOL_CALL_SCAN_DEPTH: usize = 32;
const MAX_TOOL_CALL_SCAN_NODES: usize = 4_096;

#[derive(Debug, Clone, Copy)]
struct HarnessAdapter {
    version: &'static str,
}

fn harness_adapter(harness_key: &str) -> Option<HarnessAdapter> {
    match harness_key {
        "claude_code" => Some(HarnessAdapter {
            version: "claude-code-v1",
        }),
        "codex" => Some(HarnessAdapter {
            version: "codex-v1",
        }),
        "opencode" => Some(HarnessAdapter {
            version: "opencode-v1",
        }),
        "pi" => Some(HarnessAdapter { version: "pi-v1" }),
        "oh_my_pi" => Some(HarnessAdapter {
            version: "oh-my-pi-v1",
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionCorrelationLimitation {
    ConflictingAliases,
    MalformedCandidate,
}

impl SessionCorrelationLimitation {
    fn as_str(self) -> &'static str {
        match self {
            Self::ConflictingAliases => "conflicting_aliases",
            Self::MalformedCandidate => "malformed_candidate",
        }
    }
}

#[derive(Debug)]
enum CandidateObservation {
    Valid { value: String, source: String },
    Invalid,
}

#[derive(Debug, Default)]
struct SessionResolution {
    value: Option<String>,
    source: Option<String>,
    limitation: Option<SessionCorrelationLimitation>,
}

impl SessionResolution {
    fn conflicted() -> Self {
        Self {
            limitation: Some(SessionCorrelationLimitation::ConflictingAliases),
            ..Self::default()
        }
    }
}

#[derive(Debug)]
struct EmbeddedMetadata {
    value: Value,
    source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassiveRequestMetadata {
    pub external_session_id: Option<String>,
    pub session_source: Option<String>,
    pub session_limitation: Option<SessionCorrelationLimitation>,
    pub execution_id: Option<String>,
    pub parent_execution_id: Option<String>,
    pub message_count: Option<u32>,
    pub prompt_bytes: Option<u64>,
    pub supplied_tool_count: Option<u32>,
    pub tool_schema_bytes: Option<u64>,
    pub supplied_tools: Vec<BoundedToolDefinitionFact>,
    pub adapter_version: String,
}

pub(crate) fn extract_request_metadata(
    body: &Value,
    headers: &BTreeMap<String, String>,
    inspect_body: bool,
    harness_key: &str,
) -> PassiveRequestMetadata {
    let body = if inspect_body { body } else { &Value::Null };
    let adapter = harness_adapter(harness_key);
    let codex_turn_metadata = if harness_key == "codex" {
        codex_turn_metadata(body, headers)
    } else {
        Vec::new()
    };
    let session = extract_session(body, headers, harness_key, &codex_turn_metadata);
    let (execution_id, parent_execution_id) =
        extract_lineage(body, headers, harness_key, &codex_turn_metadata);
    let message_count = body
        .get("messages")
        .and_then(Value::as_array)
        .or_else(|| body.get("input").and_then(Value::as_array))
        .and_then(|values| u32::try_from(values.len()).ok());
    let prompt_bytes = prompt_bytes(body);
    let supplied_tools = body.get("tools").and_then(Value::as_array);
    let supplied_tool_count = supplied_tools.and_then(|values| u32::try_from(values.len()).ok());
    let tool_schema_bytes = supplied_tools
        .and_then(|values| serde_json::to_vec(values).ok())
        .and_then(|bytes| u64::try_from(bytes.len()).ok());
    let supplied_tools =
        supplied_tools.map_or_else(Vec::new, |tools| bounded_supplied_tools(tools.as_slice()));

    PassiveRequestMetadata {
        external_session_id: session.value,
        session_source: session.source,
        session_limitation: session.limitation,
        execution_id,
        parent_execution_id,
        message_count,
        prompt_bytes,
        supplied_tool_count,
        tool_schema_bytes,
        supplied_tools,
        adapter_version: adapter.map_or_else(
            || "unsupported-v1".to_string(),
            |value| value.version.to_string(),
        ),
    }
}

pub(crate) struct PassiveRequestRecord<'a> {
    pub auth: &'a AuthenticatedApiKey,
    pub request_id: &'a str,
    pub request_log_id: Option<Uuid>,
    pub harness_key: &'a str,
    pub harness_label: &'a str,
    pub metadata: &'a PassiveRequestMetadata,
    pub response_body: Option<&'a Value>,
    pub occurred_at: OffsetDateTime,
    pub completed_at: OffsetDateTime,
    pub terminal_success: Option<bool>,
    pub payload_truncated: bool,
    pub requested_model_key: &'a str,
    pub operation: &'a str,
    pub request_tags: Value,
    pub boundary_group_key: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedPassiveRequestRecord {
    auth: AuthenticatedApiKey,
    request_id: String,
    request_log_id: Option<Uuid>,
    harness_key: String,
    harness_label: String,
    metadata: PassiveRequestMetadata,
    response_payload_available: bool,
    requested_model_key: String,
    operation: String,
    request_tags: Value,
    boundary_group_key: String,
    observations: Vec<InferredObservation>,
    occurred_at: OffsetDateTime,
    completed_at: OffsetDateTime,
    terminal_success: Option<bool>,
    payload_truncated: bool,
}

impl PassiveRequestRecord<'_> {
    pub(crate) fn prepare(&self) -> PreparedPassiveRequestRecord {
        PreparedPassiveRequestRecord {
            auth: self.auth.clone(),
            request_id: self.request_id.to_string(),
            request_log_id: self.request_log_id,
            harness_key: self.harness_key.to_string(),
            harness_label: self.harness_label.to_string(),
            requested_model_key: self.requested_model_key.to_string(),
            operation: self.operation.to_string(),
            request_tags: self.request_tags.clone(),
            metadata: self.metadata.clone(),
            response_payload_available: self.response_body.is_some(),
            observations: observations_for_response(self),
            boundary_group_key: self.boundary_group_key.to_string(),
            occurred_at: self.occurred_at,
            completed_at: self.completed_at,
            terminal_success: self.terminal_success,
            payload_truncated: self.payload_truncated,
        }
    }
}

pub(crate) async fn record_prepared_passive_request<S>(
    store: &S,
    input: PreparedPassiveRequestRecord,
) -> Result<Uuid, GatewayError>
where
    S: AgentSessionAnalysisRepository + BudgetRepository + IdentityRepository + Sync,
{
    let ownership_scope_key = usage_ownership_scope_key(&input.auth)?;
    let analytics_team_id = if let Some(user_id) = input.auth.owner_user_id {
        store
            .get_team_membership_for_user(user_id)
            .await?
            .map(|membership| membership.team_id)
    } else {
        input.auth.owner_team_id
    };
    let normalized_session_id = input.metadata.external_session_id.clone();
    let session_source = if input.metadata.external_session_id.is_some() {
        let now = input.completed_at;
        Some(
            store
                .upsert_agent_session_source(&AgentSessionSourceRecord {
                    agent_session_source_id: stable_uuid(
                        SESSION_SOURCE_ID_NAMESPACE,
                        &json!({
                            "scope": ownership_scope_key,
                            "adapter": input.harness_key,
                            "session": normalized_session_id,
                        })
                        .to_string(),
                    ),
                    ownership_scope_key: ownership_scope_key.clone(),
                    api_key_id: input.auth.id,
                    user_id: input.auth.owner_user_id,
                    team_id: analytics_team_id,
                    service_account_id: input.auth.owner_service_account_id,
                    actor_user_id: None,
                    adapter_namespace: input.harness_key.to_string(),
                    normalized_session_id: normalized_session_id
                        .clone()
                        .expect("session ID exists when a session candidate exists"),
                    adapter_version: input.metadata.adapter_version.clone(),
                    source_provenance: input
                        .metadata
                        .session_source
                        .clone()
                        .unwrap_or_else(|| "unreported".to_string()),
                    harness_key: input.harness_key.to_string(),
                    harness_label: input.harness_label.to_string(),
                    first_seen_at: input.occurred_at,
                    last_seen_at: input.completed_at,
                    created_at: now,
                    updated_at: now,
                })
                .await?,
        )
    } else {
        None
    };
    let session_source_id = session_source
        .as_ref()
        .map(|record| record.agent_session_source_id);
    let mut open_session = store
        .get_open_agent_session(
            &ownership_scope_key,
            session_source_id,
            &input.harness_key,
            &input.boundary_group_key,
        )
        .await?;
    if let Some(session) = open_session.as_ref()
        && input.occurred_at - session.input_watermark_at >= SESSION_IDLE_GAP
    {
        let expected_input_watermark_at = session.input_watermark_at;
        let mut finalized_session = session.clone();
        finalized_session.lifecycle = SessionLifecycleState::Finalized;
        finalized_session.ended_at = Some(expected_input_watermark_at);
        finalized_session.input_watermark_at = input.completed_at;
        finalized_session.finalized_reason = Some("idle_gap".to_string());
        finalized_session.updated_at = input.completed_at;
        if store
            .finalize_agent_session_if_unchanged(&finalized_session, expected_input_watermark_at)
            .await?
        {
            store
                .mark_agent_session_analyses_stale(finalized_session.agent_session_id, None)
                .await?;
            enqueue_analysis(
                store,
                finalized_session.agent_session_id,
                "session_finalized",
                &expected_input_watermark_at
                    .unix_timestamp_nanos()
                    .to_string(),
                input.completed_at,
            )
            .await?;
            open_session = None;
        } else {
            open_session = store
                .get_open_agent_session(
                    &ownership_scope_key,
                    session_source_id,
                    &input.harness_key,
                    &input.boundary_group_key,
                )
                .await?;
        }
    }
    while let Some(existing_session) = open_session.as_ref() {
        if store
            .count_agent_session_requests(existing_session.agent_session_id)
            .await?
            < gateway_core::MAX_AGENT_SESSION_REQUESTS
        {
            break;
        }
        let expected_watermark = existing_session.input_watermark_at;
        let mut finalized = existing_session.clone();
        finalized.lifecycle = SessionLifecycleState::Finalized;
        finalized.ended_at = Some(expected_watermark);
        finalized.finalized_reason = Some("request_limit".to_string());
        finalized.updated_at = input.completed_at;
        if store
            .finalize_agent_session_if_unchanged(&finalized, expected_watermark)
            .await?
        {
            store
                .mark_agent_session_analyses_stale(finalized.agent_session_id, None)
                .await?;
            enqueue_analysis(
                store,
                finalized.agent_session_id,
                "session_finalized",
                &expected_watermark.unix_timestamp_nanos().to_string(),
                input.completed_at,
            )
            .await?;
            open_session = None;
        } else {
            open_session = store
                .get_open_agent_session(
                    &ownership_scope_key,
                    session_source_id,
                    &input.harness_key,
                    &input.boundary_group_key,
                )
                .await?;
        }
    }
    let mut session = if let Some(session) = open_session {
        session
    } else {
        let confidence = if session_source_id.is_some() {
            Confidence::High
        } else {
            Confidence::Medium
        };
        let session = AgentSessionRecord {
            agent_session_id: stable_uuid(
                SESSION_ID_NAMESPACE,
                &json!({
                    "scope": ownership_scope_key,
                    "session": session_source_id,
                    "harness": input.harness_key,
                    "boundary": input.boundary_group_key,
                    "first_request": input.request_id,
                })
                .to_string(),
            ),
            agent_session_source_id: session_source_id,
            ownership_scope_key: ownership_scope_key.clone(),
            api_key_id: input.auth.id,
            user_id: input.auth.owner_user_id,
            team_id: analytics_team_id,
            service_account_id: input.auth.owner_service_account_id,
            actor_user_id: None,
            harness_key: input.harness_key.clone(),
            requested_model_key: input.requested_model_key.clone(),
            operation: input.operation.clone(),
            caller_class: caller_class(&input.auth).to_string(),
            request_tags: input.request_tags.clone(),
            boundary_group_key: input.boundary_group_key.clone(),
            boundary_policy_version: SESSION_BOUNDARY_POLICY_VERSION.to_string(),
            lifecycle: SessionLifecycleState::Open,
            boundary_confidence: confidence,
            started_at: input.occurred_at,
            ended_at: None,
            input_watermark_at: input.completed_at,
            finalized_reason: None,
            created_at: input.completed_at,
            updated_at: input.completed_at,
        };
        if store.insert_agent_session_if_absent(&session).await? {
            session
        } else {
            store
                .get_open_agent_session(
                    &ownership_scope_key,
                    session_source_id,
                    &input.harness_key,
                    &input.boundary_group_key,
                )
                .await?
                .ok_or_else(|| {
                    GatewayError::Internal("open agent session disappeared".to_string())
                })?
        }
    };
    let usage_event_id = store
        .get_usage_ledger_by_request_and_scope(&input.request_id, &ownership_scope_key)
        .await?
        .map(|usage| usage.usage_event_id);
    let terminal_success = if input.terminal_success == Some(false) && usage_event_id.is_some() {
        None
    } else {
        input.terminal_success
    };
    let mut limitations = Vec::new();
    if session_source_id.is_none() {
        limitations.push(LimitationCode::SessionUnobserved);
    }
    if input.metadata.session_limitation.is_some() {
        limitations.push(LimitationCode::RequestIncomplete);
    }
    if usage_event_id.is_none() {
        limitations.push(LimitationCode::UsageUnavailable);
    }
    let execution_id = input.metadata.execution_id.as_deref().map(|candidate| {
        hash_lineage_candidate(&ownership_scope_key, &input.harness_key, candidate)
    });
    let parent_execution_id = input
        .metadata
        .parent_execution_id
        .as_deref()
        .map(|candidate| {
            hash_lineage_candidate(&ownership_scope_key, &input.harness_key, candidate)
        });
    let request_inserted = store
        .append_agent_session_request(&AgentSessionRequestLinkRecord {
            agent_session_id: session.agent_session_id,
            request_id: input.request_id.clone(),
            request_log_id: input.request_log_id,
            usage_event_id,
            ordinal: 0,
            execution_id,
            parent_execution_id,
            normalized_session_id,
            correlation_confidence: session.boundary_confidence,
            limitation_codes: limitations,
            occurred_at: input.occurred_at,
            completed_at: Some(input.completed_at),
            terminal_success,
        })
        .await?;
    if !request_inserted {
        return Ok(session.agent_session_id);
    }
    if input.completed_at > session.input_watermark_at {
        session.input_watermark_at = input.completed_at;
        session.updated_at = input.completed_at;
        store.update_agent_session_window(&session).await?;
    }

    let session_correlation = if input.metadata.external_session_id.is_some() {
        "observed"
    } else {
        input
            .metadata
            .session_limitation
            .map_or("unobserved", SessionCorrelationLimitation::as_str)
    };
    let coverage = json!({
        "request_metadata": true,
        "session_correlation": session_correlation,
        "response_payload": input.response_payload_available,
        "response_payload_truncated": input.payload_truncated,
    });
    if let Some(request_log_id) = input.request_log_id {
        store
            .link_request_log_to_agent_session(&AgentRequestLogLinkRecord {
                request_log_id,
                agent_session_source_id: session_source_id,
                agent_session_id: session.agent_session_id,
                analysis_source: "passive".to_string(),
                coverage: coverage.clone(),
            })
            .await?;
    }

    let mut observations = input.observations;
    for (index, observation) in observations.iter_mut().enumerate() {
        observation.observation_id = stable_uuid(
            OBSERVATION_ID_NAMESPACE,
            &json!({
                "session": session.agent_session_id,
                "request": observation.source_request_id,
                "parser": OBSERVATION_PARSER_VERSION,
                "index": index,
                "kind": observation.kind,
                "facts": observation.facts,
            })
            .to_string(),
        );
    }
    let observation_set = AgentObservationSetRecord {
        observation_set_id: stable_uuid(
            OBSERVATION_SET_ID_NAMESPACE,
            &json!({
                "session": session.agent_session_id,
                "request": input.request_id,
                "parser": OBSERVATION_PARSER_VERSION,
                "watermark": input.completed_at.unix_timestamp_nanos(),
            })
            .to_string(),
        ),
        agent_session_id: session.agent_session_id,
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        source_watermark_at: input.completed_at,
        coverage,
        created_at: input.completed_at,
        observations,
    };
    store.append_agent_observation_set(&observation_set).await?;
    store
        .mark_agent_session_analyses_stale(session.agent_session_id, None)
        .await?;
    enqueue_analysis(
        store,
        session.agent_session_id,
        "new_input",
        &input.request_id,
        input.completed_at,
    )
    .await?;
    Ok(session.agent_session_id)
}

pub async fn enqueue_analysis<S>(
    store: &S,
    agent_session_id: Uuid,
    reason: &str,
    dedupe_key: &str,
    now: OffsetDateTime,
) -> Result<bool, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let desired_versions = desired_versions();
    store
        .enqueue_agent_analysis(&AgentAnalysisQueueRecord {
            queue_item_id: stable_uuid(
                QUEUE_ID_NAMESPACE,
                &json!({
                    "session": agent_session_id,
                    "reason": reason,
                    "versions": desired_versions,
                    "dedupe_key": dedupe_key,
                })
                .to_string(),
            ),
            agent_session_id,
            reason: reason.to_string(),
            desired_versions,
            status: AgentAnalysisQueueStatus::Pending,
            lease_owner: None,
            lease_expires_at: None,
            attempts: 0,
            max_attempts: 5,
            last_error: None,
            available_at: now,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .await
        .map_err(Into::into)
}

pub async fn finalize_idle_sessions<S>(store: &S, now: OffsetDateTime) -> Result<u64, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let cutoff = now - SESSION_IDLE_GAP;
    let mut finalized = 0_u64;
    loop {
        let page = store
            .list_agent_sessions(&AgentSessionListQuery {
                page: 1,
                page_size: gateway_core::MAX_AGENT_SESSION_PAGE_SIZE,
                lifecycle: Some(SessionLifecycleState::Open),
                started_before: Some(cutoff),
                input_watermark_before: Some(cutoff),
                ..Default::default()
            })
            .await?;
        let candidates: Vec<_> = page.items.into_iter().map(|trace| trace.session).collect();
        if candidates.is_empty() {
            break;
        }
        for mut session in candidates {
            let last_activity_at = session.input_watermark_at;
            session.lifecycle = SessionLifecycleState::Finalized;
            session.ended_at = Some(last_activity_at);
            session.input_watermark_at = now;
            session.finalized_reason = Some("idle_gap".to_string());
            session.updated_at = now;
            if !store
                .finalize_agent_session_if_unchanged(&session, last_activity_at)
                .await?
            {
                continue;
            }
            store
                .mark_agent_session_analyses_stale(session.agent_session_id, None)
                .await?;
            enqueue_analysis(
                store,
                session.agent_session_id,
                "session_finalized",
                &last_activity_at.unix_timestamp_nanos().to_string(),
                now,
            )
            .await?;
            finalized = finalized.saturating_add(1);
        }
        if page.total <= u64::from(gateway_core::MAX_AGENT_SESSION_PAGE_SIZE) {
            break;
        }
    }
    Ok(finalized)
}

pub async fn process_next_analysis<S>(
    store: &S,
    lease_owner: &str,
    now: OffsetDateTime,
    report_retention: Duration,
) -> Result<bool, GatewayError>
where
    S: AgentSessionAnalysisRepository + BudgetRepository + McpToolInvocationRepository + Sync,
{
    let Some(queue) = store
        .claim_agent_analysis(lease_owner, now, now + Duration::minutes(1))
        .await?
    else {
        return Ok(false);
    };
    let report = generate_report(
        store,
        queue.agent_session_id,
        &queue.desired_versions,
        now,
        report_retention,
    );
    tokio::pin!(report);
    let lease_interval = std::time::Duration::from_secs(20);
    let mut heartbeat =
        tokio::time::interval_at(tokio::time::Instant::now() + lease_interval, lease_interval);
    let result = loop {
        tokio::select! {
            result = &mut report => break result,
            _ = heartbeat.tick() => {
                let renewed_at = OffsetDateTime::now_utc();
                if !store
                    .renew_agent_analysis_lease(
                        queue.queue_item_id,
                        lease_owner,
                        renewed_at,
                        renewed_at + Duration::minutes(1),
                    )
                    .await?
                {
                    break Err(GatewayError::Internal(
                        "agent analysis lease was lost during report generation".to_string(),
                    ));
                }
            }
        }
    };
    match result {
        Ok(()) => {
            store
                .complete_agent_analysis(
                    queue.queue_item_id,
                    lease_owner,
                    OffsetDateTime::now_utc(),
                )
                .await?;
            Ok(true)
        }
        Err(error) => {
            let retry_at = (queue.attempts < queue.max_attempts)
                .then_some(now + Duration::seconds(i64::from(queue.attempts.max(1)) * 5));
            store
                .fail_agent_analysis(
                    queue.queue_item_id,
                    lease_owner,
                    &error.to_string(),
                    retry_at,
                    now,
                )
                .await?;
            Err(error)
        }
    }
}

async fn generate_report<S>(
    store: &S,
    agent_session_id: Uuid,
    versions: &AgentAnalysisDesiredVersions,
    now: OffsetDateTime,
    report_retention: Duration,
) -> Result<(), GatewayError>
where
    S: AgentSessionAnalysisRepository + BudgetRepository + McpToolInvocationRepository + Sync,
{
    ensure_supported_versions(versions)?;
    let trace = store
        .load_agent_session_trace(agent_session_id)
        .await?
        .ok_or_else(|| GatewayError::Internal("agent session disappeared".to_string()))?;
    if trace.session.boundary_policy_version != versions.boundary_policy_version {
        return Err(GatewayError::Internal(format!(
            "agent session boundary policy `{}` does not match queued version `{}`",
            trace.session.boundary_policy_version, versions.boundary_policy_version
        )));
    }
    if trace.requests.len() > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize {
        return Err(GatewayError::Internal(format!(
            "agent session `{agent_session_id}` exceeds the bounded request limit"
        )));
    }

    let observation_sets = store.load_agent_observation_sets(agent_session_id).await?;
    if observation_sets.is_empty() {
        return Err(GatewayError::Internal(
            "agent session has no observation set".to_string(),
        ));
    }
    if observation_sets.len() > gateway_core::MAX_AGENT_SESSION_REQUESTS as usize {
        return Err(GatewayError::Internal(format!(
            "agent session `{agent_session_id}` exceeds the bounded observation-set limit"
        )));
    }
    if observation_sets
        .iter()
        .any(|set| set.parser_version != versions.observation_parser_version)
    {
        return Err(GatewayError::Internal(
            "agent session contains observations from an unsupported parser version".to_string(),
        ));
    }
    let observation_set = observation_sets
        .last()
        .expect("observation sets are known to be non-empty");
    let observations = observation_sets
        .iter()
        .flat_map(|set| set.observations.iter().cloned())
        .collect();
    let request_metadata_count =
        observation_coverage_count(&observation_sets, "request_metadata", trace.requests.len());
    let response_payload_count =
        observation_coverage_count(&observation_sets, "response_payload", trace.requests.len());
    let truncated_payload_count =
        observation_coverage_count(&observation_sets, "response_payload_truncated", usize::MAX);

    let mut requests = Vec::with_capacity(trace.requests.len());
    let mut intervals = Vec::with_capacity(trace.requests.len());
    for link in &trace.requests {
        let usage = store
            .get_usage_ledger_by_request_and_scope(
                &link.request_id,
                &trace.session.ownership_scope_key,
            )
            .await?;
        if let Some(interval) = link
            .completed_at
            .and_then(|completed_at| ActivityInterval::new(link.occurred_at, completed_at))
        {
            intervals.push(interval);
        }
        requests.push(SessionRequestFact {
            request_id: link.request_id.clone(),
            occurred_at: link.occurred_at,
            completed_at: link.completed_at,
            terminal_success: link.terminal_success,
            usage: usage.as_ref().map(session_usage_fact),
        });
    }

    let DirectMcpEvidence {
        intervals: direct_mcp_intervals,
        snapshot_digest: direct_mcp_snapshot_digest,
    } = load_direct_mcp_evidence(store, &trace.session).await?;
    let cohort = load_successful_harness_cohort(store, &trace, versions).await?;
    let report = agent_session_analysis::analyze_session(
        &SessionTrace {
            requests,
            activity_intervals: intervals,
            observations,
            lifecycle: trace.session.lifecycle,
            boundary_confidence: trace.session.boundary_confidence,
            evidence: TraceEvidence {
                session_observed: trace.session.agent_session_source_id.is_some(),
                request_metadata_count,
                response_payload_count,
                truncated_payload_count,
                direct_mcp_intervals,
            },
        },
        &AnalysisPolicy {
            report_schema_version: versions.report_schema_version.clone(),
            analyzer_version: versions.analyzer_version.clone(),
            score_policy_version: versions.score_policy_version.clone(),
            observation_parser_version: versions.observation_parser_version.clone(),
            orchestration_gap: agent_session_analysis::DEFAULT_ORCHESTRATION_GAP,
            maturity: versions.score_maturity,
            calibration_approval_id: versions.calibration_approval_id.clone(),
        },
        cohort.as_ref().map(|value| &value.reference),
    )
    .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let analysis = AgentSessionAnalysisRecord {
        analysis_id: stable_uuid(
            ANALYSIS_ID_NAMESPACE,
            &json!({
                "session": agent_session_id,
                "watermark": trace.session.input_watermark_at.unix_timestamp_nanos(),
                "observation_set": observation_set.observation_set_id,
                "versions": versions,
                "cohort_snapshot": cohort.as_ref().map(|value| &value.snapshot_digest),
                "direct_mcp_snapshot": direct_mcp_snapshot_digest,
            })
            .to_string(),
        ),
        agent_session_id,
        boundary_policy_version: versions.boundary_policy_version.clone(),
        input_watermark_at: trace.session.input_watermark_at,
        observation_set_id: observation_set.observation_set_id,
        observation_parser_version: versions.observation_parser_version.clone(),
        pricing_policy_version: versions.pricing_policy_version.clone(),
        cohort_version: cohort.as_ref().map_or_else(
            || versions.cohort_version.clone(),
            |value| value.reference.cohort_version.clone(),
        ),
        cohort_fallback_level: cohort
            .as_ref()
            .map_or(7, |value| value.reference.fallback_level),
        cohort_sample_size: cohort.as_ref().map_or(0, |value| {
            value.reference.successful_costs_10000.len() as u64
        }),
        cohort_snapshot_digest: cohort.as_ref().map_or_else(
            || hash_identifier("no-cohort"),
            |value| value.snapshot_digest.clone(),
        ),
        analyzed_at: now,
        report,
        stale: false,
        superseded_by_analysis_id: None,
        expires_at: now + report_retention,
        ownership_scope_key: trace.session.ownership_scope_key.clone(),
        user_id: trace.session.user_id,
        service_account_id: trace.session.service_account_id,
    };
    if store.append_agent_session_analysis(&analysis).await? {
        store
            .mark_agent_session_analyses_stale(agent_session_id, Some(analysis.analysis_id))
            .await?;
    }
    Ok(())
}

struct DirectMcpEvidence {
    intervals: Vec<ActivityInterval>,
    snapshot_digest: String,
}

async fn load_direct_mcp_evidence<S>(
    store: &S,
    session: &AgentSessionRecord,
) -> Result<DirectMcpEvidence, GatewayError>
where
    S: McpToolInvocationRepository + Sync,
{
    let page_size = MAX_MCP_TOOL_INVOCATION_PAGE_SIZE;
    let mut page = 1;
    let mut intervals = Vec::new();
    let mut snapshot = Vec::new();
    loop {
        let result = store
            .list_mcp_tool_invocations(&McpToolInvocationQuery {
                page,
                page_size,
                api_key_id: Some(session.api_key_id),
                occurred_at_start: Some(session.started_at),
                occurred_at_end: Some(session.ended_at.unwrap_or(session.input_watermark_at)),
                ..McpToolInvocationQuery::default()
            })
            .await?;
        for invocation in &result.items {
            let latency_ms = invocation.latency_ms.unwrap_or_default().max(0);
            let started_at = invocation.occurred_at - Duration::milliseconds(latency_ms);
            if let Some(interval) = ActivityInterval::new(started_at, invocation.occurred_at) {
                intervals.push(interval);
            }
            snapshot.push((
                invocation.mcp_tool_invocation_id,
                invocation.occurred_at.unix_timestamp_nanos(),
                invocation.latency_ms,
            ));
        }
        if u64::from(page) * u64::from(page_size) >= result.total || result.items.is_empty() {
            break;
        }
        page = page
            .checked_add(1)
            .ok_or_else(|| GatewayError::Internal("MCP invocation page overflow".to_string()))?;
    }
    snapshot.sort_unstable();
    Ok(DirectMcpEvidence {
        intervals,
        snapshot_digest: hash_identifier(
            &serde_json::to_string(&snapshot)
                .map_err(|error| GatewayError::Internal(error.to_string()))?,
        ),
    })
}

fn observation_coverage_count(
    sets: &[AgentObservationSetRecord],
    key: &str,
    maximum: usize,
) -> u32 {
    sets.iter()
        .filter(|set| {
            set.coverage
                .get(key)
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count()
        .min(maximum)
        .try_into()
        .unwrap_or(u32::MAX)
}
fn session_usage_fact(record: &UsageLedgerRecord) -> SessionUsageFact {
    let accounting = record.normalized_usage.as_ref();
    SessionUsageFact {
        fresh_input_tokens: accounting.and_then(|value| value.fresh_input_tokens),
        cache_read_tokens: accounting.and_then(|value| value.cache_read_tokens),
        cache_creation_tokens: accounting.and_then(|value| value.cache_creation_tokens),
        output_tokens: accounting.and_then(|value| value.output_tokens),
        reasoning_tokens: accounting.and_then(|value| value.reasoning_tokens),
        provider_total_tokens: accounting.and_then(|value| value.provider_total_tokens),
        fresh_input_cost_10000: accounting
            .and_then(|value| value.fresh_input_cost_usd)
            .map(|value| value.as_scaled_i64()),
        cache_read_cost_10000: accounting
            .and_then(|value| value.cache_read_cost_usd)
            .map(|value| value.as_scaled_i64()),
        cache_creation_cost_10000: accounting
            .and_then(|value| value.cache_creation_cost_usd)
            .map(|value| value.as_scaled_i64()),
        output_cost_10000: accounting
            .and_then(|value| value.output_cost_usd)
            .map(|value| value.as_scaled_i64()),
        reasoning_cost_10000: accounting
            .and_then(|value| value.reasoning_cost_usd)
            .map(|value| value.as_scaled_i64()),
        legacy_cost_10000: accounting
            .map_or(Some(record.computed_cost_usd), |value| {
                Some(value.legacy_cost_usd)
            })
            .map(|value| value.as_scaled_i64()),
        normalized_cost_10000: accounting
            .and_then(|value| value.normalized_cost_usd)
            .map(|value| value.as_scaled_i64()),
        uncached_input_cost_10000: accounting
            .and_then(|value| value.uncached_input_cost_usd)
            .map(|value| value.as_scaled_i64()),
        provider_key: Some(record.provider_key.clone()),
        upstream_model: Some(record.upstream_model.clone()),
        pricing_policy_version: accounting.map(|value| value.pricing_policy_version.clone()),
    }
}

struct LoadedCohort {
    reference: CohortReference,
    snapshot_digest: String,
}

async fn load_successful_harness_cohort<S>(
    store: &S,
    current: &AgentSessionTraceRecord,
    versions: &AgentAnalysisDesiredVersions,
) -> Result<Option<LoadedCohort>, GatewayError>
where
    S: AgentSessionAnalysisRepository + Sync,
{
    let mut samples: [Vec<(Uuid, i64, i64)>; 4] = std::array::from_fn(|_| Vec::new());
    let mut page_number = 1;
    loop {
        let page = store
            .list_agent_sessions(&AgentSessionListQuery {
                harness_key: Some(current.session.harness_key.clone()),
                lifecycle: Some(SessionLifecycleState::Finalized),
                ownership_scope_key: Some(current.session.ownership_scope_key.clone()),
                input_watermark_before: Some(current.session.input_watermark_at),
                page: page_number,
                page_size: gateway_core::MAX_AGENT_SESSION_PAGE_SIZE,
                ..Default::default()
            })
            .await?;
        let item_count = page.items.len();
        for candidate in page.items {
            if candidate.session.agent_session_id == current.session.agent_session_id {
                continue;
            }
            let Some(analysis) = candidate.latest_analysis else {
                continue;
            };
            if analysis.report.report_schema_version != versions.report_schema_version
                || analysis.boundary_policy_version != versions.boundary_policy_version
                || analysis.observation_parser_version != versions.observation_parser_version
                || analysis.report.analyzer_version != versions.analyzer_version
                || analysis.report.score_policy_version != versions.score_policy_version
                || analysis.pricing_policy_version != versions.pricing_policy_version
                || analysis.report.gateway_outcome != GatewayOutcomeState::Succeeded
            {
                continue;
            }
            let (Some(cost), active_time) = (
                analysis.report.components.actual_cost_10000,
                analysis.report.components.active_time_ms,
            ) else {
                continue;
            };
            let sample = (analysis.analysis_id, cost, active_time);
            samples[3].push(sample);
            if candidate.session.requested_model_key == current.session.requested_model_key {
                samples[2].push(sample);
                if candidate.session.operation == current.session.operation
                    && candidate.session.caller_class == current.session.caller_class
                {
                    samples[1].push(sample);
                    if candidate.session.boundary_group_key == current.session.boundary_group_key {
                        samples[0].push(sample);
                    }
                }
            }
        }
        if item_count < gateway_core::MAX_AGENT_SESSION_PAGE_SIZE as usize {
            break;
        }
        page_number = page_number.saturating_add(1);
    }
    let Some((fallback_level, mut samples)) = samples
        .into_iter()
        .enumerate()
        .find(|(_, samples)| samples.len() >= agent_session_analysis::MIN_EXACT_COHORT_SIZE)
    else {
        return Ok(None);
    };
    samples.sort_unstable_by_key(|sample| sample.0);
    let snapshot_digest = hash_identifier(
        &serde_json::to_string(&samples)
            .map_err(|error| GatewayError::Internal(error.to_string()))?,
    );
    let (costs, active_times): (Vec<_>, Vec<_>) = samples
        .into_iter()
        .map(|(_, cost, active_time)| (cost, active_time))
        .unzip();
    Ok(Some(LoadedCohort {
        reference: CohortReference {
            cohort_version: COHORT_VERSION.to_string(),
            fallback_level: u8::try_from(fallback_level).unwrap_or(3),
            successful_costs_10000: costs,
            successful_active_time_ms: active_times,
        },
        snapshot_digest,
    }))
}

fn ensure_supported_versions(requested: &AgentAnalysisDesiredVersions) -> Result<(), GatewayError> {
    let mut supported_base = desired_versions();
    supported_base.score_maturity = agent_session_analysis::ScoreMaturity::Experimental;
    supported_base.calibration_approval_id = None;
    let mut requested_base = requested.clone();
    requested_base.score_maturity = agent_session_analysis::ScoreMaturity::Experimental;
    requested_base.calibration_approval_id = None;
    if requested_base != supported_base {
        return Err(GatewayError::Internal(format!(
            "unsupported agent analysis version tuple: requested {requested:?}, supported {supported_base:?}"
        )));
    }
    if requested.score_maturity == agent_session_analysis::ScoreMaturity::Calibrated
        && requested
            .calibration_approval_id
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err(GatewayError::Internal(
            "calibrated agent analysis requires an approval identity".to_string(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn desired_versions() -> AgentAnalysisDesiredVersions {
    let calibrated_score_enabled = std::env::var("AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
    let calibration_approval_id = std::env::var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| calibrated_score_enabled && !value.is_empty());
    let score_maturity = if calibration_approval_id.is_some() {
        agent_session_analysis::ScoreMaturity::Calibrated
    } else {
        agent_session_analysis::ScoreMaturity::Experimental
    };
    AgentAnalysisDesiredVersions {
        report_schema_version: agent_session_analysis::REPORT_SCHEMA_VERSION.to_string(),
        boundary_policy_version: SESSION_BOUNDARY_POLICY_VERSION.to_string(),
        observation_parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        analyzer_version: agent_session_analysis::ANALYZER_VERSION.to_string(),
        score_policy_version: agent_session_analysis::SCORE_POLICY_VERSION.to_string(),
        pricing_policy_version: NORMALIZED_PRICING_POLICY_VERSION.to_string(),
        cohort_version: COHORT_VERSION.to_string(),
        score_maturity,
        calibration_approval_id,
    }
}

fn observations_for_response(input: &PassiveRequestRecord<'_>) -> Vec<InferredObservation> {
    let mut observations = Vec::new();
    if input.metadata.message_count.is_some()
        || input.metadata.prompt_bytes.is_some()
        || input.metadata.supplied_tool_count.is_some()
    {
        observations.push(InferredObservation {
            observation_id: Uuid::nil(),
            kind: InferredObservationKind::ToolCallClassified,
            source_request_id: input.request_id.to_string(),
            parser_version: OBSERVATION_PARSER_VERSION.to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: input.occurred_at,
            facts: BoundedObservationFacts {
                message_count: input.metadata.message_count,
                prompt_bytes: input.metadata.prompt_bytes,
                supplied_tool_count: input.metadata.supplied_tool_count,
                tool_schema_bytes: input.metadata.tool_schema_bytes,
                supplied_tools: input.metadata.supplied_tools.clone(),
                ..Default::default()
            },
            limitations: vec![LimitationCode::ToolInventoryPotentialOnly],
        });
    }
    if let Some(response) = input.response_body {
        let mut calls = Vec::new();
        let scan_truncated = collect_tool_calls(response, &mut calls);
        let first_observation = observations.len();
        observations.extend(
            calls
                .into_iter()
                .map(|call| classify_tool_call(input, call)),
        );
        if scan_truncated && let Some(observation) = observations.get_mut(first_observation) {
            observation
                .limitations
                .push(LimitationCode::PayloadTruncated);
        }
    }
    observations
}

struct ToolCall<'a> {
    name: &'a str,
    arguments: Option<&'a str>,
}

fn collect_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) -> bool {
    let mut remaining_nodes = MAX_TOOL_CALL_SCAN_NODES;
    collect_tool_calls_bounded(value, calls, 0, &mut remaining_nodes)
}

fn collect_tool_calls_bounded<'a>(
    value: &'a Value,
    calls: &mut Vec<ToolCall<'a>>,
    depth: usize,
    remaining_nodes: &mut usize,
) -> bool {
    if calls.len() >= MAX_INFERRED_TOOL_CALLS
        || depth > MAX_TOOL_CALL_SCAN_DEPTH
        || *remaining_nodes == 0
    {
        return true;
    }
    *remaining_nodes -= 1;
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| collect_tool_calls_bounded(value, calls, depth + 1, remaining_nodes)),
        Value::Object(object) => {
            if let Some(function) = object.get("function").and_then(Value::as_object)
                && let Some(name) = function.get("name").and_then(Value::as_str)
            {
                calls.push(ToolCall {
                    name,
                    arguments: function.get("arguments").and_then(Value::as_str),
                });
            } else if matches!(
                object.get("type").and_then(Value::as_str),
                Some("function_call")
            ) && let Some(name) = object.get("name").and_then(Value::as_str)
            {
                calls.push(ToolCall {
                    name,
                    arguments: object.get("arguments").and_then(Value::as_str),
                });
            }
            object
                .values()
                .any(|value| collect_tool_calls_bounded(value, calls, depth + 1, remaining_nodes))
        }
        _ => false,
    }
}

fn classify_tool_call(input: &PassiveRequestRecord<'_>, call: ToolCall<'_>) -> InferredObservation {
    let normalized = call.name.to_ascii_lowercase();
    let kind = if normalized.contains("search") || normalized.contains("grep") {
        InferredObservationKind::FileSearchSuspected
    } else if normalized.contains("read") {
        InferredObservationKind::FileReadSuspected
    } else if normalized.contains("overwrite") {
        InferredObservationKind::FileOverwriteSuspected
    } else if normalized.contains("edit") || normalized.contains("patch") {
        InferredObservationKind::FileEditSuspected
    } else if normalized.contains("create") || normalized.contains("write") {
        InferredObservationKind::FileCreateSuspected
    } else if normalized.contains("test")
        || normalized.contains("check")
        || normalized.contains("lint")
        || normalized.contains("build")
    {
        InferredObservationKind::VerificationResultClassified
    } else {
        InferredObservationKind::ToolCallClassified
    };
    // Tool arguments may contain sensitive paths. Classify the operation but
    // never persist a deterministic path-derived identifier.
    let _arguments = call.arguments;
    InferredObservation {
        observation_id: Uuid::nil(),
        kind,
        source_request_id: input.request_id.to_string(),
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        evidence: EvidenceQuality::InferredHigh,
        occurred_at: input.completed_at,
        facts: BoundedObservationFacts {
            tool_name: Some(call.name.chars().take(128).collect()),
            ..Default::default()
        },
        limitations: vec![LimitationCode::SemanticVerificationUnavailable],
    }
}

fn caller_class(auth: &AuthenticatedApiKey) -> &'static str {
    if auth.owner_service_account_id.is_some() {
        "service_account"
    } else if auth.owner_user_id.is_some() {
        "user"
    } else {
        "api_key"
    }
}

pub(crate) fn session_boundary_group_key(tags: &RequestTags) -> String {
    let mut bespoke = tags
        .bespoke
        .iter()
        .map(|tag| (tag.key.as_str(), tag.value.as_str()))
        .collect::<Vec<_>>();
    bespoke.sort_unstable();
    hash_identifier(
        &json!({
            "service": tags.service,
            "component": tags.component,
            "environment": tags.env,
            "bespoke": bespoke,
        })
        .to_string(),
    )
}

fn stable_uuid(namespace: Uuid, canonical: &str) -> Uuid {
    Uuid::new_v5(&namespace, canonical.as_bytes())
}

fn hash_identifier(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("sha256:{digest:x}")
}

fn hash_lineage_candidate(
    ownership_scope_key: &str,
    adapter_namespace: &str,
    value: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ownership_scope_key.as_bytes());
    hasher.update([0]);
    hasher.update(adapter_namespace.as_bytes());
    hasher.update([0]);
    hasher.update(b"lineage-v1");
    hasher.update([0]);
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn normalized_identifier(value: &str, trim_http_ows: bool) -> Option<String> {
    let value = if trim_http_ows {
        value.trim_matches([' ', '\t'])
    } else {
        value
    };
    if value.is_empty()
        || value == REDACTED_VALUE
        || value.len() > MAX_EXTERNAL_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(value.to_string())
}

fn header_observations(
    headers: &BTreeMap<String, String>,
    expected: &str,
) -> Vec<CandidateObservation> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(expected))
        .filter_map(|(_, value)| {
            let trimmed = value.trim_matches([' ', '\t']);
            if trimmed == REDACTED_VALUE {
                return None;
            }
            Some(normalized_identifier(value, true).map_or(
                CandidateObservation::Invalid,
                |value| CandidateObservation::Valid {
                    value,
                    source: format!("header:{expected}"),
                },
            ))
        })
        .collect()
}

fn body_observation(body: &Value, path: &[&str], source: &str) -> Option<CandidateObservation> {
    let mut current = body;
    for segment in path {
        current = current.get(*segment)?;
    }
    let Some(value) = current.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if value == REDACTED_VALUE {
        return None;
    }
    Some(
        normalized_identifier(value, false).map_or(CandidateObservation::Invalid, |value| {
            CandidateObservation::Valid {
                value,
                source: source.to_string(),
            }
        }),
    )
}

fn metadata_observation(metadata: &EmbeddedMetadata, key: &str) -> Option<CandidateObservation> {
    let value = metadata.value.get(key)?;
    let Some(value) = value.as_str() else {
        return Some(CandidateObservation::Invalid);
    };
    if value == REDACTED_VALUE {
        return None;
    }
    Some(
        normalized_identifier(value, false).map_or(CandidateObservation::Invalid, |value| {
            CandidateObservation::Valid {
                value,
                source: format!("{}.{}", metadata.source, key),
            }
        }),
    )
}

fn resolve_session(observations: Vec<CandidateObservation>) -> SessionResolution {
    let mut accepted: Vec<(String, String)> = Vec::new();
    let mut invalid = false;
    for observation in observations {
        match observation {
            CandidateObservation::Valid { value, source } => accepted.push((value, source)),
            CandidateObservation::Invalid => invalid = true,
        }
    }
    if accepted
        .iter()
        .skip(1)
        .any(|(value, _)| value != &accepted[0].0)
    {
        return SessionResolution::conflicted();
    }
    if invalid {
        return SessionResolution {
            limitation: Some(SessionCorrelationLimitation::MalformedCandidate),
            ..SessionResolution::default()
        };
    }
    let Some((value, _)) = accepted.first() else {
        return SessionResolution::default();
    };
    let value = value.clone();
    let mut sources = Vec::new();
    for (_, source) in accepted {
        if !sources.contains(&source) {
            sources.push(source);
        }
    }
    SessionResolution {
        value: Some(value),
        source: Some(sources.join("+")),
        limitation: None,
    }
}

fn extract_session(
    body: &Value,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
    codex_metadata: &[EmbeddedMetadata],
) -> SessionResolution {
    match harness_key {
        "claude_code" => resolve_session(header_observations(headers, "x-claude-code-session-id")),
        "codex" => {
            let mut observations = header_observations(headers, "session-id");
            observations.extend(body_observation(
                body,
                &["client_metadata", "session_id"],
                "body:client_metadata.session_id",
            ));
            observations.extend(
                codex_metadata
                    .iter()
                    .filter_map(|metadata| metadata_observation(metadata, "session_id")),
            );
            resolve_session(observations)
        }
        "opencode" => extract_opencode_session(headers),
        "pi" => extract_pi_session(headers),
        "oh_my_pi" => extract_oh_my_pi_session(body, headers),
        _ => SessionResolution::default(),
    }
}

fn extract_opencode_session(headers: &BTreeMap<String, String>) -> SessionResolution {
    let mut v1 = header_observations(headers, "x-session-id");
    v1.extend(header_observations(headers, "x-session-affinity"));
    let managed = header_observations(headers, "x-opencode-session");
    if !v1.is_empty() && !managed.is_empty() {
        return SessionResolution::conflicted();
    }
    if managed.is_empty() {
        resolve_session(v1)
    } else {
        resolve_session(managed)
    }
}

fn extract_pi_session(headers: &BTreeMap<String, String>) -> SessionResolution {
    let canonical = header_observations(headers, "session_id");
    let corroborating = header_observations(headers, "x-client-request-id");
    if canonical.is_empty() {
        return SessionResolution::default();
    }
    let mut observations = canonical;
    observations.extend(corroborating);
    resolve_session(observations)
}

fn extract_oh_my_pi_session(body: &Value, headers: &BTreeMap<String, String>) -> SessionResolution {
    let mut observations = header_observations(headers, "x-claude-code-session-id");
    observations.extend(header_observations(headers, "session_id"));
    observations.extend(body_observation(body, &["session_id"], "body:session_id"));
    if let Some(user_id) = body
        .get("metadata")
        .and_then(|metadata| metadata.get("user_id"))
    {
        match user_id.as_str().and_then(parse_bounded_json_object) {
            Some(metadata) => observations.extend(
                metadata
                    .get("session_id")
                    .map(|value| {
                        value
                            .as_str()
                            .and_then(|value| normalized_identifier(value, false))
                    })
                    .map(|value| {
                        value.map_or(CandidateObservation::Invalid, |value| {
                            CandidateObservation::Valid {
                                value,
                                source: "body:metadata.user_id.session_id".to_string(),
                            }
                        })
                    }),
            ),
            None if user_id.as_str() == Some(REDACTED_VALUE) => {}
            None => observations.push(CandidateObservation::Invalid),
        }
    }
    resolve_session(observations)
}

fn parse_bounded_json_object(value: &str) -> Option<Value> {
    if value.len() > MAX_TURN_METADATA_BYTES {
        return None;
    }
    let parsed: Value = serde_json::from_str(value).ok()?;
    parsed.is_object().then_some(parsed)
}

fn codex_turn_metadata(body: &Value, headers: &BTreeMap<String, String>) -> Vec<EmbeddedMetadata> {
    let mut result = Vec::new();
    if let Some(value) = body
        .get("client_metadata")
        .and_then(|metadata| metadata.get("x-codex-turn-metadata"))
        .and_then(Value::as_str)
        .and_then(parse_bounded_json_object)
    {
        result.push(EmbeddedMetadata {
            value,
            source: "body:client_metadata.x-codex-turn-metadata".to_string(),
        });
    }
    for (_, raw) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("x-codex-turn-metadata"))
    {
        let raw = raw.trim_matches([' ', '\t']);
        if let Some(value) = parse_bounded_json_object(raw) {
            result.push(EmbeddedMetadata {
                value,
                source: "header:x-codex-turn-metadata".to_string(),
            });
        }
    }
    result
}

fn resolved_lineage_value(observations: Vec<CandidateObservation>) -> Option<String> {
    let resolution = resolve_session(observations);
    if resolution.limitation.is_none() {
        resolution.value
    } else {
        None
    }
}

fn extract_lineage(
    body: &Value,
    headers: &BTreeMap<String, String>,
    harness_key: &str,
    codex_metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    match harness_key {
        "claude_code" => (
            resolved_lineage_value(header_observations(headers, "x-claude-code-agent-id")),
            resolved_lineage_value(header_observations(
                headers,
                "x-claude-code-parent-agent-id",
            )),
        ),
        "opencode" => (
            None,
            resolved_lineage_value(header_observations(headers, "x-parent-session-id")),
        ),
        "codex" => extract_codex_lineage(body, headers, codex_metadata),
        _ => (None, None),
    }
}

fn extract_codex_lineage(
    body: &Value,
    headers: &BTreeMap<String, String>,
    metadata: &[EmbeddedMetadata],
) -> (Option<String>, Option<String>) {
    let mut thread = header_observations(headers, "thread-id");
    thread.extend(header_observations(headers, "x-client-request-id"));
    thread.extend(body_observation(
        body,
        &["client_metadata", "thread_id"],
        "body:client_metadata.thread_id",
    ));
    thread.extend(
        metadata
            .iter()
            .filter_map(|value| metadata_observation(value, "thread_id")),
    );
    let thread = resolve_session(thread);
    let execution_id = if thread.limitation.is_some() {
        None
    } else if thread.value.is_some() {
        thread.value
    } else {
        let mut turn = body_observation(
            body,
            &["client_metadata", "turn_id"],
            "body:client_metadata.turn_id",
        )
        .into_iter()
        .collect::<Vec<_>>();
        turn.extend(
            metadata
                .iter()
                .filter_map(|value| metadata_observation(value, "turn_id")),
        );
        resolved_lineage_value(turn)
    };

    let mut parent = metadata
        .iter()
        .filter_map(|value| metadata_observation(value, "parent_thread_id"))
        .collect::<Vec<_>>();
    if parent.is_empty() {
        parent.extend(
            metadata
                .iter()
                .filter_map(|value| metadata_observation(value, "forked_from_thread_id")),
        );
    }
    (execution_id, resolved_lineage_value(parent))
}

fn prompt_bytes(body: &Value) -> Option<u64> {
    let prompt = body.get("messages").or_else(|| body.get("input"))?;
    serde_json::to_vec(prompt)
        .ok()
        .and_then(|bytes| u64::try_from(bytes.len()).ok())
}

fn bounded_supplied_tools(tools: &[Value]) -> Vec<BoundedToolDefinitionFact> {
    tools
        .iter()
        .take(MAX_SUPPLIED_TOOL_FACTS)
        .filter_map(|tool| {
            let name = tool
                .pointer("/function/name")
                .or_else(|| tool.get("name"))
                .and_then(Value::as_str)?
                .chars()
                .take(MAX_TOOL_NAME_CHARS)
                .collect::<String>();
            if name.is_empty() {
                return None;
            }
            let token_estimate = serde_json::to_vec(tool)
                .ok()
                .and_then(|bytes| u64::try_from(bytes.len()).ok())?
                .div_ceil(4);
            Some(BoundedToolDefinitionFact {
                name,
                token_estimate,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keeps_only_policy_permitted_bounded_dimensions() {
        let request = json!({
            "messages": [{"role": "user", "content": "secret"}],
            "metadata": {"session_id": "unverified-body-session", "execution_id": "turn-1"},
            "tools": [{"type": "function", "function": {"name": "read", "description": "read"}}]
        });
        let headers =
            BTreeMap::from([("X-Session-Id".to_string(), "  header-session\t".to_string())]);
        let metadata = extract_request_metadata(&request, &headers, true, "opencode");
        assert_eq!(
            metadata.external_session_id.as_deref(),
            Some("header-session")
        );
        assert_eq!(
            metadata.session_source.as_deref(),
            Some("header:x-session-id")
        );
        assert_eq!(metadata.session_limitation, None);
        assert_eq!(metadata.adapter_version, "opencode-v1");
        assert_eq!(metadata.execution_id, None);
        assert_eq!(metadata.message_count, Some(1));
        assert_eq!(metadata.supplied_tool_count, Some(1));
        assert_eq!(metadata.supplied_tools.len(), 1);
        assert_eq!(metadata.supplied_tools[0].name, "read");
        assert!(metadata.supplied_tools[0].token_estimate > 0);

        let unavailable = extract_request_metadata(
            &request,
            &BTreeMap::from([("X-Session-Id".to_string(), "header-session".to_string())]),
            false,
            "opencode",
        );
        assert_eq!(
            unavailable.external_session_id.as_deref(),
            Some("header-session")
        );
        assert_eq!(unavailable.message_count, None);
        assert_eq!(unavailable.supplied_tool_count, None);
        assert!(unavailable.supplied_tools.is_empty());
    }
    #[test]
    fn metadata_accepts_direct_and_nested_tool_name_shapes() {
        let request = json!({
            "tools": [
                {"name": "search", "input_schema": {"type": "object"}},
                {"type": "function", "function": {"name": "edit", "parameters": {}}}
            ]
        });

        let metadata = extract_request_metadata(&request, &BTreeMap::new(), true, "opencode");

        assert_eq!(
            metadata
                .supplied_tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["search", "edit"]
        );
        assert!(
            metadata
                .supplied_tools
                .iter()
                .all(|tool| tool.token_estimate > 0)
        );
    }

    #[test]
    fn lineage_candidates_are_hashed_before_persistence() {
        let execution = hash_lineage_candidate("user:a", "codex", "raw-thread");
        let parent = hash_lineage_candidate("user:a", "codex", "raw-thread");
        assert_eq!(execution, parent);
        assert!(execution.starts_with("sha256:"));
        assert!(!execution.contains("raw-thread"));
        assert_ne!(
            execution,
            hash_lineage_candidate("user:b", "codex", "raw-thread")
        );
        assert_ne!(
            execution,
            hash_lineage_candidate("user:a", "claude_code", "raw-thread")
        );
    }

    #[test]
    fn session_boundary_group_is_order_independent() {
        let first = RequestTags {
            service: Some("api".to_string()),
            bespoke: vec![
                gateway_core::RequestTag {
                    key: "region".to_string(),
                    value: "east".to_string(),
                },
                gateway_core::RequestTag {
                    key: "workflow".to_string(),
                    value: "review".to_string(),
                },
            ],
            ..Default::default()
        };
        let mut reordered = first.clone();
        reordered.bespoke.reverse();

        assert_eq!(
            session_boundary_group_key(&first),
            session_boundary_group_key(&reordered)
        );
        let different_tags = RequestTags {
            service: Some("different".to_string()),
            ..first.clone()
        };
        assert_ne!(
            session_boundary_group_key(&first),
            session_boundary_group_key(&different_tags)
        );
    }
    #[test]
    fn metadata_accepts_each_verified_session_alias_with_exact_provenance() {
        let fixtures = vec![
            (
                "claude session header",
                "claude_code",
                json!({}),
                BTreeMap::from([(
                    "X-Claude-Code-Session-Id".to_string(),
                    "known-session".to_string(),
                )]),
                "header:x-claude-code-session-id",
            ),
            (
                "codex session header",
                "codex",
                json!({}),
                BTreeMap::from([("Session-Id".to_string(), "known-session".to_string())]),
                "header:session-id",
            ),
            (
                "codex client metadata",
                "codex",
                json!({"client_metadata": {"session_id": "known-session"}}),
                BTreeMap::new(),
                "body:client_metadata.session_id",
            ),
            (
                "codex turn metadata",
                "codex",
                json!({
                    "client_metadata": {
                        "x-codex-turn-metadata": "{\"session_id\":\"known-session\"}"
                    }
                }),
                BTreeMap::new(),
                "body:client_metadata.x-codex-turn-metadata.session_id",
            ),
            (
                "opencode session id",
                "opencode",
                json!({}),
                BTreeMap::from([("X-Session-Id".to_string(), "known-session".to_string())]),
                "header:x-session-id",
            ),
            (
                "opencode affinity",
                "opencode",
                json!({}),
                BTreeMap::from([(
                    "x-session-affinity".to_string(),
                    "known-session".to_string(),
                )]),
                "header:x-session-affinity",
            ),
            (
                "opencode managed provider",
                "opencode",
                json!({}),
                BTreeMap::from([(
                    "x-opencode-session".to_string(),
                    "known-session".to_string(),
                )]),
                "header:x-opencode-session",
            ),
            (
                "pi session header",
                "pi",
                json!({}),
                BTreeMap::from([("Session_Id".to_string(), "known-session".to_string())]),
                "header:session_id",
            ),
            (
                "omp claude-compatible header",
                "oh_my_pi",
                json!({}),
                BTreeMap::from([(
                    "x-claude-code-session-id".to_string(),
                    "known-session".to_string(),
                )]),
                "header:x-claude-code-session-id",
            ),
            (
                "omp official openai header",
                "oh_my_pi",
                json!({}),
                BTreeMap::from([("session_id".to_string(), "known-session".to_string())]),
                "header:session_id",
            ),
            (
                "omp anthropic metadata",
                "oh_my_pi",
                json!({
                    "metadata": {
                        "user_id": "{\"device_id\":\"device\",\"session_id\":\"known-session\"}"
                    }
                }),
                BTreeMap::new(),
                "body:metadata.user_id.session_id",
            ),
            (
                "omp openrouter body",
                "oh_my_pi",
                json!({"session_id": "known-session"}),
                BTreeMap::new(),
                "body:session_id",
            ),
        ];

        for (name, harness, body, headers, source) in fixtures {
            let metadata = extract_request_metadata(&body, &headers, true, harness);
            assert_eq!(
                metadata.external_session_id.as_deref(),
                Some("known-session"),
                "{name}"
            );
            assert_eq!(metadata.session_source.as_deref(), Some(source), "{name}");
            assert_eq!(metadata.session_limitation, None, "{name}");
            assert!(
                !metadata
                    .session_source
                    .as_deref()
                    .unwrap_or_default()
                    .contains("known-session"),
                "{name}"
            );
        }
    }

    #[test]
    fn matching_equivalent_aliases_preserve_every_source() {
        let opencode = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                ("x-session-id".to_string(), "ses_123".to_string()),
                ("x-session-affinity".to_string(), "ses_123".to_string()),
            ]),
            true,
            "opencode",
        );
        assert_eq!(opencode.external_session_id.as_deref(), Some("ses_123"));
        assert_eq!(
            opencode.session_source.as_deref(),
            Some("header:x-session-id+header:x-session-affinity")
        );

        let pi = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                ("session_id".to_string(), "session-123".to_string()),
                ("x-client-request-id".to_string(), "session-123".to_string()),
            ]),
            true,
            "pi",
        );
        assert_eq!(pi.external_session_id.as_deref(), Some("session-123"));
        assert_eq!(
            pi.session_source.as_deref(),
            Some("header:session_id+header:x-client-request-id")
        );

        let codex = extract_request_metadata(
            &json!({"client_metadata": {"session_id": "session-123"}}),
            &BTreeMap::from([("session-id".to_string(), "session-123".to_string())]),
            true,
            "codex",
        );
        assert_eq!(codex.external_session_id.as_deref(), Some("session-123"));
        assert_eq!(
            codex.session_source.as_deref(),
            Some("header:session-id+body:client_metadata.session_id")
        );
    }

    #[test]
    fn canonical_aliases_take_precedence_over_unrecognized_spoof_fields() {
        let fixtures = [
            (
                "claude",
                "claude_code",
                json!({"metadata": {"session_id": "spoof-body"}}),
                BTreeMap::from([
                    (
                        "x-claude-code-session-id".to_string(),
                        "canonical".to_string(),
                    ),
                    ("session-id".to_string(), "spoof-header".to_string()),
                ]),
                "header:x-claude-code-session-id",
            ),
            (
                "codex",
                "codex",
                json!({"session_id": "spoof-body"}),
                BTreeMap::from([
                    ("session-id".to_string(), "canonical".to_string()),
                    ("session_id".to_string(), "spoof-header".to_string()),
                ]),
                "header:session-id",
            ),
            (
                "opencode",
                "opencode",
                json!({"metadata": {"session_id": "spoof-body"}}),
                BTreeMap::from([
                    ("x-opencode-session".to_string(), "canonical".to_string()),
                    (
                        "x-opencode-session-id".to_string(),
                        "spoof-header".to_string(),
                    ),
                ]),
                "header:x-opencode-session",
            ),
            (
                "pi",
                "pi",
                json!({"session_id": "spoof-body"}),
                BTreeMap::from([
                    ("session_id".to_string(), "canonical".to_string()),
                    ("x-agent-session-id".to_string(), "spoof-header".to_string()),
                ]),
                "header:session_id",
            ),
            (
                "oh my pi",
                "oh_my_pi",
                json!({"metadata": {"session_id": "spoof-body"}}),
                BTreeMap::from([
                    (
                        "x-claude-code-session-id".to_string(),
                        "canonical".to_string(),
                    ),
                    ("session-id".to_string(), "spoof-header".to_string()),
                ]),
                "header:x-claude-code-session-id",
            ),
        ];

        for (name, harness, body, headers, source) in fixtures {
            let metadata = extract_request_metadata(&body, &headers, true, harness);
            assert_eq!(
                metadata.external_session_id.as_deref(),
                Some("canonical"),
                "{name}"
            );
            assert_eq!(metadata.session_source.as_deref(), Some(source), "{name}");
            assert_eq!(metadata.session_limitation, None, "{name}");
        }

        let blocked_body = extract_request_metadata(
            &json!({"session_id": "body-session"}),
            &BTreeMap::new(),
            false,
            "oh_my_pi",
        );
        assert_eq!(blocked_body.external_session_id, None);
        assert_eq!(blocked_body.session_limitation, None);
    }

    #[test]
    fn conflicting_session_aliases_decline_correlation_explicitly() {
        let fixtures = [
            (
                "opencode equivalent aliases",
                "opencode",
                json!({}),
                BTreeMap::from([
                    ("x-session-id".to_string(), "session-a".to_string()),
                    ("x-session-affinity".to_string(), "session-b".to_string()),
                ]),
            ),
            (
                "opencode provider branches",
                "opencode",
                json!({}),
                BTreeMap::from([
                    ("x-session-id".to_string(), "session-a".to_string()),
                    ("x-opencode-session".to_string(), "session-a".to_string()),
                ]),
            ),
            (
                "codex header and body",
                "codex",
                json!({"client_metadata": {"session_id": "session-b"}}),
                BTreeMap::from([("session-id".to_string(), "session-a".to_string())]),
            ),
            (
                "pi failed corroboration",
                "pi",
                json!({}),
                BTreeMap::from([
                    ("session_id".to_string(), "session-a".to_string()),
                    (
                        "x-client-request-id".to_string(),
                        "request-not-session".to_string(),
                    ),
                ]),
            ),
            (
                "omp wire branches",
                "oh_my_pi",
                json!({"session_id": "session-b"}),
                BTreeMap::from([("session_id".to_string(), "session-a".to_string())]),
            ),
        ];

        for (name, harness, body, headers) in fixtures {
            let metadata = extract_request_metadata(&body, &headers, true, harness);
            assert_eq!(metadata.external_session_id, None, "{name}");
            assert_eq!(metadata.session_source, None, "{name}");
            assert_eq!(
                metadata.session_limitation,
                Some(SessionCorrelationLimitation::ConflictingAliases),
                "{name}"
            );
        }
    }

    #[test]
    fn malformed_session_candidates_are_rejected_instead_of_truncated() {
        let oversized = "a".repeat(MAX_EXTERNAL_IDENTIFIER_BYTES + 1);
        let fixtures = [
            (
                "illegal header character",
                "claude_code",
                json!({}),
                BTreeMap::from([(
                    "x-claude-code-session-id".to_string(),
                    "session/../../../other".to_string(),
                )]),
            ),
            (
                "oversized header",
                "codex",
                json!({}),
                BTreeMap::from([("session-id".to_string(), oversized)]),
            ),
            (
                "non-string body",
                "codex",
                json!({"client_metadata": {"session_id": 42}}),
                BTreeMap::new(),
            ),
            (
                "body whitespace",
                "oh_my_pi",
                json!({"session_id": " padded "}),
                BTreeMap::new(),
            ),
            (
                "malformed nested metadata",
                "oh_my_pi",
                json!({"metadata": {"user_id": "not-json"}}),
                BTreeMap::new(),
            ),
        ];

        for (name, harness, body, headers) in fixtures {
            let metadata = extract_request_metadata(&body, &headers, true, harness);
            assert_eq!(metadata.external_session_id, None, "{name}");
            assert_eq!(
                metadata.session_limitation,
                Some(SessionCorrelationLimitation::MalformedCandidate),
                "{name}"
            );
        }
    }

    #[test]
    fn spoofed_or_noncanonical_aliases_never_fabricate_sessions() {
        let fixtures = [
            (
                "historical opencode alias",
                "opencode",
                json!({}),
                BTreeMap::from([(
                    "x-opencode-session-id".to_string(),
                    "fake-session".to_string(),
                )]),
            ),
            (
                "generic pi alias",
                "pi",
                json!({}),
                BTreeMap::from([("session-id".to_string(), "fake-session".to_string())]),
            ),
            (
                "pi request id alone",
                "pi",
                json!({}),
                BTreeMap::from([(
                    "x-client-request-id".to_string(),
                    "fake-session".to_string(),
                )]),
            ),
            (
                "codex prompt cache key",
                "codex",
                json!({"prompt_cache_key": "fake-session"}),
                BTreeMap::new(),
            ),
            (
                "unknown harness",
                "unknown",
                json!({"client_metadata": {"session_id": "fake-session"}}),
                BTreeMap::from([("session-id".to_string(), "fake-session".to_string())]),
            ),
        ];

        for (name, harness, body, headers) in fixtures {
            let metadata = extract_request_metadata(&body, &headers, true, harness);
            assert_eq!(metadata.external_session_id, None, "{name}");
            assert_eq!(metadata.session_source, None, "{name}");
            assert_eq!(metadata.session_limitation, None, "{name}");
        }
    }

    #[test]
    fn verified_lineage_fields_are_bounded_and_adapter_specific() {
        let claude = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                (
                    "x-claude-code-session-id".to_string(),
                    "session-a".to_string(),
                ),
                ("x-claude-code-agent-id".to_string(), "agent-a".to_string()),
                (
                    "x-claude-code-parent-agent-id".to_string(),
                    "agent-parent".to_string(),
                ),
            ]),
            true,
            "claude_code",
        );
        assert_eq!(claude.execution_id.as_deref(), Some("agent-a"));
        assert_eq!(claude.parent_execution_id.as_deref(), Some("agent-parent"));

        let opencode = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                ("x-session-id".to_string(), "session-a".to_string()),
                (
                    "x-parent-session-id".to_string(),
                    "session-parent".to_string(),
                ),
            ]),
            true,
            "opencode",
        );
        assert_eq!(opencode.execution_id, None);
        assert_eq!(
            opencode.parent_execution_id.as_deref(),
            Some("session-parent")
        );

        let codex = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                ("session-id".to_string(), "session-a".to_string()),
                ("thread-id".to_string(), "thread-a".to_string()),
                (
                    "x-client-request-id".to_string(),
                    "thread-a".to_string(),
                ),
                (
                    "x-codex-turn-metadata".to_string(),
                    "{\"session_id\":\"session-a\",\"thread_id\":\"thread-a\",\"turn_id\":\"turn-a\",\"parent_thread_id\":\"thread-parent\"}".to_string(),
                ),
            ]),
            true,
            "codex",
        );
        assert_eq!(codex.execution_id.as_deref(), Some("thread-a"));
        assert_eq!(codex.parent_execution_id.as_deref(), Some("thread-parent"));

        let turn_only = extract_request_metadata(
            &json!({
                "client_metadata": {
                    "session_id": "session-a",
                    "turn_id": "turn-a",
                    "x-codex-turn-metadata": "{\"turn_id\":\"turn-a\"}"
                }
            }),
            &BTreeMap::new(),
            true,
            "codex",
        );
        assert_eq!(turn_only.execution_id.as_deref(), Some("turn-a"));
    }

    #[test]
    fn conflicting_or_malformed_lineage_is_not_persisted_as_execution_evidence() {
        let conflicted = extract_request_metadata(
            &json!({
                "client_metadata": {
                    "session_id": "session-a",
                    "turn_id": "must-not-replace-conflicted-thread"
                }
            }),
            &BTreeMap::from([
                ("thread-id".to_string(), "thread-a".to_string()),
                (
                    "x-client-request-id".to_string(),
                    "different-thread".to_string(),
                ),
            ]),
            true,
            "codex",
        );
        assert_eq!(conflicted.external_session_id.as_deref(), Some("session-a"));
        assert_eq!(conflicted.execution_id, None);

        let malformed = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                (
                    "x-claude-code-session-id".to_string(),
                    "session-a".to_string(),
                ),
                (
                    "x-claude-code-agent-id".to_string(),
                    "not/an/opaque-id".to_string(),
                ),
                (
                    "x-claude-code-parent-agent-id".to_string(),
                    "parent-a".repeat(MAX_EXTERNAL_IDENTIFIER_BYTES),
                ),
            ]),
            true,
            "claude_code",
        );
        assert_eq!(malformed.execution_id, None);
        assert_eq!(malformed.parent_execution_id, None);
    }

    #[test]
    fn tool_classification_omits_file_identity_and_distinguishes_overwrites() {
        let input = PassiveRequestRecord {
            auth: &AuthenticatedApiKey {
                id: Uuid::nil(),
                public_id: String::new(),
                name: String::new(),
                model_grant_mode: gateway_core::ApiKeyModelGrantMode::All,
                owner_kind: gateway_core::ApiKeyOwnerKind::User,
                owner_user_id: Some(Uuid::nil()),
                owner_team_id: None,
                owner_service_account_id: None,
            },
            request_id: "request",
            request_log_id: None,
            harness_key: "test",
            harness_label: "Test",
            metadata: &PassiveRequestMetadata {
                external_session_id: None,
                session_source: None,
                session_limitation: None,
                execution_id: None,
                parent_execution_id: None,
                message_count: None,
                prompt_bytes: None,
                supplied_tool_count: None,
                tool_schema_bytes: None,
                supplied_tools: Vec::new(),
                adapter_version: "unsupported-v1".to_string(),
            },
            response_body: None,
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            completed_at: OffsetDateTime::UNIX_EPOCH,
            terminal_success: Some(true),
            payload_truncated: false,
            requested_model_key: "test-model",
            operation: "chat",
            request_tags: json!({}),
            boundary_group_key: "sha256:test",
        };
        let observation = classify_tool_call(
            &input,
            ToolCall {
                name: "edit_file",
                arguments: Some(r#"{"path":"/private/source.rs"}"#),
            },
        );
        assert_eq!(observation.kind, InferredObservationKind::FileEditSuspected);
        assert!(observation.facts.opaque_file_id.is_none());
        let overwrite = classify_tool_call(
            &input,
            ToolCall {
                name: "overwrite_file",
                arguments: None,
            },
        );
        assert_eq!(
            overwrite.kind,
            InferredObservationKind::FileOverwriteSuspected
        );
    }
    #[test]
    fn tool_call_collection_is_bounded() {
        let response = Value::Array(vec![
            json!({"function": {"name": "read_file", "arguments": "{}"}});
            MAX_INFERRED_TOOL_CALLS + 1
        ]);
        let mut calls = Vec::new();

        assert!(collect_tool_calls(&response, &mut calls));
        assert_eq!(calls.len(), MAX_INFERRED_TOOL_CALLS);
    }
}
