//! Fixed window rate limits kept in memory: the relay runs as one process.

use std::time::{Duration, Instant};

use dashmap::DashMap;

pub struct RateLimiter {
    limit: u64,
    window: Duration,
    windows: DashMap<String, (Instant, u64)>,
}

impl RateLimiter {
    pub fn new(limit: u64, window: Duration) -> Self {
        Self { limit, window, windows: DashMap::new() }
    }

    /// Counts one hit for `key`; false once the window is full.
    pub fn allow(&self, key: &str) -> bool {
        self.allow_weighted(key, 1)
    }

    /// Counts `weight` units (bytes, for a byte budget) for `key`; false once the window is full.
    pub fn allow_weighted(&self, key: &str, weight: u64) -> bool {
        let now = Instant::now();
        let mut entry = self.windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 += weight;
        entry.1 <= self.limit
    }

    /// Forgets windows that ended, so keys seen once do not stay in memory.
    pub fn sweep(&self) {
        let now = Instant::now();
        self.windows.retain(|_, (start, _)| now.duration_since(*start) < self.window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_weights_and_sweeps_ended_windows() {
        let limiter = RateLimiter::new(10, Duration::from_millis(20));
        assert!(limiter.allow_weighted("a", 6));
        assert!(!limiter.allow_weighted("a", 6));
        std::thread::sleep(Duration::from_millis(25));
        limiter.sweep();
        assert!(limiter.windows.is_empty());
        assert!(limiter.allow("a"));
    }
}
