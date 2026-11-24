//! One ordered inbox per device. A device's cursors are allocated and written while that
//! device is locked, so a reader never sees cursor n + 1 before n. A room message is stored
//! once in the room log: inbox entries point at it by room and seq instead of copying it.

use std::collections::BTreeMap;
use std::time::Duration;

use buccanet_protocol::events::{InboxBody, InboxEvent};
use futures::TryStreamExt;
use mongodb::bson::{DateTime, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::features::delivery::hub::Hub;
use crate::features::delivery::messages::MessagesRepo;
use crate::features::devices::repo::DevicesRepo;
use crate::locks::KeyedLocks;

pub const PAGE_SIZE: i64 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InboxDoc {
    device_id: String,
    cursor: i64,
    /// The event, with the sealed bytes of a room message left out: they live in the room log.
    body_json: String,
    created_at: DateTime,
}

#[derive(Clone)]
pub struct InboxRepo {
    col: Collection<InboxDoc>,
    ttl: Duration,
    devices: DevicesRepo,
    messages: MessagesRepo,
    hub: Hub,
    device_locks: KeyedLocks,
}

/// The event as stored: room messages without their sealed bytes.
fn stored_body(body: &InboxBody) -> InboxBody {
    match body {
        InboxBody::Message { room_id, seq, epoch, kind, sender_device_id, sender_kind, .. } => InboxBody::Message {
            room_id: room_id.clone(),
            seq: *seq,
            epoch: *epoch,
            kind: *kind,
            sender_device_id: sender_device_id.clone(),
            sender_kind: *sender_kind,
            mls_message: Vec::new(),
        },
        other => other.clone(),
    }
}

impl InboxRepo {
    pub fn new(db: &Database, ttl: Duration, devices: DevicesRepo, messages: MessagesRepo, hub: Hub) -> Self {
        Self { col: db.collection("inbox"), ttl, devices, messages, hub, device_locks: KeyedLocks::default() }
    }

    /// Appends one event to each recipient's inbox and wakes their streams.
    pub async fn deliver(&self, recipients: &[String], body: &InboxBody) -> AppResult<()> {
        let body_json = serde_json::to_string(&stored_body(body)).expect("inbox bodies always serialize");
        let guards = self.device_locks.lock_all(recipients).await;
        let mut entries = Vec::with_capacity(recipients.len());
        for device_id in recipients {
            let Some(cursor) = self.devices.next_inbox_cursor(device_id).await? else { continue };
            entries.push(InboxDoc { device_id: device_id.clone(), cursor, body_json: body_json.clone(), created_at: DateTime::now() });
        }
        if !entries.is_empty() {
            self.col.insert_many(entries).await?;
        }
        drop(guards);
        for device_id in recipients {
            self.hub.wake(device_id);
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
        let mut events: Vec<InboxEvent> = docs
            .into_iter()
            .filter_map(|entry| {
                let body: InboxBody = serde_json::from_str(&entry.body_json).ok()?;
                Some(InboxEvent { cursor: entry.cursor as u64, body })
            })
            .collect();
        self.attach_payloads(&mut events).await?;
        Ok(events)
    }

    /// Puts the sealed bytes back into room messages, read from the room log in one query per room.
    async fn attach_payloads(&self, events: &mut [InboxEvent]) -> AppResult<()> {
        let mut wanted: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for event in events.iter() {
            if let InboxBody::Message { room_id, seq, .. } = &event.body {
                wanted.entry(room_id.clone()).or_default().push(*seq as i64);
            }
        }
        for (room_id, seqs) in wanted {
            let payloads = self.messages.payloads(&room_id, &seqs).await?;
            for event in events.iter_mut() {
                if let InboxBody::Message { room_id: id, seq, mls_message, .. } = &mut event.body
                    && *id == room_id
                    && let Some(payload) = payloads.get(&(*seq as i64))
                {
                    mls_message.clone_from(payload);
                }
            }
        }
        Ok(())
    }

    pub async fn ensure_indexes(&self) -> mongodb::error::Result<()> {
        let unique = IndexOptions::builder().unique(true).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "device_id": 1, "cursor": 1 }).options(unique).build()).await?;
        let ttl = IndexOptions::builder().expire_after(self.ttl).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "created_at": 1 }).options(ttl).build()).await?;
        Ok(())
    }
}
