use std::{collections::HashMap, future::Future, hash::Hash, sync::Arc, time::Duration};

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

pub struct ResponseCache<K, V> {
    ttl: Duration,
    entries: Mutex<HashMap<K, Arc<CacheEntry<V>>>>,
    load_gates: Mutex<HashMap<K, Arc<Mutex<()>>>>,
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
            entries: Mutex::new(HashMap::new()),
            load_gates: Mutex::new(HashMap::new()),
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

        let load_gate = {
            let mut load_gates = self.load_gates.lock().await;
            load_gates
                .entry(key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _load_guard = load_gate.lock().await;

        if let Some(value) = self.get_fresh(&key).await {
            self.remove_load_gate(&key, &load_gate).await;
            return Ok((value, CacheStatus::Hit));
        }

        let value = match loader().await {
            Ok(value) => value,
            Err(error) => {
                self.remove_load_gate(&key, &load_gate).await;
                return Err((error, CacheStatus::Miss));
            }
        };
        self.entries.lock().await.insert(
            key.clone(),
            Arc::new(CacheEntry {
                value: value.clone(),
                expires_at: tokio::time::Instant::now() + self.ttl,
            }),
        );

        self.remove_load_gate(&key, &load_gate).await;

        Ok((value, CacheStatus::Miss))
    }

    async fn remove_load_gate(&self, key: &K, load_gate: &Arc<Mutex<()>>) {
        let mut load_gates = self.load_gates.lock().await;
        if load_gates
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, load_gate))
            && Arc::strong_count(load_gate) == 2
        {
            load_gates.remove(key);
        }
    }

    async fn get_fresh(&self, key: &K) -> Option<V> {
        let entry = {
            let mut entries = self.entries.lock().await;
            match entries.get(key) {
                Some(entry) if entry.expires_at > tokio::time::Instant::now() => Arc::clone(entry),
                Some(_) => {
                    entries.remove(key);
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
        assert!(cache.load_gates.lock().await.is_empty());
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
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }

        assert_eq!(active.load(Ordering::SeqCst), 1);
        release.add_permits(2);
        second.await.expect("second task");
        third.await.expect("third task");
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        assert!(cache.load_gates.lock().await.is_empty());
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
                    .await
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
