//! Fixed window rate limits kept in memory: the relay runs as one process.

use std::time::{Duration, Instant};

use dashmap::DashMap;

pub struct RateLimiter {
    limit: u32,
    window: Duration,
    windows: DashMap<String, (Instant, u32)>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self { limit, window, windows: DashMap::new() }
    }

    /// Counts one hit for `key`; false once the window is full.
    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut entry = self.windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.limit
    }
}
