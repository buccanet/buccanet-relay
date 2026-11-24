use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::HeaderName;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub mongodb_uri: String,
    pub mongodb_db: String,
    /// Origin of the web app; invite links and CORS use it.
    pub public_origin: String,
    pub message_ttl_days: u64,
    /// Built web app served at `/` when set, so one origin hosts relay and web client.
    pub static_dir: Option<PathBuf>,
    /// Header holding the client address, set by the reverse proxy in front of the relay.
    pub client_ip_header: Option<HeaderName>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var {0}")]
    Missing(&'static str),
    #[error("env var {0} is not valid: {1}")]
    Invalid(&'static str, String),
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::Missing(name))
}

fn parsed<T: std::str::FromStr>(name: &'static str) -> Result<T, ConfigError> {
    let raw = required(name)?;
    raw.parse().map_err(|_| ConfigError::Invalid(name, raw))
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let port: u16 = parsed("PORT")?;
        let host = std::env::var("BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let bind = format!("{host}:{port}").parse().map_err(|_| ConfigError::Invalid("BIND_HOST", host))?;
        Ok(Self {
            bind,
            mongodb_uri: required("MONGODB_URI")?,
            mongodb_db: required("MONGODB_DB")?,
            public_origin: required("PUBLIC_ORIGIN")?,
            message_ttl_days: parsed("MESSAGE_TTL_DAYS")?,
            static_dir: std::env::var("STATIC_DIR").ok().map(PathBuf::from),
            client_ip_header: match std::env::var("CLIENT_IP_HEADER") {
                Ok(name) => Some(HeaderName::try_from(name.as_str()).map_err(|_| ConfigError::Invalid("CLIENT_IP_HEADER", name))?),
                Err(_) => None,
            },
        })
    }
}
