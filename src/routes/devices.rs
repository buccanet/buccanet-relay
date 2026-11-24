use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use buccanet_protocol::api::{DeviceRegistered, DeviceView, RegisterDevice, codes};
use mongodb::bson::DateTime;

use super::ApiJson;
use crate::auth::AuthDevice;
use crate::error::{AppError, AppResult};
use crate::features::devices::repo::DeviceDoc;
use crate::state::AppState;
use crate::tokens::{hash_token, new_token};

const MAX_NAME_CHARS: usize = 60;

pub fn routes() -> Router<AppState> {
    Router::new().route("/devices", post(register)).route("/devices/me", get(me))
}

async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ApiJson(input): ApiJson<RegisterDevice>,
) -> AppResult<(StatusCode, Json<DeviceRegistered>)> {
    if !state.register_limiter.allow(&peer.ip().to_string()) {
        return Err(AppError::new(StatusCode::TOO_MANY_REQUESTS, codes::RATE_LIMITED, "Too many new devices from this address."));
    }
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::invalid("name must be 1 to 60 characters."));
    }
    if input.public_key.len() != 32 {
        return Err(AppError::invalid("publicKey must be an Ed25519 public key."));
    }
    let token = new_token();
    let device = DeviceDoc {
        id: uuid::Uuid::new_v4().to_string(),
        public_key: input.public_key,
        kind: input.kind,
        name: name.to_string(),
        token_hash: hash_token(&token),
        inbox_seq: 0,
        created_at: DateTime::now(),
    };
    state.devices.insert(&device).await?;
    Ok((StatusCode::CREATED, Json(DeviceRegistered { device_id: device.id, token })))
}

async fn me(AuthDevice(device): AuthDevice) -> Json<DeviceView> {
    Json(DeviceView { device_id: device.id, name: device.name, kind: device.kind })
}
