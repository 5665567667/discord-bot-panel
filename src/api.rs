use crate::{auth, auth_api, discord, state::{AppState, BotSnapshot}};
use axum::{extract::{Path, Query, State, WebSocketUpgrade, ws::{Message, WebSocket}}, http::{HeaderMap, HeaderValue, Method, StatusCode}, response::{IntoResponse, Response}, routing::{get, post, put}, Json, Router};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct FrontendAssets;

#[derive(Serialize)] struct Status { bot_connected: bool, guild_count: usize, uptime_seconds: u64 }
#[derive(Serialize)] struct GuildView { id: String, name: String, icon: Option<String>, bot_present: bool }
#[derive(Serialize)] struct Settings { guild_id: String, welcome_channel_id: Option<String>, welcome_message: String, mod_log_channel_id: Option<String> }
#[derive(Deserialize)] struct SettingsInput { welcome_channel_id: Option<String>, welcome_message: String, mod_log_channel_id: Option<String> }
#[derive(Deserialize)] struct ModerationInput { action: String, target_id: String, reason: String, duration_seconds: Option<u64> }
#[derive(Serialize)] struct LogEntry { id: i64, actor_id: String, action: String, target_id: Option<String>, details: String, created_at: i64 }
#[derive(Serialize)] struct WarningView { id: i64, moderator_id: String, reason: String, created_at: i64 }
#[derive(Deserialize)] struct WarningsQuery { target_id: String }

struct ApiError { status: StatusCode, message: String }
impl ApiError { fn bad(message: impl Into<String>) -> Self { Self { status: StatusCode::BAD_REQUEST, message: message.into() } } fn forbidden(message: impl Into<String>) -> Self { Self { status: StatusCode::FORBIDDEN, message: message.into() } } }
impl IntoResponse for ApiError { fn into_response(self) -> Response { (self.status, Json(json!({"error":self.message}))).into_response() } }
fn internal(error: impl std::fmt::Display) -> ApiError { tracing::error!(error=%error,"API operation failed"); ApiError { status: StatusCode::INTERNAL_SERVER_ERROR, message: "Внутренняя ошибка. Проверьте журнал приложения.".into() } }
async fn require_session(state: &AppState, headers: &HeaderMap) -> Result<auth::Session, ApiError> {
    auth::load_session(state, headers).await.map_err(internal)?.ok_or_else(|| ApiError { status: StatusCode::UNAUTHORIZED, message: "Требуется вход через Discord".into() })
}
async fn require_guild(state: &AppState, headers: &HeaderMap, guild_id: &str, permission: u64) -> Result<(auth::Session, discord::GuildAccess), ApiError> {
    let session = require_session(state, headers).await?;
    let guild = discord::authorize_guild(state, &session, guild_id, permission).await.map_err(|e| ApiError::forbidden(e.to_string()))?;
    Ok((session, guild))
}
async fn status(State(state): State<AppState>) -> impl IntoResponse {
    let bot = state.bot_status.borrow().clone();
    Json(Status { bot_connected: bot.connected, guild_count: bot.guild_count, uptime_seconds: state.started_at.elapsed().as_secs() })
}
async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<auth::UserView>, ApiError> { Ok(Json(require_session(&state, &headers).await?.user)) }
async fn guilds(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<GuildView>>, ApiError> {
    let session = require_session(&state, &headers).await?;
    let data = discord::user_guilds(&state, &session.access_token).await.map_err(|e| ApiError { status: StatusCode::BAD_GATEWAY, message: format!("Discord не вернул список серверов: {e}") })?;
    let bot: BotSnapshot = state.bot_status.borrow().clone();
    let views = data.into_iter().filter(discord::GuildAccess::can_manage).map(|g| GuildView { bot_present: false, id: g.id, name: g.name, icon: g.icon }).collect::<Vec<_>>();
    let mut views = views;
    for guild in &mut views { guild.bot_present = bot.guild_count > 0; }
    Ok(Json(views))
}
async fn get_settings(State(state): State<AppState>, headers: HeaderMap, Path(guild_id): Path<String>) -> Result<Json<Settings>, ApiError> {
    let _ = require_guild(&state, &headers, &guild_id, discord::PERMISSION_MANAGE_GUILD).await?;
    let row = sqlx::query("SELECT welcome_channel_id,welcome_message,mod_log_channel_id FROM guild_settings WHERE guild_id=?").bind(&guild_id).fetch_optional(&state.db).await.map_err(internal)?;
    let settings = match row {
        Some(row) => Settings { guild_id, welcome_channel_id: row.try_get("welcome_channel_id").map_err(internal)?, welcome_message: row.try_get("welcome_message").map_err(internal)?, mod_log_channel_id: row.try_get("mod_log_channel_id").map_err(internal)? },
        None => Settings { guild_id, welcome_channel_id: None, welcome_message: "Добро пожаловать, {user}!".into(), mod_log_channel_id: None },
    };
    Ok(Json(settings))
}
async fn save_settings(State(state): State<AppState>, headers: HeaderMap, Path(guild_id): Path<String>, Json(input): Json<SettingsInput>) -> Result<Json<Settings>, ApiError> {
    if !auth::csrf_matches(&headers) { return Err(ApiError::forbidden("CSRF-проверка не пройдена")); }
    let (session, _) = require_guild(&state, &headers, &guild_id, discord::PERMISSION_MANAGE_GUILD).await?;
    if input.welcome_message.chars().count() > 1800 { return Err(ApiError::bad("Текст приветствия слишком длинный")); }
    for channel in [&input.welcome_channel_id, &input.mod_log_channel_id].into_iter().flatten() {
        if !discord::is_snowflake(channel) { return Err(ApiError::bad("ID канала должен состоять только из цифр Discord ID")); }
    }
    let now = chrono::Utc::now().timestamp();
    sqlx::query("INSERT INTO guild_settings(guild_id,welcome_channel_id,welcome_message,mod_log_channel_id,updated_at) VALUES(?,?,?,?,?) ON CONFLICT(guild_id) DO UPDATE SET welcome_channel_id=excluded.welcome_channel_id,welcome_message=excluded.welcome_message,mod_log_channel_id=excluded.mod_log_channel_id,updated_at=excluded.updated_at")
        .bind(&guild_id).bind(&input.welcome_channel_id).bind(&input.welcome_message).bind(&input.mod_log_channel_id).bind(now).execute(&state.db).await.map_err(internal)?;
    sqlx::query("INSERT INTO action_log(guild_id,actor_id,action,details,created_at) VALUES(?,?,?,?,?)")
        .bind(&guild_id).bind(&session.user.id).bind("settings_update").bind("{}").bind(now).execute(&state.db).await.map_err(internal)?;
    Ok(Json(Settings { guild_id, welcome_channel_id: input.welcome_channel_id, welcome_message: input.welcome_message, mod_log_channel_id: input.mod_log_channel_id }))
}
async fn moderation(State(state): State<AppState>, headers: HeaderMap, Path(guild_id): Path<String>, Json(input): Json<ModerationInput>) -> Result<Json<serde_json::Value>, ApiError> {
    if !auth::csrf_matches(&headers) { return Err(ApiError::forbidden("CSRF-проверка не пройдена")); }
    let (session, guild) = require_guild(&state, &headers, &guild_id, 0).await?;
    discord::execute_moderation(&state, &session, &guild, &input.action, &input.target_id, &input.reason, input.duration_seconds).await.map_err(|e| ApiError { status: StatusCode::BAD_GATEWAY, message: e.to_string() })?;
    Ok(Json(json!({"ok":true,"message":"Действие подтверждено Discord и записано в журнал"})))
}
async fn logs(State(state): State<AppState>, headers: HeaderMap, Path(guild_id): Path<String>) -> Result<Json<Vec<LogEntry>>, ApiError> {
    let _ = require_guild(&state, &headers, &guild_id, discord::PERMISSION_MANAGE_GUILD).await?;
    let rows = sqlx::query("SELECT id,actor_id,action,target_id,details,created_at FROM action_log WHERE guild_id=? ORDER BY created_at DESC LIMIT 100").bind(guild_id).fetch_all(&state.db).await.map_err(internal)?;
    let items = rows.into_iter().map(|r| Ok(LogEntry { id:r.try_get("id")?, actor_id:r.try_get("actor_id")?, action:r.try_get("action")?, target_id:r.try_get("target_id")?, details:r.try_get("details")?, created_at:r.try_get("created_at")? })).collect::<Result<Vec<_>,sqlx::Error>>().map_err(internal)?;
    Ok(Json(items))
}
async fn warnings(State(state): State<AppState>, headers: HeaderMap, Path(guild_id): Path<String>, Query(query): Query<WarningsQuery>) -> Result<Json<Vec<WarningView>>, ApiError> {
    let _ = require_guild(&state, &headers, &guild_id, 0).await?;
    if !discord::is_snowflake(&query.target_id) { return Err(ApiError::bad("Некорректный Discord ID пользователя")); }
    let rows = sqlx::query("SELECT id,moderator_id,reason,created_at FROM warnings WHERE guild_id=? AND target_id=? ORDER BY created_at DESC LIMIT 50").bind(guild_id).bind(query.target_id).fetch_all(&state.db).await.map_err(internal)?;
    let result = rows.into_iter().map(|r| Ok(WarningView{id:r.try_get("id")?,moderator_id:r.try_get("moderator_id")?,reason:r.try_get("reason")?,created_at:r.try_get("created_at")?})).collect::<Result<Vec<_>,sqlx::Error>>().map_err(internal)?;
    Ok(Json(result))
}
async fn websocket(ws: WebSocketUpgrade, State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let _ = require_session(&state, &headers).await?;
    Ok(ws.on_upgrade(move |socket| stream_status(socket, state)).into_response())
}
async fn stream_status(mut socket: WebSocket, state: AppState) {
    let mut rx = state.bot_status.clone();
    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => break,
            changed = rx.changed() => {
                if changed.is_err() { break; }
                let text = serde_json::to_string(&*rx.borrow()).unwrap_or_else(|_| "{}".into());
                if socket.send(Message::Text(text.into())).await.is_err() { break; }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                if socket.send(Message::Ping(axum::body::Bytes::new())).await.is_err() { break; }
            }
        }
    }
}
async fn static_file(uri: axum::http::Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let path = if requested.is_empty() { "index.html" } else { requested };
    let asset = FrontendAssets::get(path).or_else(|| FrontendAssets::get("index.html"));
    match asset {
        Some(asset) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(axum::http::header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")))], asset.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Frontend assets are missing. Run npm run build in frontend.").into_response(),
    }
}

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            let value = origin.as_bytes();
            value == b"http://127.0.0.1:5173" || value == b"http://localhost:5173" || value == b"http://tauri.localhost" || value == b"tauri://localhost"
        }))
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
        .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::COOKIE, axum::http::HeaderName::from_static("x-csrf-token")]);
    Router::new()
        .route("/auth/login", get(auth_api::login))
        .route("/auth/callback", get(auth_api::callback))
        .route("/api/auth/logout", post(auth_api::logout))
        .route("/api/me", get(auth_api::me))
        .route("/api/status", get(status))
        .route("/api/guilds", get(guilds))
        .route("/api/guilds/{guild_id}/settings", get(get_settings).put(save_settings))
        .route("/api/guilds/{guild_id}/moderation", post(moderation))
        .route("/api/guilds/{guild_id}/logs", get(logs))
        .route("/api/guilds/{guild_id}/warnings", get(warnings))
        .route("/ws", get(websocket))
        .fallback(static_file)
        .layer(cors)
        .with_state(state)
}
