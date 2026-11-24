//! Wakes the open streams of a device when its inbox grows.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct Hub {
    devices: Arc<DashMap<String, Arc<Notify>>>,
}

impl Hub {
    pub fn handle(&self, device_id: &str) -> Arc<Notify> {
        self.devices.entry(device_id.to_string()).or_insert_with(|| Arc::new(Notify::new())).clone()
    }

    pub fn wake(&self, device_id: &str) {
        if let Some(notify) = self.devices.get(device_id) {
            notify.notify_waiters();
        }
    }
}
