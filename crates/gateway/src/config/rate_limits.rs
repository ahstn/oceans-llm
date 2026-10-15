use std::{collections::BTreeMap, collections::BTreeSet, time::Duration};

use anyhow::{Context, bail};
use gateway_core::{
    RateLimitMetric, RateLimitOperation, RateLimitPeriod, RateLimitPolicyShape, RateLimitScopeKind,
};
use gateway_service::{ConfiguredRateLimitPolicy, RateLimitFailureMode, RateLimitSettings};
use serde::Deserialize;

use super::models::{ModelConfig, normalize_config_model_key};

const MAX_POLICY_REFRESH_SECONDS: u64 = 3_600;

/// `rate_limits`: platform-wide defaults and how enforcement behaves. Admins
/// add specific policies in the admin UI.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitsConfig {
    /// What happens when the counter store fails: `open` (default) or `closed`.
    #[serde(default)]
    pub failure_mode: RateLimitFailureModeConfig,
    /// How often each replica reloads admin-managed policies.
    #[serde(default = "default_policy_refresh_seconds")]
    pub policy_refresh_seconds: u64,
    #[serde(default)]
    pub policies: Vec<RateLimitPolicyConfig>,
}

impl Default for RateLimitsConfig {
    fn default() -> Self {
        Self {
            failure_mode: RateLimitFailureModeConfig::default(),
            policy_refresh_seconds: default_policy_refresh_seconds(),
            policies: Vec::new(),
        }
    }
}

const fn default_policy_refresh_seconds() -> u64 {
    10
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitFailureModeConfig {
    #[default]
    Open,
    Closed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitPolicyConfig {
    pub name: String,
    pub operation: RateLimitOperation,
    /// `global`, `api_key`, `user` or `team`. Non-global scopes give each
    /// principal its own counter.
    pub scope: RateLimitScopeKind,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mcp_server: Option<String>,
    #[serde(default = "default_metric")]
    pub metric: RateLimitMetric,
    pub period: RateLimitPeriod,
    pub limit: i64,
}

const fn default_metric() -> RateLimitMetric {
    RateLimitMetric::Requests
}

impl RateLimitsConfig {
    pub(super) fn validate(
        &self,
        model_by_id: &BTreeMap<&str, &ModelConfig>,
    ) -> anyhow::Result<()> {
        if self.policy_refresh_seconds == 0
            || self.policy_refresh_seconds > MAX_POLICY_REFRESH_SECONDS
        {
            bail!(
                "rate_limits.policy_refresh_seconds must be between 1 and {MAX_POLICY_REFRESH_SECONDS}"
            );
        }
        let mut names = BTreeSet::new();
        for policy in &self.policies {
            let label = format!("rate_limits.policies `{}`", policy.name.trim());
            RateLimitPolicyShape {
                name: &policy.name,
                operation: policy.operation,
                scope_kind: policy.scope,
                has_scope_id: false,
                model_key: policy.model.as_deref(),
                has_mcp_server: policy.mcp_server.is_some(),
                metric: policy.metric,
                limit: policy.limit,
            }
            .validate()
            .map_err(|message| anyhow::anyhow!("{label}: {message}"))?;
            if !names.insert(policy.name.trim()) {
                bail!("duplicate {label}");
            }
            if let Some(model) = &policy.model {
                let model_key = normalize_config_model_key(model).context(label.clone())?;
                if !model_by_id.contains_key(model_key.as_str()) {
                    bail!("{label} references unknown model `{model_key}`");
                }
            }
            if let Some(server) = &policy.mcp_server
                && server.trim().is_empty()
            {
                bail!("{label}: mcp_server must not be empty");
            }
        }
        Ok(())
    }

    /// Policies whose MCP server is not in the registry never match. They are
    /// kept so the policy applies once a server with that key is added.
    #[must_use]
    pub fn mcp_server_keys(&self) -> BTreeSet<&str> {
        self.policies
            .iter()
            .filter_map(|policy| policy.mcp_server.as_deref())
            .map(str::trim)
            .collect()
    }

    #[must_use]
    pub fn settings(&self) -> RateLimitSettings {
        RateLimitSettings {
            failure_mode: match self.failure_mode {
                RateLimitFailureModeConfig::Open => RateLimitFailureMode::Open,
                RateLimitFailureModeConfig::Closed => RateLimitFailureMode::Closed,
            },
            policy_refresh: Duration::from_secs(self.policy_refresh_seconds),
            config_policies: self
                .policies
                .iter()
                .map(|policy| ConfiguredRateLimitPolicy {
                    name: policy.name.trim().to_string(),
                    operation: policy.operation,
                    scope_kind: policy.scope,
                    model_key: policy
                        .model
                        .as_deref()
                        .map(|model| model.trim().to_string()),
                    mcp_server_key: policy
                        .mcp_server
                        .as_deref()
                        .map(|server| server.trim().to_string()),
                    metric: policy.metric,
                    period: policy.period,
                    limit: policy.limit,
                })
                .collect(),
        }
    }
}
