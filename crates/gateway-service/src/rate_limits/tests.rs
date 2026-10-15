use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use gateway_core::{
    RateLimitConsumeResult, RateLimitCounterCharge, RateLimitCounterIncrement,
    RateLimitCounterObservation,
};
use time::Duration as TimeDuration;

use super::*;

/// In-memory counters with the store's all-or-nothing semantics.
#[derive(Default)]
struct MemoryStore {
    policies: Mutex<Vec<RateLimitPolicyRecord>>,
    counters: Mutex<HashMap<(String, i64), i64>>,
    fail: bool,
    fail_list: std::sync::atomic::AtomicBool,
    fail_add: std::sync::atomic::AtomicBool,
    /// When set, the first policy list reads the rows, then waits for a permit.
    list_gate: Option<Arc<tokio::sync::Semaphore>>,
    list_started: std::sync::atomic::AtomicBool,
    /// When set, a usage write waits for a permit before applying.
    add_gate: Option<Arc<tokio::sync::Semaphore>>,
}

#[async_trait]
impl RateLimitRepository for MemoryStore {
    async fn list_rate_limit_policies(&self) -> Result<Vec<RateLimitPolicyRecord>, StoreError> {
        if self.fail_list.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(StoreError::Unavailable("down".to_string()));
        }
        let rows = self.policies.lock().unwrap().clone();
        // Only the first list (the refresh under test) waits at the gate.
        if let Some(gate) = &self.list_gate
            && !self
                .list_started
                .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            gate.acquire().await.unwrap().forget();
        }
        Ok(rows)
    }

    async fn get_rate_limit_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<RateLimitPolicyRecord>, StoreError> {
        Ok(self
            .policies
            .lock()
            .unwrap()
            .iter()
            .find(|policy| policy.rate_limit_policy_id == policy_id)
            .cloned())
    }

    async fn insert_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<(), StoreError> {
        let mut policies = self.policies.lock().unwrap();
        if policies.iter().any(|existing| existing.name == policy.name) {
            return Err(StoreError::Conflict("name".to_string()));
        }
        policies.push(policy.clone());
        Ok(())
    }

    async fn update_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<bool, StoreError> {
        let mut policies = self.policies.lock().unwrap();
        let Some(existing) = policies
            .iter_mut()
            .find(|existing| existing.rate_limit_policy_id == policy.rate_limit_policy_id)
        else {
            return Ok(false);
        };
        *existing = policy.clone();
        Ok(true)
    }

    async fn delete_rate_limit_policy(&self, policy_id: Uuid) -> Result<bool, StoreError> {
        let mut policies = self.policies.lock().unwrap();
        let before = policies.len();
        policies.retain(|policy| policy.rate_limit_policy_id != policy_id);
        Ok(policies.len() < before)
    }

    async fn consume_rate_limits(
        &self,
        charges: &[RateLimitCounterCharge],
    ) -> Result<RateLimitConsumeResult, StoreError> {
        if self.fail {
            return Err(StoreError::Unavailable("down".to_string()));
        }
        let mut counters = self.counters.lock().unwrap();
        let observations: Vec<_> = charges
            .iter()
            .map(|charge| {
                let current = counters
                    .get(&(charge.counter_key.clone(), charge.window.start))
                    .copied()
                    .unwrap_or(0)
                    + charge.cost;
                let previous = charge
                    .window
                    .previous_start
                    .and_then(|start| counters.get(&(charge.counter_key.clone(), start)).copied())
                    .unwrap_or(0);
                RateLimitCounterObservation {
                    previous,
                    current,
                    admitted: charge.admits(previous, current),
                }
            })
            .collect();
        let admitted = observations.iter().all(|observation| observation.admitted);
        if admitted {
            for charge in charges {
                *counters
                    .entry((charge.counter_key.clone(), charge.window.start))
                    .or_default() += charge.cost;
            }
        }
        Ok(RateLimitConsumeResult {
            admitted,
            observations,
        })
    }

    async fn add_rate_limit_usage(
        &self,
        increments: &[RateLimitCounterIncrement],
    ) -> Result<(), StoreError> {
        if self.fail_add.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(StoreError::Unavailable("read only".to_string()));
        }
        if let Some(gate) = &self.add_gate {
            gate.acquire().await.unwrap().forget();
        }
        let mut counters = self.counters.lock().unwrap();
        for increment in increments {
            *counters
                .entry((increment.counter_key.clone(), increment.window.start))
                .or_default() += increment.amount;
        }
        Ok(())
    }

    async fn purge_expired_rate_limit_counters(&self, _: i64, _: i64) -> Result<u64, StoreError> {
        Ok(0)
    }
}

fn config(
    name: &str,
    operation: RateLimitOperation,
    scope_kind: RateLimitScopeKind,
    metric: RateLimitMetric,
    period: RateLimitPeriod,
    limit: i64,
) -> ConfiguredRateLimitPolicy {
    ConfiguredRateLimitPolicy {
        name: name.to_string(),
        operation,
        scope_kind,
        model_key: None,
        mcp_server_key: None,
        metric,
        period,
        limit,
    }
}

fn admin_policy(name: &str, scope_kind: RateLimitScopeKind, limit: i64) -> RateLimitPolicyRecord {
    let now = OffsetDateTime::now_utc();
    RateLimitPolicyRecord {
        rate_limit_policy_id: Uuid::new_v4(),
        name: name.to_string(),
        operation: RateLimitOperation::Inference,
        scope_kind,
        scope_id: None,
        model_key: None,
        mcp_server_id: None,
        metric: RateLimitMetric::Requests,
        period: RateLimitPeriod::Minute,
        limit,
        enabled: true,
        created_at: now,
        updated_at: now,
    }
}

struct Harness {
    store: Arc<MemoryStore>,
    now: Arc<Mutex<OffsetDateTime>>,
    limiter: Arc<RateLimiter>,
}

impl Harness {
    fn new(store: MemoryStore, policies: Vec<ConfiguredRateLimitPolicy>) -> Self {
        Self::with_mode(store, policies, RateLimitFailureMode::Open)
    }

    fn with_mode(
        store: MemoryStore,
        config_policies: Vec<ConfiguredRateLimitPolicy>,
        failure_mode: RateLimitFailureMode,
    ) -> Self {
        let store = Arc::new(store);
        // Ten seconds into a minute, so sliding estimates are easy to follow.
        let now = Arc::new(Mutex::new(
            OffsetDateTime::from_unix_timestamp(1_800_000_010).unwrap(),
        ));
        let clock = Arc::clone(&now);
        let limiter = RateLimiter::new(
            store.clone(),
            RateLimitSettings {
                failure_mode,
                policy_refresh: Duration::from_secs(60),
                config_policies,
            },
        )
        .with_clock(move || *clock.lock().unwrap());
        Self {
            store,
            now,
            limiter: Arc::new(limiter),
        }
    }

    fn advance(&self, seconds: i64) {
        *self.now.lock().unwrap() += TimeDuration::seconds(seconds);
    }

    fn now(&self) -> OffsetDateTime {
        *self.now.lock().unwrap()
    }
}

fn user_subject(user_id: Uuid, key_id: Uuid) -> RateLimitSubject<'static> {
    RateLimitSubject {
        operation: RateLimitOperation::Inference,
        api_key_id: key_id,
        user_id: Some(user_id),
        team_id: None,
        model_keys: &["fast", "fast-v2"],
        mcp_server: None,
    }
}

#[tokio::test]
async fn requests_are_rejected_at_the_limit_and_admitted_after_the_window() {
    let harness = Harness::new(
        MemoryStore::default(),
        vec![config(
            "per key",
            RateLimitOperation::Inference,
            RateLimitScopeKind::ApiKey,
            RateLimitMetric::Requests,
            RateLimitPeriod::Minute,
            2,
        )],
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    let first = harness.limiter.check(&subject).await.unwrap();
    let RateLimitDecision::Admitted(status) = first else {
        panic!("expected admit, got {first:?}");
    };
    assert_eq!((status.limit, status.remaining), (2, 1));
    assert_eq!(status.reset_after_seconds, 50);
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus { remaining: 0, .. })
    ));

    let RateLimitDecision::Rejected(rejected) = harness.limiter.check(&subject).await.unwrap()
    else {
        panic!("expected rejection");
    };
    // Both requests were in this window, so it must become the previous one
    // and start sliding out: 50s left here, then one more second.
    assert_eq!(rejected.retry_after_seconds, Some(51));
    assert_eq!(
        rejected.rejection_message(),
        "API key rate limit exceeded: 2 inference requests per minute. Retry after 51 seconds"
    );

    // Another key has its own counter.
    let other = user_subject(Uuid::new_v4(), Uuid::new_v4());
    assert!(matches!(
        harness.limiter.check(&other).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));

    harness.advance(50);
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Rejected(_)
    ));
    harness.advance(1);
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));
}

#[tokio::test]
async fn the_most_restrictive_matching_policy_wins() {
    let user_id = Uuid::new_v4();
    let store = MemoryStore::default();
    // A specific admin policy for this user is tighter than the default.
    let mut specific = admin_policy("this user", RateLimitScopeKind::User, 1);
    specific.scope_id = Some(user_id);
    // Disabled and other-model policies never apply.
    let mut disabled = admin_policy("disabled", RateLimitScopeKind::Global, 1);
    disabled.enabled = false;
    let mut other_model = admin_policy("other model", RateLimitScopeKind::Global, 1);
    other_model.model_key = Some("slow".to_string());
    // A model policy matches the requested or resolved key.
    let mut this_model = admin_policy("resolved model", RateLimitScopeKind::Global, 100);
    this_model.model_key = Some("fast-v2".to_string());
    *store.policies.lock().unwrap() = vec![specific, disabled, other_model, this_model];
    let harness = Harness::new(
        store,
        vec![config(
            "per user",
            RateLimitOperation::Inference,
            RateLimitScopeKind::User,
            RateLimitMetric::Requests,
            RateLimitPeriod::Day,
            10,
        )],
    );
    let subject = user_subject(user_id, Uuid::new_v4());

    let RateLimitDecision::Admitted(status) = harness.limiter.check(&subject).await.unwrap() else {
        panic!("expected admit");
    };
    assert_eq!(status.policy_name, "this user");
    assert_eq!(status.remaining, 0);

    let RateLimitDecision::Rejected(rejected) = harness.limiter.check(&subject).await.unwrap()
    else {
        panic!("expected rejection");
    };
    assert_eq!(rejected.policy_name, "this user");

    // The rejection charged nothing: the looser policies saw one request.
    {
        let counters = harness.store.counters.lock().unwrap();
        assert_eq!(counters.values().filter(|count| **count == 1).count(), 3);
        assert_eq!(counters.len(), 3);
    }

    // A different user only meets the default and the model policy.
    let other = user_subject(Uuid::new_v4(), Uuid::new_v4());
    let RateLimitDecision::Admitted(status) = harness.limiter.check(&other).await.unwrap() else {
        panic!("expected admit");
    };
    assert_eq!(status.policy_name, "per user");
    assert_eq!(status.remaining, 9);
}

#[tokio::test]
async fn token_limits_reject_once_recorded_usage_reaches_the_limit() {
    let harness = Harness::new(
        MemoryStore::default(),
        vec![config(
            "user tokens",
            RateLimitOperation::Inference,
            RateLimitScopeKind::User,
            RateLimitMetric::Tokens,
            RateLimitPeriod::Hour,
            1_000,
        )],
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus {
            remaining: 1_000,
            ..
        })
    ));
    harness
        .limiter
        .add_tokens(&subject, 999, harness.now())
        .await;
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus { remaining: 1, .. })
    ));
    harness
        .limiter
        .add_tokens(&subject, 50, harness.now())
        .await;
    let RateLimitDecision::Rejected(rejected) = harness.limiter.check(&subject).await.unwrap()
    else {
        panic!("expected rejection");
    };
    assert_eq!(rejected.metric, RateLimitMetric::Tokens);
    assert_eq!(rejected.remaining, 0);
    assert_eq!(
        rejected.rejection_message(),
        format!(
            "User rate limit exceeded: 1000 tokens per hour. Retry after {} seconds",
            rejected.retry_after_seconds.unwrap()
        )
    );
}

#[tokio::test]
async fn mcp_policies_match_by_server_and_skip_other_operations() {
    let server_id = Uuid::new_v4();
    let mut per_server = config(
        "notes server",
        RateLimitOperation::McpToolCall,
        RateLimitScopeKind::ApiKey,
        RateLimitMetric::Requests,
        RateLimitPeriod::Month,
        1,
    );
    per_server.mcp_server_key = Some("notes".to_string());
    let harness = Harness::new(MemoryStore::default(), vec![per_server]);
    let key_id = Uuid::new_v4();
    let call = |server_key: &'static str| RateLimitSubject {
        operation: RateLimitOperation::McpToolCall,
        api_key_id: key_id,
        user_id: None,
        team_id: None,
        model_keys: &[],
        mcp_server: Some((server_id, server_key)),
    };

    assert!(matches!(
        harness.limiter.check(&call("notes")).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));
    let RateLimitDecision::Rejected(rejected) =
        harness.limiter.check(&call("notes")).await.unwrap()
    else {
        panic!("expected rejection");
    };
    assert!(
        rejected
            .rejection_message()
            .starts_with("API key rate limit exceeded: 1 MCP tool calls per calendar month (UTC)")
    );
    assert_eq!(
        harness.limiter.check(&call("other")).await.unwrap(),
        RateLimitDecision::Unlimited
    );
    let inference = user_subject(Uuid::new_v4(), key_id);
    assert_eq!(
        harness.limiter.check(&inference).await.unwrap(),
        RateLimitDecision::Unlimited
    );
}

#[tokio::test]
async fn store_failures_follow_the_failure_mode() {
    let policies = vec![config(
        "global",
        RateLimitOperation::Inference,
        RateLimitScopeKind::Global,
        RateLimitMetric::Requests,
        RateLimitPeriod::Minute,
        1,
    )];
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    let open = Harness::new(
        MemoryStore {
            fail: true,
            ..MemoryStore::default()
        },
        policies.clone(),
    );
    assert_eq!(
        open.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::StoreUnavailable
    );

    let closed = Harness::with_mode(
        MemoryStore {
            fail: true,
            ..MemoryStore::default()
        },
        policies,
        RateLimitFailureMode::Closed,
    );
    let error = closed.limiter.check(&subject).await.unwrap_err();
    assert_eq!(error.http_status_code(), 503);
}

#[tokio::test]
async fn admin_changes_apply_on_the_next_check_and_validate_shape() {
    let harness = Harness::new(
        MemoryStore::default(),
        vec![config(
            "taken",
            RateLimitOperation::CountTokens,
            RateLimitScopeKind::ApiKey,
            RateLimitMetric::Requests,
            RateLimitPeriod::Minute,
            100,
        )],
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());
    assert_eq!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Unlimited
    );

    let policy = harness
        .limiter
        .create_admin_policy(admin_policy("global", RateLimitScopeKind::Global, 5))
        .await
        .unwrap();
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus { remaining: 4, .. })
    ));

    harness
        .limiter
        .update_admin_policy(RateLimitPolicyRecord {
            enabled: false,
            ..policy.clone()
        })
        .await
        .unwrap();
    assert_eq!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Unlimited
    );
    harness
        .limiter
        .delete_admin_policy(policy.rate_limit_policy_id)
        .await
        .unwrap();
    assert_eq!(
        harness
            .limiter
            .delete_admin_policy(policy.rate_limit_policy_id)
            .await
            .unwrap_err()
            .http_status_code(),
        404
    );

    let mut tokens_for_mcp = admin_policy("bad", RateLimitScopeKind::Global, 5);
    tokens_for_mcp.operation = RateLimitOperation::McpToolCall;
    tokens_for_mcp.metric = RateLimitMetric::Tokens;
    let error = harness
        .limiter
        .create_admin_policy(tokens_for_mcp)
        .await
        .unwrap_err();
    assert_eq!(error.http_status_code(), 400);

    let error = harness
        .limiter
        .create_admin_policy(admin_policy("taken", RateLimitScopeKind::Global, 5))
        .await
        .unwrap_err();
    assert_eq!(error.http_status_code(), 409);
}

#[tokio::test]
async fn an_unloadable_policy_set_follows_the_failure_mode() {
    let unloadable = || MemoryStore {
        fail_list: std::sync::atomic::AtomicBool::new(true),
        ..MemoryStore::default()
    };
    let config_policy = || {
        vec![config(
            "per key",
            RateLimitOperation::Inference,
            RateLimitScopeKind::ApiKey,
            RateLimitMetric::Requests,
            RateLimitPeriod::Minute,
            5,
        )]
    };
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    // Closed: admin policies are unknown, so even traffic no config policy
    // matches is rejected.
    let closed = Harness::with_mode(unloadable(), Vec::new(), RateLimitFailureMode::Closed);
    let error = closed.limiter.check(&subject).await.unwrap_err();
    assert_eq!(error.http_status_code(), 503);

    // Open: admitted without limits or headers, as for any store failure,
    // even though a config policy would match.
    let open = Harness::new(unloadable(), config_policy());
    assert_eq!(
        open.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::StoreUnavailable
    );
}

#[tokio::test]
async fn a_failed_reload_keeps_the_last_complete_policy_set() {
    let store = Arc::new(MemoryStore::default());
    *store.policies.lock().unwrap() = vec![admin_policy("admin", RateLimitScopeKind::Global, 1)];
    // A zero refresh interval reloads on every check.
    let limiter = RateLimiter::new(
        store.clone(),
        RateLimitSettings {
            failure_mode: RateLimitFailureMode::Closed,
            policy_refresh: Duration::ZERO,
            config_policies: Vec::new(),
        },
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());
    assert!(matches!(
        limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));

    store
        .fail_list
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // The admin policy from the last good load is still enforced, and the
    // closed failure mode does not reject on its own.
    assert!(matches!(
        limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Rejected(ref status) if status.policy_name == "admin"
    ));
}

#[tokio::test]
async fn token_usage_is_charged_before_record_tokens_returns() {
    let harness = Harness::new(
        MemoryStore::default(),
        vec![config(
            "daily tokens",
            RateLimitOperation::Inference,
            RateLimitScopeKind::User,
            RateLimitMetric::Tokens,
            RateLimitPeriod::Day,
            1_000,
        )],
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());
    let finished_day = RateLimitWindow::at(RateLimitPeriod::Day, harness.now()).start;

    harness.limiter.record_tokens(&subject, 40).await;
    // No yield: a sequential caller's next check already sees the usage, in
    // the window the response finished in.
    let counters = harness.store.counters.lock().unwrap().clone();
    assert_eq!(counters.len(), 1);
    let ((_, window_start), amount) = counters.into_iter().next().unwrap();
    assert_eq!((window_start, amount), (finished_day, 40));
    harness.advance(86_400);
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus {
            remaining: 1_000,
            ..
        })
    ));
}

#[tokio::test]
async fn editing_a_policy_period_starts_fresh_counters() {
    let harness = Harness::new(MemoryStore::default(), Vec::new());
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());
    let policy = harness
        .limiter
        .create_admin_policy(admin_policy("global", RateLimitScopeKind::Global, 1))
        .await
        .unwrap();
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Rejected(_)
    ));

    harness
        .limiter
        .update_admin_policy(RateLimitPolicyRecord {
            period: RateLimitPeriod::Day,
            ..policy
        })
        .await
        .unwrap();
    // The minute counter is not reused, so the daily quota starts empty.
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus { remaining: 0, .. })
    ));
    let keys: std::collections::HashSet<_> = harness
        .store
        .counters
        .lock()
        .unwrap()
        .keys()
        .map(|(key, _)| key.clone())
        .collect();
    assert_eq!(keys.len(), 2);
    assert!(
        keys.iter()
            .any(|key| key.contains("9:inference6:minute8:requests3:any0:"))
    );
    assert!(
        keys.iter()
            .any(|key| key.contains("9:inference3:day8:requests3:any0:"))
    );
}

#[tokio::test]
async fn editing_a_policy_traffic_filter_starts_fresh_counters() {
    let harness = Harness::new(MemoryStore::default(), Vec::new());
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());
    let mut fast_only = admin_policy("model", RateLimitScopeKind::Global, 1);
    fast_only.model_key = Some("fast".to_string());
    let policy = harness
        .limiter
        .create_admin_policy(fast_only)
        .await
        .unwrap();
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Rejected(_)
    ));

    // `fast-v2` is the subject's resolved model, so the policy still matches,
    // but it now counts different traffic and starts empty.
    harness
        .limiter
        .update_admin_policy(RateLimitPolicyRecord {
            model_key: Some("fast-v2".to_string()),
            ..policy.clone()
        })
        .await
        .unwrap();
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));

    // Moving it to another operation also starts fresh counters.
    harness
        .limiter
        .update_admin_policy(RateLimitPolicyRecord {
            operation: RateLimitOperation::CountTokens,
            model_key: Some("fast-v2".to_string()),
            ..policy
        })
        .await
        .unwrap();
    let counts = RateLimitSubject {
        operation: RateLimitOperation::CountTokens,
        ..subject
    };
    assert!(matches!(
        harness.limiter.check(&counts).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));
}

#[tokio::test]
async fn a_refresh_that_started_before_an_admin_change_is_not_cached() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let harness = Arc::new(Harness::new(
        MemoryStore {
            list_gate: Some(gate.clone()),
            ..MemoryStore::default()
        },
        Vec::new(),
    ));
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    // A check starts a refresh, reads the (empty) rows and stalls.
    let refreshing = {
        let harness = Arc::clone(&harness);
        tokio::spawn(async move { harness.limiter.check(&subject).await.unwrap() })
    };
    while !harness
        .store
        .list_started
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        tokio::task::yield_now().await;
    }
    // An admin creates a policy meanwhile, then the stale refresh finishes.
    harness
        .limiter
        .create_admin_policy(admin_policy("new", RateLimitScopeKind::Global, 5))
        .await
        .unwrap();
    gate.add_permits(10);
    assert_eq!(refreshing.await.unwrap(), RateLimitDecision::Unlimited);

    // The stale snapshot was not published: the next check sees the policy.
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(ref status) if status.policy_name == "new"
    ));
}

#[tokio::test]
async fn closed_mode_rejects_token_traffic_after_a_failed_usage_write() {
    let harness = Harness::with_mode(
        MemoryStore {
            fail_add: std::sync::atomic::AtomicBool::new(true),
            ..MemoryStore::default()
        },
        vec![config(
            "user tokens",
            RateLimitOperation::Inference,
            RateLimitScopeKind::User,
            RateLimitMetric::Tokens,
            RateLimitPeriod::Hour,
            1_000,
        )],
        RateLimitFailureMode::Closed,
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));

    // The write fails, so the read-only check alone would keep admitting.
    harness.limiter.record_tokens(&subject, 400).await;
    let error = harness.limiter.check(&subject).await.unwrap_err();
    assert_eq!(error.http_status_code(), 503);

    // Once the hold lapses and writes work again, traffic flows.
    harness
        .store
        .fail_add
        .store(false, std::sync::atomic::Ordering::SeqCst);
    harness.advance(5);
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(_)
    ));
    harness.limiter.record_tokens(&subject, 400).await;
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus { remaining: 600, .. })
    ));
}

#[tokio::test]
async fn checks_wait_for_detached_token_charges() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let harness = Harness::new(
        MemoryStore {
            add_gate: Some(gate.clone()),
            ..MemoryStore::default()
        },
        vec![config(
            "user tokens",
            RateLimitOperation::Inference,
            RateLimitScopeKind::User,
            RateLimitMetric::Tokens,
            RateLimitPeriod::Hour,
            1_000,
        )],
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    // The charge is held at the store until another task releases it, so a
    // check that did not wait would read the old total.
    harness.limiter.record_tokens_detached(&subject, 400);
    tokio::spawn(async move {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        gate.add_permits(1);
    });
    assert!(matches!(
        harness.limiter.check(&subject).await.unwrap(),
        RateLimitDecision::Admitted(RateLimitStatus { remaining: 600, .. })
    ));
}

#[tokio::test]
async fn enabled_policies_are_capped_per_operation() {
    let cap = gateway_core::MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION;
    let config_policies = (0..cap - 1)
        .map(|n| {
            config(
                &format!("config {n}"),
                RateLimitOperation::Inference,
                RateLimitScopeKind::ApiKey,
                RateLimitMetric::Requests,
                RateLimitPeriod::Minute,
                100,
            )
        })
        .collect();
    let harness = Harness::new(MemoryStore::default(), config_policies);

    let last = harness
        .limiter
        .create_admin_policy(admin_policy("last", RateLimitScopeKind::Global, 5))
        .await
        .unwrap();
    let error = harness
        .limiter
        .create_admin_policy(admin_policy("over", RateLimitScopeKind::Global, 5))
        .await
        .unwrap_err();
    assert_eq!(error.http_status_code(), 400);

    // Paused policies and other operations do not count, and editing an
    // enabled policy does not count it twice.
    let mut paused = admin_policy("paused", RateLimitScopeKind::Global, 5);
    paused.enabled = false;
    harness.limiter.create_admin_policy(paused).await.unwrap();
    let mut counts = admin_policy("counts", RateLimitScopeKind::Global, 5);
    counts.operation = RateLimitOperation::CountTokens;
    harness.limiter.create_admin_policy(counts).await.unwrap();
    harness
        .limiter
        .update_admin_policy(RateLimitPolicyRecord { limit: 9, ..last })
        .await
        .unwrap();
}

#[test]
fn counter_keys_cannot_collide_through_delimiters_in_names() {
    // Without length prefixes these two would both read as
    // `a:inference:minute:requests:model=x...`.
    let joined = |fields: &[&str]| encode_counter_key(fields);
    assert_ne!(
        joined(&[
            "config",
            "a",
            "inference",
            "minute",
            "requests",
            "model",
            "x:inference"
        ]),
        joined(&[
            "config",
            "a:inference",
            "minute",
            "requests",
            "model",
            "x",
            "inference"
        ]),
    );
    assert_eq!(joined(&["a", "", "bc"]), "1:a0:2:bc");
}

#[tokio::test]
async fn a_slow_failing_token_write_still_starts_a_full_hold() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let harness = Harness::with_mode(
        MemoryStore {
            fail_add: std::sync::atomic::AtomicBool::new(true),
            add_gate: Some(gate.clone()),
            ..MemoryStore::default()
        },
        vec![config(
            "user tokens",
            RateLimitOperation::Inference,
            RateLimitScopeKind::User,
            RateLimitMetric::Tokens,
            RateLimitPeriod::Hour,
            1_000,
        )],
        RateLimitFailureMode::Closed,
    );
    let subject = user_subject(Uuid::new_v4(), Uuid::new_v4());

    // The write takes ten seconds to fail: longer than the hold.
    harness.limiter.record_tokens_detached(&subject, 400);
    harness.advance(10);
    gate.add_permits(1);
    let error = harness.limiter.check(&subject).await.unwrap_err();
    assert_eq!(error.http_status_code(), 503);
}
