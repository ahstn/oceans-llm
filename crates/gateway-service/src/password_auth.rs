use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gateway_core::{GatewayError, hash_gateway_key_secret};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::authenticator::verify_gateway_key_secret;

const PASSWORD_ACCOUNT_FAILURE_LIMIT: u32 = 5;
const PASSWORD_SOURCE_FAILURE_LIMIT: u32 = 20;
const MAX_CONCURRENT_PASSWORD_ARGON2: usize = 4;
const ARGON2_SLOT_WAIT: Duration = Duration::from_secs(5);
const FAILURE_RESET_AFTER: Duration = Duration::from_secs(15 * 60);
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const MAX_TRACKED_IDENTIFIERS: usize = 10_000;

/// A valid default-parameter Argon2 hash used when no password account exists.
const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU";

type Identifier = [u8; 32];

/// The login dimension whose backoff or reserved capacity rejected an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginThrottleScope {
    /// The resolved client address has exhausted its login allowance.
    Source,
    /// The normalized account identifier has exhausted its login allowance.
    Account,
}

impl LoginThrottleScope {
    /// Stable low-cardinality label for password-login metrics.
    #[must_use]
    pub const fn metric_outcome(self) -> &'static str {
        match self {
            Self::Source => "throttled_source",
            Self::Account => "throttled_account",
        }
    }

    /// Human-readable scope for structured logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Account => "account",
        }
    }
}

/// Bounds public password authentication and backs off repeated failed logins.
///
/// Histories retain only fixed-size identifier digests, with a hard entry cap.
/// Admission reserves both dimensions before any asynchronous work; live
/// reservations pin their entries until completion or cancellation.
#[derive(Debug)]
pub struct PasswordAuthProtection {
    attempts: Mutex<LoginAttempts>,
    argon2_slots: Arc<Semaphore>,
    settings: ProtectionSettings,
}

impl Default for PasswordAuthProtection {
    fn default() -> Self {
        Self::new(ProtectionSettings::default())
    }
}

impl PasswordAuthProtection {
    fn new(settings: ProtectionSettings) -> Self {
        Self {
            attempts: Mutex::new(LoginAttempts::default()),
            argon2_slots: Arc::new(Semaphore::new(settings.max_concurrent_argon2)),
            settings,
        }
    }

    /// Reserves an attempt against both the source and normalized account.
    ///
    /// Keep the returned guard through credential lookup and verification.
    /// Dropping it without an explicit outcome records a failed attempt.
    ///
    /// # Errors
    ///
    /// Rejects backoff, exhausted in-flight allowance, or a full history whose
    /// entries are all pinned by live attempts. Neither dimension is reserved
    /// when admission fails.
    pub fn admit_login(
        &self,
        source: &str,
        normalized_account: &str,
    ) -> Result<LoginAttempt<'_>, LoginThrottleScope> {
        self.admit_login_at(source, normalized_account, Instant::now())
    }

    fn admit_login_at(
        &self,
        source: &str,
        normalized_account: &str,
        now: Instant,
    ) -> Result<LoginAttempt<'_>, LoginThrottleScope> {
        let source = Sha256::digest(source.as_bytes()).into();
        let account = Sha256::digest(normalized_account.as_bytes()).into();
        let mut attempts = self
            .attempts
            .lock()
            .expect("password attempt lock poisoned");
        let evict_source = prepare_reservation(
            &mut attempts.sources,
            &source,
            now,
            self.settings.source_failure_limit,
            &self.settings,
        )
        .map_err(|()| LoginThrottleScope::Source)?;
        let evict_account = prepare_reservation(
            &mut attempts.accounts,
            &account,
            now,
            self.settings.account_failure_limit,
            &self.settings,
        )
        .map_err(|()| LoginThrottleScope::Account)?;

        reserve(&mut attempts.sources, source, evict_source, now);
        reserve(&mut attempts.accounts, account, evict_account, now);
        Ok(LoginAttempt {
            protection: self,
            source,
            account,
            completed: false,
        })
    }

    fn complete_login(
        &self,
        source: &Identifier,
        account: &Identifier,
        outcome: LoginOutcome,
        now: Instant,
    ) {
        let mut attempts = self
            .attempts
            .lock()
            .expect("password attempt lock poisoned");
        let source = attempts
            .sources
            .get_mut(source)
            .expect("a live login pins its source history");
        source.in_flight -= 1;
        if outcome == LoginOutcome::Failure {
            source.record_failure(now, self.settings.source_failure_limit, &self.settings);
        }

        let account = attempts
            .accounts
            .get_mut(account)
            .expect("a live login pins its account history");
        account.in_flight -= 1;
        match outcome {
            LoginOutcome::Failure => {
                account.record_failure(now, self.settings.account_failure_limit, &self.settings);
            }
            LoginOutcome::Success => account.clear_failures(now),
            LoginOutcome::Unavailable => {}
        }
    }

    /// Verifies a password under bounded Argon2 admission.
    ///
    /// When `expected_hash` is absent, a fixed valid hash preserves the same
    /// expensive path for unknown and non-password accounts.
    ///
    /// # Errors
    ///
    /// Returns a rate-limit error when the bounded Argon2 pool is saturated,
    /// or an internal error when the hash or blocking task fails.
    pub async fn verify_password(
        &self,
        password: String,
        expected_hash: Option<String>,
    ) -> Result<bool, GatewayError> {
        self.run_argon2(move || {
            let expected_hash = expected_hash.as_deref().unwrap_or(DUMMY_PASSWORD_HASH);
            verify_gateway_key_secret(&password, expected_hash)
                .map_err(|error| GatewayError::Internal(error.to_string()))
        })
        .await
    }

    /// Hashes a password under the same bounded Argon2 admission.
    ///
    /// # Errors
    ///
    /// Returns a rate-limit error when the bounded Argon2 pool is saturated,
    /// or the hashing error from [`hash_gateway_key_secret`].
    pub async fn hash_password(&self, password: String) -> Result<String, GatewayError> {
        self.run_argon2(move || hash_gateway_key_secret(&password))
            .await
    }

    /// The blocking task owns its permit even if its caller is cancelled.
    async fn run_argon2<T, F>(&self, work: F) -> Result<T, GatewayError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, GatewayError> + Send + 'static,
    {
        let slot = tokio::time::timeout(
            self.settings.argon2_slot_wait,
            self.argon2_slots.clone().acquire_owned(),
        )
        .await
        .map_err(|_| password_auth_busy())?
        .map_err(|_| GatewayError::Internal("password verification is unavailable".to_string()))?;

        tokio::task::spawn_blocking(move || {
            let _slot = slot;
            work()
        })
        .await
        .map_err(|error| GatewayError::Internal(format!("password verification failed: {error}")))?
    }
}

/// Owns one source/account reservation and charges unfinished attempts on drop.
#[derive(Debug)]
#[must_use = "keep the login reservation until credential verification completes"]
pub struct LoginAttempt<'a> {
    protection: &'a PasswordAuthProtection,
    source: Identifier,
    account: Identifier,
    completed: bool,
}

impl LoginAttempt<'_> {
    /// Clears account failures without discarding other live reservations or source history.
    pub fn complete_success(self) {
        self.complete_at(LoginOutcome::Success, Instant::now());
    }

    /// Records a credential failure against both reserved dimensions.
    pub fn complete_failure(self) {
        self.complete_at(LoginOutcome::Failure, Instant::now());
    }

    /// Releases the reservation without charging a failure after an infrastructure error.
    pub fn release(self) {
        self.complete_at(LoginOutcome::Unavailable, Instant::now());
    }

    fn complete_at(mut self, outcome: LoginOutcome, now: Instant) {
        self.protection
            .complete_login(&self.source, &self.account, outcome, now);
        self.completed = true;
    }
}

impl Drop for LoginAttempt<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.protection.complete_login(
                &self.source,
                &self.account,
                LoginOutcome::Failure,
                Instant::now(),
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoginOutcome {
    Failure,
    Success,
    Unavailable,
}

fn password_auth_busy() -> GatewayError {
    GatewayError::RateLimited("password authentication is busy; retry later".to_string())
}

#[derive(Debug, Default)]
struct LoginAttempts {
    sources: HashMap<Identifier, FailureState>,
    accounts: HashMap<Identifier, FailureState>,
}

#[derive(Debug)]
struct FailureState {
    failures: u32,
    in_flight: u32,
    last_failure: Instant,
    blocked_until: Option<Instant>,
}

impl FailureState {
    fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            in_flight: 0,
            last_failure: now,
            blocked_until: None,
        }
    }

    fn clear_failures(&mut self, now: Instant) {
        self.failures = 0;
        self.last_failure = now;
        self.blocked_until = None;
    }

    fn record_failure(&mut self, now: Instant, failure_limit: u32, settings: &ProtectionSettings) {
        if now.saturating_duration_since(self.last_failure) >= settings.failure_reset_after {
            self.clear_failures(now);
        }
        self.failures = self.failures.saturating_add(1);
        self.last_failure = now;
        self.blocked_until = (self.failures >= failure_limit).then(|| {
            let exponent = self.failures.saturating_sub(failure_limit).min(6);
            let multiplier = 1_u32 << exponent;
            let backoff = settings
                .base_backoff
                .saturating_mul(multiplier)
                .min(settings.max_backoff);
            now + backoff
        });
    }
}

/// Plans eviction without changing reservations or removing either history.
fn prepare_reservation(
    entries: &mut HashMap<Identifier, FailureState>,
    key: &Identifier,
    now: Instant,
    failure_limit: u32,
    settings: &ProtectionSettings,
) -> Result<Option<Identifier>, ()> {
    if let Some(entry) = entries.get_mut(key) {
        if now.saturating_duration_since(entry.last_failure) >= settings.failure_reset_after {
            entry.clear_failures(now);
        }
        // Once backoff elapses, serialize retries even though failures remain
        // above the threshold. Earlier failures and live attempts share the
        // initial allowance, so a concurrent burst cannot bypass it.
        let allowance = failure_limit.saturating_sub(entry.failures).max(1);
        if entry.blocked_until.is_some_and(|until| until > now) || entry.in_flight >= allowance {
            return Err(());
        }
        return Ok(None);
    }
    if entries.len() < settings.max_tracked_identifiers {
        return Ok(None);
    }
    entries
        .iter()
        .filter(|(_, entry)| entry.in_flight == 0)
        .min_by_key(|(_, entry)| entry.last_failure)
        .map(|(key, _)| Some(*key))
        .ok_or(())
}

fn reserve(
    entries: &mut HashMap<Identifier, FailureState>,
    key: Identifier,
    evict: Option<Identifier>,
    now: Instant,
) {
    if let Some(evict) = evict {
        entries.remove(&evict);
    }
    entries
        .entry(key)
        .or_insert_with(|| FailureState::new(now))
        .in_flight += 1;
}

#[derive(Debug, Clone, Copy)]
struct ProtectionSettings {
    source_failure_limit: u32,
    account_failure_limit: u32,
    max_concurrent_argon2: usize,
    argon2_slot_wait: Duration,
    failure_reset_after: Duration,
    base_backoff: Duration,
    max_backoff: Duration,
    max_tracked_identifiers: usize,
}

impl Default for ProtectionSettings {
    fn default() -> Self {
        Self {
            source_failure_limit: PASSWORD_SOURCE_FAILURE_LIMIT,
            account_failure_limit: PASSWORD_ACCOUNT_FAILURE_LIMIT,
            max_concurrent_argon2: MAX_CONCURRENT_PASSWORD_ARGON2,
            argon2_slot_wait: ARGON2_SLOT_WAIT,
            failure_reset_after: FAILURE_RESET_AFTER,
            base_backoff: BASE_BACKOFF,
            max_backoff: MAX_BACKOFF,
            max_tracked_identifiers: MAX_TRACKED_IDENTIFIERS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(source_limit: u32, account_limit: u32) -> ProtectionSettings {
        ProtectionSettings {
            source_failure_limit: source_limit,
            account_failure_limit: account_limit,
            max_concurrent_argon2: 1,
            argon2_slot_wait: Duration::from_millis(20),
            ..ProtectionSettings::default()
        }
    }

    #[test]
    fn failures_back_off_both_dimensions_and_serialize_retries_until_reset() {
        let protection = PasswordAuthProtection::new(settings(2, 2));
        let mut now = Instant::now();
        protection
            .admit_login_at("source", "account", now)
            .unwrap()
            .complete_at(LoginOutcome::Failure, now);

        for delay in [1, 2, 4, 8, 16, 32, 60, 60] {
            protection
                .admit_login_at("source", "account", now)
                .unwrap()
                .complete_at(LoginOutcome::Failure, now);
            let retry_at = now + Duration::from_secs(delay);
            let before_retry = retry_at - Duration::from_nanos(1);
            assert_eq!(
                protection
                    .admit_login_at("source", "other-account", before_retry)
                    .unwrap_err(),
                LoginThrottleScope::Source
            );
            assert_eq!(
                protection
                    .admit_login_at("other-source", "account", before_retry)
                    .unwrap_err(),
                LoginThrottleScope::Account
            );
            let retry = protection
                .admit_login_at("source", "account", retry_at)
                .unwrap();
            assert_eq!(
                protection
                    .admit_login_at("other-source", "account", retry_at)
                    .unwrap_err(),
                LoginThrottleScope::Account,
                "only one retry may run after backoff"
            );
            retry.release();
            now = retry_at;
        }

        now += FAILURE_RESET_AFTER;
        let first = protection.admit_login_at("source", "account", now).unwrap();
        let second = protection.admit_login_at("source", "account", now).unwrap();
        first.release();
        second.release();
    }

    #[tokio::test]
    async fn concurrent_bursts_reserve_capacity_before_the_first_await() {
        for scope in [LoginThrottleScope::Account, LoginThrottleScope::Source] {
            let protection = Arc::new(PasswordAuthProtection::default());
            let limit = match scope {
                LoginThrottleScope::Account => PASSWORD_ACCOUNT_FAILURE_LIMIT,
                LoginThrottleScope::Source => PASSWORD_SOURCE_FAILURE_LIMIT,
            };
            let (admitted_tx, mut admitted_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut tasks = tokio::task::JoinSet::new();
            for index in 0..limit + 5 {
                let protection = protection.clone();
                let admitted_tx = admitted_tx.clone();
                tasks.spawn(async move {
                    let distinct = index.to_string();
                    let (source, account) = match scope {
                        LoginThrottleScope::Account => (distinct.as_str(), "account"),
                        LoginThrottleScope::Source => ("source", distinct.as_str()),
                    };
                    let result = protection.admit_login(source, account);
                    admitted_tx.send(result.as_ref().err().copied()).unwrap();
                    if let Ok(attempt) = result {
                        std::future::pending::<()>().await;
                        attempt.complete_failure();
                    }
                });
            }
            let mut admitted = 0;
            for _ in 0..limit + 5 {
                match admitted_rx.recv().await.unwrap() {
                    None => admitted += 1,
                    Some(rejected) => assert_eq!(rejected, scope),
                }
            }
            assert_eq!(admitted, limit);
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
    }

    #[test]
    fn success_keeps_overlapping_reservations_and_source_failures() {
        let protection = PasswordAuthProtection::new(settings(2, 3));
        let now = Instant::now();
        protection
            .admit_login_at("source", "account", now)
            .unwrap()
            .complete_at(LoginOutcome::Failure, now);
        let overlap = protection
            .admit_login_at("other-source", "account", now)
            .unwrap();
        protection
            .admit_login_at("source", "account", now)
            .unwrap()
            .complete_at(LoginOutcome::Success, now);
        let second = protection
            .admit_login_at("second-source", "account", now)
            .unwrap();
        let third = protection
            .admit_login_at("third-source", "account", now)
            .unwrap();
        assert_eq!(
            protection
                .admit_login_at("fourth-source", "account", now)
                .unwrap_err(),
            LoginThrottleScope::Account
        );
        overlap.complete_at(LoginOutcome::Failure, now);
        assert_eq!(
            protection
                .admit_login_at("fourth-source", "account", now)
                .unwrap_err(),
            LoginThrottleScope::Account,
            "failure completion must replace, not lose, its reservation"
        );
        protection
            .admit_login_at("source", "other-account", now)
            .unwrap()
            .complete_at(LoginOutcome::Failure, now);
        assert_eq!(
            protection
                .admit_login_at("source", "third-account", now)
                .unwrap_err(),
            LoginThrottleScope::Source,
            "a successful login must not clear source failures"
        );
        second.release();
        third.release();
    }

    #[test]
    fn full_pinned_histories_reject_atomically_even_after_expiry() {
        let protection = PasswordAuthProtection::new(ProtectionSettings {
            max_tracked_identifiers: 1,
            ..settings(2, 2)
        });
        let now = Instant::now();
        let first = protection.admit_login_at("source", "account", now).unwrap();
        let later = now + FAILURE_RESET_AFTER;
        assert_eq!(
            protection
                .admit_login_at("source", "new-account", later)
                .unwrap_err(),
            LoginThrottleScope::Account
        );
        assert_eq!(
            protection
                .admit_login_at("new-source", "account", later)
                .unwrap_err(),
            LoginThrottleScope::Source
        );
        let second = protection
            .admit_login_at("source", "account", later)
            .unwrap();
        assert_eq!(
            protection
                .admit_login_at("source", "account", later)
                .unwrap_err(),
            LoginThrottleScope::Source,
            "failed admission must not reserve either dimension"
        );
        first.release();
        second.release();
        protection
            .admit_login_at("new-source", "new-account", later)
            .unwrap()
            .release();
    }

    #[test]
    fn long_identifiers_have_fixed_storage_and_never_evict_live_attempts() {
        let protection = PasswordAuthProtection::new(ProtectionSettings {
            max_tracked_identifiers: 3,
            ..settings(100, 100)
        });
        let now = Instant::now();
        let pinned = protection
            .admit_login_at("pinned-source", "pinned-account", now)
            .unwrap();
        for index in 0..10 {
            let source = format!("{}-{index}", "s".repeat(64 * 1024));
            let account = format!("{}-{index}@example.test", "a".repeat(64 * 1024));
            protection
                .admit_login_at(&source, &account, now)
                .unwrap()
                .complete_at(LoginOutcome::Failure, now);
        }
        {
            let attempts = protection.attempts.lock().unwrap();
            assert_eq!(attempts.sources.len(), 3);
            assert_eq!(attempts.accounts.len(), 3);
            for key in attempts.sources.keys().chain(attempts.accounts.keys()) {
                assert_eq!(std::mem::size_of_val(key), 32);
            }
        }
        pinned.complete_at(LoginOutcome::Failure, now);
        // A guard borrows only the protection, never the caller's long strings.
        let independent = {
            let source = "long-source".repeat(10_000);
            let account = "long-account".repeat(10_000);
            protection.admit_login(&source, &account).unwrap()
        };
        independent.release();
    }

    #[test]
    fn infrastructure_release_does_not_charge_failures() {
        let protection = PasswordAuthProtection::new(settings(1, 1));
        for _ in 0..3 {
            protection
                .admit_login("source", "account")
                .unwrap()
                .release();
        }
        let now = Instant::now();
        drop(protection.admit_login("source", "account").unwrap());
        assert_eq!(
            protection
                .admit_login_at("source", "other-account", now)
                .unwrap_err(),
            LoginThrottleScope::Source
        );
        assert_eq!(
            protection
                .admit_login_at("other-source", "account", now)
                .unwrap_err(),
            LoginThrottleScope::Account
        );
    }

    #[tokio::test]
    async fn cancellation_while_waiting_for_argon2_charges_the_attempt() {
        let protection = Arc::new(PasswordAuthProtection::new(ProtectionSettings {
            argon2_slot_wait: Duration::from_secs(60),
            ..settings(2, 1)
        }));
        let slot = protection
            .argon2_slots
            .clone()
            .acquire_owned()
            .await
            .unwrap();
        let (admitted_tx, admitted_rx) = tokio::sync::oneshot::channel();
        let now = Instant::now();
        let task = {
            let protection = protection.clone();
            tokio::spawn(async move {
                let attempt = protection.admit_login("source", "account").unwrap();
                admitted_tx.send(()).unwrap();
                let _ = protection
                    .verify_password("password".to_string(), None)
                    .await;
                attempt.release();
            })
        };
        admitted_rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            protection
                .admit_login_at("other-source", "account", now)
                .unwrap_err(),
            LoginThrottleScope::Account
        );
        assert_eq!(protection.argon2_slots.available_permits(), 0);
        drop(slot);
    }

    #[tokio::test]
    async fn cancellation_charges_failure_but_keeps_the_running_argon2_permit() {
        let protection = Arc::new(PasswordAuthProtection::new(settings(2, 1)));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let now = Instant::now();
        let task = {
            let protection = protection.clone();
            tokio::spawn(async move {
                let attempt = protection.admit_login("source", "account").unwrap();
                let result = protection
                    .run_argon2(move || {
                        let _ = started_tx.send(());
                        release_rx.recv().expect("release blocking work");
                        Ok(())
                    })
                    .await;
                attempt.release();
                result
            })
        };
        started_rx.await.expect("blocking work started");
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            protection
                .admit_login_at("other-source", "account", now)
                .unwrap_err(),
            LoginThrottleScope::Account
        );
        assert_eq!(protection.argon2_slots.available_permits(), 0);
        let error = protection.run_argon2(|| Ok(())).await.unwrap_err();
        assert!(matches!(error, GatewayError::RateLimited(_)));
        tokio::time::timeout(Duration::from_millis(100), tokio::task::yield_now())
            .await
            .expect("async runtime remains responsive");
        release_tx.send(()).expect("release blocking work");
        let slot = tokio::time::timeout(Duration::from_secs(5), protection.argon2_slots.acquire())
            .await
            .expect("blocking permit is eventually released")
            .expect("semaphore remains open");
        drop(slot);
    }

    #[tokio::test]
    async fn missing_account_hash_still_runs_valid_dummy_verification() {
        let protection = PasswordAuthProtection::new(settings(100, 100));
        let password_ok = protection
            .verify_password("attacker password".to_string(), None)
            .await
            .expect("dummy hash must be valid");
        assert!(!password_ok);
    }
}
