//! One ordered inbox per device. Cursors are allocated and written under one lock, so a
//! reader never sees cursor n + 1 before n.

use std::sync::Arc;
use std::time::Duration;

use buccanet_protocol::events::{InboxBody, InboxEvent};
use futures::TryStreamExt;
use mongodb::bson::{DateTime, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::AppResult;
use crate::features::delivery::hub::Hub;
use crate::features::devices::repo::DevicesRepo;

pub const PAGE_SIZE: i64 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InboxDoc {
    device_id: String,
    cursor: i64,
    body_json: String,
    created_at: DateTime,
}

#[derive(Clone)]
pub struct InboxRepo {
    col: Collection<InboxDoc>,
    ttl: Duration,
    write_lock: Arc<Mutex<()>>,
}

impl InboxRepo {
    pub fn new(db: &Database, ttl: Duration) -> Self {
        Self { col: db.collection("inbox"), ttl, write_lock: Arc::new(Mutex::new(())) }
    }

    /// Appends one event to each recipient's inbox and wakes their streams.
    pub async fn deliver(&self, devices: &DevicesRepo, hub: &Hub, recipients: &[String], body: &InboxBody) -> AppResult<()> {
        let body_json = serde_json::to_string(body).expect("inbox bodies always serialize");
        let _guard = self.write_lock.lock().await;
        for device_id in recipients {
            let Some(cursor) = devices.next_inbox_cursor(device_id).await? else { continue };
            let entry = InboxDoc { device_id: device_id.clone(), cursor, body_json: body_json.clone(), created_at: DateTime::now() };
            self.col.insert_one(entry).await?;
        }
        drop(_guard);
        for device_id in recipients {
            hub.wake(device_id);
        }
        Ok(())
    }

    pub async fn after(&self, device_id: &str, cursor: i64) -> AppResult<Vec<InboxEvent>> {
        let docs: Vec<InboxDoc> = self
            .col
            .find(doc! { "device_id": device_id, "cursor": { "$gt": cursor } })
            .sort(doc! { "cursor": 1 })
            .limit(PAGE_SIZE)
            .await?
            .try_collect()
            .await?;
        Ok(docs
            .into_iter()
            .filter_map(|entry| {
                let body: InboxBody = serde_json::from_str(&entry.body_json).ok()?;
                Some(InboxEvent { cursor: entry.cursor as u64, body })
            })
            .collect())
    }

    pub async fn ensure_indexes(&self) -> mongodb::error::Result<()> {
        let unique = IndexOptions::builder().unique(true).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "device_id": 1, "cursor": 1 }).options(unique).build()).await?;
        let ttl = IndexOptions::builder().expire_after(self.ttl).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "created_at": 1 }).options(ttl).build()).await?;
        Ok(())
    }
}
