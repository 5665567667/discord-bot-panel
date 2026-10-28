use crate::state::{AppState, BotSnapshot};
use serenity::{async_trait, model::gateway::Ready, prelude::*};
use tokio::sync::watch;
use tracing::{error, info, warn};

struct Handler { status: watch::Sender<BotSnapshot> }
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        info!(bot = %ready.user.name, guild_count = ready.guilds.len(), "Discord Gateway connected");
        let _ = self.status.send(BotSnapshot { connected: true, guild_count: ready.guilds.len(), user: Some(ready.user.name.to_string()) });
    }
}

pub async fn run(state: AppState, status: watch::Sender<BotSnapshot>) {
    loop {
        if state.shutdown.is_cancelled() { break; }
        let intents = GatewayIntents::GUILDS;
        let mut client = match Client::builder(&state.config.bot_token, intents).event_handler(Handler { status: status.clone() }).await {
            Ok(client) => client,
            Err(error) => { error!(error=%error, "Could not initialize Discord client"); tokio::time::sleep(std::time::Duration::from_secs(5)).await; continue; }
        };
        tokio::select! {
            _ = state.shutdown.cancelled() => {
                client.shard_manager.shutdown_all().await;
                break;
            }
            result = client.start() => {
                let _ = status.send(BotSnapshot::default());
                if let Err(error) = result { warn!(error=%error, "Discord Gateway disconnected; retrying"); }
            }
        }
        tokio::select! { _ = state.shutdown.cancelled() => break, _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {} }
    }
    let _ = status.send(BotSnapshot::default());
}
