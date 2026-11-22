//! In-process cache for the budget reads on the request path.
//!
//! Without it, every chargeable request reads each applicable active budget and
//! adds up the ledger for its window, before and after the provider call. The
//! cache answers from a value whose database read started at most
//! [`BUDGET_CACHE_TTL`] ago. An expired entry is reloaded from the database,
//! which stays the source of truth: one task loads it and concurrent readers wait
//! for that load, so an expiry under load costs one query, not one per request.
//! A charge is recorded through [`BudgetCache::record_charge`], which orders the
//! ledger write against reloads so each charge is counted exactly once in this
//! process.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gateway_core::{BudgetCadence, BudgetRecord, GatewayError, Money9, budget_window_utc};
use time::OffsetDateTime;

/// How long a cached active budget or window spend answers without a database
/// read, counted from the start of the read that produced it. It bounds how far
/// enforcement can lag spend recorded by another replica, a ledger row deleted
/// outside the gateway, or a budget changed by another replica or by config
/// seeding.
pub const BUDGET_CACHE_TTL: Duration = Duration::from_secs(5);

/// Most entries kept per map. One human request uses at most two scopes and
/// three cadences, so this covers thousands of active owners. A full map drops
/// expired entries, then other entries no task is using, down to
/// [`EVICTED_FILL`] of this. An entry in use is never dropped, so the map can
/// exceed this only by the entries of requests in flight.
const MAX_CACHED_ENTRIES: usize = 16_384;

/// Fill, as a fraction of capacity (numerator over 4), that eviction stops at, so
/// a full map scans its entries once per quarter of its capacity in new keys.
const EVICTED_FILL: usize = 3;

/// Every cadence a charge can count toward.
const CADENCES: [BudgetCadence; 3] = [
    BudgetCadence::Daily,
    BudgetCadence::Weekly,
    BudgetCadence::Monthly,
];

/// Identifies one enforcement window of one budget scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SpendWindowKey {
    pub scope_key: String,
    pub cadence: BudgetCadence,
    /// Unix seconds of the window's first instant.
    pub period_start: i64,
}

impl SpendWindowKey {
    /// The `cadence` window of `scope_key` that contains `occurred_at`.
    pub(crate) fn containing(
        scope_key: &str,
        cadence: BudgetCadence,
        occurred_at: OffsetDateTime,
    ) -> Self {
        Self {
            scope_key: scope_key.to_string(),
            cadence,
            period_start: budget_window_utc(cadence, occurred_at)
                .period_start
                .unix_timestamp(),
        }
    }
}

pub(crate) struct BudgetCache {
    ttl: Duration,
    budgets: SlotMap<String, Option<BudgetRecord>>,
    spend: SlotMap<SpendWindowKey, Money9>,
}

impl BudgetCache {
    #[must_use]
    pub(crate) fn new(ttl: Duration) -> Self {
        Self::with_capacity(ttl, MAX_CACHED_ENTRIES)
    }

    #[must_use]
    fn with_capacity(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            budgets: SlotMap::new(capacity),
            spend: SlotMap::new(capacity),
        }
    }

    /// Returns the active budget for `scope_key`, loading it with `load` when the
    /// cached answer is missing or expired.
    pub(crate) async fn active_budget<F, Fut>(
        &self,
        scope_key: &str,
        load: F,
    ) -> Result<Option<BudgetRecord>, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<BudgetRecord>, GatewayError>>,
    {
        self.budgets
            .slot(scope_key.to_string(), self.ttl)
            .get_or_load(self.ttl, load)
            .await
    }

    /// Returns the spend recorded so far in a window, loading it with `load` (a
    /// ledger sum up to now) when the cached total is missing or expired.
    pub(crate) async fn window_spend<F, Fut>(
        &self,
        key: &SpendWindowKey,
        load: F,
    ) -> Result<Money9, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Money9, GatewayError>>,
    {
        self.spend
            .slot(key.clone(), self.ttl)
            .get_or_load(self.ttl, load)
            .await
    }

    /// Runs `write`, the ledger insert of `charge`, and when it stores the row,
    /// adds the charge to the cached total of every window of `scope_keys` that
    /// contains `occurred_at`, whatever its cadence.
    ///
    /// No reload of those windows reads the ledger while the write runs, so a
    /// reload either sees the committed row or runs before it and gets the charge
    /// added afterwards: never both, never neither. A window that is not cached
    /// keeps no total, and its next load reads the row. When the write fails or
    /// is cancelled, the row may still have been committed, so those windows
    /// drop their cached totals and their next read reloads them.
    pub(crate) async fn record_charge<F, Fut>(
        &self,
        scope_keys: &[String],
        occurred_at: OffsetDateTime,
        charge: Money9,
        write: F,
    ) -> Result<bool, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<bool, GatewayError>>,
    {
        let mut slots: Vec<_> = scope_keys
            .iter()
            .flat_map(|scope_key| {
                CADENCES.map(|cadence| SpendWindowKey::containing(scope_key, cadence, occurred_at))
            })
            .map(|key| self.spend.slot(key, self.ttl))
            .collect();
        // One global order for the shared gates, so recorders and reloads cannot
        // wait on each other in a cycle.
        slots.sort_unstable_by_key(|slot| Arc::as_ptr(slot) as usize);
        slots.dedup_by(|a, b| Arc::ptr_eq(a, b));
        let mut gates = Vec::with_capacity(slots.len());
        for slot in &slots {
            gates.push(slot.gate.read().await);
        }
        let unknown_outcome = ForgetOnDrop(&slots);
        let written = write().await?;
        // The outcome is known: keep the cached totals.
        std::mem::forget(unknown_outcome);
        if written {
            for slot in &slots {
                slot.update(|spent| *spent = spent.saturating_add(charge));
            }
        }
        Ok(written)
    }

    /// Forgets every cached active budget, so the next read sees a budget that
    /// this process just created, changed or deactivated.
    pub(crate) fn invalidate_budgets(&self) {
        self.budgets.clear();
    }
}

impl Default for BudgetCache {
    fn default() -> Self {
        Self::new(BUDGET_CACHE_TTL)
    }
}

/// Drops the cached values of its slots when dropped. [`BudgetCache::record_charge`]
/// holds one across the ledger write and forgets it once the outcome is known,
/// so an error or a cancelled write leaves no total that may miss the row.
struct ForgetOnDrop<'a, T>(&'a [Arc<Slot<T>>]);

impl<T> Drop for ForgetOnDrop<'_, T> {
    fn drop(&mut self) {
        for slot in self.0 {
            *slot.cached.lock().expect("budget cache lock poisoned") = None;
        }
    }
}

/// Slots by key. Every task that uses a key while its slot is in the map gets
/// the same slot, so one gate orders all loads and charges of that key.
struct SlotMap<K, T> {
    capacity: usize,
    slots: Mutex<HashMap<K, Arc<Slot<T>>>>,
}

impl<K: Eq + Hash, T: Clone> SlotMap<K, T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            slots: Mutex::new(HashMap::new()),
        }
    }

    fn slot(&self, key: K, ttl: Duration) -> Arc<Slot<T>> {
        let mut slots = self.slots.lock().expect("budget cache lock poisoned");
        if let Some(slot) = slots.get(&key) {
            return Arc::clone(slot);
        }
        if slots.len() >= self.capacity {
            // The map holds one reference; any other is a task loading the slot or
            // recording a charge through it, and its slot must stay the only one.
            // References are only cloned under this lock, so none appears here.
            let in_use = |slot: &Arc<Slot<T>>| Arc::strong_count(slot) > 1;
            slots.retain(|_, slot| in_use(slot) || slot.is_fresh(ttl));
            let mut excess = slots.len().saturating_sub(self.capacity * EVICTED_FILL / 4);
            slots.retain(|_, slot| {
                if excess == 0 || in_use(slot) {
                    return true;
                }
                excess -= 1;
                false
            });
        }
        Arc::clone(slots.entry(key).or_default())
    }

    fn clear(&self) {
        self.slots
            .lock()
            .expect("budget cache lock poisoned")
            .clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots.lock().expect("budget cache lock poisoned").len()
    }
}

struct Slot<T> {
    cached: Mutex<Option<Cached<T>>>,
    /// Held exclusively by the one task that loads this slot, from its read until
    /// it stores the value, and shared by each task recording a charge, from
    /// before its ledger write until it adds the charge.
    gate: tokio::sync::RwLock<()>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            cached: Mutex::new(None),
            gate: tokio::sync::RwLock::new(()),
        }
    }
}

struct Cached<T> {
    value: T,
    /// When the read that produced `value` started: the value is at least this
    /// recent.
    read_started_at: Instant,
}

impl<T: Clone> Slot<T> {
    async fn get_or_load<F, Fut>(&self, ttl: Duration, load: F) -> Result<T, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, GatewayError>>,
    {
        let arrived_at = Instant::now();
        if let Some(value) = self.fresh(ttl) {
            return Ok(value);
        }
        // Concurrent readers queue here behind the one load; an expired value is
        // never served.
        let _gate = self.gate.write().await;
        if let Some(value) = self.fresh(ttl).or_else(|| self.read_since(arrived_at)) {
            return Ok(value);
        }
        let read_started_at = Instant::now();
        let value = load().await?;
        *self.cached.lock().expect("budget cache lock poisoned") = Some(Cached {
            value: value.clone(),
            read_started_at,
        });
        Ok(value)
    }

    fn fresh(&self, ttl: Duration) -> Option<T> {
        self.cached_if(|cached| cached.read_started_at.elapsed() < ttl)
    }

    /// The value of a read that started at or after `instant`. A reader that
    /// waited behind that read gets its result rather than reading again, even
    /// when the read took longer than the TTL.
    fn read_since(&self, instant: Instant) -> Option<T> {
        self.cached_if(|cached| cached.read_started_at >= instant)
    }

    fn cached_if(&self, keep: impl FnOnce(&Cached<T>) -> bool) -> Option<T> {
        self.cached
            .lock()
            .expect("budget cache lock poisoned")
            .as_ref()
            .filter(|cached| keep(cached))
            .map(|cached| cached.value.clone())
    }

    fn update(&self, change: impl FnOnce(&mut T)) {
        if let Some(cached) = self
            .cached
            .lock()
            .expect("budget cache lock poisoned")
            .as_mut()
        {
            change(&mut cached.value);
        }
    }

    fn is_fresh(&self, ttl: Duration) -> bool {
        self.fresh(ttl).is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    use tokio::sync::Notify;

    use super::*;

    const SCOPE: &str = "budget:v1:user:00000000-0000-0000-0000-000000000001";

    fn at() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_795_305_600 + 60).expect("valid timestamp")
    }

    fn key() -> SpendWindowKey {
        SpendWindowKey::containing(SCOPE, BudgetCadence::Daily, at())
    }

    async fn spend(cache: &BudgetCache, loads: &AtomicUsize, ledger: Money9) -> Money9 {
        cache
            .window_spend(&key(), || async {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(ledger)
            })
            .await
            .expect("load succeeds")
    }

    /// Records `nanos` as a ledger row in `ledger` and in the cache.
    async fn charge(cache: &BudgetCache, ledger: &AtomicI64, nanos: i64) {
        let written = cache
            .record_charge(
                &[SCOPE.to_string()],
                at(),
                Money9::from_nanos(nanos),
                || async {
                    ledger.fetch_add(nanos, Ordering::SeqCst);
                    Ok(true)
                },
            )
            .await
            .expect("charge recorded");
        assert!(written);
    }

    /// The cached daily total, whatever its age.
    fn cached_total(cache: &BudgetCache) -> Option<Money9> {
        cache.spend.slot(key(), cache.ttl).fresh(Duration::MAX)
    }

    #[tokio::test]
    async fn fresh_window_spend_answers_without_a_reload() {
        let cache = BudgetCache::new(Duration::from_secs(60));
        let loads = AtomicUsize::new(0);

        assert_eq!(
            spend(&cache, &loads, Money9::from_nanos(7)).await,
            Money9::from_nanos(7)
        );
        assert_eq!(
            spend(&cache, &loads, Money9::from_nanos(99)).await,
            Money9::from_nanos(7)
        );
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn recorded_charges_add_to_the_cached_window_only() {
        let cache = BudgetCache::new(Duration::from_secs(60));
        let loads = AtomicUsize::new(0);
        let ledger = AtomicI64::new(0);

        // Not cached yet: the charge is left for the ledger load to include.
        charge(&cache, &ledger, 2_800).await;
        assert_eq!(
            spend(
                &cache,
                &loads,
                Money9::from_nanos(ledger.load(Ordering::SeqCst))
            )
            .await,
            Money9::from_nanos(2_800)
        );

        for _ in 0..1_000 {
            charge(&cache, &ledger, 2_800).await;
        }
        assert_eq!(
            spend(&cache, &loads, Money9::ZERO).await,
            Money9::from_nanos(2_802_800),
            "1,001 charges of $0.0000028"
        );
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_charge_recorded_during_a_reload_counts_once_when_the_reload_reads_first() {
        let cache = Arc::new(BudgetCache::new(Duration::ZERO));
        let ledger = Arc::new(AtomicI64::new(0));
        let (read, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let reload = tokio::spawn({
            let (cache, ledger, read, release) = (
                Arc::clone(&cache),
                Arc::clone(&ledger),
                Arc::clone(&read),
                Arc::clone(&release),
            );
            async move {
                cache
                    .window_spend(&key(), || async move {
                        let snapshot = ledger.load(Ordering::SeqCst);
                        read.notify_one();
                        release.notified().await;
                        Ok(Money9::from_nanos(snapshot))
                    })
                    .await
            }
        });
        read.notified().await;
        let recorder = tokio::spawn({
            let (cache, ledger) = (Arc::clone(&cache), Arc::clone(&ledger));
            async move { charge(&cache, &ledger, 100).await }
        });
        tokio::task::yield_now().await;
        assert_eq!(
            ledger.load(Ordering::SeqCst),
            0,
            "the ledger write waits for the reload"
        );

        release.notify_one();
        assert_eq!(
            reload.await.expect("reload task").expect("reload"),
            Money9::ZERO
        );
        recorder.await.expect("recorder task");
        assert_eq!(cached_total(&cache), Some(Money9::from_nanos(100)));
        assert_eq!(ledger.load(Ordering::SeqCst), 100);
    }

    #[tokio::test]
    async fn a_charge_recorded_during_a_reload_counts_once_when_the_write_runs_first() {
        let cache = Arc::new(BudgetCache::new(Duration::ZERO));
        let ledger = Arc::new(AtomicI64::new(0));
        cache
            .window_spend(&key(), || async { Ok(Money9::ZERO) })
            .await
            .expect("prime the cache");
        let (written, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let recorder = tokio::spawn({
            let (cache, ledger, written, release) = (
                Arc::clone(&cache),
                Arc::clone(&ledger),
                Arc::clone(&written),
                Arc::clone(&release),
            );
            async move {
                cache
                    .record_charge(
                        &[SCOPE.to_string()],
                        at(),
                        Money9::from_nanos(100),
                        || async move {
                            ledger.fetch_add(100, Ordering::SeqCst);
                            written.notify_one();
                            release.notified().await;
                            Ok(true)
                        },
                    )
                    .await
            }
        });
        written.notified().await;
        let reload = tokio::spawn({
            let (cache, ledger) = (Arc::clone(&cache), Arc::clone(&ledger));
            async move {
                cache
                    .window_spend(&key(), || async move {
                        Ok(Money9::from_nanos(ledger.load(Ordering::SeqCst)))
                    })
                    .await
            }
        });
        tokio::task::yield_now().await;
        assert!(
            !reload.is_finished(),
            "the reload waits for the ledger write"
        );

        release.notify_one();
        assert!(
            recorder
                .await
                .expect("recorder task")
                .expect("charge recorded")
        );
        assert_eq!(
            reload.await.expect("reload task").expect("reload"),
            Money9::from_nanos(100)
        );
        assert_eq!(cached_total(&cache), Some(Money9::from_nanos(100)));
    }

    #[tokio::test]
    async fn a_reload_in_progress_never_serves_the_expired_value() {
        let cache = Arc::new(BudgetCache::new(Duration::ZERO));
        cache
            .window_spend(&key(), || async { Ok(Money9::from_nanos(1)) })
            .await
            .expect("prime the cache");
        let (started, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let slow = tokio::spawn({
            let (cache, started, release) = (
                Arc::clone(&cache),
                Arc::clone(&started),
                Arc::clone(&release),
            );
            async move {
                cache
                    .window_spend(&key(), || async move {
                        started.notify_one();
                        release.notified().await;
                        Ok(Money9::from_nanos(5))
                    })
                    .await
            }
        });
        started.notified().await;
        let waiting = tokio::spawn({
            let cache = Arc::clone(&cache);
            async move {
                cache
                    .window_spend(&key(), || async { Ok(Money9::from_nanos(9)) })
                    .await
            }
        });
        tokio::task::yield_now().await;
        assert!(
            !waiting.is_finished(),
            "no expired value while the reload runs"
        );

        release.notify_one();
        assert_eq!(
            slow.await.expect("slow task").expect("reload"),
            Money9::from_nanos(5)
        );
        assert_eq!(
            waiting.await.expect("waiting task").expect("reload"),
            Money9::from_nanos(9)
        );
    }

    #[tokio::test]
    async fn a_full_cache_evicts_unused_entries_to_make_room() {
        let cache = BudgetCache::with_capacity(Duration::from_secs(60), 2);
        let loads = AtomicUsize::new(0);
        for scope in ["a", "b", "c", "c"] {
            let key = SpendWindowKey::containing(scope, BudgetCadence::Daily, at());
            let spent = cache
                .window_spend(&key, || async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    Ok(Money9::from_nanos(7))
                })
                .await
                .expect("load succeeds");
            assert_eq!(spent, Money9::from_nanos(7));
        }
        assert_eq!(cache.spend.len(), 2);
        assert_eq!(
            loads.load(Ordering::SeqCst),
            3,
            "the key that made room stays cached"
        );
    }

    #[tokio::test]
    async fn a_full_cache_keeps_one_gate_for_a_window_with_a_charge_in_flight() {
        let cache = Arc::new(BudgetCache::with_capacity(Duration::from_secs(60), 2));
        for scope in ["a", "b"] {
            let key = SpendWindowKey::containing(scope, BudgetCadence::Daily, at());
            cache
                .window_spend(&key, || async { Ok(Money9::ZERO) })
                .await
                .expect("fill the cache");
        }
        let ledger = Arc::new(AtomicI64::new(0));
        let (written, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let recorder = tokio::spawn({
            let (cache, ledger, written, release) = (
                Arc::clone(&cache),
                Arc::clone(&ledger),
                Arc::clone(&written),
                Arc::clone(&release),
            );
            async move {
                cache
                    .record_charge(
                        &[SCOPE.to_string()],
                        at(),
                        Money9::from_nanos(100),
                        || async move {
                            written.notify_one();
                            release.notified().await;
                            ledger.fetch_add(100, Ordering::SeqCst);
                            Ok(true)
                        },
                    )
                    .await
            }
        });
        written.notified().await;
        for scope in ["c", "d"] {
            let key = SpendWindowKey::containing(scope, BudgetCadence::Daily, at());
            cache
                .window_spend(&key, || async { Ok(Money9::ZERO) })
                .await
                .expect("churn the full cache");
        }
        let reader = tokio::spawn({
            let (cache, ledger) = (Arc::clone(&cache), Arc::clone(&ledger));
            async move {
                cache
                    .window_spend(&key(), || async move {
                        Ok(Money9::from_nanos(ledger.load(Ordering::SeqCst)))
                    })
                    .await
            }
        });
        tokio::task::yield_now().await;
        assert!(
            !reader.is_finished(),
            "the reader waits for the charge in flight"
        );
        assert!(
            cache.spend.len() <= 2 + CADENCES.len(),
            "only slots in use exceed the cap"
        );

        release.notify_one();
        assert!(
            recorder
                .await
                .expect("recorder task")
                .expect("charge recorded")
        );
        assert_eq!(
            reader.await.expect("reader task").expect("reload"),
            Money9::from_nanos(100)
        );
    }

    #[tokio::test]
    async fn a_failed_or_cancelled_ledger_write_drops_the_cached_totals() {
        let cache = Arc::new(BudgetCache::new(Duration::from_secs(60)));
        let prime = || async {
            cache
                .window_spend(&key(), || async { Ok(Money9::from_nanos(5)) })
                .await
                .expect("prime the cache");
        };

        prime().await;
        let failed = cache
            .record_charge(
                &[SCOPE.to_string()],
                at(),
                Money9::from_nanos(100),
                || async {
                    Err(GatewayError::Internal(
                        "connection lost after commit".to_string(),
                    ))
                },
            )
            .await;
        assert!(failed.is_err());
        assert_eq!(
            cached_total(&cache),
            None,
            "an error may hide a committed row"
        );

        prime().await;
        let cancelled = tokio::spawn({
            let cache = Arc::clone(&cache);
            async move {
                cache
                    .record_charge(&[SCOPE.to_string()], at(), Money9::from_nanos(100), || {
                        std::future::pending::<Result<bool, GatewayError>>()
                    })
                    .await
            }
        });
        tokio::task::yield_now().await;
        cancelled.abort();
        assert!(cancelled.await.expect_err("task aborted").is_cancelled());
        assert_eq!(
            cached_total(&cache),
            None,
            "a cancelled write may have committed"
        );
    }

    #[tokio::test]
    async fn a_value_ages_from_the_start_of_its_read() {
        let ttl = Duration::from_millis(30);
        let cache = BudgetCache::new(ttl);
        cache
            .window_spend(&key(), || async {
                tokio::time::sleep(2 * ttl).await;
                Ok(Money9::from_nanos(1))
            })
            .await
            .expect("slow load");
        assert!(
            !cache.spend.slot(key(), ttl).is_fresh(ttl),
            "a read that took longer than the TTL is already expired"
        );
    }

    #[tokio::test]
    async fn readers_queued_behind_a_slow_load_share_its_result() {
        let ttl = Duration::from_millis(20);
        let cache = Arc::new(BudgetCache::new(ttl));
        let loads = Arc::new(AtomicUsize::new(0));
        let ledger = Arc::new(AtomicI64::new(0));
        let (written, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        // A charge in flight holds the gate, so both readers queue behind it.
        let recorder = tokio::spawn({
            let (cache, ledger, written, release) = (
                Arc::clone(&cache),
                Arc::clone(&ledger),
                Arc::clone(&written),
                Arc::clone(&release),
            );
            async move {
                cache
                    .record_charge(
                        &[SCOPE.to_string()],
                        at(),
                        Money9::from_nanos(100),
                        || async move {
                            written.notify_one();
                            release.notified().await;
                            ledger.fetch_add(100, Ordering::SeqCst);
                            Ok(true)
                        },
                    )
                    .await
            }
        });
        written.notified().await;
        let readers: Vec<_> = (0..2)
            .map(|_| {
                let (cache, ledger, loads) =
                    (Arc::clone(&cache), Arc::clone(&ledger), Arc::clone(&loads));
                tokio::spawn(async move {
                    cache
                        .window_spend(&key(), || async move {
                            loads.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(2 * ttl).await;
                            Ok(Money9::from_nanos(ledger.load(Ordering::SeqCst)))
                        })
                        .await
                })
            })
            .collect();
        tokio::task::yield_now().await;

        release.notify_one();
        assert!(
            recorder
                .await
                .expect("recorder task")
                .expect("charge recorded")
        );
        for reader in readers {
            assert_eq!(
                reader.await.expect("reader task").expect("load"),
                Money9::from_nanos(100)
            );
        }
        assert_eq!(
            loads.load(Ordering::SeqCst),
            1,
            "the second reader takes the read that started after it arrived"
        );
    }

    #[tokio::test]
    async fn expired_window_spend_reloads_from_the_ledger() {
        let cache = BudgetCache::new(Duration::ZERO);
        let loads = AtomicUsize::new(0);

        spend(&cache, &loads, Money9::from_nanos(1)).await;
        assert_eq!(
            spend(&cache, &loads, Money9::from_nanos(5)).await,
            Money9::from_nanos(5)
        );
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_cold_reads_share_one_load() {
        let cache = Arc::new(BudgetCache::new(Duration::from_secs(60)));
        let loads = Arc::new(AtomicUsize::new(0));
        let (release, released) = tokio::sync::watch::channel(false);

        let readers: Vec<_> = (0..50)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let loads = Arc::clone(&loads);
                let mut released = released.clone();
                tokio::spawn(async move {
                    cache
                        .window_spend(&key(), || async move {
                            loads.fetch_add(1, Ordering::SeqCst);
                            released.wait_for(|open| *open).await.expect("sender kept");
                            Ok(Money9::from_nanos(42))
                        })
                        .await
                })
            })
            .collect();
        tokio::task::yield_now().await;
        release.send(true).expect("readers listening");

        for reader in readers {
            let spent = reader.await.expect("reader task").expect("load succeeds");
            assert_eq!(spent, Money9::from_nanos(42));
        }
        assert_eq!(loads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalidation_reloads_active_budgets() {
        let cache = BudgetCache::new(Duration::from_secs(60));
        let loads = AtomicUsize::new(0);
        let read = || async {
            cache
                .active_budget("budget:v1:user:x", || async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    Ok(None)
                })
                .await
                .expect("load succeeds")
        };

        read().await;
        read().await;
        cache.invalidate_budgets();
        read().await;
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_loads_are_not_cached() {
        let cache = BudgetCache::new(Duration::from_secs(60));
        let failed = cache
            .window_spend(&key(), || async {
                Err(GatewayError::Internal("database down".to_string()))
            })
            .await;
        assert!(failed.is_err());

        let loads = AtomicUsize::new(0);
        assert_eq!(
            spend(&cache, &loads, Money9::from_nanos(3)).await,
            Money9::from_nanos(3)
        );
    }
}
