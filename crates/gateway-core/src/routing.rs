use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::StoreError;

/// Selects a route when no active session binding applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    Preferred,
    #[default]
    WeightedRandom,
    RoundRobin,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRoutingPolicy {
    #[serde(default)]
    pub strategy: RoutingStrategy,
    #[serde(default)]
    pub affinity: Option<SessionAffinityPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failover: Option<crate::ProviderFailoverPolicy>,
}

impl ModelRoutingPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(affinity) = &self.affinity {
            affinity.validate()?;
        }
        if let Some(failover) = &self.failover {
            failover.validate()?;
        }
        Ok(())
    }
}

/// A successful request extends its route binding by this idle interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAffinityPolicy {
    #[serde(default = "default_idle_timeout_seconds")]
    pub idle_timeout_seconds: u32,
}

const fn default_idle_timeout_seconds() -> u32 {
    3_600
}

impl Default for SessionAffinityPolicy {
    fn default() -> Self {
        Self {
            idle_timeout_seconds: default_idle_timeout_seconds(),
        }
    }
}

impl SessionAffinityPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.idle_timeout_seconds == 0 {
            return Err("routing.affinity.idle_timeout_seconds must be positive".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingCandidate {
    pub route_id: Uuid,
    pub fingerprint: String,
    pub priority: i32,
    /// Opaque route and credential identity for shared failure cooldowns.
    pub cooldown_key: Option<String>,
}

/// Weighted ordering is computed by the service before the atomic store selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSelectionMode {
    First,
    RoundRobin,
}

#[derive(Debug, Clone)]
pub struct RouteSelectionRequest {
    pub model_id: Uuid,
    pub affinity_key: Option<String>,
    pub candidates: Vec<RoutingCandidate>,
    pub mode: RouteSelectionMode,
    pub now: OffsetDateTime,
    pub idle_timeout_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteBindingReceipt {
    pub model_id: Uuid,
    pub affinity_key: String,
    pub token: Uuid,
    pub route_id: Uuid,
    pub idle_timeout_seconds: u32,
}

#[derive(Debug, Clone)]
pub struct RouteFailureRecord {
    pub cooldown_key: Option<String>,
    pub cooldown_until: OffsetDateTime,
    pub binding: Option<RouteBindingReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSelection {
    pub route_id: Uuid,
    pub binding: Option<RouteBindingReceipt>,
    pub reused: bool,
}

/// The provider route that owns an opaque Responses API identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseRouteOrigin {
    pub model_id: Uuid,
    pub route_id: Uuid,
    pub fingerprint: String,
}

#[async_trait]
pub trait RoutingRepository: Send + Sync {
    /// Atomically reuses a valid binding or assigns one route to a new session.
    /// Returns None when cooldowns exclude every supplied candidate.
    async fn select_route(
        &self,
        request: &RouteSelectionRequest,
    ) -> Result<Option<RouteSelection>, StoreError>;

    /// Atomically extends a cooldown and removes only the failed binding token.
    async fn record_route_failure(&self, failure: &RouteFailureRecord) -> Result<(), StoreError>;

    /// Refreshes only the binding represented by this receipt after upstream success.
    async fn refresh_route_binding(
        &self,
        receipt: &RouteBindingReceipt,
        now: OffsetDateTime,
    ) -> Result<(), StoreError>;

    async fn get_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ResponseRouteOrigin>, StoreError>;

    /// Records immutable route ownership for thirty days; matching writes extend retention.
    async fn record_response_route_origin(
        &self,
        owner_key: &str,
        response_id_hash: &str,
        origin: &ResponseRouteOrigin,
        now: OffsetDateTime,
    ) -> Result<(), StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_policy_preserves_weighted_selection_without_affinity() {
        let policy: ModelRoutingPolicy = serde_json::from_value(json!({})).unwrap();
        assert_eq!(policy, ModelRoutingPolicy::default());
        assert_eq!(policy.strategy, RoutingStrategy::WeightedRandom);
        assert_eq!(policy.affinity, None);
    }

    #[test]
    fn affinity_defaults_to_one_hour() {
        let policy: ModelRoutingPolicy = serde_json::from_value(json!({
            "strategy": "round_robin",
            "affinity": {}
        }))
        .unwrap();
        assert_eq!(policy.strategy, RoutingStrategy::RoundRobin);
        assert_eq!(policy.affinity, Some(SessionAffinityPolicy::default()));
        assert_eq!(policy.validate(), Ok(()));
    }

    #[test]
    fn rejects_unknown_policy_fields_and_strategies() {
        for value in [
            json!({"stratgey": "preferred"}),
            json!({"strategy": "random"}),
            json!({"affinity": {"idle_timeout": 3600}}),
            json!({"affinity": {"idle_timeout_seconds": 4_294_967_296_u64}}),
        ] {
            assert!(serde_json::from_value::<ModelRoutingPolicy>(value).is_err());
        }
    }

    #[test]
    fn rejects_zero_idle_timeout() {
        let policy = ModelRoutingPolicy {
            affinity: Some(SessionAffinityPolicy {
                idle_timeout_seconds: 0,
            }),
            ..ModelRoutingPolicy::default()
        };
        assert!(policy.validate().is_err());
    }
}
