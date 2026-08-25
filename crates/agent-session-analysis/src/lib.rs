pub mod calibration;
mod extended;

pub use extended::{
    AnalysisMetricPolicy, BoundedFileInteractionFact, BoundedSkillFact, CacheProfileRule, CacheTtl,
    FinishReasonDiagnostics, FinishReasonItem, OutcomeDiagnostics, ReliabilityDiagnostics,
    RequestAttemptFact, SkillDiagnosticItem, SkillDiagnostics, ToolInvocationFact,
    ToolReliabilityItem, ToolServerDiagnostics,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const REPORT_SCHEMA_VERSION: &str = "agent-session-report-v6";
pub const SESSION_BOUNDARY_POLICY_VERSION: &str = "passive-gap-v2";
pub const OBSERVATION_PARSER_VERSION: &str = "passive-observations-v3";
pub const ANALYZER_VERSION: &str = "session-efficiency-v5";
pub const SCORE_POLICY_VERSION: &str = "outcome-cost-time-context-v2";
pub const DEFAULT_ORCHESTRATION_GAP: Duration = Duration::minutes(2);
pub const MIN_EXACT_COHORT_SIZE: usize = 6;

pub type AgentSessionSourceId = Uuid;
pub type AgentSessionId = Uuid;
pub type AnalysisId = Uuid;
pub type ObservationId = Uuid;
pub type ObservationSetId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecycleState {
    Open,
    Finalized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayOutcomeState {
    Succeeded,
    Partial,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreMaturity {
    #[default]
    Experimental,
    Calibrated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceQuality {
    Direct,
    InferredHigh,
    InferredLow,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferredObservationKind {
    FileReadSuspected,
    FileSearchSuspected,
    FileCreateSuspected,
    FileEditSuspected,
    FileOverwriteSuspected,
    VerificationResultClassified,
    CompactionSuspected,
    ContextResetSuspected,
    ToolCallClassified,
    SessionMetadataClassified,
    ResponseFinishClassified,
    ReworkSuspected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitationCode {
    PayloadUnavailable,
    PayloadTruncated,
    SessionUnobserved,
    RequestIncomplete,
    UsageUnavailable,
    PricingUnavailable,
    ToolInventoryPotentialOnly,
    SemanticVerificationUnavailable,
    CohortFallback,
    LateDataExcluded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedToolDefinitionFact {
    pub name: String,
    #[serde(default)]
    pub server_key: Option<String>,
    pub token_estimate: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BoundedObservationFacts {
    pub message_count: Option<u32>,
    pub prompt_bytes: Option<u64>,
    pub supplied_tool_count: Option<u32>,
    pub tool_schema_bytes: Option<u64>,
    pub tool_schema_token_estimate: Option<u64>,
    #[serde(default)]
    pub supplied_tools: Vec<BoundedToolDefinitionFact>,
    #[serde(default)]
    pub supplied_skills: Vec<BoundedSkillFact>,
    #[serde(default)]
    pub file_interactions: Vec<BoundedFileInteractionFact>,
    #[serde(default)]
    pub reasoning_config_hash: Option<String>,
    #[serde(default)]
    pub cache_requested: Option<bool>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub incomplete_reason: Option<String>,
    pub tool_name: Option<String>,
    pub tool_schema_hash: Option<String>,
    pub opaque_file_id: Option<String>,
    pub file_kind: Option<String>,
    pub result_bytes: Option<u64>,
    pub error_signature: Option<String>,
    #[serde(default)]
    pub attributes: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferredObservation {
    pub observation_id: ObservationId,
    pub kind: InferredObservationKind,
    pub source_request_id: String,
    pub parser_version: String,
    pub evidence: EvidenceQuality,
    pub occurred_at: OffsetDateTime,
    pub facts: BoundedObservationFacts,
    #[serde(default)]
    pub limitations: Vec<LimitationCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityInterval {
    pub started_at: OffsetDateTime,
    pub ended_at: OffsetDateTime,
}

impl ActivityInterval {
    #[must_use]
    pub fn new(started_at: OffsetDateTime, ended_at: OffsetDateTime) -> Option<Self> {
        (ended_at >= started_at).then_some(Self {
            started_at,
            ended_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionUsageFact {
    pub fresh_input_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub provider_total_tokens: Option<i64>,
    #[serde(default)]
    pub cache_creation_5m_tokens: Option<i64>,
    #[serde(default)]
    pub cache_creation_30m_tokens: Option<i64>,
    #[serde(default)]
    pub cache_creation_1h_tokens: Option<i64>,
    #[serde(default)]
    pub output_includes_reasoning: Option<bool>,
    pub fresh_input_cost_10000: Option<i64>,
    pub cache_read_cost_10000: Option<i64>,
    pub cache_creation_cost_10000: Option<i64>,
    pub output_cost_10000: Option<i64>,
    pub reasoning_cost_10000: Option<i64>,
    pub legacy_cost_10000: Option<i64>,
    pub normalized_cost_10000: Option<i64>,
    pub uncached_input_cost_10000: Option<i64>,
    pub provider_key: Option<String>,
    pub upstream_model: Option<String>,
    pub pricing_policy_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRequestFact {
    pub request_id: String,
    #[serde(default)]
    pub ordinal: i64,
    pub occurred_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub terminal_success: Option<bool>,
    pub usage: Option<SessionUsageFact>,
    #[serde(default)]
    pub attempts: Vec<RequestAttemptFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CohortReference {
    pub cohort_version: String,
    pub fallback_level: u8,
    pub successful_costs_10000: Vec<i64>,
    pub successful_active_time_ms: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TraceEvidence {
    pub session_observed: bool,
    pub request_metadata_count: u32,
    pub response_payload_count: u32,
    pub truncated_payload_count: u32,
    pub direct_mcp_intervals: Vec<ActivityInterval>,
    #[serde(default)]
    pub tool_invocations: Vec<ToolInvocationFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTrace {
    pub requests: Vec<SessionRequestFact>,
    pub activity_intervals: Vec<ActivityInterval>,
    pub observations: Vec<InferredObservation>,
    pub lifecycle: SessionLifecycleState,
    pub boundary_confidence: Confidence,
    pub evidence: TraceEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisPolicy {
    pub report_schema_version: String,
    pub analyzer_version: String,
    pub score_policy_version: String,
    pub observation_parser_version: String,
    pub orchestration_gap: Duration,
    pub maturity: ScoreMaturity,
    pub calibration_approval_id: Option<String>,
    #[serde(default)]
    pub configuration_version: String,
    #[serde(default)]
    pub metrics: AnalysisMetricPolicy,
    #[serde(default = "default_context_input_boundary_tokens")]
    pub context_input_boundary_tokens: i64,
    #[serde(default = "default_context_reserved_output_tokens")]
    pub context_reserved_output_tokens: i64,
    #[serde(default = "default_context_penalty_points")]
    pub context_penalty_points_per_repeated_excess: u8,
    #[serde(default)]
    pub cache_profiles: Vec<CacheProfileRule>,
}

impl Default for AnalysisPolicy {
    fn default() -> Self {
        Self {
            report_schema_version: REPORT_SCHEMA_VERSION.to_string(),
            analyzer_version: ANALYZER_VERSION.to_string(),
            score_policy_version: SCORE_POLICY_VERSION.to_string(),
            observation_parser_version: OBSERVATION_PARSER_VERSION.to_string(),
            orchestration_gap: DEFAULT_ORCHESTRATION_GAP,
            maturity: ScoreMaturity::Experimental,
            calibration_approval_id: None,
            configuration_version: String::new(),
            metrics: AnalysisMetricPolicy::default(),
            context_input_boundary_tokens: 220_000,
            context_reserved_output_tokens: 128_000,
            context_penalty_points_per_repeated_excess: 2,
            cache_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisError {
    UnsupportedVersion { field: &'static str, value: String },
    MissingCalibrationApproval,
    DuplicateRequest(String),
    InvalidRequestInterval(String),
    InvalidUsage(String),
    InvalidPolicy(&'static str),
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { field, value } => {
                write!(formatter, "unsupported {field} `{value}`")
            }
            Self::MissingCalibrationApproval => {
                formatter.write_str("calibrated analysis requires an approval identity")
            }
            Self::DuplicateRequest(request_id) => {
                write!(formatter, "duplicate session request `{request_id}`")
            }
            Self::InvalidRequestInterval(request_id) => {
                write!(
                    formatter,
                    "request `{request_id}` completes before it starts"
                )
            }
            Self::InvalidUsage(request_id) => {
                write!(formatter, "request `{request_id}` contains negative usage")
            }
            Self::InvalidPolicy(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AnalysisError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeComponent {
    pub state: GatewayOutcomeState,
    pub factor_basis_points: u16,
    pub successful_requests: u32,
    pub determinate_requests: u32,
    pub incomplete_requests: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryCoverage {
    pub outcome_percent: u8,
    pub cost_percent: u8,
    pub timing_percent: u8,
    pub payload_percent: u8,
    #[serde(default)]
    pub response_payload_count: u32,
    #[serde(default)]
    pub truncated_response_count: u32,
    pub cohort_percent: u8,
    pub overall_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TokenAndCacheDiagnostics {
    pub fresh_input_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    #[serde(default)]
    pub total_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    #[serde(default)]
    pub visible_output_tokens: Option<i64>,
    #[serde(default)]
    pub cache_creation_5m_tokens: Option<i64>,
    #[serde(default)]
    pub cache_creation_30m_tokens: Option<i64>,
    #[serde(default)]
    pub cache_creation_1h_tokens: Option<i64>,
    pub provider_total_tokens: Option<i64>,
    pub legacy_cost_10000: Option<i64>,
    pub normalized_cost_10000: Option<i64>,
    #[serde(default)]
    pub cache_read_cost_10000: Option<i64>,
    #[serde(default)]
    pub cache_creation_cost_10000: Option<i64>,
    pub uncached_input_cost_10000: Option<i64>,
    pub cache_savings_10000: Option<i64>,
    pub cache_savings_basis_points: Option<i32>,
    pub cache_read_write_ratio_basis_points: Option<i32>,
    #[serde(default)]
    pub cache_write_amplification_basis_points: Option<i32>,
    #[serde(default)]
    pub silent_cache_threshold_miss_requests: Option<u32>,
    #[serde(default, alias = "cache_key_switches")]
    pub provider_model_switches: u32,
    #[serde(default)]
    pub reasoning_config_switches: Option<u32>,
    pub pricing_policy_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextDiagnostics {
    pub initial_prompt_tokens: Option<i64>,
    pub median_prompt_tokens: Option<i64>,
    pub p90_prompt_tokens: Option<i64>,
    pub maximum_prompt_tokens: Option<i64>,
    #[serde(default = "default_context_input_boundary_tokens")]
    pub input_boundary_tokens: i64,
    #[serde(default = "default_context_reserved_output_tokens")]
    pub reserved_output_tokens: i64,
    #[serde(default)]
    pub peak_input_utilization_basis_points: Option<i32>,
    #[serde(default)]
    pub requests_over_input_boundary: u32,
    #[serde(default)]
    pub repeated_requests_over_input_boundary: u32,
    #[serde(default)]
    pub score_penalty_points: u8,
    pub prompt_growth_per_turn: Option<i64>,
    pub prompt_growth_per_active_minute: Option<i64>,
    pub suspected_compactions: u32,
    pub suspected_context_resets: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolAndChangeDiagnostics {
    pub supplied_tool_definitions: Option<u64>,
    pub supplied_tool_schema_bytes: Option<u64>,
    pub observed_tool_calls: u32,
    pub classified_tool_calls: u32,
    pub file_reads_suspected: u32,
    pub file_searches_suspected: u32,
    pub file_creates_suspected: u32,
    pub file_edits_suspected: u32,
    pub file_overwrites_suspected: u32,
    pub unique_opaque_files: u32,

    pub verification_results_classified: u32,
    pub rework_spans_suspected: u32,
    #[serde(default)]
    pub direct_mcp_calls: u32,
    pub direct_mcp_duration_ms: Option<i64>,
    #[serde(default)]
    pub tool_servers: Vec<ToolServerDiagnostics>,
}

fn default_context_input_boundary_tokens() -> i64 {
    220_000
}

fn default_context_reserved_output_tokens() -> i64 {
    128_000
}

fn default_context_penalty_points() -> u8 {
    2
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDiagnostics {
    pub token_and_cache: TokenAndCacheDiagnostics,
    pub context: ContextDiagnostics,
    pub tools_and_changes: ToolAndChangeDiagnostics,
    #[serde(default)]
    pub skills: SkillDiagnostics,
    #[serde(default)]
    pub reliability: ReliabilityDiagnostics,
    #[serde(default)]
    pub outcome: OutcomeDiagnostics,
    #[serde(default)]
    pub finish_reasons: FinishReasonDiagnostics,
    #[serde(default)]
    pub enabled_metrics: AnalysisMetricPolicy,
    pub semantic_verification_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEfficiencyComponents {
    pub outcome: OutcomeComponent,
    pub cost_efficiency_basis_points: Option<u16>,
    pub active_time_efficiency_basis_points: Option<u16>,
    pub actual_cost_10000: Option<i64>,
    pub active_time_ms: i64,
    pub wall_time_ms: i64,
    pub summed_work_time_ms: i64,
    pub excluded_gap_time_ms: i64,
    pub overlap_savings_ms: i64,
    #[serde(default)]
    pub context_penalty_points: u8,
    pub unknown_wait_time_ms: Option<i64>,
    pub cohort_version: Option<String>,
    pub cohort_fallback_level: Option<u8>,
    pub cohort_sample_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEfficiencyReport {
    pub report_schema_version: String,
    pub analyzer_version: String,
    pub score_policy_version: String,
    pub observation_parser_version: String,
    #[serde(default)]
    pub configuration_version: String,
    pub maturity: ScoreMaturity,
    pub calibration_approval_id: Option<String>,
    pub confidence: Confidence,
    pub gateway_outcome: GatewayOutcomeState,
    pub score: Option<u8>,
    pub coverage: TelemetryCoverage,
    pub components: SessionEfficiencyComponents,
    pub diagnostics: SessionDiagnostics,
    pub limitations: Vec<LimitationCode>,
}

#[must_use]
fn outcome_component(requests: &[SessionRequestFact]) -> OutcomeComponent {
    let mut successful_requests = 0_u32;
    let mut determinate_requests = 0_u32;
    let mut incomplete_requests = 0_u32;

    for request in requests {
        match request.terminal_success {
            Some(successful) => {
                determinate_requests = determinate_requests.saturating_add(1);
                successful_requests = successful_requests.saturating_add(u32::from(successful));
            }
            None => incomplete_requests = incomplete_requests.saturating_add(1),
        }
    }

    let (state, factor_basis_points) = if determinate_requests == 0 {
        (GatewayOutcomeState::Unknown, 5_000)
    } else if successful_requests == 0 {
        (GatewayOutcomeState::Failed, 0)
    } else if successful_requests == determinate_requests {
        (GatewayOutcomeState::Succeeded, 10_000)
    } else {
        let factor = (u64::from(successful_requests) * 10_000
            + u64::from(determinate_requests) / 2)
            / u64::from(determinate_requests);
        (GatewayOutcomeState::Partial, factor as u16)
    };

    OutcomeComponent {
        state,
        factor_basis_points,
        successful_requests,
        determinate_requests,
        incomplete_requests,
    }
}

#[must_use]
fn lower_is_better_efficiency_basis_points(value: i64, peers: &[i64]) -> Option<u16> {
    if value < 0 || peers.is_empty() || peers.iter().any(|peer| *peer < 0) {
        return None;
    }
    let greater = peers.iter().filter(|peer| **peer > value).count() as u64;
    let equal = peers.iter().filter(|peer| **peer == value).count() as u64;
    let doubled_midrank = greater * 2 + equal;
    let denominator = (peers.len() as u64) * 2;
    let rounded_basis_points = (doubled_midrank * 10_000 + denominator / 2) / denominator;
    Some(rounded_basis_points.clamp(100, 10_000) as u16)
}

#[must_use]
fn active_time_milliseconds(
    mut intervals: Vec<ActivityInterval>,
    orchestration_gap: Duration,
) -> i64 {
    if intervals.is_empty() {
        return 0;
    }
    intervals.sort_unstable_by_key(|interval| interval.started_at);
    let gap = orchestration_gap.max(Duration::ZERO);
    let mut total = Duration::ZERO;
    let mut current_start = intervals[0].started_at;
    let mut current_end = intervals[0].ended_at;

    for interval in intervals.into_iter().skip(1) {
        if interval.started_at <= current_end + gap {
            current_end = current_end.max(interval.ended_at);
        } else {
            total += current_end - current_start;
            current_start = interval.started_at;
            current_end = interval.ended_at;
        }
    }
    total += current_end - current_start;
    i64::try_from(total.whole_milliseconds()).unwrap_or(i64::MAX)
}

#[must_use]
fn session_efficiency_score(
    outcome_basis_points: u16,
    cost_basis_points: Option<u16>,
    time_basis_points: Option<u16>,
) -> Option<u8> {
    if cost_basis_points.is_none() && time_basis_points.is_none() {
        return None;
    }
    if outcome_basis_points == 0 {
        return Some(0);
    }
    let mut weighted_log_sum = 0.50 * (f64::from(outcome_basis_points) / 10_000.0).ln();
    let mut included_weight = 0.50;
    if let Some(cost) = cost_basis_points {
        weighted_log_sum += 0.30 * (f64::from(cost) / 10_000.0).ln();
        included_weight += 0.30;
    }
    if let Some(time) = time_basis_points {
        weighted_log_sum += 0.20 * (f64::from(time) / 10_000.0).ln();
        included_weight += 0.20;
    }
    Some(
        (100.0 * (weighted_log_sum / included_weight).exp())
            .round()
            .clamp(0.0, 100.0) as u8,
    )
}

fn validate_policy(policy: &AnalysisPolicy) -> Result<(), AnalysisError> {
    for (field, actual, expected) in [
        (
            "report schema version",
            policy.report_schema_version.as_str(),
            REPORT_SCHEMA_VERSION,
        ),
        (
            "analyzer version",
            policy.analyzer_version.as_str(),
            ANALYZER_VERSION,
        ),
        (
            "score policy version",
            policy.score_policy_version.as_str(),
            SCORE_POLICY_VERSION,
        ),
        (
            "observation parser version",
            policy.observation_parser_version.as_str(),
            OBSERVATION_PARSER_VERSION,
        ),
    ] {
        if actual != expected {
            return Err(AnalysisError::UnsupportedVersion {
                field,
                value: actual.to_string(),
            });
        }
    }
    if policy.maturity == ScoreMaturity::Calibrated
        && policy
            .calibration_approval_id
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err(AnalysisError::MissingCalibrationApproval);
    }
    if policy.context_input_boundary_tokens <= 0 {
        return Err(AnalysisError::InvalidPolicy(
            "context input boundary must be positive",
        ));
    }
    if policy.context_reserved_output_tokens < 0 {
        return Err(AnalysisError::InvalidPolicy(
            "reserved output tokens must not be negative",
        ));
    }
    if policy
        .cache_profiles
        .iter()
        .any(|profile| profile.minimum_cacheable_tokens <= 0)
    {
        return Err(AnalysisError::InvalidPolicy(
            "cache profile token minimums must be positive",
        ));
    }
    Ok(())
}

fn validate_trace(trace: &SessionTrace) -> Result<(), AnalysisError> {
    let mut request_ids = HashSet::with_capacity(trace.requests.len());
    for request in &trace.requests {
        if !request_ids.insert(request.request_id.as_str()) {
            return Err(AnalysisError::DuplicateRequest(request.request_id.clone()));
        }
        if request
            .completed_at
            .is_some_and(|completed_at| completed_at < request.occurred_at)
        {
            return Err(AnalysisError::InvalidRequestInterval(
                request.request_id.clone(),
            ));
        }
        if request.usage.as_ref().is_some_and(|usage| {
            [
                usage.fresh_input_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                usage.output_tokens,
                usage.reasoning_tokens,
                usage.provider_total_tokens,
                usage.fresh_input_cost_10000,
                usage.cache_read_cost_10000,
                usage.cache_creation_cost_10000,
                usage.output_cost_10000,
                usage.reasoning_cost_10000,
                usage.legacy_cost_10000,
                usage.normalized_cost_10000,
                usage.uncached_input_cost_10000,
            ]
            .into_iter()
            .flatten()
            .any(|value| value < 0)
        }) {
            return Err(AnalysisError::InvalidUsage(request.request_id.clone()));
        }
    }
    if trace
        .activity_intervals
        .iter()
        .chain(&trace.evidence.direct_mcp_intervals)
        .any(|interval| interval.ended_at < interval.started_at)
    {
        return Err(AnalysisError::InvalidRequestInterval(
            "activity_interval".to_string(),
        ));
    }
    Ok(())
}

fn sum_usage(
    requests: &[SessionRequestFact],
    field: fn(&SessionUsageFact) -> Option<i64>,
) -> Option<i64> {
    if requests.is_empty() {
        return None;
    }
    requests
        .iter()
        .map(|request| request.usage.as_ref().and_then(field))
        .try_fold(0_i64, |total, value| {
            value.and_then(|value| total.checked_add(value))
        })
}

fn ratio_basis_points(numerator: i64, denominator: i64) -> Option<i32> {
    if denominator <= 0 {
        return None;
    }
    let ratio = i128::from(numerator) * 10_000 / i128::from(denominator);
    Some(ratio.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32)
}

fn coverage_percent(observed: usize, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    u8::try_from((observed.saturating_mul(100) + total / 2) / total)
        .unwrap_or(100)
        .min(100)
}

fn percentile(values: &[i64], numerator: usize, denominator: usize) -> Option<i64> {
    if values.is_empty() || denominator == 0 {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    let index = ((ordered.len() - 1) * numerator).div_ceil(denominator);
    ordered.get(index).copied()
}

fn context_diagnostics(
    requests: &[SessionRequestFact],
    active_time_ms: i64,
    observations: &[InferredObservation],
    policy: &AnalysisPolicy,
) -> ContextDiagnostics {
    let mut prompt_samples = requests
        .iter()
        .filter_map(|request| {
            let prompt_tokens = request
                .usage
                .as_ref()
                .and_then(extended::total_input_tokens)?;
            Some((request.occurred_at, prompt_tokens))
        })
        .collect::<Vec<_>>();
    prompt_samples.sort_unstable_by_key(|(occurred_at, _)| *occurred_at);
    let prompt_tokens = prompt_samples
        .into_iter()
        .map(|(_, prompt_tokens)| prompt_tokens)
        .collect::<Vec<_>>();
    let first = prompt_tokens.first().copied();
    let last = prompt_tokens.last().copied();
    let prompt_growth = first
        .zip(last)
        .and_then(|(first, last)| last.checked_sub(first));
    let requests_over_input_boundary = prompt_tokens
        .iter()
        .filter(|tokens| **tokens > policy.context_input_boundary_tokens)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let repeated_requests_over_input_boundary = requests_over_input_boundary.saturating_sub(1);
    let score_penalty_points = repeated_requests_over_input_boundary
        .saturating_mul(u32::from(policy.context_penalty_points_per_repeated_excess))
        .min(100)
        .try_into()
        .unwrap_or(100);
    ContextDiagnostics {
        initial_prompt_tokens: first,
        median_prompt_tokens: percentile(&prompt_tokens, 1, 2),
        p90_prompt_tokens: percentile(&prompt_tokens, 9, 10),
        maximum_prompt_tokens: prompt_tokens.iter().max().copied(),
        input_boundary_tokens: policy.context_input_boundary_tokens,
        reserved_output_tokens: policy.context_reserved_output_tokens,
        peak_input_utilization_basis_points: prompt_tokens
            .iter()
            .max()
            .and_then(|maximum| ratio_basis_points(*maximum, policy.context_input_boundary_tokens)),
        requests_over_input_boundary,
        repeated_requests_over_input_boundary,
        score_penalty_points,
        prompt_growth_per_turn: prompt_growth.and_then(|growth| {
            i64::try_from(requests.len().saturating_sub(1))
                .ok()
                .filter(|turns| *turns > 0)
                .map(|turns| growth / turns)
        }),
        prompt_growth_per_active_minute: prompt_growth.and_then(|growth| {
            (active_time_ms > 0).then(|| growth.saturating_mul(60_000) / active_time_ms)
        }),
        suspected_compactions: observations
            .iter()
            .filter(|value| value.kind == InferredObservationKind::CompactionSuspected)
            .count()
            .try_into()
            .unwrap_or(u32::MAX),
        suspected_context_resets: observations
            .iter()
            .filter(|value| value.kind == InferredObservationKind::ContextResetSuspected)
            .count()
            .try_into()
            .unwrap_or(u32::MAX),
    }
}

fn tool_and_change_diagnostics(
    observations: &[InferredObservation],
    direct_mcp_intervals: &[ActivityInterval],
    tool_invocations: &[ToolInvocationFact],
    requests: &[SessionRequestFact],
) -> ToolAndChangeDiagnostics {
    let mut supplied_tool_definitions = None::<u64>;
    let mut supplied_tool_schema_bytes = None::<u64>;
    let mut observed_tool_calls = 0_u32;
    let mut classified_tool_calls = 0_u32;
    let mut opaque_files = BTreeSet::new();
    let mut counts = [0_u32; 8];
    let mut file_write_counts = BTreeMap::<&str, u32>::new();
    for observation in observations {
        supplied_tool_definitions =
            supplied_tool_definitions.max(observation.facts.supplied_tool_count.map(u64::from));
        supplied_tool_schema_bytes =
            supplied_tool_schema_bytes.max(observation.facts.tool_schema_bytes);
        if observation.facts.tool_name.is_some() {
            observed_tool_calls = observed_tool_calls.saturating_add(1);
            classified_tool_calls = classified_tool_calls.saturating_add(1);
        }
        if let Some(file) = observation.facts.opaque_file_id.as_deref() {
            opaque_files.insert(file);
        }
        for interaction in &observation.facts.file_interactions {
            opaque_files.insert(interaction.opaque_file_id.as_str());
            let index = match interaction.operation.as_str() {
                "read" => Some(0),
                "search" => Some(1),
                "create" => Some(2),
                "edit" => Some(3),
                "overwrite" => Some(4),
                "verify" => Some(5),
                _ => None,
            };
            if let Some(index) = index {
                counts[index] = counts[index].saturating_add(1);
                if matches!(index, 2..=4) {
                    file_write_counts
                        .entry(interaction.opaque_file_id.as_str())
                        .and_modify(|count| *count = count.saturating_add(1))
                        .or_insert(1);
                }
            }
        }
        if observation.facts.file_interactions.is_empty() {
            let index = match observation.kind {
                InferredObservationKind::FileReadSuspected => Some(0),
                InferredObservationKind::FileSearchSuspected => Some(1),
                InferredObservationKind::FileCreateSuspected => Some(2),
                InferredObservationKind::FileEditSuspected => Some(3),
                InferredObservationKind::FileOverwriteSuspected => Some(4),
                InferredObservationKind::VerificationResultClassified => Some(5),
                InferredObservationKind::ReworkSuspected => Some(6),
                _ => None,
            };
            if let Some(index) = index {
                counts[index] = counts[index].saturating_add(1);
            }
        }
    }
    let repeated_writes = file_write_counts.values().fold(0_u32, |total, count| {
        total.saturating_add(count.saturating_sub(1))
    });
    counts[6] = counts[6].max(repeated_writes);
    let direct_mcp_calls = direct_mcp_intervals.len().try_into().unwrap_or(u32::MAX);
    let direct_mcp_duration_ms = (!direct_mcp_intervals.is_empty()).then(|| {
        direct_mcp_intervals.iter().fold(0_i64, |total, interval| {
            let duration = (interval.ended_at - interval.started_at).whole_milliseconds();
            total.saturating_add(i64::try_from(duration).unwrap_or(i64::MAX))
        })
    });
    ToolAndChangeDiagnostics {
        supplied_tool_definitions,
        supplied_tool_schema_bytes,
        observed_tool_calls,
        classified_tool_calls,
        file_reads_suspected: counts[0],
        file_searches_suspected: counts[1],
        file_creates_suspected: counts[2],
        file_edits_suspected: counts[3],
        file_overwrites_suspected: counts[4],
        unique_opaque_files: opaque_files.len().try_into().unwrap_or(u32::MAX),
        verification_results_classified: counts[5],
        rework_spans_suspected: counts[6],
        direct_mcp_calls,
        direct_mcp_duration_ms,
        tool_servers: extended::tool_server_diagnostics(observations, tool_invocations, requests),
    }
}

mod analysis;

pub use analysis::analyze_session;

#[cfg(test)]
mod tests;
