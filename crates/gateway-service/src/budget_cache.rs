//! In-process cache for the budget reads on the request path.
//!
//! Without it, every chargeable request reads each applicable active budget and
//! adds up the ledger for its window, before and after the provider call. The
//! cache keeps both for at most [`BUDGET_CACHE_TTL`]. An expired entry is
//! reloaded from the database, which stays the source of truth: one task loads
//! it and concurrent readers wait for that load, so an expiry under load costs
//! one query, not one per request. A charge is recorded through
//! [`BudgetCache::record_charge`], which orders the ledger write against reloads
//! so each charge is counted exactly once in this process.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gateway_core::{BudgetCadence, BudgetRecord, GatewayError, Money9, budget_window_utc};
use time::OffsetDateTime;

/// How long a cached active budget or window spend answers without a database
/// read. It bounds how far enforcement can lag spend recorded by another replica,
/// a ledger row deleted outside the gateway, or a budget changed by another
/// replica or by config seeding.
pub const BUDGET_CACHE_TTL: Duration = Duration::from_secs(5);

/// Most entries kept per map. One human request uses at most two scopes and
/// three cadences, so this covers thousands of active owners. Past it, entries
/// that are neither fresh nor in use are pruned, at most once per TTL, and a key
/// that still does not fit is answered without caching.
const MAX_CACHED_ENTRIES: usize = 16_384;

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
    /// keeps no total, and its next load reads the row.
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
        let written = write().await?;
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

struct SlotMap<K, T> {
    capacity: usize,
    inner: Mutex<SlotMapInner<K, T>>,
}

struct SlotMapInner<K, T> {
    slots: HashMap<K, Arc<Slot<T>>>,
    pruned_at: Option<Instant>,
}

impl<K: Eq + Hash, T: Clone> SlotMap<K, T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(SlotMapInner {
                slots: HashMap::new(),
                pruned_at: None,
            }),
        }
    }

    fn slot(&self, key: K, ttl: Duration) -> Arc<Slot<T>> {
        let mut inner = self.inner.lock().expect("budget cache lock poisoned");
        if let Some(slot) = inner.slots.get(&key) {
            return Arc::clone(slot);
        }
        if inner.slots.len() >= self.capacity {
            if inner.pruned_at.is_none_or(|at| at.elapsed() >= ttl) {
                // A slot another task holds is loading or recording a charge.
                inner
                    .slots
                    .retain(|_, slot| slot.is_fresh(ttl) || Arc::strong_count(slot) > 1);
                inner.pruned_at = Some(Instant::now());
            }
            if inner.slots.len() >= self.capacity {
                return Arc::new(Slot::default());
            }
        }
        Arc::clone(inner.slots.entry(key).or_default())
    }

    fn clear(&self) {
        self.inner
            .lock()
            .expect("budget cache lock poisoned")
            .slots
            .clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("budget cache lock poisoned")
            .slots
            .len()
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
    loaded_at: Instant,
}

impl<T: Clone> Slot<T> {
    async fn get_or_load<F, Fut>(&self, ttl: Duration, load: F) -> Result<T, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, GatewayError>>,
    {
        if let Some(value) = self.fresh(ttl) {
            return Ok(value);
        }
        // Concurrent readers queue here behind the one load; an expired value is
        // never served.
        let _gate = self.gate.write().await;
        if let Some(value) = self.fresh(ttl) {
            return Ok(value);
        }
        let value = load().await?;
        *self.cached.lock().expect("budget cache lock poisoned") = Some(Cached {
            value: value.clone(),
            loaded_at: Instant::now(),
        });
        Ok(value)
    }

    fn fresh(&self, ttl: Duration) -> Option<T> {
        self.cached
            .lock()
            .expect("budget cache lock poisoned")
            .as_ref()
            .filter(|cached| cached.loaded_at.elapsed() < ttl)
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
    async fn a_full_cache_answers_new_keys_without_growing() {
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
            4,
            "the key past the cap is loaded on each read"
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
