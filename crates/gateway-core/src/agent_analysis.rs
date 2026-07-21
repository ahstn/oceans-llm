use agent_session_analysis::{
    AgentSessionId, AgentTaskId, AnalysisId, Confidence, GatewayOutcomeState, InferredObservation,
    LimitationCode, ObservationSetId, ScoreMaturity, TaskEfficiencyReport, TaskLifecycleState,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    pub agent_session_id: AgentSessionId,
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
pub struct AgentTaskWindowRecord {
    pub agent_task_id: AgentTaskId,
    pub agent_session_id: Option<AgentSessionId>,
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
    pub lifecycle: TaskLifecycleState,
    pub boundary_confidence: Confidence,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub input_watermark_at: OffsetDateTime,
    pub finalized_reason: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskRequestLinkRecord {
    pub agent_task_id: AgentTaskId,
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
    pub agent_session_id: Option<AgentSessionId>,
    pub agent_task_id: AgentTaskId,
    pub analysis_source: String,
    pub coverage: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentObservationSetRecord {
    pub observation_set_id: ObservationSetId,
    pub agent_task_id: AgentTaskId,
    pub parser_version: String,
    pub source_watermark_at: OffsetDateTime,
    pub coverage: Value,
    pub created_at: OffsetDateTime,
    pub observations: Vec<InferredObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskAnalysisRecord {
    pub analysis_id: AnalysisId,
    pub agent_task_id: AgentTaskId,
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
    pub report: TaskEfficiencyReport,
    pub stale: bool,
    pub superseded_by_analysis_id: Option<AnalysisId>,
    pub expires_at: OffsetDateTime,
    pub ownership_scope_key: String,
    pub user_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskTraceRecord {
    pub task: AgentTaskWindowRecord,
    pub session: Option<AgentSessionRecord>,
    pub requests: Vec<AgentTaskRequestLinkRecord>,
    pub latest_observation_set: Option<AgentObservationSetRecord>,
    pub latest_analysis: Option<AgentTaskAnalysisRecord>,
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
    pub agent_task_id: AgentTaskId,
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
pub struct AgentTaskListQuery {
    pub page: u32,
    pub page_size: u32,
    pub ownership_scope_key: Option<String>,
    pub agent_session_id: Option<AgentSessionId>,
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
    pub lifecycle: Option<TaskLifecycleState>,
    pub started_after: Option<OffsetDateTime>,
    pub started_before: Option<OffsetDateTime>,
    pub score_confidence: Option<Confidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskListPage {
    pub items: Vec<AgentTaskTraceRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

pub const MAX_AGENT_TASK_PAGE_SIZE: u32 = 200;

#[async_trait]
pub trait AgentSessionAnalysisRepository {
    async fn upsert_agent_session(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<AgentSessionRecord, StoreError>;

    async fn load_agent_session(
        &self,
        agent_session_id: AgentSessionId,
    ) -> Result<Option<AgentSessionRecord>, StoreError>;

    async fn get_open_agent_task(
        &self,
        ownership_scope_key: &str,
        agent_session_id: Option<AgentSessionId>,
        harness_key: &str,
        boundary_group_key: &str,
    ) -> Result<Option<AgentTaskWindowRecord>, StoreError>;

    async fn insert_agent_task_if_absent(
        &self,
        task: &AgentTaskWindowRecord,
    ) -> Result<bool, StoreError>;

    async fn update_agent_task_window(
        &self,
        task: &AgentTaskWindowRecord,
    ) -> Result<(), StoreError>;
    async fn finalize_agent_task_if_unchanged(
        &self,
        task: &AgentTaskWindowRecord,
        expected_input_watermark_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    /// Persists the link with the next task-local ordinal under a database lock.
    ///
    /// `link.ordinal` is an output field and is ignored on append.
    async fn append_agent_task_request(
        &self,
        link: &AgentTaskRequestLinkRecord,
    ) -> Result<bool, StoreError>;

    async fn append_agent_observation_set(
        &self,
        set: &AgentObservationSetRecord,
    ) -> Result<bool, StoreError>;

    async fn link_request_log_to_agent_task(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError>;

    async fn load_agent_task_trace(
        &self,
        agent_task_id: AgentTaskId,
    ) -> Result<Option<AgentTaskTraceRecord>, StoreError>;

    async fn append_agent_task_analysis(
        &self,
        analysis: &AgentTaskAnalysisRecord,
    ) -> Result<bool, StoreError>;

    async fn list_agent_tasks(
        &self,
        query: &AgentTaskListQuery,
    ) -> Result<AgentTaskListPage, StoreError>;

    async fn mark_agent_task_analyses_stale(
        &self,
        agent_task_id: AgentTaskId,
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
