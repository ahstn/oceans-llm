//! In-process cache for the budget reads on the request path.
//!
//! Without it, every chargeable request reads each applicable active budget and
//! adds up the ledger for its window, before and after the provider call. The
//! cache keeps both for [`BUDGET_CACHE_TTL`]: a recorded charge is added to the
//! cached window spend, and an expired entry is reloaded from the database, which
//! stays the source of truth. One task reloads an entry while the others keep
//! using the expired value, so an expiry under load costs one query, not one per
//! request.

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

/// Entries kept per map before expired ones are pruned. One human request uses at
/// most two scopes, so this covers thousands of active owners.
const MAX_CACHED_ENTRIES: usize = 16_384;

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
        Self {
            ttl,
            budgets: SlotMap::default(),
            spend: SlotMap::default(),
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

    /// Adds a charge that was just written to the ledger to the cached total of
    /// each window of `scope_key` that contains `occurred_at`, whatever its
    /// cadence. A window that is not cached is skipped: its next load reads the
    /// charge from the ledger.
    pub(crate) fn add_charge_to_scope(
        &self,
        scope_key: &str,
        occurred_at: OffsetDateTime,
        charge: Money9,
    ) {
        for cadence in [
            BudgetCadence::Daily,
            BudgetCadence::Weekly,
            BudgetCadence::Monthly,
        ] {
            self.add_charge(
                &SpendWindowKey::containing(scope_key, cadence, occurred_at),
                charge,
            );
        }
    }

    fn add_charge(&self, key: &SpendWindowKey, charge: Money9) {
        if let Some(slot) = self.spend.existing(key) {
            slot.update(|spent| *spent = spent.saturating_add(charge));
        }
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
    slots: Mutex<HashMap<K, Arc<Slot<T>>>>,
}

impl<K, T> Default for SlotMap<K, T> {
    fn default() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash, T: Clone> SlotMap<K, T> {
    fn slot(&self, key: K, ttl: Duration) -> Arc<Slot<T>> {
        let mut slots = self.slots.lock().expect("budget cache lock poisoned");
        if slots.len() >= MAX_CACHED_ENTRIES && !slots.contains_key(&key) {
            slots.retain(|_, slot| slot.is_fresh(ttl) || slot.is_loading());
        }
        Arc::clone(slots.entry(key).or_default())
    }

    fn existing(&self, key: &K) -> Option<Arc<Slot<T>>> {
        self.slots
            .lock()
            .expect("budget cache lock poisoned")
            .get(key)
            .cloned()
    }

    fn clear(&self) {
        self.slots
            .lock()
            .expect("budget cache lock poisoned")
            .clear();
    }
}

struct Slot<T> {
    cached: Mutex<Option<Cached<T>>>,
    /// Held by the one task that loads this slot.
    load: tokio::sync::Mutex<()>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            cached: Mutex::new(None),
            load: tokio::sync::Mutex::new(()),
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
        let _loading = match self.load.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // Another task is reloading: an expired value is good enough meanwhile.
                if let Some(value) = self.any() {
                    return Ok(value);
                }
                self.load.lock().await
            }
        };
        // The task that held the lock may have loaded a fresh value.
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

    fn any(&self) -> Option<T> {
        self.cached
            .lock()
            .expect("budget cache lock poisoned")
            .as_ref()
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

    fn is_loading(&self) -> bool {
        self.load.try_lock().is_err()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn key() -> SpendWindowKey {
        SpendWindowKey {
            scope_key: "budget:v1:user:00000000-0000-0000-0000-000000000001".to_string(),
            cadence: BudgetCadence::Daily,
            period_start: 1_795_305_600,
        }
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

        // Not cached yet: the charge is left for the ledger load to include.
        cache.add_charge(&key(), Money9::from_nanos(2_800));
        assert_eq!(
            spend(&cache, &loads, Money9::from_nanos(2_800)).await,
            Money9::from_nanos(2_800)
        );

        for _ in 0..1_000 {
            cache.add_charge(&key(), Money9::from_nanos(2_800));
        }
        assert_eq!(
            spend(&cache, &loads, Money9::ZERO).await,
            Money9::from_nanos(2_802_800),
            "1,001 charges of $0.0000028"
        );
        assert_eq!(loads.load(Ordering::SeqCst), 1);
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
