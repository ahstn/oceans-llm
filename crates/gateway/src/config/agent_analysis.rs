use std::env;

use anyhow::bail;
use gateway_service::{AnalysisMetricPolicy, AnalysisPolicy, CacheProfileRule, CacheTtl};
use serde::Deserialize;

const fn default_enabled() -> bool {
    true
}

const MAX_RETENTION_DAYS: u64 = 36_500;

#[derive(Debug, Clone, Copy)]
pub struct AgentAnalysisRuntimeCapabilities {
    pub passive_analysis_enabled: bool,
    pub shadow_diagnostics_visible: bool,
    pub calibrated_score_visible: bool,
    pub team_admin_analytics_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentAnalysisAccessDecision {
    pub allowed: bool,
    pub score_visible: bool,
    pub shadow_visible: bool,
}

impl AgentAnalysisRuntimeCapabilities {
    #[must_use]
    pub fn access_for(self, platform_admin: bool, team_admin: bool) -> AgentAnalysisAccessDecision {
        let score_visible = self.calibrated_score_visible
            && (platform_admin || (team_admin && self.team_admin_analytics_enabled));
        let shadow_visible = platform_admin && self.shadow_diagnostics_visible;
        AgentAnalysisAccessDecision {
            allowed: score_visible || shadow_visible,
            score_visible,
            shadow_visible,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAnalysisConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub shadow_diagnostics_enabled: bool,
    #[serde(default)]
    pub calibrated_score_enabled: bool,
    #[serde(default)]
    pub calibration_approval_id: Option<String>,
    #[serde(default)]
    pub team_admin_enabled: bool,
    #[serde(default = "default_report_retention_days")]
    pub report_retention_days: u64,
    #[serde(default = "default_queue_retention_days")]
    pub queue_retention_days: u64,
    #[serde(default = "default_context_input_boundary_tokens")]
    pub context_input_boundary_tokens: i64,
    #[serde(default = "default_context_reserved_output_tokens")]
    pub context_reserved_output_tokens: i64,
    #[serde(default = "default_context_penalty_points")]
    pub context_penalty_points_per_repeated_excess: u8,
    #[serde(default)]
    pub metrics: AgentAnalysisMetricsConfig,
    #[serde(default)]
    pub cache_profiles: Vec<AgentAnalysisCacheProfileConfig>,
}

impl Default for AgentAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shadow_diagnostics_enabled: false,
            calibrated_score_enabled: false,
            calibration_approval_id: None,
            team_admin_enabled: false,
            report_retention_days: default_report_retention_days(),
            queue_retention_days: default_queue_retention_days(),
            context_input_boundary_tokens: default_context_input_boundary_tokens(),
            context_reserved_output_tokens: default_context_reserved_output_tokens(),
            context_penalty_points_per_repeated_excess: default_context_penalty_points(),
            metrics: AgentAnalysisMetricsConfig::default(),
            cache_profiles: Vec::new(),
        }
    }
}

impl AgentAnalysisConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        if self
            .calibration_approval_id
            .as_ref()
            .is_some_and(|value| value.len() > 256 || value.trim() != value)
        {
            bail!("agent_analysis.calibration_approval_id must be trimmed and at most 256 bytes");
        }
        if self.report_retention_days > MAX_RETENTION_DAYS
            || self.queue_retention_days > MAX_RETENTION_DAYS
        {
            bail!("agent analysis retention must not exceed 36500 days");
        }
        if self.context_input_boundary_tokens <= 0 {
            bail!("agent_analysis.context_input_boundary_tokens must be > 0");
        }
        if self.context_reserved_output_tokens < 0 {
            bail!("agent_analysis.context_reserved_output_tokens must be >= 0");
        }
        for profile in &self.cache_profiles {
            profile.validate()?;
        }
        Ok(())
    }

    pub fn resolve(&self) -> anyhow::Result<LoadedAgentAnalysis> {
        let calibrated_score_visible = environment_flag(
            "AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED",
            self.calibrated_score_enabled,
        )?;
        let team_admin_analytics_enabled =
            environment_flag("AGENT_ANALYSIS_TEAM_ADMIN_ENABLED", self.team_admin_enabled)?;
        let calibration_approval_id = env::var("AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID")
            .ok()
            .or_else(|| self.calibration_approval_id.clone())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if calibrated_score_visible && calibration_approval_id.is_none() {
            bail!(
                "calibrated agent analysis requires agent_analysis.calibration_approval_id or AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID"
            );
        }
        if calibration_approval_id
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        {
            bail!("agent analysis calibration approval ID must not exceed 256 bytes");
        }
        if team_admin_analytics_enabled && !calibrated_score_visible {
            bail!("team agent analytics require calibrated score visibility");
        }

        let mut cache_profiles = self
            .cache_profiles
            .iter()
            .map(AgentAnalysisCacheProfileConfig::to_rule)
            .collect::<Vec<_>>();
        cache_profiles.extend(default_cache_profiles());

        Ok(LoadedAgentAnalysis {
            capabilities: AgentAnalysisRuntimeCapabilities {
                passive_analysis_enabled: environment_flag("AGENT_ANALYSIS_ENABLED", self.enabled)?,
                shadow_diagnostics_visible: environment_flag(
                    "AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED",
                    self.shadow_diagnostics_enabled,
                )?,
                calibrated_score_visible,
                team_admin_analytics_enabled,
            },
            policy: AnalysisPolicy {
                maturity: if calibrated_score_visible {
                    gateway_core::ScoreMaturity::Calibrated
                } else {
                    gateway_core::ScoreMaturity::Experimental
                },
                calibration_approval_id,
                metrics: AnalysisMetricPolicy {
                    token_metrics: self.metrics.tokens,
                    cache_metrics: self.metrics.cache,
                    context_metrics: self.metrics.context,
                    tool_metrics: self.metrics.tools,
                    skill_metrics: self.metrics.skills,
                    reliability_metrics: self.metrics.reliability,
                    outcome_metrics: self.metrics.outcomes,
                    finish_reason_metrics: self.metrics.finish_reasons,
                },
                context_input_boundary_tokens: self.context_input_boundary_tokens,
                context_reserved_output_tokens: self.context_reserved_output_tokens,
                context_penalty_points_per_repeated_excess: self
                    .context_penalty_points_per_repeated_excess,
                cache_profiles,
                ..AnalysisPolicy::default()
            },
            report_retention: retention_days(
                "AGENT_ANALYSIS_REPORT_RETENTION_DAYS",
                self.report_retention_days,
            ),
            queue_retention: retention_days(
                "AGENT_ANALYSIS_QUEUE_RETENTION_DAYS",
                self.queue_retention_days,
            ),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAnalysisMetricsConfig {
    #[serde(default = "default_enabled")]
    pub tokens: bool,
    #[serde(default = "default_enabled")]
    pub cache: bool,
    #[serde(default = "default_enabled")]
    pub context: bool,
    #[serde(default = "default_enabled")]
    pub tools: bool,
    #[serde(default = "default_enabled")]
    pub skills: bool,
    #[serde(default = "default_enabled")]
    pub reliability: bool,
    #[serde(default = "default_enabled")]
    pub outcomes: bool,
    #[serde(default = "default_enabled")]
    pub finish_reasons: bool,
}

impl Default for AgentAnalysisMetricsConfig {
    fn default() -> Self {
        Self {
            tokens: true,
            cache: true,
            context: true,
            tools: true,
            skills: true,
            reliability: true,
            outcomes: true,
            finish_reasons: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAnalysisCacheProfileConfig {
    #[serde(default)]
    pub provider_key_contains: Option<String>,
    #[serde(default)]
    pub upstream_model_contains: Option<String>,
    pub minimum_cacheable_tokens: i64,
    #[serde(default)]
    pub default_ttl: AgentAnalysisCacheTtlConfig,
}

impl AgentAnalysisCacheProfileConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self
            .provider_key_contains
            .as_deref()
            .is_none_or(str::is_empty)
            && self
                .upstream_model_contains
                .as_deref()
                .is_none_or(str::is_empty)
        {
            bail!("agent analysis cache profiles require a provider or model match");
        }
        if self.minimum_cacheable_tokens <= 0 {
            bail!("agent analysis cache profile token minimums must be > 0");
        }
        Ok(())
    }

    fn to_rule(&self) -> CacheProfileRule {
        CacheProfileRule {
            provider_key_contains: self.provider_key_contains.clone(),
            upstream_model_contains: self.upstream_model_contains.clone(),
            minimum_cacheable_tokens: self.minimum_cacheable_tokens,
            default_ttl: self.default_ttl.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentAnalysisCacheTtlConfig {
    #[default]
    FiveMinutes,
    ThirtyMinutes,
    OneHour,
    Unknown,
}

impl From<AgentAnalysisCacheTtlConfig> for CacheTtl {
    fn from(value: AgentAnalysisCacheTtlConfig) -> Self {
        match value {
            AgentAnalysisCacheTtlConfig::FiveMinutes => Self::FiveMinutes,
            AgentAnalysisCacheTtlConfig::ThirtyMinutes => Self::ThirtyMinutes,
            AgentAnalysisCacheTtlConfig::OneHour => Self::OneHour,
            AgentAnalysisCacheTtlConfig::Unknown => Self::Unknown,
        }
    }
}

pub struct LoadedAgentAnalysis {
    pub capabilities: AgentAnalysisRuntimeCapabilities,
    pub policy: AnalysisPolicy,
    pub report_retention: time::Duration,
    pub queue_retention: time::Duration,
}

fn environment_flag(name: &str, default: bool) -> anyhow::Result<bool> {
    let value = match env::var(name) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(default),
        Err(env::VarError::NotUnicode(_)) => bail!("{name} must be valid UTF-8"),
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{name} must be a boolean"),
    }
}

fn retention_days(key: &str, default: u64) -> time::Duration {
    let days = env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
        .min(MAX_RETENTION_DAYS);
    time::Duration::days(i64::try_from(days).expect("retention limit fits i64"))
}

fn default_cache_profiles() -> Vec<CacheProfileRule> {
    use CacheTtl::{FiveMinutes, ThirtyMinutes};

    [
        (Some("bedrock"), Some("gpt-5.6"), 1_024, ThirtyMinutes),
        (
            Some("bedrock"),
            Some("claude-sonnet-4-5"),
            4_096,
            FiveMinutes,
        ),
        (
            Some("bedrock"),
            Some("claude-haiku-4-5"),
            4_096,
            FiveMinutes,
        ),
        (Some("bedrock"), Some("claude-opus-4-5"), 4_096, FiveMinutes),
        (Some("bedrock"), Some("claude-opus-4-6"), 4_096, FiveMinutes),
        (None, Some("claude-opus-5"), 512, FiveMinutes),
        (None, Some("claude-opus-4-7"), 2_048, FiveMinutes),
        (None, Some("claude-opus-4-6"), 4_096, FiveMinutes),
        (None, Some("claude-opus-4-5"), 4_096, FiveMinutes),
        (None, Some("claude-haiku-4-5"), 4_096, FiveMinutes),
        (None, Some("claude-sonnet"), 1_024, FiveMinutes),
        (None, Some("gpt-5.6"), 1_024, ThirtyMinutes),
    ]
    .into_iter()
    .map(
        |(
            provider_key_contains,
            upstream_model_contains,
            minimum_cacheable_tokens,
            default_ttl,
        )| {
            CacheProfileRule {
                provider_key_contains: provider_key_contains.map(str::to_string),
                upstream_model_contains: upstream_model_contains.map(str::to_string),
                minimum_cacheable_tokens,
                default_ttl,
            }
        },
    )
    .collect()
}

const fn default_report_retention_days() -> u64 {
    90
}

const fn default_queue_retention_days() -> u64 {
    7
}

const fn default_context_input_boundary_tokens() -> i64 {
    220_000
}

const fn default_context_reserved_output_tokens() -> i64 {
    128_000
}

const fn default_context_penalty_points() -> u8 {
    2
}
