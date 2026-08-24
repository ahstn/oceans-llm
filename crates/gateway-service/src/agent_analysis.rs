use std::collections::{BTreeMap, BTreeSet};

use agent_session_analysis::{
    ActivityInterval, AnalysisPolicy, BoundedFileInteractionFact, BoundedSkillFact,
    CohortReference, OBSERVATION_PARSER_VERSION, RequestAttemptFact,
    SESSION_BOUNDARY_POLICY_VERSION, SessionRequestFact, SessionTrace, SessionUsageFact,
    ToolInvocationFact, TraceEvidence,
};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisQueueRecord, AgentAnalysisQueueStatus,
    AgentObservationSetRecord, AgentRequestLogLinkRecord, AgentSessionAnalysisRecord,
    AgentSessionAnalysisRepository, AgentSessionListQuery, AgentSessionRecord,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, AgentSessionTraceRecord,
    AuthenticatedApiKey, BoundedObservationFacts, BoundedToolDefinitionFact, BudgetRepository,
    Confidence, EvidenceQuality, GatewayError, GatewayOutcomeState, IdentityRepository,
    InferredObservation, InferredObservationKind, LimitationCode, MAX_AGENT_SESSION_NESTED_FACTS,
    MAX_MCP_TOOL_INVOCATION_PAGE_SIZE, McpToolInvocationQuery, McpToolInvocationRepository, Money4,
    RequestLogRepository, RequestTags, SessionLifecycleState, StoreError, UsageLedgerRecord,
    UsagePricingStatus,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

const MAX_SUPPLIED_TOOL_FACTS: usize = 256;
const MAX_TOOL_NAME_CHARS: usize = 128;
const MAX_SKILL_FACTS: usize = 256;
const MAX_FILE_INTERACTION_FACTS: usize = 256;
const MAX_RELIABILITY_EVENTS: usize = 2_048;
const MAX_SESSION_WINDOW_CAS_ATTEMPTS: usize = 8;
const MAX_IDLE_FINALIZATION_PAGES: usize = 100;
const MAX_DIRECT_MCP_SCAN_PAGES: u32 = 100;
const MAX_COHORT_SCAN_PAGES: u32 = 25;
const MAX_COHORT_SAMPLES_PER_LEVEL: usize = 2_000;
const COHORT_LOOKBACK: Duration = Duration::days(90);
use uuid::Uuid;

use crate::redaction::REDACTED_VALUE;
use crate::{budget_scopes::usage_ownership_scope_key, service::scaled_cost_for_tokens};

pub const COHORT_VERSION: &str = "successful-boundary-group-v2";
pub const PRICING_POLICY_VERSION: &str = "usage-ledger-cache-v1";
pub const SESSION_IDLE_GAP: Duration = Duration::minutes(30);
pub const REPORT_RETENTION: Duration = Duration::days(90);

mod ingestion;
mod report_builder;
mod session_resolution;
mod worker;

pub(crate) use ingestion::{
    PassiveRequestRecord, record_prepared_passive_request, session_boundary_group_key,
};
use report_builder::generate_report;
pub(crate) use session_resolution::{
    PassiveRequestMetadata, SessionCorrelationLimitation, extract_request_metadata,
};
use session_resolution::{hash_identifier, hash_lineage_candidate, stable_uuid};
use worker::ensure_supported_versions;
pub use worker::{
    desired_versions, desired_versions_for_policy, enqueue_analysis,
    enqueue_analysis_with_versions, finalize_idle_sessions, process_next_analysis,
};

#[cfg(test)]
use ingestion::{
    ToolCall, classify_tool_call, collect_tool_calls, response_finish_reasons,
    scope_file_identifiers, tool_inventory_limitations,
};
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

fn tool_inventory_is_estimated(
    supplied_tool_count: Option<u32>,
    retained_tool_count: usize,
) -> bool {
    let retained_tool_count = u32::try_from(retained_tool_count).unwrap_or(u32::MAX);
    supplied_tool_count.is_some_and(|supplied_count| supplied_count > retained_tool_count)
}

#[cfg(test)]
mod tests;
