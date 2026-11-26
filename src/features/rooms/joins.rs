//! Joining and leaving: requests, the approve step (Add commit + Welcome) and removals.

use axum::http::StatusCode;
use buccanet_protocol::api::{Accepted, Approve, JoinRequested, PendingRequest, RemoveMember, codes};
use buccanet_protocol::events::InboxBody;
use buccanet_protocol::framing::{is_welcome, key_package_owner};
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::commits::{check_commit, deliver_commit, locked_admin_room, store_commit};
use crate::features::rooms::policy::{has_room_for_one_more, pending_cap};
use crate::features::rooms::repo::{MemberDoc, RequestDoc, RequestStatus, RequestVia, RoomDoc};
use crate::features::rooms::service::{admin_room, live_invite, member_room};
use crate::state::AppState;

/// How long a device an admin rejected waits before it asks the same room again.
const REJECTED_WAIT_MS: i64 = 3_600_000;

/// A join request through an invite link.
pub async fn request(state: &AppState, token: &str, device: &DeviceDoc, key_package: Vec<u8>) -> AppResult<JoinRequested> {
    if !state.limits.join.allow(&device.id) {
        return Err(AppError::rate_limited("Too many join requests from this device."));
    }
    let (_, room) = live_invite(state, token).await?;
    request_into(state, Requested { room: &room, device, via: RequestVia::Invite }, key_package).await
}

/// Who asks to join which room, and how the request came.
pub struct Requested<'a> {
    pub room: &'a RoomDoc,
    pub device: &'a DeviceDoc,
    pub via: RequestVia,
}

/// Refuses a member, a key package that is not the requester's, and a device an admin of the
/// room rejected less than an hour ago.
async fn check_requester(state: &AppState, asked: &Requested<'_>, key_package: &[u8]) -> AppResult<()> {
    let (room, device) = (asked.room, asked.device);
    if room.member(&device.id).is_some() {
        return Err(AppError::new(StatusCode::CONFLICT, codes::ALREADY_MEMBER, "This device is already in the room."));
    }
    let owner = key_package_owner(key_package).map_err(|error| AppError::invalid(error.to_string()))?;
    if owner.identity != device.id || owner.signature_key != device.public_key {
        return Err(AppError::invalid("The key package must belong to the requesting device."));
    }
    let since = DateTime::from_millis(DateTime::now().timestamp_millis() - REJECTED_WAIT_MS);
    if state.rooms.rejected_since(&room.id, &device.id, since).await? {
        return Err(AppError::rate_limited("An admin of this room turned this device down less than an hour ago. Try again later."));
    }
    Ok(())
}

/// Records a join request into the room, or refreshes the one waiting, and tells the room admins.
pub async fn request_into(state: &AppState, asked: Requested<'_>, key_package: Vec<u8>) -> AppResult<JoinRequested> {
    check_requester(state, &asked, &key_package).await?;
    let (room, device) = (asked.room, asked.device);
    let request_id = match state.rooms.pending_request(&room.id, &device.id).await? {
        Some(existing) if existing.key_package == key_package => {
            return Ok(JoinRequested { request_id: existing.id, room_id: room.id.clone() });
        }
        Some(existing) => {
            state.rooms.replace_request_key(&existing.id, &key_package).await?;
            existing.id
        }
        None => {
            check_waiting_list(state, &asked).await?;
            insert_request(state, &asked, &key_package).await?
        }
    };
    let event = InboxBody::JoinRequested {
        room_id: room.id.clone(),
        request_id: request_id.clone(),
        device_id: device.id.clone(),
        name: device.name.clone(),
        kind: device.kind,
        key_package,
    };
    state.inbox.deliver(&room.admin_ids(), &event).await?;
    Ok(JoinRequested { request_id, room_id: room.id.clone() })
}

/// Refuses a new request once the room's waiting list is full. A listed room with no free seat
/// answers room_full.
async fn check_waiting_list(state: &AppState, asked: &Requested<'_>) -> AppResult<()> {
    let (room, via) = (asked.room, asked.via);
    let waiting = state.rooms.count_pending(&room.id, via).await?;
    if waiting < pending_cap(via, room.members.len(), &room.policy) {
        return Ok(());
    }
    if via == RequestVia::Listing && !has_room_for_one_more(room.members.len(), &room.policy) {
        return Err(AppError::new(StatusCode::CONFLICT, codes::ROOM_FULL, "The room reached its member limit."));
    }
    Err(AppError::rate_limited("This room has too many requests waiting. Try again later."))
}

async fn insert_request(state: &AppState, asked: &Requested<'_>, key_package: &[u8]) -> AppResult<String> {
    let request = RequestDoc {
        id: uuid::Uuid::new_v4().to_string(),
        room_id: asked.room.id.clone(),
        device_id: asked.device.id.clone(),
        key_package: key_package.to_vec(),
        status: RequestStatus::Pending,
        created_at: DateTime::now(),
        via: asked.via,
        decided_at: None,
    };
    state.rooms.insert_request(&request).await.map_err(|error| match error.kind.as_ref() {
        mongodb::error::ErrorKind::Write(_) => {
            AppError::new(StatusCode::CONFLICT, codes::INVALID_INPUT, "A request from this device is already waiting.")
        }
        _ => error.into(),
    })?;
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

/// A request of this room that nobody decided yet.
async fn pending_request(state: &AppState, room: &RoomDoc, request_id: &str) -> AppResult<RequestDoc> {
    let request = state.rooms.request_by_id(&room.id, request_id).await?.ok_or_else(|| AppError::not_found("This request"))?;
    if request.status != RequestStatus::Pending || room.member(&request.device_id).is_some() {
        return Err(AppError::invalid("This request was already decided."));
    }
    Ok(request)
}

pub async fn approve(state: &AppState, room_id: &str, request_id: &str, device: &DeviceDoc, input: Approve) -> AppResult<Accepted> {
    let (_guard, room) = locked_admin_room(state, room_id, device).await?;
    let request = pending_request(state, &room, request_id).await?;
    if !has_room_for_one_more(room.members.len(), &room.policy) {
        return Err(AppError::new(StatusCode::CONFLICT, codes::ROOM_FULL, "The room reached its member limit."));
    }
    check_commit(&room, &input.commit)?;
    if !is_welcome(&input.welcome) {
        return Err(AppError::invalid("Expected an MLS Welcome."));
    }
    if !state.rooms.decide_request(request_id, RequestStatus::Approved).await? {
        return Err(AppError::invalid("This request was already decided."));
    }
    let accepted = store_commit(state, &room, &device.id, format!("approve:{request_id}"), input.commit.clone()).await?;
    let member = MemberDoc { device_id: request.device_id.clone(), admin: false, joined_at: DateTime::now(), sealed: 0 };
    state.rooms.add_member(room_id, &member).await?;
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
    deliver_commit(state, room, approver, accepted, input.commit, &room.member_ids_except(&approver.id)).await?;
    let welcome = InboxBody::Welcome { room_id: room.id.clone(), policy: room.policy, start_seq: accepted.seq + 1, welcome: input.welcome };
    state.inbox.deliver(std::slice::from_ref(&request.device_id), &welcome).await?;
    let requester = state.devices.by_id(&request.device_id).await?.ok_or_else(|| AppError::not_found("This device"))?;
    let joined =
        InboxBody::MemberJoined { room_id: room.id.clone(), device_id: requester.id.clone(), name: requester.name, kind: requester.kind };
    let mut everyone = room.member_ids_except("");
    everyone.push(requester.id);
    state.inbox.deliver(&everyone, &joined).await
}

pub async fn reject(state: &AppState, room_id: &str, request_id: &str, device: &DeviceDoc) -> AppResult<()> {
    let (_guard, room) = locked_admin_room(state, room_id, device).await?;
    let request = pending_request(state, &room, request_id).await?;
    if !state.rooms.decide_request(request_id, RequestStatus::Rejected).await? {
        return Err(AppError::invalid("This request was already decided."));
    }
    let event = InboxBody::JoinRejected { room_id: room_id.to_string(), request_id: request_id.to_string() };
    state.inbox.deliver(std::slice::from_ref(&request.device_id), &event).await
}

pub async fn remove(state: &AppState, room_id: &str, target: &str, device: &DeviceDoc, input: RemoveMember) -> AppResult<Accepted> {
    let (_guard, room) = locked_admin_room(state, room_id, device).await?;
    if room.member(target).is_none() || target == device.id {
        return Err(AppError::invalid("Pick another member of this room."));
    }
    check_commit(&room, &input.commit)?;
    let accepted = store_commit(state, &room, &device.id, format!("remove:{target}:{}", room.epoch), input.commit.clone()).await?;
    state.rooms.remove_member(room_id, target).await?;
    let remaining: Vec<String> = room.member_ids_except(&device.id).into_iter().filter(|id| id != target).collect();
    deliver_commit(state, &room, device, &accepted, input.commit, &remaining).await?;
    let removed = InboxBody::MemberRemoved { room_id: room.id.clone(), device_id: target.to_string() };
    state.inbox.deliver(&room.member_ids_except(""), &removed).await?;
    Ok(accepted)
}

/// A member leaves: the room admins are told, and one of their devices commits the removal.
/// The last admin cannot leave: another member has to be made admin first.
pub async fn leave(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<()> {
    let room = member_room(state, room_id, device).await?;
    let admins = room.admin_ids();
    if admins == [device.id.clone()] {
        return Err(AppError::forbidden("The last admin of a room cannot leave it. Make another member admin first."));
    }
    let event = InboxBody::MemberLeaving { room_id: room.id.clone(), device_id: device.id.clone() };
    state.inbox.deliver(&admins, &event).await
}
