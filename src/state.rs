use crate::config::Config;
use serde::Serialize;
use sqlx::SqlitePool;
use std::time::Instant;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default, Serialize)]
pub struct BotSnapshot {
    pub connected: bool,
    pub guild_count: usize,
    pub user: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: SqlitePool,
    pub http: reqwest::Client,
    pub bot_status: watch::Receiver<BotSnapshot>,
    pub shutdown: CancellationToken,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(config: Config, db: SqlitePool, bot_status: watch::Receiver<BotSnapshot>, shutdown: CancellationToken) -> Self {
        Self { config, db, http: reqwest::Client::builder().https_only(true).user_agent("DiscordBotPanel/0.1").build().expect("reqwest client configuration is valid"), bot_status, shutdown, started_at: Instant::now() }
    }
}
