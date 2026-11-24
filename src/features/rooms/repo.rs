use buccanet_protocol::api::RoomPolicy;
use futures::TryStreamExt;
use mongodb::bson::{DateTime, doc, to_bson};
use std::time::Duration;

use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};

/// Join requests, decided or not, are deleted after this long.
const REQUEST_TTL: Duration = Duration::from_secs(30 * 86_400);
/// An expired invite still answers "expired" for this long, then is deleted.
const EXPIRED_INVITE_KEPT: Duration = Duration::from_secs(7 * 86_400);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberDoc {
    pub device_id: String,
    pub admin: bool,
    pub joined_at: DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomDoc {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(with = "serde_bytes")]
    pub group_id: Vec<u8>,
    pub policy: RoomPolicy,
    pub epoch: i64,
    pub last_seq: i64,
    pub agent_run: i64,
    pub members: Vec<MemberDoc>,
    pub created_at: DateTime,
}

impl RoomDoc {
    pub fn member(&self, device_id: &str) -> Option<&MemberDoc> {
        self.members.iter().find(|member| member.device_id == device_id)
    }

    pub fn admin_ids(&self) -> Vec<String> {
        self.members.iter().filter(|member| member.admin).map(|member| member.device_id.clone()).collect()
    }

    pub fn member_ids_except(&self, device_id: &str) -> Vec<String> {
        self.members.iter().filter(|member| member.device_id != device_id).map(|member| member.device_id.clone()).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteDoc {
    #[serde(rename = "_id")]
    pub id: String,
    pub room_id: String,
    pub token_hash: String,
    pub created_by: String,
    pub expires_at: DateTime,
    pub revoked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDoc {
    #[serde(rename = "_id")]
    pub id: String,
    pub room_id: String,
    pub device_id: String,
    #[serde(with = "serde_bytes")]
    pub key_package: Vec<u8>,
    pub status: RequestStatus,
    pub created_at: DateTime,
}

/// What changes on a room when a message is accepted.
pub struct RoomAdvance {
    pub epoch: i64,
    pub last_seq: i64,
    pub agent_run: i64,
}

#[derive(Clone)]
pub struct RoomsRepo {
    rooms: Collection<RoomDoc>,
    invites: Collection<InviteDoc>,
    requests: Collection<RequestDoc>,
}

impl RoomsRepo {
    pub fn new(db: &Database) -> Self {
        Self { rooms: db.collection("rooms"), invites: db.collection("invites"), requests: db.collection("join_requests") }
    }

    pub async fn insert(&self, room: &RoomDoc) -> mongodb::error::Result<()> {
        self.rooms.insert_one(room).await.map(|_| ())
    }

    pub async fn by_id(&self, id: &str) -> mongodb::error::Result<Option<RoomDoc>> {
        self.rooms.find_one(doc! { "_id": id }).await
    }

    pub async fn for_member(&self, device_id: &str) -> mongodb::error::Result<Vec<RoomDoc>> {
        self.rooms.find(doc! { "members.device_id": device_id }).sort(doc! { "created_at": -1 }).await?.try_collect().await
    }

    pub async fn advance(&self, id: &str, change: RoomAdvance) -> mongodb::error::Result<()> {
        let set = doc! { "epoch": change.epoch, "last_seq": change.last_seq, "agent_run": change.agent_run };
        self.rooms.update_one(doc! { "_id": id }, doc! { "$set": set }).await.map(|_| ())
    }

    pub async fn add_member(&self, id: &str, member: &MemberDoc) -> mongodb::error::Result<()> {
        let member = to_bson(member).expect("members always serialize");
        self.rooms.update_one(doc! { "_id": id }, doc! { "$push": { "members": member } }).await.map(|_| ())
    }

    pub async fn remove_member(&self, id: &str, device_id: &str) -> mongodb::error::Result<()> {
        self.rooms.update_one(doc! { "_id": id }, doc! { "$pull": { "members": { "device_id": device_id } } }).await.map(|_| ())
    }

    pub async fn insert_invite(&self, invite: &InviteDoc) -> mongodb::error::Result<()> {
        self.invites.insert_one(invite).await.map(|_| ())
    }

    pub async fn invite_by_token_hash(&self, token_hash: &str) -> mongodb::error::Result<Option<InviteDoc>> {
        self.invites.find_one(doc! { "token_hash": token_hash }).await
    }

    pub async fn revoke_invite(&self, room_id: &str, invite_id: &str) -> mongodb::error::Result<bool> {
        let result = self.invites.update_one(doc! { "_id": invite_id, "room_id": room_id }, doc! { "$set": { "revoked": true } }).await?;
        Ok(result.matched_count == 1)
    }

    pub async fn pending_request(&self, room_id: &str, device_id: &str) -> mongodb::error::Result<Option<RequestDoc>> {
        self.requests.find_one(doc! { "room_id": room_id, "device_id": device_id, "status": "pending" }).await
    }

    pub async fn insert_request(&self, request: &RequestDoc) -> mongodb::error::Result<()> {
        self.requests.insert_one(request).await.map(|_| ())
    }

    pub async fn replace_request_key(&self, id: &str, key_package: &[u8]) -> mongodb::error::Result<()> {
        let bytes = mongodb::bson::Binary { subtype: mongodb::bson::spec::BinarySubtype::Generic, bytes: key_package.to_vec() };
        self.requests.update_one(doc! { "_id": id }, doc! { "$set": { "key_package": bytes } }).await.map(|_| ())
    }

    pub async fn request_by_id(&self, room_id: &str, id: &str) -> mongodb::error::Result<Option<RequestDoc>> {
        self.requests.find_one(doc! { "_id": id, "room_id": room_id }).await
    }

    pub async fn pending_requests(&self, room_id: &str) -> mongodb::error::Result<Vec<RequestDoc>> {
        self.requests.find(doc! { "room_id": room_id, "status": "pending" }).sort(doc! { "created_at": 1 }).await?.try_collect().await
    }

    pub async fn count_pending(&self, room_id: &str) -> mongodb::error::Result<u64> {
        self.requests.count_documents(doc! { "room_id": room_id, "status": "pending" }).await
    }

    /// Decides a request that is still pending; false when it was decided already.
    pub async fn decide_request(&self, id: &str, status: RequestStatus) -> mongodb::error::Result<bool> {
        let status = to_bson(&status).expect("status always serializes");
        let result = self.requests.update_one(doc! { "_id": id, "status": "pending" }, doc! { "$set": { "status": status } }).await?;
        Ok(result.modified_count == 1)
    }

    pub async fn ensure_indexes(&self) -> mongodb::error::Result<()> {
        let unique = || IndexOptions::builder().unique(true).build();
        self.rooms.create_index(IndexModel::builder().keys(doc! { "group_id": 1 }).options(unique()).build()).await?;
        self.rooms.create_index(IndexModel::builder().keys(doc! { "members.device_id": 1 }).build()).await?;
        self.invites.create_index(IndexModel::builder().keys(doc! { "token_hash": 1 }).options(unique()).build()).await?;
        self.requests.create_index(IndexModel::builder().keys(doc! { "room_id": 1, "status": 1 }).build()).await?;
        let one_pending = IndexOptions::builder().unique(true).partial_filter_expression(doc! { "status": "pending" }).build();
        let pending_key = doc! { "room_id": 1, "device_id": 1 };
        self.requests.create_index(IndexModel::builder().keys(pending_key).options(one_pending).build()).await?;
        let requests_ttl = IndexOptions::builder().expire_after(REQUEST_TTL).build();
        self.requests.create_index(IndexModel::builder().keys(doc! { "created_at": 1 }).options(requests_ttl).build()).await?;
        let invites_ttl = IndexOptions::builder().expire_after(EXPIRED_INVITE_KEPT).build();
        self.invites.create_index(IndexModel::builder().keys(doc! { "expires_at": 1 }).options(invites_ttl).build()).await?;
        Ok(())
    }
}
