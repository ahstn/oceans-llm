//! Rate-limit policies and the window arithmetic shared by every store.
//!
//! See `docs/adr/2026-10-15-gateway-rate-limits-and-quotas.md`.

use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, Time};
use uuid::Uuid;

/// The kind of traffic a policy counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitOperation {
    /// Chat Completions, Responses, Messages, Embeddings and Decisions.
    Inference,
    CountTokens,
    McpToolCall,
}

impl RateLimitOperation {
    pub const ALL: [Self; 3] = [Self::Inference, Self::CountTokens, Self::McpToolCall];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inference => "inference",
            Self::CountTokens => "count_tokens",
            Self::McpToolCall => "mcp_tool_call",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Human-readable name for caller-facing messages.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Inference => "inference request",
            Self::CountTokens => "token count",
            Self::McpToolCall => "MCP tool call",
        }
    }
}

/// Who shares a counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitScopeKind {
    /// One counter for all callers.
    Global,
    ApiKey,
    User,
    Team,
}

impl RateLimitScopeKind {
    pub const ALL: [Self; 4] = [Self::Global, Self::ApiKey, Self::User, Self::Team];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::ApiKey => "api_key",
            Self::User => "user",
            Self::Team => "team",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Global => "gateway-wide",
            Self::ApiKey => "API key",
            Self::User => "user",
            Self::Team => "team",
        }
    }
}

/// What a counter adds up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitMetric {
    Requests,
    /// Total tokens from the provider's usage report; inference only.
    Tokens,
}

impl RateLimitMetric {
    pub const ALL: [Self; 2] = [Self::Requests, Self::Tokens];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::Tokens => "tokens",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// The window a limit applies to. `Minute` and `Hour` are sliding windows;
/// `Day` and `Month` are fixed UTC calendar windows (quotas).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitPeriod {
    Minute,
    Hour,
    Day,
    Month,
}

impl RateLimitPeriod {
    pub const ALL: [Self; 4] = [Self::Minute, Self::Hour, Self::Day, Self::Month];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Month => "month",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    #[must_use]
    pub const fn is_sliding(self) -> bool {
        matches!(self, Self::Minute | Self::Hour)
    }
}

/// An admin-managed policy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitPolicyRecord {
    pub rate_limit_policy_id: Uuid,
    pub name: String,
    pub operation: RateLimitOperation,
    pub scope_kind: RateLimitScopeKind,
    /// `None` gives each principal of `scope_kind` its own counter.
    pub scope_id: Option<Uuid>,
    pub model_key: Option<String>,
    pub mcp_server_id: Option<Uuid>,
    pub metric: RateLimitMetric,
    pub period: RateLimitPeriod,
    pub limit: i64,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl RateLimitPolicyRecord {
    /// Whether the two policies enforce the same limit, ignoring timestamps.
    #[must_use]
    pub fn same_settings(&self, other: &Self) -> bool {
        self.name == other.name
            && self.operation == other.operation
            && self.scope_kind == other.scope_kind
            && self.scope_id == other.scope_id
            && self.model_key == other.model_key
            && self.mcp_server_id == other.mcp_server_id
            && self.metric == other.metric
            && self.period == other.period
            && self.limit == other.limit
            && self.enabled == other.enabled
    }
}

/// The longest policy name config and the admin API accept, in characters.
pub const MAX_RATE_LIMIT_POLICY_NAME_LEN: usize = 100;

/// The most enabled policies, config and admin together, one operation may
/// have. Every request is checked against its operation's policies, so this
/// bounds the per-request matching and counter work.
pub const MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION: usize = 100;

/// The filters and scope a policy matches on, for shape validation.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitPolicyShape<'a> {
    pub name: &'a str,
    pub operation: RateLimitOperation,
    pub scope_kind: RateLimitScopeKind,
    pub has_scope_id: bool,
    pub model_key: Option<&'a str>,
    pub has_mcp_server: bool,
    pub metric: RateLimitMetric,
    pub limit: i64,
}

impl RateLimitPolicyShape<'_> {
    /// Rejects combinations that could never match or could never be counted.
    /// Config and the admin API share these rules; the table repeats them.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_RATE_LIMIT_POLICY_NAME_LEN {
            return Err(format!(
                "name must be 1 to {MAX_RATE_LIMIT_POLICY_NAME_LEN} characters"
            ));
        }
        if self.limit <= 0 {
            return Err("limit must be greater than 0".to_string());
        }
        if self.scope_kind == RateLimitScopeKind::Global && self.has_scope_id {
            return Err("a global policy cannot target a specific principal".to_string());
        }
        if self.metric == RateLimitMetric::Tokens && self.operation != RateLimitOperation::Inference
        {
            return Err("token limits apply only to the `inference` operation".to_string());
        }
        if self.has_mcp_server && self.operation != RateLimitOperation::McpToolCall {
            return Err("an MCP server filter applies only to `mcp_tool_call`".to_string());
        }
        if let Some(model_key) = self.model_key {
            if self.operation == RateLimitOperation::McpToolCall {
                return Err("a model filter does not apply to `mcp_tool_call`".to_string());
            }
            if model_key.trim().is_empty() {
                return Err("model must not be empty".to_string());
            }
        }
        Ok(())
    }
}

/// The counter window that contains one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimitWindow {
    pub period: RateLimitPeriod,
    /// Unix seconds.
    pub start: i64,
    /// Unix seconds, exclusive.
    pub end: i64,
    /// Start of the previous window for sliding periods.
    pub previous_start: Option<i64>,
    /// Share of the previous window still inside the sliding window, in `[0, 1]`.
    pub previous_weight: f64,
}

impl RateLimitWindow {
    #[must_use]
    pub fn at(period: RateLimitPeriod, now: OffsetDateTime) -> Self {
        let now_seconds = now.unix_timestamp();
        match period {
            RateLimitPeriod::Minute | RateLimitPeriod::Hour => {
                let length = if period == RateLimitPeriod::Minute {
                    60
                } else {
                    3_600
                };
                let start = now_seconds - now_seconds.rem_euclid(length);
                let elapsed =
                    (now_seconds - start) as f64 + f64::from(now.nanosecond()) / 1_000_000_000.0;
                Self {
                    period,
                    start,
                    end: start + length,
                    previous_start: Some(start - length),
                    previous_weight: (1.0 - elapsed / length as f64).clamp(0.0, 1.0),
                }
            }
            RateLimitPeriod::Day => {
                let start = midnight(now.date());
                Self::calendar(period, start, start + 86_400)
            }
            RateLimitPeriod::Month => {
                let date = now.date();
                let first = first_of_month(date.year(), date.month());
                let next = if date.month() == Month::December {
                    first_of_month(date.year() + 1, Month::January)
                } else {
                    first_of_month(date.year(), date.month().next())
                };
                Self::calendar(period, first, next)
            }
        }
    }

    fn calendar(period: RateLimitPeriod, start: i64, end: i64) -> Self {
        Self {
            period,
            start,
            end,
            previous_start: None,
            previous_weight: 0.0,
        }
    }

    #[must_use]
    pub const fn length(&self) -> i64 {
        self.end - self.start
    }

    /// Sliding windows keep their row for one more window, because the next
    /// window reads it as `previous`.
    #[must_use]
    pub const fn expires_at(&self) -> i64 {
        if self.period.is_sliding() {
            self.end + self.length()
        } else {
            self.end
        }
    }

    /// The weighted share of the previous window, rounded down.
    #[must_use]
    pub fn weighted_previous(&self, previous: i64) -> i64 {
        (previous as f64 * self.previous_weight).floor() as i64
    }

    /// Seconds from `now` until the current window ends, at least 1.
    #[must_use]
    pub fn reset_after_seconds(&self, now: OffsetDateTime) -> u64 {
        seconds_until(now, self.end as f64)
    }

    /// Seconds from `now` until none of the counted usage weighs on the limit
    /// any more, at least 1. A calendar window frees everything at its end. A
    /// sliding window does not: at its end the current count becomes the
    /// previous one at full weight, and slides out over the next window.
    #[must_use]
    pub fn quota_reset_after_seconds(&self, now: OffsetDateTime, current: i64) -> u64 {
        if self.period.is_sliding() && current > 0 {
            seconds_until(now, (self.end + self.length()) as f64)
        } else {
            self.reset_after_seconds(now)
        }
    }

    /// Seconds until a charge of `need` fits under `limit`, given the counts
    /// read before this request was charged. At least 1.
    #[must_use]
    pub fn retry_after_seconds(
        &self,
        now: OffsetDateTime,
        limit: i64,
        need: i64,
        previous: i64,
        current_before: i64,
    ) -> u64 {
        if need > limit {
            return self.reset_after_seconds(now);
        }
        if !self.period.is_sliding() {
            return self.reset_after_seconds(now);
        }
        // The weighted previous count is rounded down, so a charge fits once
        // `previous × (1 − fraction)` drops strictly below `room + 1`.
        let length = self.length() as f64;
        if current_before + need <= limit {
            // The current window has room: wait for the previous window to
            // slide far enough out of the estimate.
            let room = (limit - current_before - need + 1) as f64;
            let fraction = if previous <= 0 {
                0.0
            } else {
                (1.0 - room / previous as f64).clamp(0.0, 1.0)
            };
            return seconds_after(now, self.start as f64 + fraction * length);
        }
        // The current window is full on its own: once it becomes the previous
        // window, wait for enough of it to slide out.
        let fraction = (1.0 - (limit - need + 1) as f64 / current_before as f64).clamp(0.0, 1.0);
        seconds_after(now, self.end as f64 + fraction * length)
    }
}

fn midnight(date: Date) -> i64 {
    date.with_time(Time::MIDNIGHT).assume_utc().unix_timestamp()
}

fn first_of_month(year: i32, month: Month) -> i64 {
    Date::from_calendar_date(year, month, 1).map_or(0, midnight)
}

fn unix_seconds(now: OffsetDateTime) -> f64 {
    now.unix_timestamp() as f64 + f64::from(now.nanosecond()) / 1_000_000_000.0
}

/// Whole seconds until `target`, at least 1.
fn seconds_until(now: OffsetDateTime, target: f64) -> u64 {
    ((target - unix_seconds(now)).ceil() as i64).max(1) as u64
}

/// The first whole second strictly after `target`, at least 1.
fn seconds_after(now: OffsetDateTime, target: f64) -> u64 {
    ((target - unix_seconds(now)).floor() as i64 + 1).max(1) as u64
}

/// One counter a request must fit under.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitCounterCharge {
    pub counter_key: String,
    pub window: RateLimitWindow,
    pub limit: i64,
    /// Added to the current window when the request is admitted. `0` checks
    /// the counter without charging it (token limits are charged afterwards).
    pub cost: i64,
}

impl RateLimitCounterCharge {
    /// Whether the counts read inside the store transaction admit this charge.
    /// `current` already includes `cost`.
    #[must_use]
    pub fn admits(&self, previous: i64, current: i64) -> bool {
        let estimate = self.window.weighted_previous(previous) + current;
        if self.cost > 0 {
            estimate <= self.limit
        } else {
            estimate < self.limit
        }
    }
}

/// Counts read for one charge, in the order the charges were given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitCounterObservation {
    pub previous: i64,
    /// Including this request's cost when it was charged.
    pub current: i64,
    pub admitted: bool,
}

/// The outcome of one atomic check. When any charge is rejected nothing is
/// charged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitConsumeResult {
    pub admitted: bool,
    pub observations: Vec<RateLimitCounterObservation>,
}

/// An unconditional addition to a counter, such as tokens used by a finished
/// request.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitCounterIncrement {
    pub counter_key: String,
    pub window: RateLimitWindow,
    pub amount: i64,
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn policy_names_are_limited_in_characters_not_bytes() {
        let valid = |name: &str| {
            RateLimitPolicyShape {
                name,
                operation: RateLimitOperation::Inference,
                scope_kind: RateLimitScopeKind::User,
                has_scope_id: false,
                model_key: None,
                has_mcp_server: false,
                metric: RateLimitMetric::Requests,
                limit: 1,
            }
            .validate()
            .is_ok()
        };
        let longest = "制".repeat(MAX_RATE_LIMIT_POLICY_NAME_LEN);
        assert!(longest.len() > MAX_RATE_LIMIT_POLICY_NAME_LEN);
        assert!(valid(&longest));
        assert!(!valid(&format!("{longest}制")));
    }

    #[test]
    fn minute_window_weights_the_previous_window_by_time_left() {
        let window =
            RateLimitWindow::at(RateLimitPeriod::Minute, datetime!(2026-10-15 10:00:15 UTC));

        assert_eq!(window.end - window.start, 60);
        assert_eq!(window.previous_start, Some(window.start - 60));
        assert!((window.previous_weight - 0.75).abs() < 1e-9);
        assert_eq!(window.weighted_previous(10), 7);
        assert_eq!(window.expires_at(), window.end + 60);
    }

    #[test]
    fn calendar_windows_reset_at_utc_boundaries() {
        let day = RateLimitWindow::at(RateLimitPeriod::Day, datetime!(2026-10-15 23:59:59 UTC));
        assert_eq!(day.start, datetime!(2026-10-15 00:00 UTC).unix_timestamp());
        assert_eq!(day.end, datetime!(2026-10-16 00:00 UTC).unix_timestamp());
        assert_eq!(day.previous_start, None);
        assert_eq!(day.weighted_previous(1_000), 0);

        let december = RateLimitWindow::at(RateLimitPeriod::Month, datetime!(2026-12-31 12:00 UTC));
        assert_eq!(
            december.start,
            datetime!(2026-12-01 00:00 UTC).unix_timestamp()
        );
        assert_eq!(
            december.end,
            datetime!(2027-01-01 00:00 UTC).unix_timestamp()
        );
        assert_eq!(december.expires_at(), december.end);
    }

    #[test]
    fn charges_admit_up_to_the_limit() {
        let window =
            RateLimitWindow::at(RateLimitPeriod::Minute, datetime!(2026-10-15 10:00:30 UTC));
        let request = RateLimitCounterCharge {
            counter_key: "k".to_string(),
            window,
            limit: 10,
            cost: 1,
        };
        // floor(8 * 0.5) = 4, plus 6 in the current window including this one.
        assert!(request.admits(8, 6));
        assert!(!request.admits(8, 7));

        let tokens = RateLimitCounterCharge { cost: 0, ..request };
        assert!(tokens.admits(8, 5));
        assert!(!tokens.admits(8, 6));
    }

    #[test]
    fn retry_after_waits_for_the_previous_window_to_slide_out() {
        let now = datetime!(2026-10-15 10:00:30 UTC);
        let window = RateLimitWindow::at(RateLimitPeriod::Minute, now);

        // limit 10, 10 in the previous window, 5 now: the weighted previous
        // must round down to 4, so just over half the window must pass.
        assert_eq!(window.retry_after_seconds(now, 10, 1, 10, 5), 1);
        let start =
            RateLimitWindow::at(RateLimitPeriod::Minute, datetime!(2026-10-15 10:00:05 UTC));
        assert_eq!(
            start.retry_after_seconds(datetime!(2026-10-15 10:00:05 UTC), 10, 1, 10, 5),
            26
        );
        // The current window alone is full: wait until just into the next one.
        assert_eq!(window.retry_after_seconds(now, 10, 1, 0, 10), 31);
        assert_eq!(window.retry_after_seconds(now, 10, 1, 0, 20), 61);
        // Calendar quotas wait for the boundary.
        let day = RateLimitWindow::at(RateLimitPeriod::Day, now);
        assert_eq!(day.retry_after_seconds(now, 10, 1, 0, 10), 50_370);
    }
}
