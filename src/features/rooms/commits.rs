//! Commits: the MLS changes only a room admin makes. Each one is checked against the room's
//! epoch, stored as the next room message, and fanned out to the other members.

use buccanet_protocol::api::Accepted;
use buccanet_protocol::events::{InboxBody, MessageKind};
use buccanet_protocol::framing::read_framing;
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::delivery::messages::MessageDoc;
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::policy::epoch_matches;
use crate::features::rooms::repo::{RoomAdvance, RoomDoc};
use crate::features::rooms::service::admin_room;
use crate::locks::KeyedGuard;
use crate::state::AppState;

/// Checks that `commit` is a commit of this room at its current epoch.
pub fn check_commit(room: &RoomDoc, commit: &[u8]) -> AppResult<()> {
    let framing = read_framing(commit).map_err(|error| AppError::invalid(error.to_string()))?;
    if framing.group_id != room.group_id || framing.kind != MessageKind::Commit {
        return Err(AppError::invalid("Expected a commit of this room."));
    }
    if !epoch_matches(room.epoch as u64, framing.epoch) {
        return Err(AppError::stale_epoch(room.epoch as u64));
    }
    Ok(())
}

/// Stores a commit as the next room message and moves the room to the next epoch.
pub async fn store_commit(state: &AppState, room: &RoomDoc, sender: &str, client_id: String, commit: Vec<u8>) -> AppResult<Accepted> {
    let seq = room.last_seq + 1;
    let message = MessageDoc {
        id: uuid::Uuid::new_v4().to_string(),
        room_id: room.id.clone(),
        seq,
        epoch: room.epoch,
        kind: MessageKind::Commit,
        sender: sender.to_string(),
        client_message_id: client_id,
        payload: commit,
        created_at: DateTime::now(),
    };
    state.messages.insert(&message).await?;
    state.rooms.advance(&room.id, RoomAdvance { epoch: room.epoch + 1, last_seq: seq, agent_run: room.agent_run }).await?;
    Ok(Accepted { seq: seq as u64, epoch: (room.epoch + 1) as u64 })
}

/// The admin's room, locked, read again once the lock is held.
pub async fn locked_admin_room(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<(KeyedGuard, RoomDoc)> {
    admin_room(state, room_id, device).await?;
    let guard = state.lock_room(room_id).await;
    Ok((guard, admin_room(state, room_id, device).await?))
}

/// Sends a stored commit to `recipients`, every member but its sender that stays in the room.
pub async fn deliver_commit(
    state: &AppState,
    room: &RoomDoc,
    sender: &DeviceDoc,
    accepted: &Accepted,
    commit: Vec<u8>,
    recipients: &[String],
) -> AppResult<()> {
    let message = InboxBody::Message {
        room_id: room.id.clone(),
        seq: accepted.seq,
        epoch: accepted.epoch - 1,
        kind: MessageKind::Commit,
        sender_device_id: sender.id.clone(),
        sender_kind: sender.kind,
        mls_message: commit,
    };
    state.inbox.deliver(recipients, &message).await
}
