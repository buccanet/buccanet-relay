use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use buccanet_protocol::api::{JoinRequested, ListingView, RequestJoin, SetListing};

use super::ApiJson;
use crate::auth::AuthDevice;
use crate::client_addr::ClientAddr;
use crate::error::{AppError, AppResult};
use crate::features::rooms::listings;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/rooms/{room_id}/listing", post(set))
        .route("/listings/{slug}", get(show))
        .route("/listings/{slug}/requests", post(request_join))
}

async fn set(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(room_id): Path<String>,
    ApiJson(input): ApiJson<SetListing>,
) -> AppResult<Json<ListingView>> {
    Ok(Json(listings::set(&state, &room_id, &device, input).await?))
}

async fn show(State(state): State<AppState>, ClientAddr(client): ClientAddr, Path(slug): Path<String>) -> AppResult<Json<ListingView>> {
    if !state.limits.preview.allow(&client) {
        return Err(AppError::rate_limited("Too many listing lookups from this address."));
    }
    Ok(Json(listings::show(&state, &slug).await?))
}

async fn request_join(
    State(state): State<AppState>,
    AuthDevice(device): AuthDevice,
    Path(slug): Path<String>,
    ApiJson(input): ApiJson<RequestJoin>,
) -> AppResult<(StatusCode, Json<JoinRequested>)> {
    Ok((StatusCode::CREATED, Json(listings::request(&state, &slug, &device, input.key_package).await?)))
}
