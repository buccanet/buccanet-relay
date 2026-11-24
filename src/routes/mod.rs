//! Transport only: routing, extraction, security headers and CORS.

mod devices;
mod rooms;
mod stream;

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequest};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue};
use axum::http::Method;
use axum::routing::get;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::config::Config;
use crate::error::AppError;
use crate::state::AppState;

const BODY_LIMIT_BYTES: usize = 256 * 1024;

/// JSON body whose rejection is a regular `{error, message}` answer.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct ApiJson<T>(pub T);

impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        AppError::invalid(rejection.body_text())
    }
}

pub fn router(state: AppState, config: &Config) -> Router {
    let api = Router::new()
        .merge(devices::routes())
        .merge(rooms::routes())
        .merge(stream::routes())
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(cors(config));
    let app = Router::new()
        .nest(buccanet_protocol::API_PREFIX, api)
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state);
    let app = match &config.static_dir {
        Some(dir) => app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")))),
        None => app,
    };
    security_headers(app)
}

fn cors(config: &Config) -> CorsLayer {
    let origin = HeaderValue::from_str(&config.public_origin).expect("PUBLIC_ORIGIN is a valid header value");
    CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE, HeaderName::from_static("last-event-id")])
}

fn security_headers(app: Router) -> Router {
    let headers = [
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "strict-origin-when-cross-origin"),
        ("x-frame-options", "DENY"),
        ("cross-origin-opener-policy", "same-origin"),
    ];
    headers.into_iter().fold(app, |app, (name, value)| {
        app.layer(SetResponseHeaderLayer::if_not_present(HeaderName::from_static(name), HeaderValue::from_static(value)))
    })
}
