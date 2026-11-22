use std::collections::BTreeMap;

use agent_session_analysis::{
    ActivityInterval, AnalysisPolicy, OBSERVATION_PARSER_VERSION, RequestAttemptFact,
    SESSION_BOUNDARY_POLICY_VERSION, SessionRequestFact, SessionTrace, SessionUsageFact,
    ToolInvocationFact, TraceEvidence,
    identity::{
        SessionCorrelationLimitation,
        hashing::{hash_identifier, stable_uuid},
    },
    observations::{
        assign_observation_ids, observation_set_id,
        request::PassiveRequestMetadata,
        response::{ResponseObservationInput, observations_for_response},
        scope_file_identifiers, truncate_nested_facts,
    },
};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisQueueRecord, AgentAnalysisQueueStatus,
    AgentObservationSetRecord, AgentRequestLogLinkRecord, AgentSessionAnalysisRecord,
    AgentSessionAnalysisRepository, AgentSessionListQuery, AgentSessionRecord,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, AgentSessionTraceRecord,
    AuthenticatedApiKey, BudgetRepository, Confidence, GatewayError, IdentityRepository,
    InferredObservation, LimitationCode, MAX_AGENT_SESSION_NESTED_FACTS,
    McpToolInvocationRepository, Money4, RequestLogRepository, RequestTags, SessionLifecycleState,
    StoreError, UsageLedgerRecord, UsagePricingStatus,
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

const MAX_RELIABILITY_EVENTS: usize = 2_048;
const MAX_SESSION_WINDOW_CAS_ATTEMPTS: usize = 8;
const MAX_IDLE_FINALIZATION_PAGES: usize = 100;
use uuid::Uuid;

use crate::{budget_scopes::usage_ownership_scope_key, service::scaled_cost_for_tokens};

pub const PRICING_POLICY_VERSION: &str = "usage-ledger-cache-v1";
pub const SESSION_IDLE_GAP: Duration = Duration::minutes(30);
pub const REPORT_RETENTION: Duration = Duration::days(90);

mod ingestion;
mod report_builder;
mod worker;

pub(crate) use ingestion::{
    PassiveRequestRecord, record_prepared_passive_request, session_boundary_group_key,
};
use report_builder::generate_report;
use worker::ensure_supported_versions;
pub use worker::{
    desired_versions, desired_versions_for_policy, enqueue_analysis,
    enqueue_analysis_with_versions, finalize_idle_sessions, process_next_analysis,
};

const SESSION_SOURCE_ID_NAMESPACE: Uuid = Uuid::from_u128(0xc3fc5f3b_56a6_4d1f_99fe_f8ba6d1cc9e1);
const SESSION_ID_NAMESPACE: Uuid = Uuid::from_u128(0x1674a48a_0679_4983_848a_9f6fb626e40d);
const ANALYSIS_ID_NAMESPACE: Uuid = Uuid::from_u128(0x6e390d51_3f14_4cee_9b85_c0b238fe99a2);
const QUEUE_ID_NAMESPACE: Uuid = Uuid::from_u128(0x08d4bc47_3379_4137_843c_3e63be6d500d);

#[cfg(test)]
mod tests;
