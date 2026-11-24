use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use buccanet_protocol::api::{ErrorBody, codes};

/// Every failure leaves the relay as `{error, message}` with a stable code.
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, codes::UNAUTHORIZED, "A valid device token is required.")
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, codes::FORBIDDEN, message)
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, codes::NOT_FOUND, format!("{what} does not exist."))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, codes::INVALID_INPUT, message)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, codes::RATE_LIMITED, message)
    }

    pub fn too_many_streams() -> Self {
        Self::rate_limited("This device has too many open streams. Close one first.")
    }

    pub fn stale_epoch(room_epoch: u64) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            codes::STALE_EPOCH,
            format!("The room is at epoch {room_epoch}. Catch up from your inbox and send again."),
        )
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = ErrorBody { error: self.code.to_string(), message: self.message };
        (self.status, Json(body)).into_response()
    }
}

impl From<mongodb::error::Error> for AppError {
    fn from(error: mongodb::error::Error) -> Self {
        tracing::error!(?error, "database failure");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, codes::INTERNAL, "Something went wrong on our side.")
    }
}

impl From<mongodb::bson::ser::Error> for AppError {
    fn from(error: mongodb::bson::ser::Error) -> Self {
        tracing::error!(?error, "serialization failure");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, codes::INTERNAL, "Something went wrong on our side.")
    }
}
