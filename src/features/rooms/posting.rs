//! Application messages: sealed text from a member, fanned out to every other member.

use axum::http::StatusCode;
use buccanet_protocol::api::{Accepted, PostMessage, codes};
use buccanet_protocol::events::{InboxBody, MessageKind};
use buccanet_protocol::framing::read_framing;
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::delivery::messages::MessageDoc;
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::policy::{Burst, agent_burst, epoch_matches};
use crate::features::rooms::repo::{RoomAdvance, RoomDoc};
use crate::features::rooms::service::member_room;
use crate::state::AppState;

const MAX_CLIENT_ID_LEN: usize = 100;
/// Largest sealed message: a history share of 64 KB with its MLS framing fits.
const MAX_MESSAGE_BYTES: usize = 100 * 1024;

/// Checks the request alone, before the room is read.
fn check_input(input: &PostMessage) -> AppResult<()> {
    if input.client_message_id.is_empty() || input.client_message_id.len() > MAX_CLIENT_ID_LEN {
        return Err(AppError::invalid("clientMessageId must be 1 to 100 characters."));
    }
    if input.mls_message.len() > MAX_MESSAGE_BYTES {
        return Err(AppError::invalid("This message is too large."));
    }
    Ok(())
}

/// Counts the message against the room's pace and the device's byte budget.
fn check_pace(state: &AppState, room_id: &str, device: &DeviceDoc, bytes: usize) -> AppResult<()> {
    if !state.limits.room.allow(room_id) {
        return Err(AppError::rate_limited("This room is receiving too many messages. Slow down a little."));
    }
    if !state.limits.upload.allow_weighted(&device.id, bytes as u64) {
        return Err(AppError::rate_limited("This device sent too much data this hour."));
    }
    Ok(())
}

/// A device numbers the messages it seals; a number it already used comes from a stale copy of
/// its state, whose keys every member has already consumed, so nobody could read the message.
fn check_device_seq(room: &RoomDoc, device_id: &str, device_seq: Option<u64>) -> AppResult<()> {
    let (Some(sent), Some(member)) = (device_seq, room.member(device_id)) else { return Ok(()) };
    if sent as i64 > member.sealed {
        return Ok(());
    }
    Err(AppError::new(
        StatusCode::CONFLICT,
        codes::STALE_STATE,
        "This copy of the device is out of date: another copy already sent from it. Use the device's current state.",
    ))
}

pub async fn post(state: &AppState, room_id: &str, device: &DeviceDoc, input: PostMessage) -> AppResult<Accepted> {
    check_input(&input)?;
    member_room(state, room_id, device).await?;
    check_pace(state, room_id, device, input.mls_message.len())?;
    let _guard = state.lock_room(room_id).await;
    let room = member_room(state, room_id, device).await?;
    if let Some(existing) = state.messages.by_client_id(room_id, &device.id, &input.client_message_id).await? {
        return Ok(Accepted { seq: existing.seq as u64, epoch: existing.epoch as u64 });
    }
    let framing = read_framing(&input.mls_message).map_err(|error| AppError::invalid(error.to_string()))?;
    if framing.group_id != room.group_id {
        return Err(AppError::invalid("This message belongs to another group."));
    }
    if framing.kind == MessageKind::Commit {
        return Err(AppError::invalid("Commits go through the approve and remove routes."));
    }
    if !epoch_matches(room.epoch as u64, framing.epoch) {
        return Err(AppError::stale_epoch(room.epoch as u64));
    }
    check_device_seq(&room, &device.id, input.device_seq)?;
    let run = match agent_burst(device.kind, framing.kind, room.agent_run as u32, &room.policy) {
        Burst::Accept(run) => run,
        Burst::Exceeded => {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                codes::AGENT_BURST,
                "The agents reached the room's limit of messages in a row. A human has to speak first.",
            ));
        }
    };
    let seq = room.last_seq + 1;
    let message = MessageDoc {
        id: uuid::Uuid::new_v4().to_string(),
        room_id: room.id.clone(),
        seq,
        epoch: room.epoch,
        kind: framing.kind,
        sender: device.id.clone(),
        client_message_id: input.client_message_id,
        payload: input.mls_message.clone(),
        created_at: DateTime::now(),
    };
    state.messages.insert(&message).await?;
    state.rooms.advance(&room.id, RoomAdvance { epoch: room.epoch, last_seq: seq, agent_run: run as i64 }).await?;
    if let Some(sealed) = input.device_seq {
        state.rooms.set_member_sealed(&room.id, &device.id, sealed as i64).await?;
    }
    let event = InboxBody::Message {
        room_id: room.id.clone(),
        seq: seq as u64,
        epoch: room.epoch as u64,
        kind: framing.kind,
        sender_device_id: device.id.clone(),
        sender_kind: device.kind,
        mls_message: input.mls_message,
    };
    state.inbox.deliver(&room.member_ids_except(&device.id), &event).await?;
    Ok(Accepted { seq: seq as u64, epoch: room.epoch as u64 })
}
