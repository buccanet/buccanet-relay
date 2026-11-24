//! Wakes the open streams of a device when its inbox grows, and bounds how many are open.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;
use tokio::sync::Notify;

/// Live streams one device may hold at once.
pub const STREAMS_PER_DEVICE: usize = 4;
/// Live streams the whole relay holds at once.
pub const STREAMS_TOTAL: usize = 20_000;

#[derive(Clone, Default)]
pub struct Hub {
    devices: Arc<DashMap<String, Arc<Notify>>>,
    open: Arc<DashMap<String, usize>>,
    total: Arc<AtomicUsize>,
}

/// A place for one live stream, given back when the stream ends.
pub struct StreamSlot {
    hub: Hub,
    device_id: String,
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        self.hub.total.fetch_sub(1, Ordering::SeqCst);
        self.hub.open.remove_if_mut(&self.device_id, |_, count| {
            *count -= 1;
            *count == 0
        });
    }
}

impl Hub {
    /// A slot for a new stream of this device, unless it or the relay holds too many.
    pub fn open_stream(&self, device_id: &str) -> Option<StreamSlot> {
        if self.total.load(Ordering::SeqCst) >= STREAMS_TOTAL {
            return None;
        }
        let mut count = self.open.entry(device_id.to_string()).or_insert(0);
        if *count >= STREAMS_PER_DEVICE {
            return None;
        }
        *count += 1;
        self.total.fetch_add(1, Ordering::SeqCst);
        Some(StreamSlot { hub: self.clone(), device_id: device_id.to_string() })
    }

    /// Forgets the wake handles no stream holds anymore.
    pub fn sweep(&self) {
        self.devices.retain(|_, notify| Arc::strong_count(notify) > 1);
    }

    pub fn handle(&self, device_id: &str) -> Arc<Notify> {
        self.devices.entry(device_id.to_string()).or_insert_with(|| Arc::new(Notify::new())).clone()
    }

    pub fn wake(&self, device_id: &str) {
        if let Some(notify) = self.devices.get(device_id) {
            notify.notify_waiters();
        }
    }
}
