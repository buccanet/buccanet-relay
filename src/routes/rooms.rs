use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use buccanet_protocol::api::{
    Accepted, Approve, CreateRoom, InviteCreated, InvitePreview, JoinRequested, PendingRequest, PostMessage, RemoveMember, RequestJoin,
    RoomView, SetAdmins,
};

use super::ApiJson;
use crate::auth::AuthDevice;
use crate::client_addr::ClientAddr;
use crate::error::{AppError, AppResult};
use crate::features::rooms::{admins, joins, posting, service};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/rooms", post(create).get(list))
        .route("/rooms/{room_id}", get(show))
        .route("/rooms/{room_id}/invites", post(create_invite))
        .route("/rooms/{room_id}/invites/{invite_id}/revoke", post(revoke_invite))
        .route("/rooms/{room_id}/requests", get(pending))
        .route("/rooms/{room_id}/requests/{request_id}/approve", post(approve))
        .route("/rooms/{room_id}/requests/{request_id}/reject", post(reject))
        .route("/rooms/{room_id}/messages", post(post_message))
        .route("/rooms/{room_id}/members/{device_id}/remove", post(remove))
        .route("/rooms/{room_id}/admins", post(set_admins))
        .route("/rooms/{room_id}/leave", post(leave))
        .route("/invites/{token}", get(preview))
        .route("/invites/{token}/requests", post(request_join))
}

async fn create(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    ApiJson(input): ApiJson<CreateRoom>,
) -> AppResult<(StatusCode, Json<RoomView>)> {
    Ok((StatusCode::CREATED, Json(service::create(&state, &device, input).await?)))
}

async fn list(State(state): State<AppState>, AuthDevice(device): AuthDevice) -> AppResult<Json<Vec<RoomView>>> {
    Ok(Json(service::list(&state, &device).await?))
}

async fn show(State(state): State<AppState>, AuthDevice(device): AuthDevice, Path(room_id): Path<String>) -> AppResult<Json<RoomView>> {
    let room = service::member_room(&state, &room_id, &device).await?;
    Ok(Json(service::view(&state, &room).await?))
}

async fn create_invite(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(room_id): Path<String>,
) -> AppResult<(StatusCode, Json<InviteCreated>)> {
    Ok((StatusCode::CREATED, Json(service::create_invite(&state, &room_id, &device).await?)))
}

async fn revoke_invite(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path((room_id, invite_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    service::revoke_invite(&state, &room_id, &invite_id, &device).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pending(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(room_id): Path<String>,
) -> AppResult<Json<Vec<PendingRequest>>> {
    Ok(Json(joins::pending(&state, &room_id, &device).await?))
}

async fn approve(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path((room_id, request_id)): Path<(String, String)>,
    ApiJson(input): ApiJson<Approve>,
) -> AppResult<Json<Accepted>> {
    Ok(Json(joins::approve(&state, &room_id, &request_id, &device, input).await?))
}

async fn reject(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path((room_id, request_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    joins::reject(&state, &room_id, &request_id, &device).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn post_message(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(room_id): Path<String>,
    ApiJson(input): ApiJson<PostMessage>,
) -> AppResult<(StatusCode, Json<Accepted>)> {
    Ok((StatusCode::CREATED, Json(posting::post(&state, &room_id, &device, input).await?)))
}

async fn remove(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path((room_id, target)): Path<(String, String)>,
    ApiJson(input): ApiJson<RemoveMember>,
) -> AppResult<Json<Accepted>> {
    Ok(Json(joins::remove(&state, &room_id, &target, &device, input).await?))
}

async fn set_admins(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(room_id): Path<String>,
    ApiJson(input): ApiJson<SetAdmins>,
) -> AppResult<Json<Accepted>> {
    Ok(Json(admins::set(&state, &room_id, &device, input).await?))
}

async fn leave(State(state): State<AppState>, AuthDevice(device): AuthDevice, Path(room_id): Path<String>) -> AppResult<StatusCode> {
    joins::leave(&state, &room_id, &device).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn preview(
    State(state): State<AppState>,
    ClientAddr(client): ClientAddr,
    Path(token): Path<String>,
) -> AppResult<Json<InvitePreview>> {
    if !state.limits.preview.allow(&client) {
        return Err(AppError::rate_limited("Too many invite lookups from this address."));
    }
    Ok(Json(service::preview(&state, &token).await?))
}

async fn request_join(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(token): Path<String>,
    ApiJson(input): ApiJson<RequestJoin>,
) -> AppResult<(StatusCode, Json<JoinRequested>)> {
    Ok((StatusCode::CREATED, Json(joins::request(&state, &token, &device, input.key_package).await?)))
}
