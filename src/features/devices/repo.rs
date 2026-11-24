use buccanet_protocol::api::DeviceKind;
use mongodb::bson::{DateTime, doc};
use mongodb::options::{IndexOptions, ReturnDocument};
use mongodb::{Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDoc {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(with = "serde_bytes")]
    pub public_key: Vec<u8>,
    pub kind: DeviceKind,
    pub name: String,
    pub token_hash: String,
    /// Last cursor handed out in this device's inbox.
    pub inbox_seq: i64,
    pub created_at: DateTime,
}

#[derive(Clone)]
pub struct DevicesRepo {
    col: Collection<DeviceDoc>,
}

impl DevicesRepo {
    pub fn new(db: &Database) -> Self {
        Self { col: db.collection("devices") }
    }

    pub async fn insert(&self, device: &DeviceDoc) -> mongodb::error::Result<()> {
        self.col.insert_one(device).await.map(|_| ())
    }

    pub async fn by_id(&self, id: &str) -> mongodb::error::Result<Option<DeviceDoc>> {
        self.col.find_one(doc! { "_id": id }).await
    }

    pub async fn by_ids(&self, ids: &[String]) -> mongodb::error::Result<Vec<DeviceDoc>> {
        use futures::TryStreamExt;
        self.col.find(doc! { "_id": { "$in": ids } }).await?.try_collect().await
    }

    pub async fn by_token_hash(&self, token_hash: &str) -> mongodb::error::Result<Option<DeviceDoc>> {
        self.col.find_one(doc! { "token_hash": token_hash }).await
    }

    /// Reserves the next inbox cursor of a device.
    pub async fn next_inbox_cursor(&self, id: &str) -> mongodb::error::Result<Option<i64>> {
        let updated = self
            .col
            .find_one_and_update(doc! { "_id": id }, doc! { "$inc": { "inbox_seq": 1 } })
            .return_document(ReturnDocument::After)
            .await?;
        Ok(updated.map(|device| device.inbox_seq))
    }

    pub async fn ensure_indexes(&self) -> mongodb::error::Result<()> {
        let unique = IndexOptions::builder().unique(true).build();
        self.col.create_index(IndexModel::builder().keys(doc! { "token_hash": 1 }).options(unique).build()).await?;
        Ok(())
    }
}
