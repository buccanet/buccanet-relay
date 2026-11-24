//! The sealed room log. Kept for the TTL so a device that was offline can still catch up
//! through its inbox; the relay never decrypts any of it.

use std::collections::HashMap;
use std::time::Duration;

use futures::TryStreamExt;

use buccanet_protocol::events::MessageKind;
use mongodb::bson::{DateTime, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDoc {
    #[serde(rename = "_id")]
    pub id: String,
    pub room_id: String,
    pub seq: i64,
    pub epoch: i64,
    pub kind: MessageKind,
    pub sender: String,
    pub client_message_id: String,
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
    pub created_at: DateTime,
}

#[derive(Clone)]
pub struct MessagesRepo {
    col: Collection<MessageDoc>,
    ttl: Duration,
}

impl MessagesRepo {
    pub fn new(db: &Database, ttl: Duration) -> Self {
        Self { col: db.collection("messages"), ttl }
    }

    pub async fn insert(&self, message: &MessageDoc) -> mongodb::error::Result<()> {
        self.col.insert_one(message).await.map(|_| ())
    }

    pub async fn by_client_id(&self, room_id: &str, sender: &str, client_message_id: &str) -> mongodb::error::Result<Option<MessageDoc>> {
        self.col.find_one(doc! { "room_id": room_id, "sender": sender, "client_message_id": client_message_id }).await
    }

    /// The sealed bytes of some messages of a room, by seq.
    pub async fn payloads(&self, room_id: &str, seqs: &[i64]) -> mongodb::error::Result<HashMap<i64, Vec<u8>>> {
        let messages: Vec<MessageDoc> = self.col.find(doc! { "room_id": room_id, "seq": { "$in": seqs } }).await?.try_collect().await?;
        Ok(messages.into_iter().map(|message| (message.seq, message.payload)).collect())
    }

    pub async fn ensure_indexes(&self) -> mongodb::error::Result<()> {
        let unique = || IndexOptions::builder().unique(true).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "room_id": 1, "seq": 1 }).options(unique()).build()).await?;
        self.col
            .create_index(IndexModel::builder().keys(doc! { "room_id": 1, "sender": 1, "client_message_id": 1 }).options(unique()).build())
            .await?;
        let ttl = IndexOptions::builder().expire_after(self.ttl).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "created_at": 1 }).options(ttl).build()).await?;
        Ok(())
    }
}
