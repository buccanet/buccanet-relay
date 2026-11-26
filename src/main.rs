//! Buccanet relay. Transport only: it stores and forwards sealed MLS messages and never holds a key.

mod auth;
mod client_addr;
mod config;
mod error;
mod features;
mod limits;
mod locks;
mod routes;
mod state;
mod tokens;

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("health") {
        return health().await;
    }
    serve().await
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?)).init();
    let config = config::Config::from_env()?;
    let state = state::AppState::connect(&config).await?;
    state.ensure_indexes().await?;
    tokio::spawn(sweep(state.clone()));
    let app = routes::router(state, &config);
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!("relay listening on {}", config.bind);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

/// Every few minutes, forgets what idle limits and streams left in memory and deletes the
/// listings that no longer stand.
async fn sweep(state: state::AppState) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(300));
    loop {
        every.tick().await;
        state.sweep();
        if let Err(error) = features::rooms::listings::sweep(&state).await {
            tracing::warn!(message = error.message, "the listing sweep failed");
        }
    }
}

/// `buccanet-relay health`: succeeds when the relay of this container answers `/healthz`, for
/// container health checks in an image that ships no HTTP client.
async fn health() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("PORT")?;
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>()?)).await?;
    stream.write_all(b"GET /healthz HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").await?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await?;
    if !answer.starts_with("HTTP/1.0 200") && !answer.starts_with("HTTP/1.1 200") {
        return Err("the relay is not healthy".into());
    }
    Ok(())
}

/// Stops on Ctrl+C or on the SIGTERM a container runtime sends.
async fn shutdown() {
    let mut terminate = signal(SignalKind::terminate()).expect("the SIGTERM handler installs");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
    tracing::info!("relay stopping");
}
