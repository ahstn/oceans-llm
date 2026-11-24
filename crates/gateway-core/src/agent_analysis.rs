use agent_session_analysis::{
    AgentSessionId, AgentSessionSourceId, AnalysisId, Confidence, GatewayOutcomeState,
    InferredObservation, LimitationCode, ObservationSetId, ScoreMaturity, SessionEfficiencyReport,
    SessionLifecycleState,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Money9, RequestAttemptRecord, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionSourceRecord {
    pub agent_session_source_id: AgentSessionSourceId,
    pub ownership_scope_key: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub normalized_session_id: String,
    pub adapter_namespace: String,
    pub adapter_version: String,
    pub source_provenance: String,
    pub harness_key: String,
    pub harness_label: String,
    pub first_seen_at: OffsetDateTime,
    pub last_seen_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    pub agent_session_id: AgentSessionId,
    pub agent_session_source_id: Option<AgentSessionSourceId>,
    pub ownership_scope_key: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub harness_key: String,
    pub requested_model_key: String,
    pub operation: String,
    pub caller_class: String,
    pub request_tags: Value,
    pub boundary_group_key: String,
    pub boundary_policy_version: String,
    pub lifecycle: SessionLifecycleState,
    pub boundary_confidence: Confidence,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub input_watermark_at: OffsetDateTime,
    pub finalized_reason: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRequestLinkRecord {
    pub agent_session_id: AgentSessionId,
    pub request_id: String,
    pub request_log_id: Option<Uuid>,
    pub usage_event_id: Option<Uuid>,
    /// Assigned atomically by the repository when appending; decoded records contain the stored value.
    pub ordinal: i64,
    pub execution_id: Option<String>,
    pub parent_execution_id: Option<String>,
    pub normalized_session_id: Option<String>,
    pub correlation_confidence: Confidence,
    pub limitation_codes: Vec<LimitationCode>,
    pub occurred_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub terminal_success: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequestLogLinkRecord {
    pub request_log_id: Uuid,
    pub agent_session_source_id: Option<AgentSessionSourceId>,
    pub agent_session_id: AgentSessionId,
    pub analysis_source: String,
    pub coverage: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentObservationSetRecord {
    pub observation_set_id: ObservationSetId,
    pub agent_session_id: AgentSessionId,
    pub parser_version: String,
    pub source_watermark_at: OffsetDateTime,
    pub coverage: Value,
    pub created_at: OffsetDateTime,
    pub observations: Vec<InferredObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionAnalysisRecord {
    pub analysis_id: AnalysisId,
    pub agent_session_id: AgentSessionId,
    pub configuration_version: String,
    pub boundary_policy_version: String,
    pub input_watermark_at: OffsetDateTime,
    pub input_generation: i64,
    pub observation_set_id: ObservationSetId,
    pub observation_parser_version: String,
    pub pricing_policy_version: String,
    pub cohort_version: String,
    pub cohort_fallback_level: u8,
    pub cohort_sample_size: u64,
    pub cohort_snapshot_digest: String,
    pub direct_mcp_snapshot_digest: String,
    pub analyzed_at: OffsetDateTime,
    pub report: SessionEfficiencyReport,
    pub stale: bool,
    pub superseded_by_analysis_id: Option<AnalysisId>,
    pub expires_at: OffsetDateTime,
    pub ownership_scope_key: String,
    pub user_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionTraceRecord {
    pub session: AgentSessionRecord,
    pub input_generation: i64,
    pub session_source: Option<AgentSessionSourceRecord>,
    pub requests: Vec<AgentSessionRequestLinkRecord>,
    pub latest_observation_set: Option<AgentObservationSetRecord>,
    pub latest_analysis: Option<AgentSessionAnalysisRecord>,
}

/// Listing data deliberately excludes request rows and observation facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionSummaryRecord {
    pub session: AgentSessionRecord,
    pub input_generation: i64,
    pub request_count: u64,
    pub session_source_hash: Option<String>,
    pub harness_label: Option<String>,
    pub latest_analysis: Option<AgentSessionAnalysisRecord>,
    #[serde(default)]
    pub spend: AgentSessionSpendRecord,
}

/// Usage ledger figures for a session's linked requests, read when the session is listed.
/// Unlike the analysis report's cost, they are live while the session is still open, and an
/// unpriced request is counted rather than voiding the whole total.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionSpendRecord {
    /// Sum of priced usage cost; `None` when no linked request has priced usage.
    pub priced_cost: Option<Money9>,
    /// Linked requests without priced usage: unpriced, failed before usage, or usage missing.
    pub unpriced_request_count: u64,
    /// Provider of the most recent linked request that has a request log.
    pub latest_provider_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAnalysisQueueStatus {
    Pending,
    Leased,
    Completed,
    Failed,
}

impl AgentAnalysisQueueStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "leased" => Some(Self::Leased),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAnalysisDesiredVersions {
    pub report_schema_version: String,
    pub boundary_policy_version: String,
    pub observation_parser_version: String,
    pub analyzer_version: String,
    pub score_policy_version: String,
    pub pricing_policy_version: String,
    pub cohort_version: String,
    #[serde(default)]
    pub configuration_version: String,
    #[serde(default)]
    pub score_maturity: ScoreMaturity,
    #[serde(default)]
    pub calibration_approval_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAnalysisQueueRecord {
    pub queue_item_id: Uuid,
    pub agent_session_id: AgentSessionId,
    pub reason: String,
    pub desired_versions: AgentAnalysisDesiredVersions,
    pub status: AgentAnalysisQueueStatus,
    pub lease_owner: Option<String>,
    pub lease_epoch: i64,
    pub lease_expires_at: Option<OffsetDateTime>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_error: Option<String>,
    pub available_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
}

/// A fencing identity: a reclaimed lease is distinct even for the same worker.
#[derive(Debug, Clone, Copy)]
pub struct AgentAnalysisLease<'a> {
    pub queue_item_id: Uuid,
    pub owner: &'a str,
    pub epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentSessionListQuery {
    pub page: u32,
    pub page_size: u32,
    pub ownership_scope_key: Option<String>,
    pub agent_session_source_id: Option<AgentSessionSourceId>,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub harness_key: Option<String>,
    pub requested_model_key: Option<String>,
    pub operation: Option<String>,
    pub caller_class: Option<String>,
    pub gateway_outcome: Option<GatewayOutcomeState>,
    pub score_maturity: Option<ScoreMaturity>,
    pub minimum_coverage_percent: Option<u8>,
    pub normalized_session_id: Option<String>,
    pub request_tag_key: Option<String>,
    pub request_tag_value: Option<String>,
    pub lifecycle: Option<SessionLifecycleState>,
    pub started_after: Option<OffsetDateTime>,
    pub started_before: Option<OffsetDateTime>,
    pub input_watermark_before: Option<OffsetDateTime>,
    pub score_confidence: Option<Confidence>,
    /// Case-insensitive substring match on the requested model key or the owner's name/email.
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionListPage {
    pub items: Vec<AgentSessionSummaryRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

pub const MAX_AGENT_SESSION_PAGE_SIZE: u32 = 200;
pub const MAX_AGENT_SESSION_REQUESTS: u64 = 1_000;
pub const MAX_AGENT_SESSION_NESTED_FACTS: usize = 2_048;

/// One atomic session input. Observation-only input supports parser recomputation.
#[derive(Debug, Clone, Copy)]
pub struct AgentSessionInput<'a> {
    pub request: Option<&'a AgentSessionRequestLinkRecord>,
    pub observations: Option<AgentSessionObservationInput<'a>>,
    pub analysis: Option<&'a AgentAnalysisQueueRecord>,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentSessionObservationInput<'a> {
    pub set: &'a AgentObservationSetRecord,
    pub truncated_set: &'a AgentObservationSetRecord,
    pub maximum_nested_facts: usize,
}

impl<'a> AgentSessionInput<'a> {
    #[must_use]
    pub const fn request(request: &'a AgentSessionRequestLinkRecord) -> Self {
        Self {
            request: Some(request),
            observations: None,
            analysis: None,
        }
    }

    #[must_use]
    pub const fn observations(set: &'a AgentObservationSetRecord) -> Self {
        Self::bounded_observations(set, set, MAX_AGENT_SESSION_NESTED_FACTS)
    }

    #[must_use]
    pub const fn bounded_observations(
        set: &'a AgentObservationSetRecord,
        truncated_set: &'a AgentObservationSetRecord,
        maximum_nested_facts: usize,
    ) -> Self {
        Self {
            request: None,
            observations: Some(AgentSessionObservationInput {
                set,
                truncated_set,
                maximum_nested_facts,
            }),
            analysis: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentSessionInputAppendResult {
    pub request_inserted: bool,
    pub observations_inserted: bool,
    pub nested_facts_truncated: bool,
}

#[async_trait]
pub trait AgentSessionTraceRepository {
    async fn upsert_agent_session_source(
        &self,
        session: &AgentSessionSourceRecord,
    ) -> Result<AgentSessionSourceRecord, StoreError>;

    async fn load_agent_session_source(
        &self,
        agent_session_source_id: AgentSessionSourceId,
    ) -> Result<Option<AgentSessionSourceRecord>, StoreError>;

    async fn get_open_agent_session(
        &self,
        ownership_scope_key: &str,
        agent_session_source_id: Option<AgentSessionSourceId>,
        harness_key: &str,
        boundary_group_key: &str,
    ) -> Result<Option<AgentSessionRecord>, StoreError>;

    async fn insert_agent_session_if_absent(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<bool, StoreError>;

    async fn finalize_agent_session_if_unchanged(
        &self,
        session: &AgentSessionRecord,
        expected_input_watermark_at: OffsetDateTime,
        analysis: Option<&AgentAnalysisQueueRecord>,
    ) -> Result<bool, StoreError>;

    /// Atomically appends a request and its observations under one session-writer lock.
    ///
    /// Request ordinals are assigned by the repository; duplicates consume no counters.
    async fn append_agent_session_input(
        &self,
        input: AgentSessionInput<'_>,
    ) -> Result<AgentSessionInputAppendResult, StoreError>;

    async fn list_idle_agent_sessions(
        &self,
        cutoff: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<AgentSessionRecord>, StoreError>;

    async fn list_agent_session_request_attempts(
        &self,
        agent_session_id: AgentSessionId,
        limit: u32,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError>;

    async fn load_agent_observation_sets(
        &self,
        agent_session_id: AgentSessionId,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError>;

    /// Loads only the requested parser's sets and child observations, filtering before the
    /// 1,001-set overflow sentinel limit. Historical parser versions do not consume that limit.
    async fn load_agent_observation_sets_for_parser(
        &self,
        agent_session_id: AgentSessionId,
        parser_version: &str,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError>;

    async fn link_request_log_to_agent_session(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError>;

    async fn load_agent_session_trace(
        &self,
        agent_session_id: AgentSessionId,
    ) -> Result<Option<AgentSessionTraceRecord>, StoreError>;
}

#[async_trait]
pub trait AgentSessionReportRepository {
    /// Publishes current fenced work; returns false for replays or superseded work.
    async fn append_agent_session_analysis(
        &self,
        analysis: &AgentSessionAnalysisRecord,
        lease: Option<AgentAnalysisLease<'_>>,
    ) -> Result<bool, StoreError>;

    async fn list_agent_sessions(
        &self,
        query: &AgentSessionListQuery,
    ) -> Result<AgentSessionListPage, StoreError>;
}

#[async_trait]
pub trait AgentAnalysisQueueRepository {
    /// Coalesces requests per session. IDs deduplicate requests; lease/status fields are store-owned.
    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError>;

    async fn claim_agent_analysis(
        &self,
        lease_owner: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<Option<AgentAnalysisQueueRecord>, StoreError>;

    async fn renew_agent_analysis_lease(
        &self,
        lease: AgentAnalysisLease<'_>,
        updated_at: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    async fn complete_agent_analysis(
        &self,
        lease: AgentAnalysisLease<'_>,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError>;

    async fn fail_agent_analysis(
        &self,
        lease: AgentAnalysisLease<'_>,
        error: &str,
        retry_at: Option<OffsetDateTime>,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError>;

    async fn purge_expired_agent_analysis(
        &self,
        report_cutoff: OffsetDateTime,
        queue_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError>;

    async fn purge_agent_analysis_before(
        &self,
        request_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError>;

    async fn delete_agent_analysis_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError>;
}

pub trait AgentSessionAnalysisRepository:
    AgentSessionTraceRepository + AgentSessionReportRepository + AgentAnalysisQueueRepository
{
}

impl<T> AgentSessionAnalysisRepository for T where
    T: AgentSessionTraceRepository + AgentSessionReportRepository + AgentAnalysisQueueRepository
{
}
