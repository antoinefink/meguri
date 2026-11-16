// SPDX-License-Identifier: MIT
use crate::{Error, Result, crypto};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Reserves bytes before U5–U7 staging writes. Keep the guard until the staged
/// files have been removed or durably adopted (including all error cleanup).
#[derive(Clone)]
pub struct TempBudget(Arc<Mutex<(u64, u64)>>);
pub struct TempReservation {
    budget: TempBudget,
    bytes: u64,
}
impl TempBudget {
    pub fn new(limit: std::num::NonZeroU64) -> Self {
        Self(Arc::new(Mutex::new((limit.get(), 0))))
    }
    pub fn reserve(&self, bytes: u64) -> Result<TempReservation> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| Error::new("storage_unavailable"))?;
        let used = state
            .1
            .checked_add(bytes)
            .filter(|n| *n <= state.0)
            .ok_or(Error::new("storage_unavailable"))?;
        state.1 = used;
        Ok(TempReservation {
            budget: self.clone(),
            bytes,
        })
    }
    pub fn used(&self) -> u64 {
        self.0.lock().expect("budget mutex").1
    }
}
impl Drop for TempReservation {
    fn drop(&mut self) {
        self.budget.0.lock().expect("budget mutex").1 -= self.bytes;
    }
}

/// Two running jobs plus eight admitted waiters, acquired before spawn_blocking.
pub struct HashPool {
    admitted: Arc<Semaphore>,
    workers: Arc<Semaphore>,
}
impl Default for HashPool {
    fn default() -> Self {
        Self {
            admitted: Arc::new(Semaphore::new(10)),
            workers: Arc::new(Semaphore::new(2)),
        }
    }
}
impl HashPool {
    #[cfg(test)]
    pub(crate) fn test_hold_admission(&self) -> Vec<OwnedSemaphorePermit> {
        (0..10)
            .map(|_| self.admitted.clone().try_acquire_owned().unwrap())
            .collect()
    }
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let admission = self
            .admitted
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::limited(1))?;
        let worker = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::new("storage_unavailable"))?;
        tokio::task::spawn_blocking(move || {
            let _guards = (admission, worker);
            f()
        })
        .await
        .map_err(|_| Error::new("internal_error"))?
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Ip(IpAddr),
    Name([u8; 32]),
    Capability([u8; 32]),
}
pub struct Attempts {
    entries: Mutex<HashMap<Key, VecDeque<Instant>>>,
}
impl Default for Attempts {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}
impl Attempts {
    pub fn username(&self, ip: IpAddr, name: &str) -> Result<()> {
        self.charge(
            ip,
            Key::Name(crypto::digest(name.as_bytes())),
            Instant::now(),
        )
    }
    pub fn capability(&self, ip: IpAddr, value: &str) -> Result<()> {
        self.charge(
            ip,
            Key::Capability(crypto::digest(value.as_bytes())),
            Instant::now(),
        )
    }
    fn charge(&self, ip: IpAddr, other: Key, now: Instant) -> Result<()> {
        const WINDOW: Duration = Duration::from_secs(300);
        let mut entries = self.entries.lock().map_err(|_| Error::limited(300))?;
        entries.retain(|_, attempts| {
            while attempts
                .front()
                .is_some_and(|t| now.duration_since(*t) >= WINDOW)
            {
                attempts.pop_front();
            }
            !attempts.is_empty()
        });
        let keys = [(Key::Ip(ip), 20), (other, 10)];
        let needed = keys
            .iter()
            .filter(|(k, _)| !entries.contains_key(k))
            .count();
        if entries.len() + needed > 10_000 {
            return Err(Error::limited(300));
        }
        let mut retry = 0;
        for (key, max) in &keys {
            if let Some(values) = entries.get(key) {
                if values.len() >= *max {
                    retry = retry.max(300 - now.duration_since(values[0]).as_secs());
                }
            }
        }
        if retry != 0 {
            return Err(Error::limited(retry));
        }
        for (key, _) in keys {
            entries.entry(key).or_default().push_back(now);
        }
        Ok(())
    }
}

/// Collection connections are admitted separately from metadata (default 16).
#[derive(Clone)]
pub struct CollectionCapacity(Arc<Semaphore>);
impl Default for CollectionCapacity {
    fn default() -> Self {
        Self(Arc::new(Semaphore::new(16)))
    }
}
impl CollectionCapacity {
    pub fn acquire(&self) -> Result<OwnedSemaphorePermit> {
        self.0
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::new("storage_unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_budget_bounds_overflow_and_release() {
        let budget = TempBudget::new(std::num::NonZeroU64::new(100).unwrap());
        let first = budget.reserve(70).unwrap();
        assert!(budget.reserve(31).is_err());
        assert!(budget.reserve(u64::MAX).is_err());
        let other = budget.reserve(30).unwrap();
        drop(first);
        assert_eq!(budget.used(), 30);
        drop(other);
        assert_eq!(budget.used(), 0);
        assert!(budget.reserve(100).is_ok());
    }
    #[test]
    fn budgets_expire_and_count_success_attempts() {
        let a = Attempts::default();
        let ip = "127.0.0.1".parse().unwrap();
        let now = Instant::now();
        let key = Key::Name([1; 32]);
        for _ in 0..10 {
            a.charge(ip, key.clone(), now).unwrap();
        }
        assert_eq!(
            a.charge(ip, key.clone(), now).unwrap_err().code,
            "rate_limited"
        );
        a.charge(ip, key, now + Duration::from_secs(300)).unwrap();
    }
    #[test]
    fn distinct_buckets_fail_closed() {
        let a = Attempts::default();
        let now = Instant::now();
        for n in 0..5000_u32 {
            a.charge(
                IpAddr::V4(n.into()),
                Key::Name(crypto::digest(&n.to_le_bytes())),
                now,
            )
            .unwrap();
        }
        assert_eq!(a.entries.lock().unwrap().len(), 10_000);
        assert!(
            a.charge("192.0.2.1".parse().unwrap(), Key::Capability([0; 32]), now)
                .is_err()
        );
        assert_eq!(a.entries.lock().unwrap().len(), 10_000);
    }
    #[tokio::test]
    async fn hash_queue_is_bounded_before_spawn() {
        let pool = Arc::new(HashPool::default());
        let (arrived_tx, mut arrived_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let p = pool.clone();
            let tx = arrived_tx.clone();
            let r = release.clone();
            tasks.push(tokio::spawn(async move {
                p.run(move || {
                    tx.send(()).unwrap();
                    let (m, c) = &*r;
                    let mut v = m.lock().unwrap();
                    while !*v {
                        v = c.wait(v).unwrap();
                    }
                    Ok(())
                })
                .await
            }));
        }
        arrived_rx.recv().await.unwrap();
        arrived_rx.recv().await.unwrap();
        // Wait for admission of all ten without timing assumptions.
        while pool.admitted.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pool.run(|| Ok(())).await.unwrap_err().code, "rate_limited");
        *release.0.lock().unwrap() = true;
        release.1.notify_all();
        for task in tasks {
            task.await.unwrap().unwrap();
        }
    }
}
