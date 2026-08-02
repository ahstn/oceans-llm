pub mod calibration;
mod extended;

pub use extended::{
    AnalysisMetricPolicy, BoundedFileInteractionFact, BoundedSkillFact, CacheProfileRule, CacheTtl,
    FinishReasonDiagnostics, FinishReasonItem, OutcomeDiagnostics, ReliabilityDiagnostics,
    RequestAttemptFact, SkillDiagnosticItem, SkillDiagnostics, ToolInvocationFact,
    ToolReliabilityItem, ToolServerDiagnostics, default_cache_profiles,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const REPORT_SCHEMA_VERSION: &str = "agent-session-report-v5";
pub const SESSION_BOUNDARY_POLICY_VERSION: &str = "passive-gap-v2";
pub const OBSERVATION_PARSER_VERSION: &str = "passive-observations-v3";
pub const ANALYZER_VERSION: &str = "session-efficiency-v4";
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
    #[serde(default = "default_cache_profiles")]
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
            cache_profiles: default_cache_profiles(),
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
    pub uncached_input_cost_10000: Option<i64>,
    pub cache_savings_10000: Option<i64>,
    pub cache_savings_basis_points: Option<i32>,
    pub cache_read_write_ratio_basis_points: Option<i32>,
    #[serde(default)]
    pub cache_write_amplification_basis_points: Option<i32>,
    #[serde(default)]
    pub silent_cache_threshold_miss_requests: Option<u32>,
    #[serde(default)]
    pub cache_key_switches: u32,
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
            if observation.kind == InferredObservationKind::ToolCallClassified {
                classified_tool_calls = classified_tool_calls.saturating_add(1);
            }
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

pub fn analyze_session(
    trace: &SessionTrace,
    policy: &AnalysisPolicy,
    cohort: Option<&CohortReference>,
) -> Result<SessionEfficiencyReport, AnalysisError> {
    validate_policy(policy)?;
    validate_trace(trace)?;
    let outcome = outcome_component(&trace.requests);
    let actual_cost_10000 = sum_usage(&trace.requests, |usage| usage.normalized_cost_10000);
    let summed_work_time_ms = trace
        .activity_intervals
        .iter()
        .chain(&trace.evidence.direct_mcp_intervals)
        .fold(0_i64, |total, interval| {
            let duration = (interval.ended_at - interval.started_at).whole_milliseconds();
            total.saturating_add(i64::try_from(duration).unwrap_or(i64::MAX))
        });
    let mut activity_intervals = trace.activity_intervals.clone();
    activity_intervals.extend(trace.evidence.direct_mcp_intervals.iter().cloned());
    let concurrent_time_ms = active_time_milliseconds(activity_intervals.clone(), Duration::ZERO);
    let active_time_ms = active_time_milliseconds(activity_intervals, policy.orchestration_gap);
    let started_at = trace
        .requests
        .iter()
        .map(|request| request.occurred_at)
        .min();
    let ended_at = trace
        .requests
        .iter()
        .map(|request| request.completed_at.unwrap_or(request.occurred_at))
        .max();
    let wall_time_ms = started_at
        .zip(ended_at)
        .map(|(start, end)| i64::try_from((end - start).whole_milliseconds()).unwrap_or(i64::MAX))
        .unwrap_or_default()
        .max(0);
    let excluded_gap_time_ms = wall_time_ms.saturating_sub(active_time_ms).max(0);
    let overlap_savings_ms = summed_work_time_ms
        .saturating_sub(concurrent_time_ms)
        .max(0);
    let scoring_cohort = cohort.filter(|cohort| {
        cohort.fallback_level > 0
            || (cohort.successful_costs_10000.len() >= MIN_EXACT_COHORT_SIZE
                && cohort.successful_active_time_ms.len() >= MIN_EXACT_COHORT_SIZE)
    });
    let cost_efficiency_basis_points = scoring_cohort.and_then(|cohort| {
        actual_cost_10000.and_then(|cost| {
            lower_is_better_efficiency_basis_points(cost, &cohort.successful_costs_10000)
        })
    });
    let active_time_efficiency_basis_points = scoring_cohort.and_then(|cohort| {
        lower_is_better_efficiency_basis_points(active_time_ms, &cohort.successful_active_time_ms)
    });
    let context = if policy.metrics.context_metrics {
        context_diagnostics(&trace.requests, active_time_ms, &trace.observations, policy)
    } else {
        ContextDiagnostics {
            input_boundary_tokens: policy.context_input_boundary_tokens,
            reserved_output_tokens: policy.context_reserved_output_tokens,
            ..ContextDiagnostics::default()
        }
    };
    let score = session_efficiency_score(
        outcome.factor_basis_points,
        cost_efficiency_basis_points,
        active_time_efficiency_basis_points,
    )
    .map(|score| score.saturating_sub(context.score_penalty_points));

    let request_count = trace.requests.len();
    let outcome_coverage = coverage_percent(outcome.determinate_requests as usize, request_count);
    let cost_coverage = coverage_percent(
        trace
            .requests
            .iter()
            .filter(|request| {
                request
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.normalized_cost_10000)
                    .is_some()
            })
            .count(),
        request_count,
    );
    let timing_coverage = coverage_percent(
        trace
            .requests
            .iter()
            .filter(|request| request.completed_at.is_some())
            .count(),
        request_count,
    );
    let payload_coverage = coverage_percent(
        usize::try_from(trace.evidence.response_payload_count).unwrap_or(usize::MAX),
        request_count,
    );
    let cohort_coverage = u8::from(scoring_cohort.is_some()) * 100;
    let overall_coverage = u8::try_from(
        (u16::from(outcome_coverage)
            + u16::from(cost_coverage)
            + u16::from(timing_coverage)
            + u16::from(payload_coverage)
            + u16::from(cohort_coverage))
            / 5,
    )
    .unwrap_or(100);
    let coverage = TelemetryCoverage {
        outcome_percent: outcome_coverage,
        cost_percent: cost_coverage,
        timing_percent: timing_coverage,
        payload_percent: payload_coverage,
        cohort_percent: cohort_coverage,
        overall_percent: overall_coverage,
    };

    let normalized_cost = sum_usage(&trace.requests, |usage| usage.normalized_cost_10000);
    let uncached_input_cost = sum_usage(&trace.requests, |usage| usage.uncached_input_cost_10000);
    let cache_savings = uncached_input_cost
        .zip(sum_usage(&trace.requests, |usage| {
            usage
                .fresh_input_cost_10000
                .zip(usage.cache_read_cost_10000)
                .and_then(|(fresh, read)| fresh.checked_add(read))
                .zip(usage.cache_creation_cost_10000)
                .and_then(|(partial, creation)| partial.checked_add(creation))
        }))
        .and_then(|(uncached, actual)| uncached.checked_sub(actual));
    let cache_read = sum_usage(&trace.requests, |usage| usage.cache_read_tokens);
    let cache_creation = sum_usage(&trace.requests, |usage| usage.cache_creation_tokens);
    let pricing_policy_versions = trace
        .requests
        .iter()
        .filter_map(|request| request.usage.as_ref()?.pricing_policy_version.as_deref())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut token_and_cache = TokenAndCacheDiagnostics {
        fresh_input_tokens: sum_usage(&trace.requests, |usage| usage.fresh_input_tokens),
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        total_input_tokens: sum_usage(&trace.requests, extended::total_input_tokens),
        output_tokens: sum_usage(&trace.requests, |usage| usage.output_tokens),
        reasoning_tokens: sum_usage(&trace.requests, |usage| usage.reasoning_tokens),
        visible_output_tokens: sum_usage(&trace.requests, extended::visible_output_tokens),
        cache_creation_5m_tokens: extended::cache_creation_tokens_for_ttl(
            &trace.requests,
            &policy.cache_profiles,
            CacheTtl::FiveMinutes,
        ),
        cache_creation_30m_tokens: extended::cache_creation_tokens_for_ttl(
            &trace.requests,
            &policy.cache_profiles,
            CacheTtl::ThirtyMinutes,
        ),
        cache_creation_1h_tokens: extended::cache_creation_tokens_for_ttl(
            &trace.requests,
            &policy.cache_profiles,
            CacheTtl::OneHour,
        ),
        provider_total_tokens: sum_usage(&trace.requests, |usage| usage.provider_total_tokens),
        legacy_cost_10000: sum_usage(&trace.requests, |usage| usage.legacy_cost_10000),
        normalized_cost_10000: normalized_cost,
        uncached_input_cost_10000: uncached_input_cost,
        cache_savings_10000: cache_savings,
        cache_savings_basis_points: cache_savings
            .zip(uncached_input_cost)
            .and_then(|(savings, uncached)| ratio_basis_points(savings, uncached)),
        cache_read_write_ratio_basis_points: cache_read
            .zip(cache_creation)
            .and_then(|(read, write)| ratio_basis_points(read, write)),
        cache_write_amplification_basis_points: cache_creation
            .zip(cache_read)
            .and_then(|(write, read)| ratio_basis_points(write, read)),
        silent_cache_threshold_miss_requests: extended::silent_cache_threshold_misses(
            &trace.requests,
            &trace.observations,
            &policy.cache_profiles,
        ),
        cache_key_switches: extended::cache_key_switches(&trace.requests),
        reasoning_config_switches: extended::reasoning_config_switches(&trace.observations),
        pricing_policy_versions,
    };
    if !policy.metrics.token_metrics {
        token_and_cache.fresh_input_tokens = None;
        token_and_cache.total_input_tokens = None;
        token_and_cache.output_tokens = None;
        token_and_cache.reasoning_tokens = None;
        token_and_cache.visible_output_tokens = None;
        token_and_cache.provider_total_tokens = None;
    }
    if !policy.metrics.cache_metrics {
        token_and_cache.cache_read_tokens = None;
        token_and_cache.cache_creation_tokens = None;
        token_and_cache.cache_creation_5m_tokens = None;
        token_and_cache.cache_creation_30m_tokens = None;
        token_and_cache.cache_creation_1h_tokens = None;
        token_and_cache.uncached_input_cost_10000 = None;
        token_and_cache.cache_savings_10000 = None;
        token_and_cache.cache_savings_basis_points = None;
        token_and_cache.cache_read_write_ratio_basis_points = None;
        token_and_cache.cache_write_amplification_basis_points = None;
        token_and_cache.silent_cache_threshold_miss_requests = None;
        token_and_cache.cache_key_switches = 0;
        token_and_cache.reasoning_config_switches = None;
    }
    let measured_tools_and_changes =
        if policy.metrics.tool_metrics || policy.metrics.outcome_metrics {
            tool_and_change_diagnostics(
                &trace.observations,
                &trace.evidence.direct_mcp_intervals,
                &trace.evidence.tool_invocations,
                &trace.requests,
            )
        } else {
            ToolAndChangeDiagnostics::default()
        };
    let file_writes = measured_tools_and_changes
        .file_creates_suspected
        .saturating_add(measured_tools_and_changes.file_edits_suspected)
        .saturating_add(measured_tools_and_changes.file_overwrites_suspected);
    let skills = if policy.metrics.skill_metrics {
        extended::skill_diagnostics(&trace.observations)
    } else {
        SkillDiagnostics::default()
    };
    let reliability = if policy.metrics.reliability_metrics {
        extended::reliability_diagnostics(&trace.requests, &trace.evidence, &trace.observations)
    } else {
        ReliabilityDiagnostics::default()
    };
    let outcome_diagnostics = if policy.metrics.outcome_metrics {
        extended::outcome_diagnostics(
            &trace.requests,
            &trace.observations,
            &trace.evidence,
            extended::OutcomeMetricInputs {
                gateway_outcome: outcome.state,
                actual_cost_10000,
                file_writes,
                unique_files: measured_tools_and_changes.unique_opaque_files,
                rework: measured_tools_and_changes.rework_spans_suspected,
                verification: measured_tools_and_changes.verification_results_classified,
            },
        )
    } else {
        OutcomeDiagnostics::default()
    };
    let finish_reasons = if policy.metrics.finish_reason_metrics {
        extended::finish_reason_diagnostics(&trace.observations)
    } else {
        FinishReasonDiagnostics::default()
    };
    let tools_and_changes = if policy.metrics.tool_metrics {
        measured_tools_and_changes
    } else {
        ToolAndChangeDiagnostics::default()
    };
    let diagnostics = SessionDiagnostics {
        token_and_cache,
        context,
        skills,
        reliability,
        outcome: outcome_diagnostics,
        finish_reasons,
        tools_and_changes,
        enabled_metrics: policy.metrics.clone(),
        semantic_verification_available: false,
    };

    let mut limitations = vec![LimitationCode::SemanticVerificationUnavailable];
    if outcome.incomplete_requests > 0 {
        limitations.push(LimitationCode::RequestIncomplete);
    }
    if !trace.evidence.session_observed {
        limitations.push(LimitationCode::SessionUnobserved);
    }
    if trace.evidence.response_payload_count == 0 {
        limitations.push(LimitationCode::PayloadUnavailable);
    }
    if trace.evidence.truncated_payload_count > 0 {
        limitations.push(LimitationCode::PayloadTruncated);
    }
    if trace.requests.iter().any(|request| request.usage.is_none()) {
        limitations.push(LimitationCode::UsageUnavailable);
    }
    if actual_cost_10000.is_none() {
        limitations.push(LimitationCode::PricingUnavailable);
    }
    if cohort.is_some_and(|cohort| cohort.fallback_level > 0) {
        limitations.push(LimitationCode::CohortFallback);
    }
    limitations.extend(
        trace
            .observations
            .iter()
            .flat_map(|observation| observation.limitations.iter().copied()),
    );
    limitations.sort_unstable_by_key(|value| *value as u8);
    limitations.dedup();

    let confidence = if trace.lifecycle == SessionLifecycleState::Open
        || outcome.state == GatewayOutcomeState::Unknown
        || score.is_none()
        || overall_coverage < 60
    {
        Confidence::Low
    } else if trace.boundary_confidence == Confidence::High
        && overall_coverage >= 80
        && cohort.is_some_and(|cohort| {
            cohort.fallback_level == 0
                && cohort.successful_costs_10000.len() >= MIN_EXACT_COHORT_SIZE
                && cohort.successful_active_time_ms.len() >= MIN_EXACT_COHORT_SIZE
        })
    {
        Confidence::High
    } else {
        Confidence::Medium
    };

    Ok(SessionEfficiencyReport {
        report_schema_version: policy.report_schema_version.clone(),
        analyzer_version: policy.analyzer_version.clone(),
        score_policy_version: policy.score_policy_version.clone(),
        observation_parser_version: policy.observation_parser_version.clone(),
        configuration_version: policy.configuration_version.clone(),
        maturity: policy.maturity,
        calibration_approval_id: policy.calibration_approval_id.clone(),
        confidence,
        gateway_outcome: outcome.state,
        score,
        coverage,
        components: SessionEfficiencyComponents {
            outcome,
            cost_efficiency_basis_points,
            active_time_efficiency_basis_points,
            actual_cost_10000,
            active_time_ms,
            wall_time_ms,
            summed_work_time_ms,
            excluded_gap_time_ms,
            overlap_savings_ms,
            context_penalty_points: diagnostics.context.score_penalty_points,
            unknown_wait_time_ms: None,
            cohort_version: cohort.map(|cohort| cohort.cohort_version.clone()),
            cohort_fallback_level: cohort.map(|cohort| cohort.fallback_level),
            cohort_sample_size: cohort.map_or(0, |cohort| {
                cohort
                    .successful_costs_10000
                    .len()
                    .min(cohort.successful_active_time_ms.len())
            }),
        },
        diagnostics,
        limitations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        id: &str,
        second: i64,
        success: Option<bool>,
        cost: Option<i64>,
    ) -> SessionRequestFact {
        let occurred_at = OffsetDateTime::UNIX_EPOCH + Duration::seconds(second);
        SessionRequestFact {
            request_id: id.to_string(),
            occurred_at,
            completed_at: Some(occurred_at + Duration::seconds(1)),
            terminal_success: success,
            usage: Some(SessionUsageFact {
                fresh_input_tokens: Some(100),
                cache_read_tokens: Some(0),
                output_tokens: Some(10),
                legacy_cost_10000: cost,
                normalized_cost_10000: cost,
                uncached_input_cost_10000: cost,
                pricing_policy_version: Some("test-pricing-v1".to_string()),
                ..SessionUsageFact::default()
            }),
            attempts: Vec::new(),
        }
    }

    fn trace(requests: Vec<SessionRequestFact>) -> SessionTrace {
        let activity_intervals = requests
            .iter()
            .filter_map(|request| {
                request
                    .completed_at
                    .and_then(|end| ActivityInterval::new(request.occurred_at, end))
            })
            .collect();
        SessionTrace {
            requests,
            activity_intervals,
            observations: vec![],
            lifecycle: SessionLifecycleState::Finalized,
            boundary_confidence: Confidence::High,
            evidence: TraceEvidence {
                session_observed: true,
                request_metadata_count: 1,
                response_payload_count: 1,
                truncated_payload_count: 0,
                direct_mcp_intervals: vec![],
                tool_invocations: Vec::new(),
            },
        }
    }

    #[test]
    fn outcome_counts_requests_once_and_excludes_incomplete_requests() {
        let outcome = outcome_component(&[
            request("a", 0, Some(true), Some(1)),
            request("b", 1, Some(false), Some(1)),
            request("c", 2, None, Some(1)),
        ]);
        assert_eq!(outcome.state, GatewayOutcomeState::Partial);
        assert_eq!(outcome.factor_basis_points, 5_000);
        assert_eq!(outcome.successful_requests, 1);
        assert_eq!(outcome.determinate_requests, 2);
        assert_eq!(outcome.incomplete_requests, 1);
    }

    #[test]
    fn unknown_outcome_uses_disclosed_half_prior() {
        let outcome = outcome_component(&[request("a", 0, None, None)]);
        assert_eq!(outcome.state, GatewayOutcomeState::Unknown);
        assert_eq!(outcome.factor_basis_points, 5_000);
    }

    #[test]
    fn midrank_survival_handles_ties_and_policy_clamps() {
        assert_eq!(
            lower_is_better_efficiency_basis_points(2, &[1, 2, 2, 3]),
            Some(5_000)
        );
        assert_eq!(lower_is_better_efficiency_basis_points(99, &[1]), Some(100));
        assert_eq!(
            lower_is_better_efficiency_basis_points(0, &[1]),
            Some(10_000)
        );
        assert_eq!(lower_is_better_efficiency_basis_points(-1, &[1]), None);
    }

    #[test]
    fn active_time_unions_overlap_and_bounded_gaps() {
        let epoch = OffsetDateTime::UNIX_EPOCH;
        let intervals = vec![
            ActivityInterval::new(epoch, epoch + Duration::seconds(10)).expect("interval"),
            ActivityInterval::new(epoch + Duration::seconds(5), epoch + Duration::seconds(20))
                .expect("interval"),
            ActivityInterval::new(epoch + Duration::seconds(30), epoch + Duration::seconds(40))
                .expect("interval"),
        ];
        assert_eq!(
            active_time_milliseconds(intervals, Duration::seconds(15)),
            40_000
        );
    }

    #[test]
    fn failed_session_scores_zero() {
        assert_eq!(
            session_efficiency_score(0, Some(10_000), Some(10_000)),
            Some(0)
        );
    }

    #[test]
    fn score_renormalizes_when_one_efficiency_component_is_missing() {
        assert_eq!(
            session_efficiency_score(10_000, Some(2_500), None),
            Some(59)
        );
        assert_eq!(
            session_efficiency_score(10_000, None, Some(2_500)),
            Some(67)
        );
        assert_eq!(session_efficiency_score(10_000, None, None), None);
    }

    #[test]
    fn diagnostic_ratios_saturate_instead_of_disappearing() {
        assert_eq!(ratio_basis_points(i64::MAX, 1), Some(i32::MAX));
        assert_eq!(ratio_basis_points(i64::MIN, 1), Some(i32::MIN));
        assert_eq!(ratio_basis_points(1, 0), None);
    }

    #[test]
    fn report_keeps_score_optional_without_a_cohort_and_exposes_coverage() {
        let report = analyze_session(
            &trace(vec![request("a", 0, Some(true), Some(100))]),
            &AnalysisPolicy::default(),
            None,
        )
        .expect("report");
        assert_eq!(report.gateway_outcome, GatewayOutcomeState::Succeeded);
        assert_eq!(report.score, None);
        assert_eq!(report.confidence, Confidence::Low);
        assert_eq!(report.coverage.outcome_percent, 100);
        assert_eq!(report.coverage.cohort_percent, 0);
        assert_eq!(
            report.diagnostics.token_and_cache.normalized_cost_10000,
            Some(100)
        );
    }

    #[test]
    fn exact_cohort_requires_six_successful_sessions() {
        let cohort = CohortReference {
            cohort_version: "exact-v1".to_string(),
            fallback_level: 0,
            successful_costs_10000: vec![100; MIN_EXACT_COHORT_SIZE - 1],
            successful_active_time_ms: vec![1_000; MIN_EXACT_COHORT_SIZE - 1],
        };
        let report = analyze_session(
            &trace(vec![request("a", 0, Some(true), Some(100))]),
            &AnalysisPolicy::default(),
            Some(&cohort),
        )
        .expect("report");

        assert_eq!(report.score, None);
        assert_eq!(
            report.components.cohort_sample_size,
            MIN_EXACT_COHORT_SIZE - 1
        );
        assert_eq!(report.confidence, Confidence::Low);
    }

    #[test]
    fn trace_validation_rejects_duplicates_negative_usage_and_invalid_intervals() {
        let duplicate = trace(vec![
            request("same", 0, Some(true), Some(1)),
            request("same", 1, Some(true), Some(1)),
        ]);
        assert!(matches!(
            analyze_session(&duplicate, &AnalysisPolicy::default(), None),
            Err(AnalysisError::DuplicateRequest(id)) if id == "same"
        ));

        let mut negative = request("negative", 0, Some(true), Some(1));
        negative.usage.as_mut().expect("usage").fresh_input_tokens = Some(-1);
        assert!(matches!(
            analyze_session(&trace(vec![negative]), &AnalysisPolicy::default(), None),
            Err(AnalysisError::InvalidUsage(id)) if id == "negative"
        ));

        let mut invalid_interval = request("interval", 0, Some(true), Some(1));
        invalid_interval.completed_at = Some(invalid_interval.occurred_at - Duration::SECOND);
        assert!(matches!(
            analyze_session(
                &trace(vec![invalid_interval]),
                &AnalysisPolicy::default(),
                None
            ),
            Err(AnalysisError::InvalidRequestInterval(id)) if id == "interval"
        ));
    }

    #[test]
    fn policy_rejects_unknown_versions_and_unapproved_calibration() {
        let input = trace(vec![request("a", 0, Some(true), Some(1))]);
        let unsupported = AnalysisPolicy {
            analyzer_version: "future".to_string(),
            ..AnalysisPolicy::default()
        };
        assert!(matches!(
            analyze_session(&input, &unsupported, None),
            Err(AnalysisError::UnsupportedVersion {
                field: "analyzer version",
                ..
            })
        ));
        let unapproved = AnalysisPolicy {
            maturity: ScoreMaturity::Calibrated,
            ..AnalysisPolicy::default()
        };
        assert_eq!(
            analyze_session(&input, &unapproved, None),
            Err(AnalysisError::MissingCalibrationApproval)
        );
    }

    #[test]
    fn report_scores_complete_trace_and_exposes_cache_and_overlap_diagnostics() {
        let cohort = CohortReference {
            cohort_version: "exact-v1".to_string(),
            fallback_level: 0,
            successful_costs_10000: vec![50; MIN_EXACT_COHORT_SIZE],
            successful_active_time_ms: vec![500; MIN_EXACT_COHORT_SIZE],
        };
        let mut session_request = request("a", 0, Some(true), Some(100));
        let usage = session_request.usage.as_mut().expect("usage");
        usage.cache_read_tokens = Some(50);
        usage.cache_creation_tokens = Some(10);
        usage.fresh_input_cost_10000 = Some(100);
        usage.cache_read_cost_10000 = Some(10);
        usage.cache_creation_cost_10000 = Some(20);
        usage.uncached_input_cost_10000 = Some(200);
        let mut session_trace = trace(vec![session_request]);
        session_trace.evidence.direct_mcp_intervals = vec![
            ActivityInterval::new(
                OffsetDateTime::UNIX_EPOCH,
                OffsetDateTime::UNIX_EPOCH + Duration::milliseconds(500),
            )
            .expect("interval"),
        ];

        let report = analyze_session(&session_trace, &AnalysisPolicy::default(), Some(&cohort))
            .expect("report");
        assert_eq!(report.score, Some(10));
        assert_eq!(report.gateway_outcome, GatewayOutcomeState::Succeeded);
        assert_eq!(report.components.active_time_ms, 1_000);
        assert_eq!(report.components.summed_work_time_ms, 1_500);
        assert_eq!(report.components.overlap_savings_ms, 500);
        assert_eq!(
            report.diagnostics.token_and_cache.cache_savings_10000,
            Some(70)
        );
        assert_eq!(
            report
                .diagnostics
                .token_and_cache
                .cache_savings_basis_points,
            Some(3_500)
        );
        assert_eq!(report.diagnostics.tools_and_changes.direct_mcp_calls, 1);
        assert_eq!(
            report.diagnostics.tools_and_changes.direct_mcp_duration_ms,
            Some(500)
        );
    }

    #[test]
    fn orchestration_gaps_do_not_create_negative_overlap_savings() {
        let session_trace = trace(vec![
            request("a", 0, Some(true), Some(100)),
            request("b", 10, Some(true), Some(100)),
        ]);

        let report =
            analyze_session(&session_trace, &AnalysisPolicy::default(), None).expect("report");

        assert_eq!(report.components.summed_work_time_ms, 2_000);
        assert_eq!(report.components.active_time_ms, 11_000);
        assert_eq!(report.components.overlap_savings_ms, 0);
    }

    #[test]
    fn report_exposes_extended_cache_context_reliability_and_change_diagnostics() {
        let mut session_request = request("a", 0, Some(true), Some(100));
        let usage = session_request.usage.as_mut().expect("usage");
        usage.cache_read_tokens = Some(50);
        usage.cache_creation_tokens = Some(10);
        usage.cache_creation_5m_tokens = Some(4);
        usage.cache_creation_1h_tokens = Some(6);
        usage.reasoning_tokens = Some(5);
        usage.provider_total_tokens = Some(165);
        usage.output_includes_reasoning = Some(true);
        session_request.attempts = vec![
            RequestAttemptFact {
                request_id: "a".to_string(),
                attempt_number: 1,
                produced_final_response: false,
                retryable: true,
                status: "provider_error".to_string(),
                status_code: Some(500),
                error_code: Some("upstream".to_string()),
                latency_ms: Some(100),
                provider_key: "anthropic".to_string(),
                upstream_model: "claude".to_string(),
                occurred_at_unix_ms: 0,
            },
            RequestAttemptFact {
                request_id: "a".to_string(),
                attempt_number: 2,
                produced_final_response: true,
                retryable: false,
                status: "succeeded".to_string(),
                status_code: Some(200),
                error_code: None,
                latency_ms: Some(900),
                provider_key: "openai".to_string(),
                upstream_model: "gpt".to_string(),
                occurred_at_unix_ms: 100,
            },
        ];
        let mut session_trace = trace(vec![session_request]);
        session_trace.observations = vec![InferredObservation {
            observation_id: Uuid::nil(),
            kind: InferredObservationKind::FileEditSuspected,
            source_request_id: "a".to_string(),
            parser_version: "test".to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            facts: BoundedObservationFacts {
                supplied_tools: vec![BoundedToolDefinitionFact {
                    name: "github.search_code".to_string(),
                    server_key: Some("github".to_string()),
                    token_estimate: 120,
                }],
                supplied_skills: vec![BoundedSkillFact {
                    name: "review".to_string(),
                    description_token_estimate: Some(50),
                    body_token_estimate: Some(500),
                    resource_token_estimate: Some(100),
                    used: true,
                    abandoned: Some(false),
                }],
                file_interactions: vec![
                    BoundedFileInteractionFact {
                        opaque_file_id: "file-1".to_string(),
                        operation: "edit".to_string(),
                        tool_name: Some("edit".to_string()),
                        succeeded: Some(true),
                        error_signature: None,
                    },
                    BoundedFileInteractionFact {
                        opaque_file_id: "file-1".to_string(),
                        operation: "edit".to_string(),
                        tool_name: Some("edit".to_string()),
                        succeeded: Some(true),
                        error_signature: None,
                    },
                    BoundedFileInteractionFact {
                        opaque_file_id: "file-1".to_string(),
                        operation: "verify".to_string(),
                        tool_name: Some("test".to_string()),
                        succeeded: Some(true),
                        error_signature: None,
                    },
                ],
                finish_reason: Some("length".to_string()),
                ..BoundedObservationFacts::default()
            },
            limitations: Vec::new(),
        }];
        session_trace.evidence.tool_invocations = vec![ToolInvocationFact {
            request_id: "a".to_string(),
            server_key: Some("github".to_string()),
            tool_key: "github.search_code".to_string(),
            status: "failed".to_string(),
            error_code: Some("rate_limited".to_string()),
            latency_ms: Some(200),
            result_payload_truncated: false,
            occurred_at_unix_ms: 0,
        }];

        let report =
            analyze_session(&session_trace, &AnalysisPolicy::default(), None).expect("report");

        assert_eq!(
            report.diagnostics.token_and_cache.cache_creation_5m_tokens,
            Some(4)
        );
        assert_eq!(
            report.diagnostics.token_and_cache.cache_creation_1h_tokens,
            Some(6)
        );
        assert_eq!(report.diagnostics.reliability.wasted_attempts, 1);
        assert_eq!(report.diagnostics.reliability.fallback_attempts, 1);
        assert_eq!(report.diagnostics.reliability.failed_tool_invocations, 1);
        assert_eq!(
            report.diagnostics.outcome.rework_ratio_basis_points,
            Some(5_000)
        );
        assert_eq!(
            report.diagnostics.outcome.verification_rate_basis_points,
            Some(5_000)
        );
        assert_eq!(report.diagnostics.skills.used_skill_count, Some(1));
        assert_eq!(report.diagnostics.finish_reasons.items[0].reason, "length");
        let mut outcome_only_policy = AnalysisPolicy::default();
        outcome_only_policy.metrics.tool_metrics = false;
        let outcome_only = analyze_session(&session_trace, &outcome_only_policy, None)
            .expect("outcome-only report");
        assert_eq!(
            outcome_only.diagnostics.outcome.rework_ratio_basis_points,
            Some(5_000)
        );
        assert_eq!(
            outcome_only
                .diagnostics
                .outcome
                .verification_rate_basis_points,
            Some(5_000)
        );
        assert_eq!(
            outcome_only
                .diagnostics
                .tools_and_changes
                .unique_opaque_files,
            0
        );
    }

    #[test]
    fn cache_profiles_classify_aggregate_writes_and_truncated_payloads_remain_unknown() {
        let mut cache_request = request("cache", 0, Some(true), Some(100));
        let usage = cache_request.usage.as_mut().expect("usage");
        usage.cache_creation_tokens = Some(120);
        usage.provider_key = Some("openai".to_string());
        usage.upstream_model = Some("gpt-5".to_string());
        let cache_policy = AnalysisPolicy {
            cache_profiles: vec![CacheProfileRule {
                provider_key_contains: Some("openai".to_string()),
                upstream_model_contains: Some("gpt".to_string()),
                minimum_cacheable_tokens: 1,
                default_ttl: CacheTtl::ThirtyMinutes,
            }],
            ..AnalysisPolicy::default()
        };
        let cache_report = analyze_session(&trace(vec![cache_request]), &cache_policy, None)
            .expect("cache report");
        assert_eq!(
            cache_report
                .diagnostics
                .token_and_cache
                .cache_creation_30m_tokens,
            Some(120)
        );
        assert_eq!(
            cache_report
                .diagnostics
                .token_and_cache
                .cache_creation_5m_tokens,
            Some(0)
        );

        let mut truncated_trace = trace(vec![request("truncated", 0, Some(true), Some(100))]);
        truncated_trace.evidence.response_payload_count = 1;
        truncated_trace.evidence.truncated_payload_count = 1;
        let truncated_report =
            analyze_session(&truncated_trace, &AnalysisPolicy::default(), None).expect("report");
        assert_eq!(
            truncated_report
                .diagnostics
                .outcome
                .file_signal_coverage_percent,
            0
        );
        assert_eq!(truncated_report.diagnostics.outcome.zero_outcome, None);
    }

    #[test]
    fn tool_schema_diagnostics_use_actual_per_request_exposure() {
        let observation = |request_id: &str, tools: Vec<(&str, u64)>| InferredObservation {
            observation_id: Uuid::new_v4(),
            kind: InferredObservationKind::SessionMetadataClassified,
            source_request_id: request_id.to_string(),
            parser_version: "test".to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            facts: BoundedObservationFacts {
                supplied_tools: tools
                    .into_iter()
                    .map(|(name, token_estimate)| BoundedToolDefinitionFact {
                        server_key: Some("github".to_string()),
                        name: name.to_string(),
                        token_estimate,
                    })
                    .collect(),
                ..BoundedObservationFacts::default()
            },
            limitations: Vec::new(),
        };
        let mut first_request = request("a", 0, Some(true), Some(100));
        first_request
            .usage
            .as_mut()
            .expect("first usage")
            .fresh_input_cost_10000 = Some(100);
        let mut second_request = request("b", 2, Some(true), Some(100));
        second_request
            .usage
            .as_mut()
            .expect("second usage")
            .fresh_input_cost_10000 = Some(100);
        let mut session_trace = trace(vec![first_request, second_request]);
        session_trace.observations = vec![
            observation("a", vec![("search", 100), ("read", 50)]),
            observation("b", vec![("search", 120)]),
        ];

        let report =
            analyze_session(&session_trace, &AnalysisPolicy::default(), None).expect("report");
        let github = &report.diagnostics.tools_and_changes.tool_servers[0];
        assert_eq!(github.exposed_tool_definitions, 2);
        assert_eq!(github.schema_token_estimate_per_request, 135);
        assert_eq!(github.estimated_uncached_schema_cost_10000, Some(270));
    }

    #[test]
    fn report_serialization_is_deterministic_and_round_trips() {
        let cohort = CohortReference {
            cohort_version: "exact-v1".to_string(),
            fallback_level: 0,
            successful_costs_10000: vec![50; MIN_EXACT_COHORT_SIZE],
            successful_active_time_ms: vec![500; MIN_EXACT_COHORT_SIZE],
        };
        let session_trace = trace(vec![request("a", 0, Some(true), Some(100))]);
        let report = analyze_session(&session_trace, &AnalysisPolicy::default(), Some(&cohort))
            .expect("report");
        let recomputed = analyze_session(&session_trace, &AnalysisPolicy::default(), Some(&cohort))
            .expect("report");
        let first = serde_json::to_string(&report).expect("serialize");
        let second = serde_json::to_string(&recomputed).expect("serialize");
        assert_eq!(first, second);
        assert_eq!(
            serde_json::from_str::<SessionEfficiencyReport>(&first).expect("deserialize"),
            report
        );
    }
}
