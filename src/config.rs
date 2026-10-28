use anyhow::{Context, Result, bail};
use std::{env, net::SocketAddr};

#[derive(Clone)]
pub struct Config {
    pub bot_token: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub cookie_secure: bool,
    pub encryption_key: [u8; 32],
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();
        let bot_token = env::var("DISCORD_BOT_TOKEN").context("DISCORD_BOT_TOKEN is missing in .env")?;
        let client_id = env::var("DISCORD_CLIENT_ID").context("DISCORD_CLIENT_ID is missing in .env")?;
        let client_secret = env::var("DISCORD_CLIENT_SECRET").context("DISCORD_CLIENT_SECRET is missing in .env")?;
        let redirect_uri = env::var("DISCORD_REDIRECT_URI").context("DISCORD_REDIRECT_URI is missing in .env")?;
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into()).parse().context("BIND_ADDR must be a socket address")?;
        if !bind_addr.ip().is_loopback() {
            bail!("BIND_ADDR must use a loopback address unless an explicit trusted deployment is configured");
        }
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/panel.db?mode=rwc".into());
        let cookie_secure = env::var("COOKIE_SECURE").unwrap_or_else(|_| "false".into()) == "true";
        let key_hex = env::var("APP_ENCRYPTION_KEY").context("APP_ENCRYPTION_KEY must be set to a random 64-character hex key")?;
        let decoded = hex::decode(key_hex).context("APP_ENCRYPTION_KEY must be valid hexadecimal")?;
        let encryption_key: [u8; 32] = decoded.try_into().map_err(|_| anyhow::anyhow!("APP_ENCRYPTION_KEY must decode to exactly 32 bytes"))?;
        Ok(Self { bot_token, client_id, client_secret, redirect_uri, bind_addr, database_url, cookie_secure, encryption_key })
    }
}
