//! Rooms, membership views and invites.

use buccanet_protocol::api::{CreateRoom, InviteCreated, InvitePreview, Member, RoomView, codes};
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::policy::validate_policy;
use crate::features::rooms::repo::{InviteDoc, MemberDoc, RoomDoc};
use crate::state::AppState;
use crate::tokens::{hash_token, new_token};

const INVITE_TTL_MS: i64 = 7 * 24 * 3600 * 1000;

/// A room the device belongs to; non members get the same 404 as a missing room.
pub async fn member_room(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<RoomDoc> {
    let room = state.rooms.by_id(room_id).await?.ok_or_else(|| AppError::not_found("This room"))?;
    if room.member(&device.id).is_none() {
        return Err(AppError::not_found("This room"));
    }
    Ok(room)
}

pub async fn admin_room(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<RoomDoc> {
    let room = member_room(state, room_id, device).await?;
    if !room.member(&device.id).is_some_and(|member| member.admin) {
        return Err(AppError::forbidden("Only a room admin can do this."));
    }
    Ok(room)
}

pub async fn view(state: &AppState, room: &RoomDoc) -> AppResult<RoomView> {
    let ids: Vec<String> = room.members.iter().map(|member| member.device_id.clone()).collect();
    let devices = state.devices.by_ids(&ids).await?;
    let members = room
        .members
        .iter()
        .filter_map(|member| {
            let device = devices.iter().find(|device| device.id == member.device_id)?;
            Some(Member { device_id: device.id.clone(), name: device.name.clone(), kind: device.kind, admin: member.admin })
        })
        .collect();
    Ok(RoomView {
        room_id: room.id.clone(),
        group_id: room.group_id.clone(),
        policy: room.policy,
        epoch: room.epoch as u64,
        last_seq: room.last_seq as u64,
        members,
    })
}

pub async fn create(state: &AppState, device: &DeviceDoc, input: CreateRoom) -> AppResult<RoomView> {
    validate_policy(&input.policy).map_err(AppError::invalid)?;
    if input.group_id.is_empty() || input.group_id.len() > 64 {
        return Err(AppError::invalid("groupId must be 1 to 64 bytes."));
    }
    let room = RoomDoc {
        id: uuid::Uuid::new_v4().to_string(),
        group_id: input.group_id,
        policy: input.policy,
        epoch: 0,
        last_seq: 0,
        agent_run: 0,
        members: vec![MemberDoc { device_id: device.id.clone(), admin: true, joined_at: DateTime::now() }],
        created_at: DateTime::now(),
    };
    state.rooms.insert(&room).await.map_err(|error| match error.kind.as_ref() {
        mongodb::error::ErrorKind::Write(_) => AppError::invalid("A room already uses this groupId."),
        _ => error.into(),
    })?;
    view(state, &room).await
}

pub async fn list(state: &AppState, device: &DeviceDoc) -> AppResult<Vec<RoomView>> {
    let rooms = state.rooms.for_member(&device.id).await?;
    let mut views = Vec::with_capacity(rooms.len());
    for room in &rooms {
        views.push(view(state, room).await?);
    }
    Ok(views)
}

pub async fn create_invite(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<InviteCreated> {
    member_room(state, room_id, device).await?;
    let token = new_token();
    let expires_at = DateTime::from_millis(DateTime::now().timestamp_millis() + INVITE_TTL_MS);
    let invite = InviteDoc {
        id: uuid::Uuid::new_v4().to_string(),
        room_id: room_id.to_string(),
        token_hash: hash_token(&token),
        created_by: device.id.clone(),
        expires_at,
        revoked: false,
    };
    state.rooms.insert_invite(&invite).await?;
    let url = format!("{}/join/{token}", state.public_origin);
    Ok(InviteCreated { invite_id: invite.id, token, url, expires_at: iso(expires_at) })
}

pub async fn revoke_invite(state: &AppState, room_id: &str, invite_id: &str, device: &DeviceDoc) -> AppResult<()> {
    admin_room(state, room_id, device).await?;
    if !state.rooms.revoke_invite(room_id, invite_id).await? {
        return Err(AppError::not_found("This invite"));
    }
    Ok(())
}

/// A live invite and its room: not revoked, not expired, and made by someone still in the room.
pub async fn live_invite(state: &AppState, token: &str) -> AppResult<(InviteDoc, RoomDoc)> {
    let invite = state.rooms.invite_by_token_hash(&hash_token(token)).await?.ok_or_else(|| AppError::not_found("This invite"))?;
    let room = state.rooms.by_id(&invite.room_id).await?.ok_or_else(|| AppError::not_found("This room"))?;
    if invite.revoked || invite.expires_at < DateTime::now() || room.member(&invite.created_by).is_none() {
        return Err(AppError::new(axum::http::StatusCode::GONE, codes::INVITE_EXPIRED, "This invite link expired or was revoked."));
    }
    Ok((invite, room))
}

pub async fn preview(state: &AppState, token: &str) -> AppResult<InvitePreview> {
    let (invite, room) = live_invite(state, token).await?;
    Ok(InvitePreview {
        room_id: room.id,
        member_count: room.members.len() as u32,
        approval: room.policy.approval,
        expires_at: iso(invite.expires_at),
    })
}

fn iso(date: DateTime) -> String {
    date.try_to_rfc3339_string().expect("relay dates are always in range")
}
