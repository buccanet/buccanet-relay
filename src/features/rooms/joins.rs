//! Joining and leaving: requests, the approve step (Add commit + Welcome) and removals.

use axum::http::StatusCode;
use buccanet_protocol::api::{Accepted, Approve, JoinRequested, PendingRequest, RemoveMember, codes};
use buccanet_protocol::events::{InboxBody, MessageKind};
use buccanet_protocol::framing::{is_welcome, key_package_owner, read_framing};
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::delivery::messages::MessageDoc;
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::policy::{epoch_matches, has_room_for_one_more};
use crate::features::rooms::repo::{MemberDoc, RequestDoc, RequestStatus, RoomAdvance, RoomDoc};
use crate::features::rooms::service::{admin_room, live_invite};
use crate::state::AppState;

pub async fn request(state: &AppState, token: &str, device: &DeviceDoc, key_package: Vec<u8>) -> AppResult<JoinRequested> {
    let invite = live_invite(state, token).await?;
    let room = state.rooms.by_id(&invite.room_id).await?.ok_or_else(|| AppError::not_found("This room"))?;
    if room.member(&device.id).is_some() {
        return Err(AppError::new(StatusCode::CONFLICT, codes::ALREADY_MEMBER, "This device is already in the room."));
    }
    let owner = key_package_owner(&key_package).map_err(|error| AppError::invalid(error.to_string()))?;
    if owner.identity != device.id || owner.signature_key != device.public_key {
        return Err(AppError::invalid("The key package must belong to the requesting device."));
    }
    let request_id = match state.rooms.pending_request(&room.id, &device.id).await? {
        Some(existing) => {
            state.rooms.replace_request_key(&existing.id, &key_package).await?;
            existing.id
        }
        None => insert_request(state, &room.id, &device.id, &key_package).await?,
    };
    let event = InboxBody::JoinRequested {
        room_id: room.id.clone(),
        request_id: request_id.clone(),
        device_id: device.id.clone(),
        name: device.name.clone(),
        kind: device.kind,
        key_package,
    };
    state.inbox.deliver(&state.devices, &state.hub, &room.admin_ids(), &event).await?;
    Ok(JoinRequested { request_id, room_id: room.id })
}

async fn insert_request(state: &AppState, room_id: &str, device_id: &str, key_package: &[u8]) -> AppResult<String> {
    let request = RequestDoc {
        id: uuid::Uuid::new_v4().to_string(),
        room_id: room_id.to_string(),
        device_id: device_id.to_string(),
        key_package: key_package.to_vec(),
        status: RequestStatus::Pending,
        created_at: DateTime::now(),
    };
    state.rooms.insert_request(&request).await?;
    Ok(request.id)
}

pub async fn pending(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<Vec<PendingRequest>> {
    admin_room(state, room_id, device).await?;
    let requests = state.rooms.pending_requests(room_id).await?;
    let ids: Vec<String> = requests.iter().map(|request| request.device_id.clone()).collect();
    let devices = state.devices.by_ids(&ids).await?;
    Ok(requests
        .into_iter()
        .filter_map(|request| {
            let requester = devices.iter().find(|candidate| candidate.id == request.device_id)?;
            Some(PendingRequest {
                request_id: request.id,
                device_id: requester.id.clone(),
                name: requester.name.clone(),
                kind: requester.kind,
                key_package: request.key_package,
            })
        })
        .collect())
}

/// Checks that `commit` is a commit of this room at its current epoch.
fn check_commit(room: &RoomDoc, commit: &[u8]) -> AppResult<()> {
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
async fn store_commit(state: &AppState, room: &RoomDoc, sender: &str, client_id: String, commit: Vec<u8>) -> AppResult<Accepted> {
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

pub async fn approve(state: &AppState, room_id: &str, request_id: &str, device: &DeviceDoc, input: Approve) -> AppResult<Accepted> {
    let lock = state.room_lock(room_id);
    let _guard = lock.lock().await;
    let room = admin_room(state, room_id, device).await?;
    let request = state.rooms.request_by_id(room_id, request_id).await?.ok_or_else(|| AppError::not_found("This request"))?;
    if request.status != RequestStatus::Pending {
        return Err(AppError::invalid("This request was already decided."));
    }
    if !has_room_for_one_more(room.members.len(), &room.policy) {
        return Err(AppError::new(StatusCode::CONFLICT, codes::ROOM_FULL, "The room reached its member limit."));
    }
    check_commit(&room, &input.commit)?;
    if !is_welcome(&input.welcome) {
        return Err(AppError::invalid("Expected an MLS Welcome."));
    }
    let accepted = store_commit(state, &room, &device.id, format!("approve:{request_id}"), input.commit.clone()).await?;
    let member = MemberDoc { device_id: request.device_id.clone(), admin: false, joined_at: DateTime::now() };
    state.rooms.add_member(room_id, &member).await?;
    state.rooms.set_request_status(request_id, RequestStatus::Approved).await?;
    announce_join(state, &room, device, &request, input, &accepted).await?;
    Ok(accepted)
}

async fn announce_join(
    state: &AppState,
    room: &RoomDoc,
    approver: &DeviceDoc,
    request: &RequestDoc,
    input: Approve,
    accepted: &Accepted,
) -> AppResult<()> {
    let commit = InboxBody::Message {
        room_id: room.id.clone(),
        seq: accepted.seq,
        epoch: accepted.epoch - 1,
        kind: MessageKind::Commit,
        sender_device_id: approver.id.clone(),
        sender_kind: approver.kind,
        mls_message: input.commit,
    };
    state.inbox.deliver(&state.devices, &state.hub, &room.member_ids_except(&approver.id), &commit).await?;
    let welcome = InboxBody::Welcome { room_id: room.id.clone(), policy: room.policy, start_seq: accepted.seq + 1, welcome: input.welcome };
    state.inbox.deliver(&state.devices, &state.hub, std::slice::from_ref(&request.device_id), &welcome).await?;
    let requester = state.devices.by_id(&request.device_id).await?.ok_or_else(|| AppError::not_found("This device"))?;
    let joined = InboxBody::MemberJoined { room_id: room.id.clone(), device_id: requester.id.clone(), name: requester.name, kind: requester.kind };
    let mut everyone = room.member_ids_except("");
    everyone.push(requester.id);
    state.inbox.deliver(&state.devices, &state.hub, &everyone, &joined).await
}

pub async fn reject(state: &AppState, room_id: &str, request_id: &str, device: &DeviceDoc) -> AppResult<()> {
    admin_room(state, room_id, device).await?;
    let request = state.rooms.request_by_id(room_id, request_id).await?.ok_or_else(|| AppError::not_found("This request"))?;
    if request.status != RequestStatus::Pending {
        return Err(AppError::invalid("This request was already decided."));
    }
    state.rooms.set_request_status(request_id, RequestStatus::Rejected).await?;
    let event = InboxBody::JoinRejected { room_id: room_id.to_string(), request_id: request_id.to_string() };
    state.inbox.deliver(&state.devices, &state.hub, std::slice::from_ref(&request.device_id), &event).await
}

pub async fn remove(state: &AppState, room_id: &str, target: &str, device: &DeviceDoc, input: RemoveMember) -> AppResult<Accepted> {
    let lock = state.room_lock(room_id);
    let _guard = lock.lock().await;
    let room = admin_room(state, room_id, device).await?;
    if room.member(target).is_none() || target == device.id {
        return Err(AppError::invalid("Pick another member of this room."));
    }
    check_commit(&room, &input.commit)?;
    let accepted = store_commit(state, &room, &device.id, format!("remove:{target}:{}", room.epoch), input.commit.clone()).await?;
    state.rooms.remove_member(room_id, target).await?;
    let commit = InboxBody::Message {
        room_id: room.id.clone(),
        seq: accepted.seq,
        epoch: accepted.epoch - 1,
        kind: MessageKind::Commit,
        sender_device_id: device.id.clone(),
        sender_kind: device.kind,
        mls_message: input.commit,
    };
    let remaining: Vec<String> = room.member_ids_except(&device.id).into_iter().filter(|id| id != target).collect();
    state.inbox.deliver(&state.devices, &state.hub, &remaining, &commit).await?;
    let removed = InboxBody::MemberRemoved { room_id: room.id.clone(), device_id: target.to_string() };
    state.inbox.deliver(&state.devices, &state.hub, &room.member_ids_except(""), &removed).await?;
    Ok(accepted)
}
