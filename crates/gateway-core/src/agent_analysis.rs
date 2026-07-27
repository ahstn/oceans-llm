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

use crate::StoreError;

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
    pub boundary_policy_version: String,
    pub input_watermark_at: OffsetDateTime,
    pub observation_set_id: ObservationSetId,
    pub observation_parser_version: String,
    pub pricing_policy_version: String,
    pub cohort_version: String,
    pub cohort_fallback_level: u8,
    pub cohort_sample_size: u64,
    pub cohort_snapshot_digest: String,
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
    pub session_source: Option<AgentSessionSourceRecord>,
    pub requests: Vec<AgentSessionRequestLinkRecord>,
    pub latest_observation_set: Option<AgentObservationSetRecord>,
    pub latest_analysis: Option<AgentSessionAnalysisRecord>,
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
    pub lease_expires_at: Option<OffsetDateTime>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_error: Option<String>,
    pub available_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionListPage {
    pub items: Vec<AgentSessionTraceRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

pub const MAX_AGENT_SESSION_PAGE_SIZE: u32 = 200;
pub const MAX_AGENT_SESSION_REQUESTS: u64 = 1_000;

#[async_trait]
pub trait AgentSessionAnalysisRepository {
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

    async fn update_agent_session_window(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<(), StoreError>;
    async fn finalize_agent_session_if_unchanged(
        &self,
        session: &AgentSessionRecord,
        expected_input_watermark_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    /// Persists the link with the next session-local ordinal under a database lock.
    ///
    /// `link.ordinal` is an output field and is ignored on append.
    async fn append_agent_session_request(
        &self,
        link: &AgentSessionRequestLinkRecord,
    ) -> Result<bool, StoreError>;

    async fn count_agent_session_requests(
        &self,
        agent_session_id: AgentSessionId,
    ) -> Result<u64, StoreError>;

    async fn append_agent_observation_set(
        &self,
        set: &AgentObservationSetRecord,
    ) -> Result<bool, StoreError>;

    async fn load_agent_observation_sets(
        &self,
        agent_session_id: AgentSessionId,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError>;

    async fn link_request_log_to_agent_session(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError>;

    async fn load_agent_session_trace(
        &self,
        agent_session_id: AgentSessionId,
    ) -> Result<Option<AgentSessionTraceRecord>, StoreError>;

    async fn append_agent_session_analysis(
        &self,
        analysis: &AgentSessionAnalysisRecord,
    ) -> Result<bool, StoreError>;

    async fn list_agent_sessions(
        &self,
        query: &AgentSessionListQuery,
    ) -> Result<AgentSessionListPage, StoreError>;

    async fn mark_agent_session_analyses_stale(
        &self,
        agent_session_id: AgentSessionId,
        superseded_by: Option<AnalysisId>,
    ) -> Result<u64, StoreError>;

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
        queue_item_id: Uuid,
        lease_owner: &str,
        updated_at: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    async fn complete_agent_analysis(
        &self,
        queue_item_id: Uuid,
        lease_owner: &str,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError>;

    async fn fail_agent_analysis(
        &self,
        queue_item_id: Uuid,
        lease_owner: &str,
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
