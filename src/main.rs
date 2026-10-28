mod api;
mod auth;
mod bot;
mod config;
mod crypto;
mod db;
mod discord;
mod state;

use anyhow::Result;
use config::Config;
use state::{AppState, BotSnapshot};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    let (status_tx, status_rx) = watch::channel(BotSnapshot::default());
    let shutdown = CancellationToken::new();
    let state = AppState::new(config.clone(), pool, status_rx, shutdown.clone());

    let bot_state = state.clone();
    let bot_task = tokio::spawn(async move { bot::run(bot_state, status_tx).await });

    let app = api::router(state).layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(address = %config.bind_addr, "Discord Bot Panel API started");
    let shutdown_signal = shutdown.clone();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => tracing::info!("Ctrl+C received"),
            _ = shutdown_signal.cancelled() => tracing::info!("Shutdown requested"),
        }
    });
    server.await?;
    shutdown.cancel();
    if tokio::time::timeout(std::time::Duration::from_secs(10), bot_task).await.is_err() {
        tracing::warn!("Bot task did not stop in time");
    }
    Ok(())
}
