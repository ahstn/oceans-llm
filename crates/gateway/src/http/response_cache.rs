use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    hash::Hash,
    sync::Arc,
    time::Duration,
};

use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    Hit,
    Miss,
}

struct CacheEntry<V> {
    value: V,
    expires_at: tokio::time::Instant,
}

/// Most entries kept. At this size an insert first drops the entry that
/// expires soonest, which is an expired one when there is any.
const MAX_ENTRIES: usize = 4096;

/// The cached values, and their keys in the order they expire. Every entry
/// lives for the same TTL, so that is the order they were inserted in, and
/// eviction pops from the front instead of scanning the map.
struct Entries<K, V> {
    map: HashMap<K, Arc<CacheEntry<V>>>,
    /// May hold keys since replaced or removed; those are skipped, and the
    /// queue is compacted once it grows past twice the cap.
    expiry_order: VecDeque<(K, tokio::time::Instant)>,
}

impl<K: Clone + Eq + Hash, V> Entries<K, V> {
    fn insert(&mut self, key: K, entry: CacheEntry<V>) {
        if !self.map.contains_key(&key) {
            while self.map.len() >= MAX_ENTRIES {
                let Some((oldest, expires_at)) = self.expiry_order.pop_front() else {
                    break;
                };
                if self
                    .map
                    .get(&oldest)
                    .is_some_and(|current| current.expires_at == expires_at)
                {
                    self.map.remove(&oldest);
                }
            }
        }
        self.expiry_order.push_back((key.clone(), entry.expires_at));
        self.map.insert(key, Arc::new(entry));
        if self.expiry_order.len() > 2 * MAX_ENTRIES {
            let map = &self.map;
            self.expiry_order.retain(|(key, expires_at)| {
                map.get(key)
                    .is_some_and(|current| current.expires_at == *expires_at)
            });
        }
    }
}

pub struct ResponseCache<K, V> {
    ttl: Duration,
    entries: Mutex<Entries<K, V>>,
    /// Held only to look up or remove a gate, never across an await, so a
    /// share can leave it from `Drop`.
    load_gates: LoadGates<K>,
}

type LoadGates<K> = std::sync::Mutex<HashMap<K, Arc<Mutex<()>>>>;

/// A caller's share of a key's load gate. Dropping the last share removes the
/// gate, whether the load finished, failed or the caller was cancelled, so
/// abandoned loads leave no gates behind.
struct LoadGateShare<'a, K: Eq + Hash> {
    gates: &'a LoadGates<K>,
    key: K,
    gate: Arc<Mutex<()>>,
}

impl<K: Eq + Hash> Drop for LoadGateShare<'_, K> {
    fn drop(&mut self) {
        let mut gates = self
            .gates
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // The map holds one reference and this share the other.
        if gates
            .get(&self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.gate))
            && Arc::strong_count(&self.gate) == 2
        {
            gates.remove(&self.key);
        }
    }
}

impl<K, V> ResponseCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(Entries {
                map: HashMap::new(),
                expiry_order: VecDeque::new(),
            }),
            load_gates: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_or_load<E, F, Fut>(
        &self,
        key: K,
        loader: F,
    ) -> Result<(V, CacheStatus), (E, CacheStatus)>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>>,
    {
        if let Some(value) = self.get_fresh(&key).await {
            return Ok((value, CacheStatus::Hit));
        }

        let share = {
            let mut load_gates = self
                .load_gates
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            LoadGateShare {
                gates: &self.load_gates,
                key: key.clone(),
                gate: load_gates
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(())))
                    .clone(),
            }
        };
        let _load_guard = share.gate.lock().await;

        if let Some(value) = self.get_fresh(&key).await {
            return Ok((value, CacheStatus::Hit));
        }

        let value = match loader().await {
            Ok(value) => value,
            Err(error) => return Err((error, CacheStatus::Miss)),
        };
        self.entries.lock().await.insert(
            key.clone(),
            CacheEntry {
                value: value.clone(),
                expires_at: tokio::time::Instant::now() + self.ttl,
            },
        );

        Ok((value, CacheStatus::Miss))
    }

    async fn get_fresh(&self, key: &K) -> Option<V> {
        let entry = {
            let mut entries = self.entries.lock().await;
            match entries.map.get(key) {
                Some(entry) if entry.expires_at > tokio::time::Instant::now() => Arc::clone(entry),
                Some(_) => {
                    entries.map.remove(key);
                    return None;
                }
                None => return None,
            }
        };

        Some(entry.value.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn concurrent_misses_share_one_load() {
        let cache = Arc::new(ResponseCache::new(Duration::from_secs(30)));
        let load_count = Arc::new(AtomicUsize::new(0));

        let first = load(&cache, &load_count);
        let second = load(&cache, &load_count);
        let (first, second) = tokio::join!(first, second);

        assert_eq!(first.expect("first response").0, 42);
        assert_eq!(second.expect("second response").0, 42);
        assert_eq!(load_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fresh_entries_stay_within_the_bound() {
        let cache = ResponseCache::new(Duration::from_secs(30));
        for key in 0..=MAX_ENTRIES {
            cache
                .get_or_load(key, || async { Ok::<_, ()>(key) })
                .await
                .expect("response");
        }
        let entries = cache.entries.lock().await;
        assert_eq!(entries.map.len(), MAX_ENTRIES);
        // The first key, the soonest to expire, made room for the last.
        assert!(!entries.map.contains_key(&0));
        assert!(entries.map.contains_key(&MAX_ENTRIES));
    }

    #[tokio::test]
    async fn cancelled_loads_leave_no_load_gate() {
        let cache = ResponseCache::new(Duration::from_secs(30));
        let cancelled = tokio::time::timeout(
            Duration::from_millis(10),
            cache.get_or_load(1, || async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok::<_, ()>(1)
            }),
        )
        .await;
        assert!(cancelled.is_err());
        assert!(cache.load_gates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expired_values_are_reloaded() {
        let cache = ResponseCache::new(Duration::ZERO);
        let first = cache
            .get_or_load("7d", || async { Ok::<_, ()>(1) })
            .await
            .expect("first response");
        let second = cache
            .get_or_load("7d", || async { Ok::<_, ()>(2) })
            .await
            .expect("second response");

        assert_eq!(first, (1, CacheStatus::Miss));
        assert_eq!(second, (2, CacheStatus::Miss));
    }

    #[tokio::test]
    async fn failed_loads_do_not_leave_gates_registered() {
        let cache = ResponseCache::<&str, usize>::new(Duration::from_secs(30));

        let result = cache
            .get_or_load("7d", || async { Err::<usize, _>("load failed") })
            .await;

        assert_eq!(result, Err(("load failed", CacheStatus::Miss)));
        assert!(cache.load_gates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_loads_remain_single_flight_while_waiters_drain() {
        use tokio::sync::Semaphore;

        let cache = Arc::new(ResponseCache::<&str, usize>::new(Duration::from_secs(30)));
        let started = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        let first = tokio::spawn(blocking_failure(
            Arc::clone(&cache),
            Arc::clone(&started),
            Arc::clone(&release),
            Arc::clone(&active),
            Arc::clone(&max_active),
        ));
        started.acquire().await.expect("first loader").forget();

        let second = tokio::spawn(blocking_failure(
            Arc::clone(&cache),
            Arc::clone(&started),
            Arc::clone(&release),
            Arc::clone(&active),
            Arc::clone(&max_active),
        ));
        wait_for_gate_holders(&cache, 3).await;
        release.add_permits(1);
        first.await.expect("first task");
        started.acquire().await.expect("second loader").forget();

        let third = tokio::spawn(blocking_failure(
            Arc::clone(&cache),
            Arc::clone(&started),
            Arc::clone(&release),
            Arc::clone(&active),
            Arc::clone(&max_active),
        ));
        wait_for_gate_holders(&cache, 3).await;

        assert_eq!(active.load(Ordering::SeqCst), 1);
        release.add_permits(2);
        second.await.expect("second task");
        third.await.expect("third task");
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        assert!(cache.load_gates.lock().unwrap().is_empty());
    }

    async fn load(
        cache: &ResponseCache<&'static str, usize>,
        load_count: &AtomicUsize,
    ) -> Result<(usize, CacheStatus), ((), CacheStatus)> {
        cache
            .get_or_load("7d", || async {
                load_count.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(42)
            })
            .await
    }

    async fn blocking_failure(
        cache: Arc<ResponseCache<&'static str, usize>>,
        started: Arc<tokio::sync::Semaphore>,
        release: Arc<tokio::sync::Semaphore>,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    ) {
        let result = cache
            .get_or_load("7d", || async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(current, Ordering::SeqCst);
                started.add_permits(1);
                release.acquire().await.expect("release loader").forget();
                active.fetch_sub(1, Ordering::SeqCst);
                Err::<usize, _>(())
            })
            .await;
        assert_eq!(result, Err(((), CacheStatus::Miss)));
    }

    async fn wait_for_gate_holders(cache: &ResponseCache<&'static str, usize>, minimum: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let holder_count = cache
                    .load_gates
                    .lock()
                    .unwrap()
                    .get("7d")
                    .map_or(0, Arc::strong_count);
                if holder_count >= minimum {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("waiter registered");
    }
}
