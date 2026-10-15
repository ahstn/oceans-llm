//! Request-path rate limits and quotas for inference, token counts and MCP
//! tool calls.
//!
//! Policies come from config (held in memory) and from the admin API (rows in
//! the store, cached per replica). Every enabled policy that matches a request
//! applies, and the most restrictive one decides. Counters live in the store so
//! that limits hold across gateway replicas. See
//! `docs/adr/2026-10-15-gateway-rate-limits-and-quotas.md`.

use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};

use gateway_core::{
    AuthenticatedApiKey, GatewayError, RateLimitCounterCharge, RateLimitCounterIncrement,
    RateLimitMetric, RateLimitOperation, RateLimitPeriod, RateLimitPolicyRecord,
    RateLimitPolicyShape, RateLimitRepository, RateLimitScopeKind, RateLimitWindow, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// How often each replica deletes expired counter rows, and how many per pass.
const PURGE_INTERVAL_SECONDS: i64 = 60;
const PURGE_BATCH: i64 = 500;

/// What to do when the counter store fails.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RateLimitFailureMode {
    /// Admit the request without rate-limit headers.
    #[default]
    Open,
    /// Reject the request with 503.
    Closed,
}

/// A platform-wide default from `rate_limits.policies` in config. Config
/// policies give each principal of `scope_kind` its own counter; they never
/// name a specific key, user or team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredRateLimitPolicy {
    pub name: String,
    pub operation: RateLimitOperation,
    pub scope_kind: RateLimitScopeKind,
    pub model_key: Option<String>,
    pub mcp_server_key: Option<String>,
    pub metric: RateLimitMetric,
    pub period: RateLimitPeriod,
    pub limit: i64,
}

#[derive(Debug, Clone)]
pub struct RateLimitSettings {
    pub failure_mode: RateLimitFailureMode,
    pub policy_refresh: Duration,
    pub config_policies: Vec<ConfiguredRateLimitPolicy>,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            failure_mode: RateLimitFailureMode::Open,
            policy_refresh: Duration::from_secs(10),
            config_policies: Vec::new(),
        }
    }
}

/// The caller and target of one request, as policies match them.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitSubject<'a> {
    pub operation: RateLimitOperation,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    /// The requested model key and the key it resolved to.
    pub model_keys: &'a [&'a str],
    /// The MCP server id and key.
    pub mcp_server: Option<(Uuid, &'a str)>,
}

impl<'a> RateLimitSubject<'a> {
    #[must_use]
    pub fn new(operation: RateLimitOperation, auth: &AuthenticatedApiKey) -> Self {
        Self {
            operation,
            api_key_id: auth.id,
            user_id: auth.owner_user_id,
            team_id: auth.owner_team_id,
            model_keys: &[],
            mcp_server: None,
        }
    }

    #[must_use]
    pub fn with_models(mut self, model_keys: &'a [&'a str]) -> Self {
        self.model_keys = model_keys;
        self
    }

    #[must_use]
    pub fn with_mcp_server(mut self, server_id: Uuid, server_key: &'a str) -> Self {
        self.mcp_server = Some((server_id, server_key));
        self
    }
}

/// Where a policy came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitPolicySource {
    Config,
    Admin(Uuid),
}

/// The tightest policy that applied to a request, for response headers and
/// the rejection message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitStatus {
    pub policy_name: String,
    pub source: RateLimitPolicySource,
    pub operation: RateLimitOperation,
    pub scope_kind: RateLimitScopeKind,
    pub metric: RateLimitMetric,
    pub period: RateLimitPeriod,
    pub limit: i64,
    pub remaining: i64,
    pub reset_after_seconds: u64,
    /// Set when the request was rejected.
    pub retry_after_seconds: Option<u64>,
}

impl RateLimitStatus {
    /// Names the operation, metric, period and scope kind, but no ids.
    #[must_use]
    pub fn rejection_message(&self) -> String {
        let metric = match self.metric {
            RateLimitMetric::Requests => format!("{}s", self.operation.label()),
            RateLimitMetric::Tokens => "tokens".to_string(),
        };
        let window = if self.period.is_sliding() {
            format!("per {}", self.period.as_str())
        } else {
            format!("per calendar {} (UTC)", self.period.as_str())
        };
        format!(
            "{} rate limit exceeded: {} {metric} {window}. Retry after {} seconds",
            capitalize(self.scope_kind.label()),
            self.limit,
            self.retry_after_seconds.unwrap_or(self.reset_after_seconds),
        )
    }

    /// The error the HTTP layer returns for a rejection.
    #[must_use]
    pub fn to_error(&self) -> GatewayError {
        GatewayError::RateLimited(self.rejection_message())
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// No policy matched.
    Unlimited,
    Admitted(RateLimitStatus),
    Rejected(RateLimitStatus),
    /// The counter store failed and the failure mode is `open`.
    StoreUnavailable,
}

impl RateLimitDecision {
    #[must_use]
    pub fn status(&self) -> Option<&RateLimitStatus> {
        match self {
            Self::Admitted(status) | Self::Rejected(status) => Some(status),
            Self::Unlimited | Self::StoreUnavailable => None,
        }
    }

    #[must_use]
    pub const fn outcome(&self) -> &'static str {
        match self {
            Self::Unlimited => "unlimited",
            Self::Admitted(_) => "admitted",
            Self::Rejected(_) => "rejected",
            Self::StoreUnavailable => "store_error",
        }
    }
}

/// A policy compiled for matching.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivePolicy {
    source: RateLimitPolicySource,
    name: String,
    operation: RateLimitOperation,
    scope_kind: RateLimitScopeKind,
    scope_id: Option<Uuid>,
    model_key: Option<String>,
    mcp_server: Option<ServerFilter>,
    metric: RateLimitMetric,
    period: RateLimitPeriod,
    limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServerFilter {
    Id(Uuid),
    Key(String),
}

impl ActivePolicy {
    fn from_config(policy: &ConfiguredRateLimitPolicy) -> Self {
        Self {
            source: RateLimitPolicySource::Config,
            name: policy.name.clone(),
            operation: policy.operation,
            scope_kind: policy.scope_kind,
            scope_id: None,
            model_key: policy.model_key.clone(),
            mcp_server: policy.mcp_server_key.clone().map(ServerFilter::Key),
            metric: policy.metric,
            period: policy.period,
            limit: policy.limit,
        }
    }

    fn from_record(policy: &RateLimitPolicyRecord) -> Self {
        Self {
            source: RateLimitPolicySource::Admin(policy.rate_limit_policy_id),
            name: policy.name.clone(),
            operation: policy.operation,
            scope_kind: policy.scope_kind,
            scope_id: policy.scope_id,
            model_key: policy.model_key.clone(),
            mcp_server: policy.mcp_server_id.map(ServerFilter::Id),
            metric: policy.metric,
            period: policy.period,
            limit: policy.limit,
        }
    }

    /// The counter this subject shares under this policy, or `None` when the
    /// policy does not apply to it.
    fn counter_key(&self, subject: &RateLimitSubject<'_>) -> Option<String> {
        if self.operation != subject.operation {
            return None;
        }
        if let Some(model_key) = &self.model_key
            && !subject.model_keys.contains(&model_key.as_str())
        {
            return None;
        }
        if let Some(filter) = &self.mcp_server {
            let (server_id, server_key) = subject.mcp_server?;
            let matches = match filter {
                ServerFilter::Id(id) => *id == server_id,
                ServerFilter::Key(key) => key == server_key,
            };
            if !matches {
                return None;
            }
        }
        let principal = match self.scope_kind {
            RateLimitScopeKind::Global => None,
            RateLimitScopeKind::ApiKey => Some(subject.api_key_id),
            RateLimitScopeKind::User => Some(subject.user_id?),
            RateLimitScopeKind::Team => Some(subject.team_id?),
        };
        if let Some(scope_id) = self.scope_id
            && principal != Some(scope_id)
        {
            return None;
        }
        let policy = match &self.source {
            RateLimitPolicySource::Config => format!("config:{}", self.name),
            RateLimitPolicySource::Admin(id) => format!("policy:{id}"),
        };
        Some(match principal {
            Some(principal) => format!("{policy}|{}:{principal}", self.scope_kind.as_str()),
            None => format!("{policy}|global"),
        })
    }
}

struct PolicyCache {
    loaded_at: Instant,
    policies: Arc<Vec<ActivePolicy>>,
}

type Clock = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;

pub struct RateLimiter {
    store: Option<Arc<dyn RateLimitRepository>>,
    settings: RateLimitSettings,
    config_policies: Vec<ActivePolicy>,
    cache: RwLock<Option<PolicyCache>>,
    refresh: tokio::sync::Mutex<()>,
    last_purge: AtomicI64,
    clock: Clock,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RateLimiter")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl RateLimiter {
    #[must_use]
    pub fn new(store: Arc<dyn RateLimitRepository>, settings: RateLimitSettings) -> Self {
        Self::build(Some(store), settings)
    }

    /// A limiter with no policies, for tests and tools that do not serve traffic.
    #[must_use]
    pub fn disabled() -> Self {
        Self::build(None, RateLimitSettings::default())
    }

    fn build(store: Option<Arc<dyn RateLimitRepository>>, settings: RateLimitSettings) -> Self {
        let config_policies = settings
            .config_policies
            .iter()
            .map(ActivePolicy::from_config)
            .collect();
        Self {
            store,
            settings,
            config_policies,
            cache: RwLock::new(None),
            refresh: tokio::sync::Mutex::new(()),
            last_purge: AtomicI64::new(0),
            clock: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Replaces the clock, so tests can move between windows.
    #[must_use]
    pub fn with_clock(
        mut self,
        clock: impl Fn() -> OffsetDateTime + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    #[must_use]
    pub fn config_policies(&self) -> &[ConfiguredRateLimitPolicy] {
        &self.settings.config_policies
    }

    /// Checks every request-count policy and every exhausted token policy that
    /// matches, charging request counters only when all of them admit it.
    pub async fn check(
        &self,
        subject: &RateLimitSubject<'_>,
    ) -> Result<RateLimitDecision, GatewayError> {
        let policies = self.policies().await;
        let now = (self.clock)();
        let matched: Vec<(&ActivePolicy, RateLimitCounterCharge)> = policies
            .iter()
            .filter_map(|policy| {
                let counter_key = policy.counter_key(subject)?;
                Some((
                    policy,
                    RateLimitCounterCharge {
                        counter_key,
                        window: RateLimitWindow::at(policy.period, now),
                        limit: policy.limit,
                        cost: match policy.metric {
                            RateLimitMetric::Requests => 1,
                            RateLimitMetric::Tokens => 0,
                        },
                    },
                ))
            })
            .collect();
        if matched.is_empty() {
            return Ok(RateLimitDecision::Unlimited);
        }
        let Some(store) = &self.store else {
            return Ok(RateLimitDecision::Unlimited);
        };
        let charges: Vec<_> = matched.iter().map(|(_, charge)| charge.clone()).collect();
        let result = match store.consume_rate_limits(&charges).await {
            Ok(result) => result,
            Err(error) => return self.store_failure(subject.operation, error),
        };
        self.maybe_purge(store, now);

        let mut statuses = matched
            .iter()
            .zip(&result.observations)
            .map(|((policy, charge), observation)| {
                let estimate =
                    charge.window.weighted_previous(observation.previous) + observation.current;
                let retry_after_seconds = (!observation.admitted).then(|| {
                    charge.window.retry_after_seconds(
                        now,
                        charge.limit,
                        charge.cost.max(1),
                        observation.previous,
                        observation.current - charge.cost,
                    )
                });
                RateLimitStatus {
                    policy_name: policy.name.clone(),
                    source: policy.source.clone(),
                    operation: policy.operation,
                    scope_kind: policy.scope_kind,
                    metric: policy.metric,
                    period: policy.period,
                    limit: policy.limit,
                    remaining: (policy.limit - estimate).max(0),
                    reset_after_seconds: charge.window.reset_after_seconds(now),
                    retry_after_seconds,
                }
            })
            .collect::<Vec<_>>();

        if result.admitted {
            // Report the policy with the least room left.
            statuses.sort_by_key(|status| (status.remaining, status.limit));
            return Ok(RateLimitDecision::Admitted(statuses.swap_remove(0)));
        }
        // Report the rejecting policy the caller must wait longest for.
        let rejected = statuses
            .into_iter()
            .filter(|status| status.retry_after_seconds.is_some())
            .max_by_key(|status| (status.retry_after_seconds, std::cmp::Reverse(status.limit)))
            .ok_or_else(|| {
                GatewayError::Internal("rate limit rejection without a policy".into())
            })?;
        tracing::debug!(
            operation = subject.operation.as_str(),
            policy = %rejected.policy_name,
            scope_kind = rejected.scope_kind.as_str(),
            metric = rejected.metric.as_str(),
            period = rejected.period.as_str(),
            retry_after_seconds = rejected.retry_after_seconds,
            "rate limit rejected request"
        );
        Ok(RateLimitDecision::Rejected(rejected))
    }

    fn store_failure(
        &self,
        operation: RateLimitOperation,
        error: StoreError,
    ) -> Result<RateLimitDecision, GatewayError> {
        tracing::warn!(
            operation = operation.as_str(),
            failure_mode = ?self.settings.failure_mode,
            error = %error,
            "rate limit counter store failed"
        );
        match self.settings.failure_mode {
            RateLimitFailureMode::Open => Ok(RateLimitDecision::StoreUnavailable),
            RateLimitFailureMode::Closed => Err(GatewayError::Store(StoreError::Unavailable(
                "rate limits cannot be checked right now".to_string(),
            ))),
        }
    }

    /// Charges token policies after a response finished. Runs in the
    /// background; a failure only loses this request's tokens.
    pub fn record_tokens(self: &Arc<Self>, subject: &RateLimitSubject<'_>, total_tokens: i64) {
        if total_tokens <= 0 || self.store.is_none() {
            return;
        }
        let limiter = Arc::clone(self);
        let operation = subject.operation;
        let api_key_id = subject.api_key_id;
        let user_id = subject.user_id;
        let team_id = subject.team_id;
        let model_keys: Vec<String> = subject.model_keys.iter().map(ToString::to_string).collect();
        tokio::spawn(async move {
            let model_keys: Vec<&str> = model_keys.iter().map(String::as_str).collect();
            let subject = RateLimitSubject {
                operation,
                api_key_id,
                user_id,
                team_id,
                model_keys: &model_keys,
                mcp_server: None,
            };
            limiter.add_tokens(&subject, total_tokens).await;
        });
    }

    async fn add_tokens(&self, subject: &RateLimitSubject<'_>, total_tokens: i64) {
        let Some(store) = &self.store else {
            return;
        };
        let now = (self.clock)();
        let increments: Vec<_> = self
            .policies()
            .await
            .iter()
            .filter(|policy| policy.metric == RateLimitMetric::Tokens)
            .filter_map(|policy| {
                Some(RateLimitCounterIncrement {
                    counter_key: policy.counter_key(subject)?,
                    window: RateLimitWindow::at(policy.period, now),
                    amount: total_tokens,
                })
            })
            .collect();
        if increments.is_empty() {
            return;
        }
        if let Err(error) = store.add_rate_limit_usage(&increments).await {
            tracing::warn!(error = %error, "recording rate-limited token usage failed");
        }
    }

    fn maybe_purge(&self, store: &Arc<dyn RateLimitRepository>, now: OffsetDateTime) {
        let now = now.unix_timestamp();
        let last = self.last_purge.load(Ordering::Relaxed);
        if now - last < PURGE_INTERVAL_SECONDS
            || self
                .last_purge
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        let store = Arc::clone(store);
        tokio::spawn(async move {
            if let Err(error) = store
                .purge_expired_rate_limit_counters(now, PURGE_BATCH)
                .await
            {
                tracing::warn!(error = %error, "purging expired rate limit counters failed");
            }
        });
    }

    /// The enabled policies, reloading admin policies when the cache is stale.
    /// A failed reload keeps the previous set.
    async fn policies(&self) -> Arc<Vec<ActivePolicy>> {
        if let Some(policies) = self.cached(false) {
            return policies;
        }
        let _guard = self.refresh.lock().await;
        if let Some(policies) = self.cached(false) {
            return policies;
        }
        let mut policies = self.config_policies.clone();
        if let Some(store) = &self.store {
            match store.list_rate_limit_policies().await {
                Ok(records) => policies.extend(
                    records
                        .iter()
                        .filter(|record| record.enabled)
                        .map(ActivePolicy::from_record),
                ),
                Err(error) => {
                    tracing::warn!(error = %error, "loading rate limit policies failed");
                    if let Some(stale) = self.cached(true) {
                        self.store_cache(Arc::clone(&stale));
                        return stale;
                    }
                }
            }
        }
        let policies = Arc::new(policies);
        self.store_cache(Arc::clone(&policies));
        policies
    }

    fn cached(&self, allow_stale: bool) -> Option<Arc<Vec<ActivePolicy>>> {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        cache
            .as_ref()
            .filter(|cache| allow_stale || cache.loaded_at.elapsed() < self.settings.policy_refresh)
            .map(|cache| Arc::clone(&cache.policies))
    }

    fn store_cache(&self, policies: Arc<Vec<ActivePolicy>>) {
        *self
            .cache
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(PolicyCache {
            loaded_at: Instant::now(),
            policies,
        });
    }

    /// Makes the next check reload policies. Other replicas pick changes up
    /// within `policy_refresh`.
    pub fn invalidate(&self) {
        *self
            .cache
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
    }

    fn admin_store(&self) -> Result<&Arc<dyn RateLimitRepository>, GatewayError> {
        self.store
            .as_ref()
            .ok_or_else(|| GatewayError::Internal("rate limits are not configured".to_string()))
    }

    pub async fn list_admin_policies(&self) -> Result<Vec<RateLimitPolicyRecord>, GatewayError> {
        Ok(self.admin_store()?.list_rate_limit_policies().await?)
    }

    pub async fn get_admin_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<RateLimitPolicyRecord, GatewayError> {
        self.admin_store()?
            .get_rate_limit_policy(policy_id)
            .await?
            .ok_or_else(|| {
                GatewayError::Store(StoreError::NotFound(format!(
                    "rate limit policy `{policy_id}`"
                )))
            })
    }

    pub async fn create_admin_policy(
        &self,
        policy: RateLimitPolicyRecord,
    ) -> Result<RateLimitPolicyRecord, GatewayError> {
        self.validate_admin_policy(&policy)?;
        self.admin_store()?
            .insert_rate_limit_policy(&policy)
            .await
            .map_err(name_conflict)?;
        self.invalidate();
        Ok(policy)
    }

    pub async fn update_admin_policy(
        &self,
        policy: RateLimitPolicyRecord,
    ) -> Result<RateLimitPolicyRecord, GatewayError> {
        self.validate_admin_policy(&policy)?;
        let updated = self
            .admin_store()?
            .update_rate_limit_policy(&policy)
            .await
            .map_err(name_conflict)?;
        if !updated {
            return Err(GatewayError::Store(StoreError::NotFound(format!(
                "rate limit policy `{}`",
                policy.rate_limit_policy_id
            ))));
        }
        self.invalidate();
        Ok(policy)
    }

    pub async fn delete_admin_policy(&self, policy_id: Uuid) -> Result<(), GatewayError> {
        if !self
            .admin_store()?
            .delete_rate_limit_policy(policy_id)
            .await?
        {
            return Err(GatewayError::Store(StoreError::NotFound(format!(
                "rate limit policy `{policy_id}`"
            ))));
        }
        self.invalidate();
        Ok(())
    }

    fn validate_admin_policy(&self, policy: &RateLimitPolicyRecord) -> Result<(), GatewayError> {
        RateLimitPolicyShape {
            name: &policy.name,
            operation: policy.operation,
            scope_kind: policy.scope_kind,
            has_scope_id: policy.scope_id.is_some(),
            model_key: policy.model_key.as_deref(),
            has_mcp_server: policy.mcp_server_id.is_some(),
            metric: policy.metric,
            limit: policy.limit,
        }
        .validate()
        .map_err(GatewayError::InvalidRequest)?;
        if self
            .settings
            .config_policies
            .iter()
            .any(|config| config.name == policy.name)
        {
            return Err(GatewayError::Store(StoreError::Conflict(format!(
                "rate limit policy name `{}` is already used in config",
                policy.name
            ))));
        }
        Ok(())
    }
}

fn name_conflict(error: StoreError) -> GatewayError {
    match error {
        StoreError::Conflict(_) => GatewayError::Store(StoreError::Conflict(
            "a rate limit policy with this name already exists".to_string(),
        )),
        other => GatewayError::Store(other),
    }
}

#[cfg(test)]
mod tests;
