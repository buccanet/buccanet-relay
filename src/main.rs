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

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

/// Every few minutes, forgets what idle limits and streams left in memory.
async fn sweep(state: state::AppState) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(300));
    loop {
        every.tick().await;
        state.sweep();
    }
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("relay stopping");
}
