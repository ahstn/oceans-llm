//! Backend-neutral helpers for rate-limit policies and counters.

use gateway_core::{
    RateLimitCounterCharge, RateLimitMetric, RateLimitOperation, RateLimitPeriod,
    RateLimitPolicyRecord, RateLimitScopeKind, StoreError,
};

use crate::shared::{parse_uuid, unix_to_datetime};

pub(crate) const POLICY_COLUMNS: &str = "rate_limit_policy_id, name, operation, scope_kind, \
     scope_id, model_key, mcp_server_id, metric, period, limit_value, enabled, created_at, \
     updated_at";

/// One policy row as both backends read it, before parsing.
pub(crate) struct RawPolicyRow {
    pub rate_limit_policy_id: String,
    pub name: String,
    pub operation: String,
    pub scope_kind: String,
    pub scope_id: Option<String>,
    pub model_key: Option<String>,
    pub mcp_server_id: Option<String>,
    pub metric: String,
    pub period: String,
    pub limit: i64,
    pub enabled: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl RawPolicyRow {
    pub(crate) fn decode(self) -> Result<RateLimitPolicyRecord, StoreError> {
        Ok(RateLimitPolicyRecord {
            rate_limit_policy_id: parse_uuid(&self.rate_limit_policy_id)?,
            name: self.name,
            operation: parse(&self.operation, RateLimitOperation::from_db, "operation")?,
            scope_kind: parse(&self.scope_kind, RateLimitScopeKind::from_db, "scope kind")?,
            scope_id: self.scope_id.as_deref().map(parse_uuid).transpose()?,
            model_key: self.model_key,
            mcp_server_id: self.mcp_server_id.as_deref().map(parse_uuid).transpose()?,
            metric: parse(&self.metric, RateLimitMetric::from_db, "metric")?,
            period: parse(&self.period, RateLimitPeriod::from_db, "period")?,
            limit: self.limit,
            enabled: self.enabled == 1,
            created_at: unix_to_datetime(self.created_at)?,
            updated_at: unix_to_datetime(self.updated_at)?,
        })
    }
}

fn parse<T>(value: &str, from_db: fn(&str) -> Option<T>, what: &str) -> Result<T, StoreError> {
    from_db(value)
        .ok_or_else(|| StoreError::Serialization(format!("unknown rate limit {what} `{value}`")))
}

/// Indices of `charges` in lock order. Every transaction takes counter rows in
/// the same order, so two concurrent checks cannot deadlock.
pub(crate) fn lock_order(charges: &[RateLimitCounterCharge]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..charges.len()).collect();
    order.sort_by(|left, right| {
        let (left, right) = (&charges[*left], &charges[*right]);
        (left.counter_key.as_str(), left.window.start)
            .cmp(&(right.counter_key.as_str(), right.window.start))
    });
    order
}

/// Bounded so a purge never holds a long write lock.
pub(crate) const MAX_PURGE_BATCH: i64 = 5_000;
