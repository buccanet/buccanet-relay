//! Locks keyed by id (a room, a device), created on first use and dropped with their last
//! holder, so ids that come and go never pile up in memory.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{Mutex, OwnedMutexGuard};

type Table = Arc<DashMap<String, Arc<Mutex<()>>>>;

#[derive(Clone, Default)]
pub struct KeyedLocks {
    table: Table,
}

pub struct KeyedGuard {
    table: Table,
    key: String,
    guard: Option<OwnedMutexGuard<()>>,
}

impl KeyedLocks {
    pub async fn lock(&self, key: &str) -> KeyedGuard {
        let mutex = self.table.entry(key.to_string()).or_default().clone();
        let guard = mutex.lock_owned().await;
        KeyedGuard { table: self.table.clone(), key: key.to_string(), guard: Some(guard) }
    }

    /// Locks every key, in sorted order so two callers never wait on each other.
    pub async fn lock_all(&self, keys: &[String]) -> Vec<KeyedGuard> {
        let mut sorted = keys.to_vec();
        sorted.sort();
        sorted.dedup();
        let mut guards = Vec::with_capacity(sorted.len());
        for key in &sorted {
            guards.push(self.lock(key).await);
        }
        guards
    }
}

impl Drop for KeyedGuard {
    fn drop(&mut self) {
        self.guard.take();
        self.table.remove_if(&self.key, |_, mutex| Arc::strong_count(mutex) == 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn entries_leave_with_their_last_holder() {
        let locks = KeyedLocks::default();
        let first = locks.lock("a").await;
        let waiting = tokio::spawn({
            let locks = locks.clone();
            async move { drop(locks.lock("a").await) }
        });
        tokio::task::yield_now().await;
        drop(first);
        waiting.await.unwrap();
        assert!(locks.table.is_empty());
        drop(locks.lock_all(&["b".into(), "a".into(), "b".into()]).await);
        assert!(locks.table.is_empty());
    }
}
