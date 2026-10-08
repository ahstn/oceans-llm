//! Bounded retry and cooldown settings for an explicitly configured routing pool.
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ProviderFailureKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderFailoverPolicy {
    pub max_retries_per_route: u32,
    pub max_attempts: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub quota_cooldown_seconds: u32,
    pub transient_cooldown_seconds: u32,
    pub max_cooldown_seconds: u32,
}

impl Default for ProviderFailoverPolicy {
    fn default() -> Self {
        Self {
            max_retries_per_route: 2,
            max_attempts: 6,
            initial_backoff_ms: 250,
            max_backoff_ms: 2_000,
            quota_cooldown_seconds: 300,
            transient_cooldown_seconds: 30,
            max_cooldown_seconds: 86_400,
        }
    }
}

impl ProviderFailoverPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_retries_per_route > 5 || !(1..=32).contains(&self.max_attempts) {
            return Err(
                "routing.failover requires at most 5 retries per route and 1 to 32 total attempts"
                    .into(),
            );
        }
        if self.initial_backoff_ms == 0
            || self.initial_backoff_ms > self.max_backoff_ms
            || self.max_backoff_ms > 30_000
        {
            return Err(
                "routing.failover requires 0 < initial_backoff_ms <= max_backoff_ms <= 30000"
                    .into(),
            );
        }
        if self.quota_cooldown_seconds == 0
            || self.transient_cooldown_seconds == 0
            || self.max_cooldown_seconds < self.quota_cooldown_seconds
            || self.max_cooldown_seconds < self.transient_cooldown_seconds
            || self.max_cooldown_seconds > 604_800
        {
            return Err("routing.failover cooldowns must be positive, no greater than max_cooldown_seconds, and capped at 604800 seconds".into());
        }
        Ok(())
    }

    /// A long provider wait skips same-route retries instead of retrying too early.
    pub fn retry_delay(
        &self,
        retries_so_far: u32,
        retry_after: Option<Duration>,
    ) -> Option<Duration> {
        if retries_so_far >= self.max_retries_per_route {
            return None;
        }
        let cap = Duration::from_millis(self.max_backoff_ms);
        if retry_after.is_some_and(|delay| delay > cap) {
            return None;
        }
        let factor = 1_u64.checked_shl(retries_so_far).unwrap_or(u64::MAX);
        let backoff = Duration::from_millis(
            self.initial_backoff_ms
                .saturating_mul(factor)
                .min(self.max_backoff_ms),
        );
        Some(backoff.max(retry_after.unwrap_or_default()))
    }

    pub fn cooldown(&self, kind: ProviderFailureKind, retry_after: Option<Duration>) -> Duration {
        let seconds = match kind {
            ProviderFailureKind::Quota | ProviderFailureKind::Credential => {
                self.quota_cooldown_seconds
            }
            ProviderFailureKind::Transient => self.transient_cooldown_seconds,
            ProviderFailureKind::Terminal => return Duration::ZERO,
        };
        Duration::from_secs(u64::from(seconds))
            .max(retry_after.unwrap_or_default())
            .min(Duration::from_secs(u64::from(self.max_cooldown_seconds)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_policy_has_bounded_defaults_and_rejects_misspelled_options() {
        let policy: ProviderFailoverPolicy = serde_json::from_value(json!({})).unwrap();
        assert_eq!(policy, ProviderFailoverPolicy::default());
        assert!(policy.validate().is_ok());
        assert!(serde_json::from_value::<ProviderFailoverPolicy>(json!({"retries": 2})).is_err());
    }

    #[test]
    fn retry_after_never_causes_an_early_retry_or_an_unbounded_wait() {
        let policy = ProviderFailoverPolicy::default();
        assert_eq!(
            policy.retry_delay(0, None),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            policy.retry_delay(1, None),
            Some(Duration::from_millis(500))
        );
        assert_eq!(policy.retry_delay(2, None), None);
        assert_eq!(
            policy.retry_delay(0, Some(Duration::from_secs(1))),
            Some(Duration::from_secs(1))
        );
        assert_eq!(policy.retry_delay(0, Some(Duration::from_secs(3600))), None);
        assert_eq!(
            policy.cooldown(ProviderFailureKind::Quota, None),
            Duration::from_secs(300)
        );
        assert_eq!(
            policy.cooldown(ProviderFailureKind::Transient, Some(Duration::MAX)),
            Duration::from_secs(86400)
        );
        assert_eq!(
            policy.cooldown(ProviderFailureKind::Terminal, None),
            Duration::ZERO
        );
    }

    #[test]
    fn validates_attempt_backoff_and_cooldown_bounds() {
        for value in [
            json!({"max_attempts": 0}),
            json!({"max_attempts": 33}),
            json!({"max_retries_per_route": 6}),
            json!({"initial_backoff_ms": 0}),
            json!({"max_backoff_ms": 100}),
            json!({"max_backoff_ms": 30001}),
            json!({"quota_cooldown_seconds": 0}),
            json!({"transient_cooldown_seconds": 0}),
            json!({"max_cooldown_seconds": 10}),
            json!({"max_cooldown_seconds": 604801}),
        ] {
            let policy: ProviderFailoverPolicy = serde_json::from_value(value.clone()).unwrap();
            assert!(policy.validate().is_err(), "{value}");
        }
    }
}
