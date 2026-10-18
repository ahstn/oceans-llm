//! Request-path rate limits and quotas for inference, token counts and MCP
//! tool calls.
//!
//! Policies come from config (held in memory) and from the admin API (rows in
//! the store, cached per replica). Every enabled policy that matches a request
//! applies, and the most restrictive one decides. Counters live in the store so
//! that limits hold across gateway replicas. See
//! `docs/adr/2026-10-15-gateway-rate-limits-and-quotas.md`.

use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use gateway_core::{
    AuthenticatedApiKey, GatewayError, MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION,
    RateLimitCounterCharge, RateLimitCounterIncrement, RateLimitMetric, RateLimitOperation,
    RateLimitPeriod, RateLimitPolicyRecord, RateLimitPolicyShape, RateLimitRepository,
    RateLimitScopeKind, RateLimitWindow, StoreError, Updated,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// How often each replica deletes expired counter rows, and how many per pass.
const PURGE_INTERVAL_SECONDS: i64 = 60;
const PURGE_BATCH: i64 = 500;
/// At most this many batches per interval (50,000 rows a minute per replica).
const PURGE_MAX_BATCHES: usize = 100;

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
    /// The subject for a credential. `api_key_id` is the credential's id: an
    /// API key's, or for an OAuth caller its grant's, so `api_key` scope
    /// counts each authorized client on its own, as it does each key. A
    /// user's team is not on the credential; the limiter looks it up for
    /// team-scoped policies.
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

    /// Whether the policy's operation and traffic filters match the request,
    /// whoever makes it.
    fn matches_traffic(&self, subject: &RateLimitSubject<'_>) -> bool {
        if self.operation != subject.operation {
            return false;
        }
        if let Some(model_key) = &self.model_key
            && !subject.model_keys.contains(&model_key.as_str())
        {
            return false;
        }
        match (&self.mcp_server, subject.mcp_server) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(ServerFilter::Id(id)), Some((server_id, _))) => *id == server_id,
            (Some(ServerFilter::Key(key)), Some((_, server_key))) => key == server_key,
        }
    }

    /// The counter this subject shares under this policy, or `None` when the
    /// policy does not apply to it.
    fn counter_key(&self, subject: &RateLimitSubject<'_>) -> Option<String> {
        if !self.matches_traffic(subject) {
            return None;
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
        // Everything that decides what a counter counts is part of its
        // identity: the operation, traffic filter, period and metric. Editing
        // any of them starts fresh counters instead of reusing rows that
        // counted other traffic or were sized for another window.
        let (source_kind, source_value) = match &self.source {
            RateLimitPolicySource::Config => ("config", self.name.clone()),
            RateLimitPolicySource::Admin(id) => ("policy", id.to_string()),
        };
        let (filter_kind, filter_value) = match (&self.model_key, &self.mcp_server) {
            (Some(model_key), _) => ("model", model_key.clone()),
            (None, Some(ServerFilter::Id(id))) => ("server", id.to_string()),
            (None, Some(ServerFilter::Key(key))) => ("server-key", key.clone()),
            (None, None) => ("any", String::new()),
        };
        let principal = principal.map(|id| id.to_string()).unwrap_or_default();
        Some(encode_counter_key(&[
            source_kind,
            &source_value,
            self.operation.as_str(),
            self.period.as_str(),
            self.metric.as_str(),
            filter_kind,
            &filter_value,
            self.scope_kind.as_str(),
            &principal,
        ]))
    }
}

/// Joins counter-key fields unambiguously: each is written as
/// `<byte length>:<value>`, so names or model keys containing delimiters can
/// never make two different policies share a key.
fn encode_counter_key(fields: &[&str]) -> String {
    let mut key = String::new();
    for field in fields {
        key.push_str(&field.len().to_string());
        key.push(':');
        key.push_str(field);
    }
    key
}

/// The policies a check runs against. `complete` is false when admin policies
/// could not be loaded and no earlier set was available, so only config
/// policies are known.
#[derive(Clone)]
struct PolicySet {
    /// Grouped by operation, so a check only looks at policies for its own
    /// kind of traffic.
    by_operation: Arc<HashMap<RateLimitOperation, Vec<ActivePolicy>>>,
    complete: bool,
}

impl PolicySet {
    fn new(policies: Vec<ActivePolicy>, complete: bool) -> Self {
        let mut by_operation: HashMap<RateLimitOperation, Vec<ActivePolicy>> = HashMap::new();
        for policy in policies {
            by_operation
                .entry(policy.operation)
                .or_default()
                .push(policy);
        }
        Self {
            by_operation: Arc::new(by_operation),
            complete,
        }
    }

    fn for_operation(&self, operation: RateLimitOperation) -> &[ActivePolicy] {
        self.by_operation.get(&operation).map_or(&[], Vec::as_slice)
    }
}

struct PolicyCache {
    loaded_at: Instant,
    set: PolicySet,
}

/// How long an incomplete policy set is reused before the store is retried.
const INCOMPLETE_POLICY_RETRY: Duration = Duration::from_secs(1);
/// In `closed` mode, how long token policies reject after a token usage write
/// failed. Once it lapses, admitted requests probe the store again.
const TOKEN_WRITE_FAILURE_HOLD_SECONDS: i64 = 5;
/// The longest a check waits for this replica's detached token charges.
const PENDING_CHARGE_WAIT: Duration = Duration::from_secs(1);
/// Marks a pending charge whose counters were not known when it started.
/// Counter keys are length-prefixed, so this can never equal one.
const ANY_PENDING_COUNTER: &str = "*";

type Clock = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;

/// Marks a token charge's counters as pending until dropped, then wakes
/// waiting checks. Dropping also covers a charge task that panicked.
struct PendingCharge {
    keys: Vec<String>,
    pending: Arc<std::sync::Mutex<HashMap<String, usize>>>,
    done: Arc<tokio::sync::Notify>,
}

impl PendingCharge {
    fn start(
        keys: Vec<String>,
        pending: &Arc<std::sync::Mutex<HashMap<String, usize>>>,
        done: &Arc<tokio::sync::Notify>,
    ) -> Self {
        let mut map = pending.lock().unwrap_or_else(|poison| poison.into_inner());
        for key in &keys {
            *map.entry(key.clone()).or_default() += 1;
        }
        drop(map);
        Self {
            keys,
            pending: Arc::clone(pending),
            done: Arc::clone(done),
        }
    }
}

impl Drop for PendingCharge {
    fn drop(&mut self) {
        let mut map = self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for key in &self.keys {
            if let Some(count) = map.get_mut(key) {
                *count -= 1;
                if *count == 0 {
                    map.remove(key);
                }
            }
        }
        drop(map);
        self.done.notify_waiters();
    }
}

/// Finds the team a user belongs to, so team-scoped policies also count the
/// requests members make with their own keys and OAuth grants.
#[async_trait::async_trait]
pub trait RateLimitTeamLookup: Send + Sync {
    async fn team_for_user(&self, user_id: Uuid) -> Result<Option<Uuid>, StoreError>;
}

#[async_trait::async_trait]
impl<T> RateLimitTeamLookup for T
where
    T: gateway_core::IdentityRepository + ?Sized,
{
    async fn team_for_user(&self, user_id: Uuid) -> Result<Option<Uuid>, StoreError> {
        Ok(self
            .get_team_membership_for_user(user_id)
            .await?
            .map(|membership| membership.team_id))
    }
}

/// One lookup's outcome, shared with every caller waiting on it. A failure
/// keeps its message, since `StoreError` cannot be cloned.
type TeamLookupOutcome = Result<Option<Uuid>, String>;
type TeamLookupFlights =
    std::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::OnceCell<TeamLookupOutcome>>>>;

/// A caller's share of one user's team lookup. Dropping it, whether the
/// lookup succeeded, failed or was cancelled, removes the entry if it is still
/// this lookup's, so failed lookups leave nothing behind.
struct TeamLookupFlight<'a> {
    flights: &'a TeamLookupFlights,
    user_id: Uuid,
    cell: Arc<tokio::sync::OnceCell<TeamLookupOutcome>>,
}

impl<'a> TeamLookupFlight<'a> {
    fn join(flights: &'a TeamLookupFlights, user_id: Uuid) -> Self {
        let cell = flights
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .entry(user_id)
            .or_default()
            .clone();
        Self {
            flights,
            user_id,
            cell,
        }
    }
}

impl Drop for TeamLookupFlight<'_> {
    fn drop(&mut self) {
        let mut flights = self
            .flights
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if flights
            .get(&self.user_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cell))
        {
            flights.remove(&self.user_id);
        }
    }
}

/// Users whose team is cached at once. Past this the cache starts over.
const MAX_CACHED_MEMBER_TEAMS: usize = 10_000;

pub struct RateLimiter {
    store: Option<Arc<dyn RateLimitRepository>>,
    team_lookup: Option<Arc<dyn RateLimitTeamLookup>>,
    /// Each user's team, as of when it was read, for `policy_refresh`.
    member_teams: std::sync::Mutex<HashMap<Uuid, (Instant, Option<Uuid>)>>,
    /// Team lookups in flight, so concurrent requests from one uncached user
    /// share a single lookup.
    member_team_lookups: TeamLookupFlights,
    settings: RateLimitSettings,
    config_policies: Vec<ActivePolicy>,
    cache: RwLock<Option<PolicyCache>>,
    refresh: tokio::sync::Mutex<()>,
    /// Bumped by every invalidation, so a refresh that started earlier cannot
    /// publish the snapshot it read before an admin change.
    generation: AtomicU64,
    last_purge: AtomicI64,
    /// Unix seconds of the last failed token usage write, or 0.
    token_write_failed_at: AtomicI64,
    /// Token counters with a charge still being written, and a wake-up for
    /// checks waiting on them.
    pending_charges: Arc<std::sync::Mutex<HashMap<String, usize>>>,
    charges_done: Arc<tokio::sync::Notify>,
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
            team_lookup: None,
            member_teams: std::sync::Mutex::new(HashMap::new()),
            member_team_lookups: std::sync::Mutex::new(HashMap::new()),
            settings,
            config_policies,
            cache: RwLock::new(None),
            refresh: tokio::sync::Mutex::new(()),
            generation: AtomicU64::new(0),
            last_purge: AtomicI64::new(0),
            token_write_failed_at: AtomicI64::new(0),
            pending_charges: Arc::default(),
            charges_done: Arc::new(tokio::sync::Notify::new()),
            clock: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Resolves the team of a user whose credential carries none, for
    /// team-scoped policies. Without it those policies count only team
    /// (service account) keys.
    #[must_use]
    pub fn with_team_lookup(mut self, lookup: Arc<dyn RateLimitTeamLookup>) -> Self {
        self.team_lookup = Some(lookup);
        self
    }

    /// The user's team from the cache, when it was read recently.
    fn cached_member_team(&self, user_id: Uuid) -> Option<Option<Uuid>> {
        let teams = self
            .member_teams
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        teams
            .get(&user_id)
            .filter(|(read_at, _)| read_at.elapsed() < self.settings.policy_refresh)
            .map(|(_, team)| *team)
    }

    /// Whether `subject` needs its user's team looked up for `set`: some
    /// team policy, of `metric` when given, matches the request's traffic.
    fn needs_member_team(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'_>,
        metric: Option<RateLimitMetric>,
    ) -> bool {
        self.team_lookup.is_some()
            && subject.team_id.is_none()
            && subject.user_id.is_some()
            && set.for_operation(subject.operation).iter().any(|policy| {
                policy.scope_kind == RateLimitScopeKind::Team
                    && metric.is_none_or(|metric| policy.metric == metric)
                    && policy.matches_traffic(subject)
            })
    }

    /// `subject` with its user's team filled in, when a team policy of
    /// `metric` (any, when `None`) matches the request. A failed lookup is an
    /// error, which callers handle like any other store failure.
    async fn with_member_team<'a>(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'a>,
        metric: Option<RateLimitMetric>,
    ) -> Result<RateLimitSubject<'a>, StoreError> {
        let mut subject = *subject;
        if !self.needs_member_team(set, &subject, metric) {
            return Ok(subject);
        }
        let (Some(lookup), Some(user_id)) = (&self.team_lookup, subject.user_id) else {
            return Ok(subject);
        };
        if let Some(team) = self.cached_member_team(user_id) {
            subject.team_id = team;
            return Ok(subject);
        }
        let flight = TeamLookupFlight::join(&self.member_team_lookups, user_id);
        // Callers already waiting share a failure too, rather than retrying
        // the lookup one after another; the next request starts a new one.
        let team = flight
            .cell
            .get_or_init(|| async {
                lookup
                    .team_for_user(user_id)
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .clone()
            .map_err(StoreError::Unavailable)?;
        let mut teams = self
            .member_teams
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if teams.len() >= MAX_CACHED_MEMBER_TEAMS {
            teams.clear();
        }
        teams.insert(user_id, (Instant::now(), team));
        subject.team_id = team;
        Ok(subject)
    }

    /// [`Self::with_member_team`] for a token charge. A failed lookup charges
    /// the other counters and marks token counters stale, as a failed usage
    /// write does, so `closed` mode refuses token checks it can no longer trust.
    async fn charge_subject<'a>(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'a>,
    ) -> RateLimitSubject<'a> {
        match self
            .with_member_team(set, subject, Some(RateLimitMetric::Tokens))
            .await
        {
            Ok(subject) => subject,
            Err(error) => {
                tracing::warn!(error = %error, "could not read the user's team for a token charge");
                self.mark_token_write_failed();
                *subject
            }
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
        let set = self.policies().await;
        if !set.complete {
            // Admin policies are unknown, so this request may be one they
            // limit: follow the failure mode as for any other store failure.
            return self.store_failure(
                subject.operation,
                StoreError::Unavailable("rate limit policies could not be loaded".to_string()),
            );
        }
        // Without the team, team policies cannot be checked: `closed` mode
        // refuses the request; `open` mode checks the other policies.
        let subject = &match self.with_member_team(&set, subject, None).await {
            Ok(subject) => subject,
            Err(error) if self.settings.failure_mode == RateLimitFailureMode::Closed => {
                return self.store_failure(subject.operation, error);
            }
            Err(error) => {
                tracing::warn!(error = %error, "could not read the user's team for rate limits");
                *subject
            }
        };
        let now = (self.clock)();
        let matched: Vec<(&ActivePolicy, RateLimitCounterCharge)> = set
            .for_operation(subject.operation)
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
        if let Some(store) = &self.store {
            // Expired counters are cleaned up even when nothing matches, so
            // removing every policy does not strand old rows.
            self.maybe_purge(store, now);
        }
        if matched.is_empty() {
            return Ok(RateLimitDecision::Unlimited);
        }
        // Only this request's token counters matter: wait for their charges
        // still in flight on this replica, and nothing else.
        let token_keys: Vec<&str> = matched
            .iter()
            .filter(|(policy, _)| policy.metric == RateLimitMetric::Tokens)
            .map(|(_, charge)| charge.counter_key.as_str())
            .collect();
        let pending_charge_timed_out = !self.wait_for_pending_charges(&token_keys).await;
        // In `closed` mode a token check cannot trust a counter whose pending
        // charge has not landed, any more than one whose write failed.
        let token_counters_stale = self.token_writes_failing(now)
            || (pending_charge_timed_out
                && self.settings.failure_mode == RateLimitFailureMode::Closed);
        if token_counters_stale
            && matched
                .iter()
                .any(|(policy, _)| policy.metric == RateLimitMetric::Tokens)
        {
            return self.store_failure(
                subject.operation,
                StoreError::Unavailable("token usage could not be recorded".to_string()),
            );
        }
        let Some(store) = &self.store else {
            return Ok(RateLimitDecision::Unlimited);
        };
        let charges: Vec<_> = matched.iter().map(|(_, charge)| charge.clone()).collect();
        let result = match store.consume_rate_limits(&charges).await {
            Ok(result) => result,
            Err(error) => return self.store_failure(subject.operation, error),
        };

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
                    // A rejected check charged nothing, so its own cost is
                    // not usage still to slide out.
                    reset_after_seconds: charge.window.quota_reset_after_seconds(
                        now,
                        if result.admitted {
                            observation.current
                        } else {
                            observation.current - charge.cost
                        },
                    ),
                    retry_after_seconds,
                }
            })
            .collect::<Vec<_>>();

        if result.admitted {
            // Report the policy with the smallest share of its limit left.
            // Request and token limits count different units, so their
            // remaining amounts cannot be compared directly.
            statuses.sort_by(|left, right| {
                let share = |status: &RateLimitStatus, other: &RateLimitStatus| {
                    i128::from(status.remaining) * i128::from(other.limit.max(1))
                };
                share(left, right)
                    .cmp(&share(right, left))
                    .then(left.limit.cmp(&right.limit))
            });
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

    /// Charges token policies once a response has finished, before the caller
    /// sees the end of it, so a sequential caller's next check sees the usage.
    /// Makes no store call when no token policy matches; a failed write only
    /// loses this request's tokens.
    pub async fn record_tokens(&self, subject: &RateLimitSubject<'_>, total_tokens: i64) {
        if total_tokens <= 0 || self.store.is_none() {
            return;
        }
        self.add_tokens(subject, total_tokens, (self.clock)()).await;
    }

    /// Starts a token charge on its own task and returns its handle. The
    /// normal path awaits the handle, so the charge has landed before the
    /// response completes, as callers on other replicas need. If that caller
    /// is cancelled, or cannot await (a dropped stream), the task still
    /// finishes, and checks on this replica for the same counters wait for it.
    pub fn record_tokens_detached(
        self: &Arc<Self>,
        subject: &RateLimitSubject<'_>,
        total_tokens: i64,
    ) -> Option<tokio::task::JoinHandle<()>> {
        if total_tokens <= 0 || self.store.is_none() {
            return None;
        }
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let finished_at = (self.clock)();
        // With a fresh cached set, the task charges exactly that set, so the
        // counters marked pending here are the ones it writes, even if the
        // cache refreshes to other policies before it runs. Without one (just
        // after an invalidation, or once it is stale) the task loads the set
        // itself and every token check waits.
        let snapshot = self.cached(false);
        // The same goes for a member's team: from the cache the marked
        // counters are exact; otherwise the task looks it up and every token
        // check waits.
        let mut subject = *subject;
        let team_unknown = match &snapshot {
            Some(set) if self.needs_member_team(set, &subject, Some(RateLimitMetric::Tokens)) => {
                match subject
                    .user_id
                    .and_then(|user| self.cached_member_team(user))
                {
                    Some(team) => {
                        subject.team_id = team;
                        false
                    }
                    None => true,
                }
            }
            _ => false,
        };
        let keys = match &snapshot {
            Some(set) if !team_unknown => set
                .for_operation(subject.operation)
                .iter()
                .filter(|policy| policy.metric == RateLimitMetric::Tokens)
                .filter_map(|policy| policy.counter_key(&subject))
                .collect(),
            _ => vec![ANY_PENDING_COUNTER.to_string()],
        };
        let pending = PendingCharge::start(keys, &self.pending_charges, &self.charges_done);
        let limiter = Arc::clone(self);
        let operation = subject.operation;
        let api_key_id = subject.api_key_id;
        let user_id = subject.user_id;
        let team_id = subject.team_id;
        let model_keys: Vec<String> = subject.model_keys.iter().map(ToString::to_string).collect();
        Some(runtime.spawn(async move {
            let _pending = pending;
            let model_keys: Vec<&str> = model_keys.iter().map(String::as_str).collect();
            let subject = RateLimitSubject {
                operation,
                api_key_id,
                user_id,
                team_id,
                model_keys: &model_keys,
                mcp_server: None,
            };
            let set = match snapshot {
                Some(set) => set,
                None => limiter.policies().await,
            };
            let subject = limiter.charge_subject(&set, &subject).await;
            limiter
                .charge_tokens(&set, &subject, total_tokens, finished_at)
                .await;
        }))
    }

    /// Waits for charges in flight on this replica to any of `keys`. Returns
    /// false when some were still pending at the timeout.
    async fn wait_for_pending_charges(&self, keys: &[&str]) -> bool {
        let any_pending = || {
            let map = self
                .pending_charges
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            map.contains_key(ANY_PENDING_COUNTER) || keys.iter().any(|key| map.contains_key(*key))
        };
        if keys.is_empty() || !any_pending() {
            return true;
        }
        let drained = async {
            loop {
                let notified = self.charges_done.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if !any_pending() {
                    return;
                }
                notified.await;
            }
        };
        let finished = tokio::time::timeout(PENDING_CHARGE_WAIT, drained)
            .await
            .is_ok();
        if !finished {
            tracing::warn!("checking rate limits before a pending token charge finished");
        }
        finished
    }

    async fn add_tokens(
        &self,
        subject: &RateLimitSubject<'_>,
        total_tokens: i64,
        finished_at: OffsetDateTime,
    ) {
        let set = self.policies().await;
        let subject = self.charge_subject(&set, subject).await;
        self.charge_tokens(&set, &subject, total_tokens, finished_at)
            .await;
    }

    /// Charges `total_tokens` to the token policies in `set` that match.
    async fn charge_tokens(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'_>,
        total_tokens: i64,
        finished_at: OffsetDateTime,
    ) {
        let Some(store) = &self.store else {
            return;
        };
        if !set.complete {
            // Admin policies are unknown, so charging only the config subset
            // would lose this usage silently. Treat it as a failed write.
            tracing::warn!("recording rate-limited token usage without the admin policies");
            self.mark_token_write_failed();
            return;
        }
        let increments: Vec<_> = set
            .for_operation(subject.operation)
            .iter()
            .filter(|policy| policy.metric == RateLimitMetric::Tokens)
            .filter_map(|policy| {
                Some(RateLimitCounterIncrement {
                    counter_key: policy.counter_key(subject)?,
                    window: RateLimitWindow::at(policy.period, finished_at),
                    amount: total_tokens,
                })
            })
            .collect();
        if increments.is_empty() {
            return;
        }
        let started_at = (self.clock)().unix_timestamp();
        match store.add_rate_limit_usage(&increments).await {
            Ok(()) => {
                // Only a write that started after the recorded failure shows
                // the store has recovered; a concurrent one that started
                // earlier must not clear another write's failure.
                let failed_at = self.token_write_failed_at.load(Ordering::SeqCst);
                if failed_at != 0 && failed_at < started_at {
                    let _ = self.token_write_failed_at.compare_exchange(
                        failed_at,
                        0,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    );
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "recording rate-limited token usage failed");
                self.mark_token_write_failed();
            }
        }
    }

    /// Stamps when a failure was seen: a write that took longer than the hold
    /// to fail must still start a full hold.
    fn mark_token_write_failed(&self) {
        self.token_write_failed_at
            .store((self.clock)().unix_timestamp().max(1), Ordering::SeqCst);
    }

    /// Whether `closed` mode should reject token-limited traffic because a
    /// recent token usage write failed, so unrecorded usage cannot keep
    /// passing a check that only reads the counter.
    fn token_writes_failing(&self, now: OffsetDateTime) -> bool {
        if self.settings.failure_mode != RateLimitFailureMode::Closed {
            return false;
        }
        let failed_at = self.token_write_failed_at.load(Ordering::Relaxed);
        failed_at != 0 && now.unix_timestamp() - failed_at < TOKEN_WRITE_FAILURE_HOLD_SECONDS
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
            // Each delete is bounded; keep going while batches come back full,
            // so a replica that creates more rows than one batch per interval
            // still drains its backlog.
            for _ in 0..PURGE_MAX_BATCHES {
                match store
                    .purge_expired_rate_limit_counters(now, PURGE_BATCH)
                    .await
                {
                    Ok(deleted) if deleted >= PURGE_BATCH as u64 => tokio::task::yield_now().await,
                    Ok(_) => return,
                    Err(error) => {
                        tracing::warn!(error = %error, "purging expired rate limit counters failed");
                        return;
                    }
                }
            }
        });
    }

    /// The enabled policies, reloading admin policies when the cache is stale.
    /// A failed reload keeps the previous complete set. Without one, the set
    /// holds config policies only, is marked incomplete, and is retried after
    /// `INCOMPLETE_POLICY_RETRY`.
    async fn policies(&self) -> PolicySet {
        if let Some(set) = self.cached(false) {
            return set;
        }
        let _guard = self.refresh.lock().await;
        if let Some(set) = self.cached(false) {
            return set;
        }
        let generation = self.generation.load(Ordering::Acquire);
        let mut policies = self.config_policies.clone();
        let mut complete = true;
        if let Some(store) = &self.store {
            match store.list_enabled_rate_limit_policies().await {
                Ok(records) => policies.extend(records.iter().map(ActivePolicy::from_record)),
                Err(error) => {
                    tracing::warn!(error = %error, "loading rate limit policies failed");
                    if let Some(stale) = self.cached(true).filter(|set| set.complete) {
                        self.store_cache(stale.clone(), generation);
                        return stale;
                    }
                    complete = false;
                }
            }
        }
        let set = PolicySet::new(policies, complete);
        self.store_cache(set.clone(), generation);
        set
    }

    fn cached(&self, allow_stale: bool) -> Option<PolicySet> {
        let cache = self
            .cache
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        cache
            .as_ref()
            .filter(|cache| {
                let ttl = if cache.set.complete {
                    self.settings.policy_refresh
                } else {
                    INCOMPLETE_POLICY_RETRY
                };
                allow_stale || cache.loaded_at.elapsed() < ttl
            })
            .map(|cache| cache.set.clone())
    }

    /// Publishes a loaded set unless an invalidation happened since the load
    /// began; the caller still uses its set, and the next check reloads.
    fn store_cache(&self, set: PolicySet, generation: u64) {
        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        if self.generation.load(Ordering::Acquire) != generation {
            return;
        }
        *cache = Some(PolicyCache {
            loaded_at: Instant::now(),
            set,
        });
    }

    /// Makes the next check reload policies. Other replicas pick changes up
    /// within `policy_refresh`.
    pub fn invalidate(&self) {
        let mut cache = self
            .cache
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        self.generation.fetch_add(1, Ordering::AcqRel);
        *cache = None;
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
        self.ensure_policy_capacity(&policy).await?;
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
    ) -> Result<Updated<RateLimitPolicyRecord>, GatewayError> {
        self.validate_admin_policy(&policy)?;
        self.ensure_policy_capacity(&policy).await?;
        let Some(updated) = self
            .admin_store()?
            .update_rate_limit_policy(&policy)
            .await
            .map_err(name_conflict)?
        else {
            return Err(GatewayError::Store(StoreError::NotFound(format!(
                "rate limit policy `{}`",
                policy.rate_limit_policy_id
            ))));
        };
        if updated.changed() {
            self.invalidate();
        }
        Ok(updated)
    }

    /// Deletes the policy and returns it as it was when deleted.
    pub async fn delete_admin_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<RateLimitPolicyRecord, GatewayError> {
        let Some(deleted) = self
            .admin_store()?
            .delete_rate_limit_policy(policy_id)
            .await?
        else {
            return Err(GatewayError::Store(StoreError::NotFound(format!(
                "rate limit policy `{policy_id}`"
            ))));
        };
        self.invalidate();
        Ok(deleted)
    }

    /// Fails when a config policy shares its name with an admin policy, or
    /// when config and enabled admin policies together exceed the
    /// per-operation cap, so a gateway never starts with an ambiguous or
    /// oversized set (for example, after config policies were added to a full
    /// database).
    pub async fn validate_policy_set(&self) -> Result<(), GatewayError> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let admin = store.list_rate_limit_policies().await?;
        if let Some(policy) = admin.iter().find(|policy| {
            self.settings
                .config_policies
                .iter()
                .any(|config| config.name == policy.name)
        }) {
            return Err(GatewayError::InvalidRequest(format!(
                "rate limit policy name `{}` is used both in config and by an admin policy; rename one of them",
                policy.name
            )));
        }
        for operation in RateLimitOperation::ALL {
            let count = admin
                .iter()
                .filter(|policy| policy.enabled && policy.operation == operation)
                .count()
                + self
                    .settings
                    .config_policies
                    .iter()
                    .filter(|policy| policy.operation == operation)
                    .count();
            if count > MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION {
                return Err(GatewayError::InvalidRequest(format!(
                    "{count} rate limit policies are enabled for `{}` (config and admin combined); at most {MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION} are allowed. Pause admin policies or remove config ones",
                    operation.as_str()
                )));
            }
        }
        Ok(())
    }

    /// Rejects enabling a policy that would take its operation past
    /// `MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION`. Two admins enabling
    /// policies at the same moment can each pass; the cap bounds cost, not
    /// correctness, so that is acceptable.
    async fn ensure_policy_capacity(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<(), GatewayError> {
        if !policy.enabled {
            return Ok(());
        }
        let others = self
            .admin_store()?
            .list_enabled_rate_limit_policies()
            .await?
            .into_iter()
            .filter(|other| {
                other.operation == policy.operation
                    && other.rate_limit_policy_id != policy.rate_limit_policy_id
            })
            .count();
        let from_config = self
            .settings
            .config_policies
            .iter()
            .filter(|config| config.operation == policy.operation)
            .count();
        if others + from_config >= MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION {
            return Err(GatewayError::InvalidRequest(format!(
                "at most {MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION} rate limit policies can be enabled for `{}`; pause or delete one first",
                policy.operation.as_str()
            )));
        }
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
