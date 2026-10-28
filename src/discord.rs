use crate::{auth::{Session, UserView}, state::AppState};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MANAGE_GUILD: u64 = 1 << 5;
const ADMINISTRATOR: u64 = 1 << 3;
const KICK_MEMBERS: u64 = 1 << 1;
const BAN_MEMBERS: u64 = 1 << 2;
const MANAGE_MESSAGES: u64 = 1 << 13;
const MODERATE_MEMBERS: u64 = 1 << 40;

#[derive(Clone, Deserialize, Serialize)]
pub struct GuildAccess { pub id: String, pub name: String, pub icon: Option<String>, pub owner: bool, pub permissions: String }
impl GuildAccess {
    pub fn permission_bits(&self) -> u64 { self.permissions.parse().unwrap_or(0) }
    pub fn can_manage(&self) -> bool { self.owner || self.permission_bits() & (ADMINISTRATOR | MANAGE_GUILD) != 0 }
}
#[derive(Deserialize)]
struct GuildRole { id: String, position: i64, permissions: String }
#[derive(Deserialize)]
struct GuildMember { roles: Vec<String> }
#[derive(Deserialize)]
struct BotUser { id: String }

pub async fn user_guilds(state: &AppState, access: &str) -> Result<Vec<GuildAccess>> {
    let response = state.http.get("https://discord.com/api/v10/users/@me/guilds").bearer_auth(access).send().await?.error_for_status()?;
    Ok(response.json().await?)
}

pub async fn authorize_guild(state: &AppState, session: &Session, guild_id: &str, needed: u64) -> Result<GuildAccess> {
    if !is_snowflake(guild_id) { bail!("Некорректный идентификатор Discord-сервера"); }
    let guild = user_guilds(state, &session.access_token).await?.into_iter().find(|g| g.id == guild_id).context("Сервер не найден среди доступных вашему аккаунту")?;
    if !guild.can_manage() || (!guild.owner && guild.permission_bits() & needed != needed) {
        bail!("У вашего Discord-аккаунта недостаточно разрешений для этого действия");
    }
    Ok(guild)
}

pub fn is_snowflake(value: &str) -> bool { !value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit()) }

fn discord_client(state: &AppState) -> reqwest::RequestBuilder {
    state.http.get("https://discord.com/api/v10").header("Authorization", format!("Bot {}", state.config.bot_token))
}
async fn get_roles(state: &AppState, guild: &str) -> Result<Vec<GuildRole>> {
    Ok(discord_client(state).get(format!("https://discord.com/api/v10/guilds/{guild}/roles")).send().await?.error_for_status()?.json().await?)
}
async fn get_member(state: &AppState, guild: &str, user: &str) -> Result<GuildMember> {
    Ok(discord_client(state).get(format!("https://discord.com/api/v10/guilds/{guild}/members/{user}")).send().await?.error_for_status()?.json().await?)
}
fn role_position(roles: &[GuildRole], member: &GuildMember, guild_id: &str) -> i64 {
    member.roles.iter().filter_map(|id| roles.iter().find(|r| &r.id == id).map(|r| r.position)).max().unwrap_or_else(|| roles.iter().find(|r| r.id == guild_id).map(|r| r.position).unwrap_or(0))
}
fn role_permissions(roles: &[GuildRole], member: &GuildMember, guild_id: &str) -> u64 {
    roles.iter().filter(|role| role.id == guild_id || member.roles.contains(&role.id)).filter_map(|role| role.permissions.parse::<u64>().ok()).fold(0, |all, next| all | next)
}

pub async fn ensure_bot_can_act(state: &AppState, session: &Session, guild: &GuildAccess, target: Option<&str>, needed: u64) -> Result<()> {
    let me: BotUser = discord_client(state).get("https://discord.com/api/v10/users/@me").send().await?.error_for_status()?.json().await?;
    let roles = get_roles(state, &guild.id).await?;
    let bot_member = get_member(state, &guild.id, &me.id).await.context("Бот не состоит на этом сервере")?;
    let bot_permissions = role_permissions(&roles, &bot_member, &guild.id);
    if bot_permissions & ADMINISTRATOR == 0 && bot_permissions & needed != needed { bail!("У бота не хватает Discord-разрешений для этого действия"); }
    if let Some(target_id) = target {
        if !is_snowflake(target_id) { bail!("Некорректный Discord ID пользователя"); }
        if target_id == guild.id { bail!("Это действие нельзя выполнить над владельцем сервера"); }
        let target_member = get_member(state, &guild.id, target_id).await.context("Пользователь не найден на сервере")?;
        let bot_top = role_position(&roles, &bot_member, &guild.id);
        let target_top = role_position(&roles, &target_member, &guild.id);
        if target_top >= bot_top { bail!("Роль бота должна быть выше высшей роли пользователя"); }
        if !guild.owner && guild.permission_bits() & ADMINISTRATOR == 0 {
            let actor_member = get_member(state, &guild.id, &session.user.id).await.context("Не удалось проверить ваши роли на сервере")?;
            if role_position(&roles, &actor_member, &guild.id) <= target_top { bail!("Нельзя применять это действие к пользователю с равной или более высокой ролью"); }
        }
    }
    Ok(())
}

pub async fn execute_moderation(state: &AppState, session: &Session, guild: &GuildAccess, action: &str, target: &str, reason: &str, duration: Option<u64>) -> Result<()> {
    if !is_snowflake(target) { bail!("Некорректный Discord ID пользователя"); }
    let (permission, endpoint, method) = match action {
        "ban" => (BAN_MEMBERS, format!("/guilds/{}/bans/{target}", guild.id), "PUT"),
        "kick" => (KICK_MEMBERS, format!("/guilds/{}/members/{target}", guild.id), "DELETE"),
        "timeout" => (MODERATE_MEMBERS, format!("/guilds/{}/members/{target}", guild.id), "PATCH"),
        "warn" => (MANAGE_MESSAGES, String::new(), "LOCAL"),
        _ => bail!("Неизвестное действие модерации"),
    };
    authorize_guild(state, session, &guild.id, permission).await?;
    if action != "warn" { ensure_bot_can_act(state, session, guild, Some(target), permission).await?; }
    let safe_reason = if reason.trim().is_empty() { "Причина не указана" } else { reason.trim() };
    if safe_reason.chars().count() > 400 { bail!("Причина должна быть короче 400 символов"); }
    if action == "timeout" {
        let seconds = duration.unwrap_or(600).clamp(1, 2_419_200);
        let until = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);
        let url = format!("https://discord.com/api/v10{endpoint}");
        state.http.patch(url).header("Authorization", format!("Bot {}", state.config.bot_token)).header("X-Audit-Log-Reason", urlencoding::encode(safe_reason).to_string()).json(&json!({"communication_disabled_until": until.to_rfc3339()})).send().await?.error_for_status()?;
    } else if action == "ban" || action == "kick" {
        let url = format!("https://discord.com/api/v10{endpoint}");
        let request = if method == "PUT" { state.http.put(url) } else { state.http.delete(url) };
        let mut request = request.header("Authorization", format!("Bot {}", state.config.bot_token)).header("X-Audit-Log-Reason", urlencoding::encode(safe_reason).to_string());
        if action == "ban" { request = request.json(&json!({"delete_message_seconds": 0})); }
        request.send().await?.error_for_status()?;
    }
    let now = chrono::Utc::now().timestamp();
    let mut tx = state.db.begin().await?;
    if action == "warn" {
        sqlx::query("INSERT INTO warnings(guild_id,target_id,moderator_id,reason,created_at) VALUES(?,?,?,?,?)")
            .bind(&guild.id).bind(target).bind(&session.user.id).bind(safe_reason).bind(now).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO action_log(guild_id,actor_id,action,target_id,details,created_at) VALUES(?,?,?,?,?,?)")
        .bind(&guild.id).bind(&session.user.id).bind(action).bind(target).bind(json!({"reason":safe_reason}).to_string()).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn warnings(state: &AppState, guild_id: &str, target_id: &str) -> Result<Vec<serde_json::Value>> {
    let rows = sqlx::query("SELECT id,moderator_id,reason,created_at FROM warnings WHERE guild_id=? AND target_id=? ORDER BY created_at DESC LIMIT 50")
        .bind(guild_id).bind(target_id).fetch_all(&state.db).await?;
    use sqlx::Row;
    Ok(rows.into_iter().map(|row| json!({"id":row.get::<i64,_>("id"),"moderator_id":row.get::<String,_>("moderator_id"),"reason":row.get::<String,_>("reason"),"created_at":row.get::<i64,_>("created_at")})).collect())
}

pub const PERMISSION_MANAGE_GUILD: u64 = MANAGE_GUILD;
