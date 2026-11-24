use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::features::devices::repo::DeviceDoc;
use crate::state::AppState;
use crate::tokens::hash_token;

/// The device behind the bearer token of a request, rate limited per device.
pub struct AuthDevice(pub DeviceDoc);

impl FromRequestParts<AppState> for AuthDevice {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(AppError::unauthorized)?;
        let device = state.devices.by_token_hash(&hash_token(token)).await?.ok_or_else(AppError::unauthorized)?;
        if !state.limits.device.allow(&device.id) {
            return Err(AppError::rate_limited("Slow down a little."));
        }
        Ok(AuthDevice(device))
    }
}
