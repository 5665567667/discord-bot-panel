# Discord Bot Panel: объединённая документация

Этот документ объединяет материалы о проекте, Tauri-приложении, BAT-меню и диалогах подтверждения действий. Он содержит проектные заготовки и кодовые фрагменты. Это не готовый архив или проверенный релиз. Rust-сборка, frontend-сборка, тесты, OAuth и запуск в Windows не выполнялись.

## Содержание

1. Архитектура
2. Общие настройки и база данных
3. Rust backend
4. React frontend и стиль панели
5. Настольное приложение Tauri
6. Подтверждение действий
7. Единый BAT-файл
8. Установка и проверки
9. Ограничения и обязательные доработки

## Архитектура

```text
DiscordBotPanel/
├── Start all in one.bat
├── Cargo.toml
├── Cargo.lock
├── .env.example
├── .gitignore
├── README.md
├── migrations/0001_initial.sql
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── db.rs
│   ├── oauth.rs
│   ├── api.rs
│   ├── bot.rs
│   └── state.rs
├── desktop/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── capabilities/default.json
│   └── src/main.rs
└── frontend/
    ├── package.json
    ├── index.html
    ├── vite.config.ts
    ├── tsconfig.json
    └── src/
        ├── main.tsx
        ├── App.tsx
        ├── Manager.tsx
        ├── components/ConfirmAction.tsx
        ├── style.css
        └── manager.css
```

Rust отвечает за API, Discord Gateway и процессы приложения. React/TypeScript отвечает за UI. Tauri открывает интерфейс в отдельном окне Windows. SQLite хранит локальные данные.

## Общие настройки и база данных

### `Cargo.toml`

```toml
[package]
name = "discord-bot-panel"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0"
axum = { version = "0.8", features = ["ws"] }
chrono = { version = "0.4", features = ["serde"] }
dotenvy = "0.15"
rand = "0.8"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serenity = { version = "0.12", default-features = false, features = ["builder", "client", "gateway", "http", "model", "rustls_backend"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sha2 = "0.10"
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "sqlite", "chrono", "macros", "migrate"] }
subtle = "2.6"
tokio = { version = "1", features = ["full"] }
tower-http = { version = "0.6", features = ["cors", "trace", "limit"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
url = "2.5"
urlencoding = "2"
```

Cargo должен сформировать `Cargo.lock`. Точные версии зависимостей закрепите и проверяйте на Windows.

### `.env.example`

```dotenv
DISCORD_BOT_TOKEN=
DISCORD_CLIENT_ID=
DISCORD_CLIENT_SECRET=
DISCORD_REDIRECT_URI=http://127.0.0.1:8787/auth/callback
PUBLIC_BASE_URL=http://127.0.0.1:8787
DATABASE_URL=sqlite://data/panel.db
BIND_ADDR=127.0.0.1:8787
COOKIE_SECURE=false
```

Не коммитьте `.env` и не вставляйте секреты во frontend.

### `.gitignore`

```gitignore
.env
/target
/frontend/node_modules
/frontend/dist
/data/*.db
/data/*.db-shm
/data/*.db-wal
/logs/*
/backups/*
```

### `migrations/0001_initial.sql`

```sql
CREATE TABLE IF NOT EXISTS users (
  discord_id TEXT PRIMARY KEY,
  username TEXT NOT NULL,
  avatar TEXT,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS sessions (
  id_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(discord_id) ON DELETE CASCADE,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS sessions_expiry ON sessions(expires_at);
CREATE TABLE IF NOT EXISTS guild_settings (
  guild_id TEXT PRIMARY KEY,
  welcome_channel_id TEXT,
  welcome_message TEXT NOT NULL DEFAULT 'Добро пожаловать, {user}!'
);
CREATE TABLE IF NOT EXISTS warnings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  guild_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  moderator_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS warnings_target ON warnings(guild_id, target_id);
CREATE TABLE IF NOT EXISTS action_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  guild_id TEXT,
  actor_id TEXT NOT NULL,
  action TEXT NOT NULL,
  target_id TEXT,
  details TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS security_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event TEXT NOT NULL,
  details TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS command_settings (
  guild_id TEXT NOT NULL,
  command TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
  PRIMARY KEY(guild_id, command)
);
CREATE TABLE IF NOT EXISTS automation_rules (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  guild_id TEXT NOT NULL,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
  config TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS punishments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  guild_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  moderator_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

## Rust backend

### `src/config.rs`

```rust
use anyhow::{Context, Result};
use std::env;

#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub bind_addr: String,
    pub database_url: String,
    pub cookie_secure: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();
        Ok(Self {
            token: env::var("DISCORD_BOT_TOKEN").context("Нет DISCORD_BOT_TOKEN в .env")?,
            client_id: env::var("DISCORD_CLIENT_ID").context("Нет DISCORD_CLIENT_ID в .env")?,
            client_secret: env::var("DISCORD_CLIENT_SECRET").context("Нет DISCORD_CLIENT_SECRET в .env")?,
            redirect_uri: env::var("DISCORD_REDIRECT_URI").context("Нет DISCORD_REDIRECT_URI в .env")?,
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into()),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/panel.db".into()),
            cookie_secure: env::var("COOKIE_SECURE").unwrap_or_else(|_| "false".into()) == "true",
        })
    }
}
```

### `src/state.rs`

```rust
use sqlx::SqlitePool;
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub http: reqwest::Client,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub cookie_secure: bool,
    pub bot_ready: watch::Receiver<bool>,
}
```

### `src/db.rs`

```rust
use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub async fn connect(url: &str) -> Result<SqlitePool> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }
    let pool = SqlitePoolOptions::new().max_connections(8).connect(url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
```

### `src/oauth.rs`

```rust
use axum::{extract::{Query, State}, http::{header, HeaderMap, HeaderValue}, response::{IntoResponse, Redirect}, Json};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::state::AppState;

#[derive(Deserialize)] pub struct Callback { code: String, state: String }
#[derive(Deserialize)] struct TokenReply { access_token: String }
#[derive(Deserialize)] struct DiscordUser { id: String, username: String, avatar: Option<String> }
#[derive(Serialize)] struct UserView { id: String, username: String, avatar: Option<String> }
fn cookie(name: &str, value: &str, secure: bool) -> String {
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age=600{}", if secure { "; Secure" } else { "" })
}
pub async fn login(State(s): State<AppState>) -> impl IntoResponse {
    let state: String = rand::thread_rng().sample_iter(&Alphanumeric).take(48).map(char::from).collect();
    let url = format!("https://discord.com/oauth2/authorize?client_id={}&response_type=code&redirect_uri={}&scope=identify%20guilds&state={}", urlencoding::encode(&s.client_id), urlencoding::encode(&s.redirect_uri), state);
    let mut headers = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&cookie("oauth_state", &state, s.cookie_secure)) { headers.insert(header::SET_COOKIE, v); }
    (headers, Redirect::temporary(&url))
}
pub async fn callback(State(s): State<AppState>, headers: HeaderMap, Query(q): Query<Callback>) -> impl IntoResponse {
    let expected = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|v| v.split(';').find_map(|c| c.trim().strip_prefix("oauth_state=")));
    if expected != Some(q.state.as_str()) { return (axum::http::StatusCode::BAD_REQUEST, "Проверка OAuth state не пройдена").into_response(); }
    let form = [("client_id", s.client_id.as_str()), ("client_secret", s.client_secret.as_str()), ("grant_type", "authorization_code"), ("code", q.code.as_str()), ("redirect_uri", s.redirect_uri.as_str())];
    let response = match s.http.post("https://discord.com/api/oauth2/token").form(&form).send().await {
        Ok(r) => match r.error_for_status() { Ok(r) => r, Err(_) => return (axum::http::StatusCode::BAD_GATEWAY, "Discord отклонил обмен кода").into_response() },
        Err(_) => return (axum::http::StatusCode::BAD_GATEWAY, "Не удалось связаться с Discord").into_response(),
    };
    let token: TokenReply = match response.json().await { Ok(v) => v, Err(_) => return (axum::http::StatusCode::BAD_GATEWAY, "Некорректный ответ OAuth").into_response() };
    let response = match s.http.get("https://discord.com/api/users/@me").bearer_auth(token.access_token).send().await {
        Ok(r) => match r.error_for_status() { Ok(r) => r, Err(_) => return (axum::http::StatusCode::BAD_GATEWAY, "Не удалось получить профиль Discord").into_response() },
        Err(_) => return (axum::http::StatusCode::BAD_GATEWAY, "Не удалось связаться с Discord").into_response(),
    };
    let user: DiscordUser = match response.json().await { Ok(v) => v, Err(_) => return (axum::http::StatusCode::BAD_GATEWAY, "Не удалось прочитать профиль Discord").into_response() };
    if sqlx::query("INSERT INTO users(discord_id,username,avatar) VALUES(?,?,?) ON CONFLICT(discord_id) DO UPDATE SET username=excluded.username,avatar=excluded.avatar,updated_at=CURRENT_TIMESTAMP").bind(&user.id).bind(&user.username).bind(&user.avatar).execute(&s.db).await.is_err() { return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Не удалось сохранить профиль").into_response(); }
    let raw: String = rand::thread_rng().sample_iter(&Alphanumeric).take(64).map(char::from).collect();
    let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
    if sqlx::query("INSERT INTO sessions(id_hash,user_id,expires_at) VALUES(?,?,datetime('now','+7 days'))").bind(hash).bind(&user.id).execute(&s.db).await.is_err() { return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Не удалось создать сессию").into_response(); }
    let mut h = HeaderMap::new();
    let val = format!("session={raw}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800{}", if s.cookie_secure { "; Secure" } else { "" });
    if let Ok(v) = HeaderValue::from_str(&val) { h.insert(header::SET_COOKIE, v); }
    (h, Redirect::to("/")).into_response()
}
pub async fn me(State(s): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(raw) = session_cookie(&headers) else { return (axum::http::StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"Требуется вход через Discord"}))).into_response() };
    let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let row = sqlx::query_as::<_, (String,String,Option<String>)>("SELECT u.discord_id,u.username,u.avatar FROM sessions s JOIN users u ON u.discord_id=s.user_id WHERE s.id_hash=? AND s.expires_at>datetime('now')").bind(hash).fetch_optional(&s.db).await.ok().flatten();
    match row { Some((id, username, avatar)) => Json(UserView{id,username,avatar}).into_response(), None => (axum::http::StatusCode::UNAUTHORIZED, "Сессия истекла").into_response() }
}
pub fn session_cookie(headers: &HeaderMap) -> Option<String> { headers.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|c| c.trim().strip_prefix("session=").map(str::to_owned)) }
```

### `src/api.rs`

```rust
use axum::{extract::{State, ws::{Message, WebSocket, WebSocketUpgrade}}, http::HeaderMap, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use sha2::{Digest, Sha256};
use crate::{oauth, state::AppState};

#[derive(Serialize)] struct Status { bot_connected: bool, guild_count: u64, uptime_seconds: u64 }
async fn status(State(s): State<AppState>) -> impl IntoResponse {
    Json(Status { bot_connected: *s.bot_ready.borrow(), guild_count: 0, uptime_seconds: 0 })
}
async fn guilds(State(s): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(raw) = oauth::session_cookie(&headers) else { return (axum::http::StatusCode::UNAUTHORIZED, "Требуется вход").into_response() };
    let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let user: Option<String> = sqlx::query_scalar("SELECT user_id FROM sessions WHERE id_hash=? AND expires_at>datetime('now')").bind(hash).fetch_optional(&s.db).await.ok().flatten();
    let Some(_user) = user else { return (axum::http::StatusCode::UNAUTHORIZED, "Сессия истекла").into_response() };
    Json(serde_json::json!({"guilds":[]})).into_response()
}
async fn socket(ws: WebSocketUpgrade, State(s): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if oauth::session_cookie(&headers).is_none() { return (axum::http::StatusCode::UNAUTHORIZED, "Требуется вход").into_response(); }
    ws.on_upgrade(move |socket| stream_status(socket, s))
}
async fn stream_status(mut socket: WebSocket, s: AppState) {
    let mut rx=s.bot_ready.clone();
    loop { if rx.changed().await.is_err(){break;} let value=serde_json::json!({"bot_connected":*rx.borrow()}).to_string(); if socket.send(Message::Text(value.into())).await.is_err(){break;} }
}
pub fn router(state: AppState) -> Router { Router::new().route("/auth/login",get(oauth::login)).route("/auth/callback",get(oauth::callback)).route("/api/me",get(oauth::me)).route("/api/status",get(status)).route("/api/guilds",get(guilds)).route("/ws",get(socket)).with_state(state) }
```

### `src/bot.rs`

```rust
use serenity::{async_trait, model::gateway::Ready, prelude::*};
use tokio::sync::watch;
use tracing::{error, info};

struct Handler { ready: watch::Sender<bool> }
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        info!(user=%ready.user.name, "Discord Gateway подключён");
        let _ = self.ready.send(true);
    }
}
pub async fn run(token: String, ready: watch::Sender<bool>) {
    loop {
        let result = async {
            let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS | GatewayIntents::GUILD_MESSAGES;
            let mut client = Client::builder(&token, intents).event_handler(Handler { ready: ready.clone() }).await?;
            client.start().await
        }.await;
        let _ = ready.send(false);
        if let Err(e) = result { error!(error=%e, "Discord Gateway остановлен; повтор через пять секунд"); }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
```

### `src/main.rs`

```rust
mod api;
mod bot;
mod config;
mod db;
mod oauth;
mod state;

use anyhow::Result;
use config::Config;
use state::AppState;
use tokio::sync::watch;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))).init();
    let c = Config::from_env()?;
    let db = db::connect(&c.database_url).await?;
    let (ready_tx, ready_rx) = watch::channel(false);
    let state = AppState { db, http: reqwest::Client::builder().https_only(true).build()?, client_id:c.client_id, client_secret:c.client_secret, redirect_uri:c.redirect_uri, cookie_secure:c.cookie_secure, bot_ready:ready_rx };
    let bot_task=tokio::spawn(bot::run(c.token, ready_tx));
    let app=api::router(state).layer(TraceLayer::new_for_http());
    let listener=tokio::net::TcpListener::bind(&c.bind_addr).await?;
    tracing::info!(address=%c.bind_addr,"HTTP API запущен");
    axum::serve(listener,app).with_graceful_shutdown(async { let _=tokio::signal::ctrl_c().await; }).await?;
    bot_task.abort();
    Ok(())
}
```

## React frontend и стиль панели

### `frontend/package.json`

```json
{
  "name":"discord-bot-panel-ui",
  "private":true,
  "version":"0.1.0",
  "type":"module",
  "scripts":{"dev":"vite --host 127.0.0.1","build":"tsc -b && vite build","typecheck":"tsc --noEmit"},
  "dependencies":{"@vitejs/plugin-react":"4.3.4","lucide-react":"0.468.0","react":"18.3.1","react-dom":"18.3.1","recharts":"2.15.0"},
  "devDependencies":{"typescript":"5.7.2","vite":"6.0.5","tailwindcss":"3.4.16","postcss":"8.4.49","autoprefixer":"10.4.20"}
}
```

Для Tauri добавьте `@tauri-apps/api` в dependencies и `@tauri-apps/cli` в devDependencies. Зафиксируйте package lock и используйте `npm ci` для воспроизводимых сборок.

### `frontend/index.html`

```html
<!doctype html><html lang="ru"><head><meta charset="UTF-8"/><meta name="viewport" content="width=device-width,initial-scale=1.0"/><title>Discord Bot Panel</title></head><body><div id="root"></div><script type="module" src="/src/main.tsx"></script></body></html>
```

### `frontend/vite.config.ts`

```ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({ plugins: [react()], server: { host: '127.0.0.1', port: 5173 } });
```

### `frontend/tsconfig.json`

```json
{"compilerOptions":{"target":"ES2022","useDefineForClassFields":true,"lib":["ES2022","DOM","DOM.Iterable"],"allowJs":false,"skipLibCheck":true,"esModuleInterop":true,"allowImportingTsExtensions":true,"resolveJsonModule":true,"module":"ESNext","moduleResolution":"Bundler","strict":true,"jsx":"react-jsx","noEmit":true},"include":["src"]}
```

### `frontend/src/main.tsx`

Для настольного приложения импортируйте `Manager`. Для отдельного браузерного интерфейса импортируйте `App`.

```tsx
import React from 'react';
import { createRoot } from 'react-dom/client';
import Manager from './Manager';
import './style.css';
import './manager.css';
createRoot(document.getElementById('root')!).render(<React.StrictMode><Manager /></React.StrictMode>);
```

Веб-панель App содержит узкую боковую панель, статус Gateway, карточки метрик, график активности, журнал и состояние сервиса. Данные без backend-источника должны отображаться как тире, не фиктивными числами. Полный компонент веб-панели в прежнем объединённом файле был представлен как отсылка к отдельному документу, поэтому здесь этот компонент следует добавить из соответствующей версии.

## Настольное приложение Tauri

### `desktop/Cargo.toml`

```toml
[package]
name = "discord-bot-panel-desktop"
version = "0.1.0"
edition = "2021"
build = "build.rs"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
chrono = { version = "0.4", features = ["clock"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2"
tauri-plugin-shell = "2"
tokio = { version = "1", features = ["process", "time", "io-util", "sync"] }
```

### `desktop/build.rs`

```rust
fn main() { tauri_build::build() }
```

### `desktop/tauri.conf.json`

```json
{
  "$schema":"https://schema.tauri.app/config/2",
  "productName":"Discord Bot Panel",
  "version":"0.1.0",
  "identifier":"local.discord-bot-panel",
  "build":{"beforeDevCommand":"npm run dev --prefix ../frontend","devUrl":"http://127.0.0.1:5173","beforeBuildCommand":"npm run build --prefix ../frontend","frontendDist":"../frontend/dist"},
  "app":{"windows":[{"title":"Discord Bot Panel","width":1240,"height":820,"minWidth":920,"minHeight":640,"resizable":true,"center":true}],"security":{"csp":"default-src 'self'; connect-src ipc: http://ipc.localhost http://127.0.0.1:8787 ws://127.0.0.1:8787; img-src 'self' asset: http://asset.localhost data: https://cdn.discordapp.com; style-src 'self' 'unsafe-inline'; script-src 'self'"}},
  "bundle":{"active":true,"targets":["nsis","msi"],"shortDescription":"Локальная панель управления Discord-ботом","category":"Utility"}
}
```

### `desktop/capabilities/default.json`

```json
{
  "$schema":"../gen/schemas/desktop-schema.json",
  "identifier":"default",
  "description":"Разрешения локального окна панели",
  "windows":["main"],
  "permissions":["core:default","dialog:default","shell:allow-open"]
}
```

Tauri-команды должны обеспечивать установку зависимостей, сборку, старт и остановку backend, проверку API, чтение журнала и резервную копию. Предыдущий шаблон Tauri требует исправления: ресурсы backend не упакованы, конфигурация не переносится в каталог данных, открытие панели нужно реализовать корректным AppHandle, а закрытие окна должно завершать только дочерний процесс приложения. Не используйте шаблон как production-код без компиляции и тестирования.

### `frontend/src/Manager.tsx`

Управляющий интерфейс вызывает Rust-команды через `invoke` и отображает кнопки запуска, остановки, установки, резервного копирования и просмотра журнала. Результат показывается только после ответа backend.

```tsx
import { invoke } from '@tauri-apps/api/core';

type ActionResult = { ok: boolean; message: string };
async function runAction(command: string, setNotice: (text: string) => void) {
  try {
    const result = await invoke<ActionResult>(command);
    setNotice(result.message);
  } catch (error) {
    setNotice(`Не удалось выполнить действие. ${String(error)}`);
  }
}
```

### `frontend/src/manager.css`

```css
.manager{min-height:100vh;color:#f4f4fb;background:radial-gradient(ellipse at 75% 0%,#4d347333,transparent 44%),#0b0d15;font-family:'DM Sans',sans-serif}.mrail{position:fixed;inset:18px auto 18px 18px;width:66px;border:1px solid #ffffff14;border-radius:21px;background:#151824dd;backdrop-filter:blur(20px);display:flex;flex-direction:column;align-items:center;padding:12px 8px;gap:10px}.mlogo{width:43px;height:43px;border-radius:15px;background:linear-gradient(140deg,#9473ff,#5644bb);display:grid;place-items:center}.mrail button{width:44px;height:43px;border:0;border-radius:13px;background:transparent;color:#9a9db0;display:grid;place-items:center;cursor:pointer}.mrail button:hover{background:#9278ff25;color:white}.mspacer{flex:1}.muser{border-radius:50%;width:34px;height:34px;display:grid;place-items:center;background:#282b3a;color:#c4b6ff;font-size:11px}.mcontent{margin-left:105px;padding:35px clamp(20px,4vw,58px);max-width:1450px}.mhead{display:flex;justify-content:space-between;align-items:center;margin-bottom:24px}.mhead small,.hero small{font-size:10px;letter-spacing:1.6px;color:#8e92a9;font-weight:700}.mhead h1{font-size:27px;margin:5px 0 0;letter-spacing:-.6px}.statuspill{border:1px solid #ffffff14;background:#151824;border-radius:30px;padding:10px 14px;color:#b9bbca;font-size:11px;display:flex;align-items:center;gap:8px}.online,.offline{width:8px;height:8px;border-radius:50%;display:inline-block;background:#53d4a0;box-shadow:0 0 12px #53d4a066}.offline{background:#85899a;box-shadow:none}.hero,.tiles article,.actions,.logs{border:1px solid #ffffff14;background:linear-gradient(135deg,#191c29e8,#141722e8);border-radius:19px;backdrop-filter:blur(20px);box-shadow:0 15px 45px #0002}.hero{position:relative;overflow:hidden;padding:27px 30px;min-height:175px;background:radial-gradient(ellipse at 85% 30%,#7651db35,transparent 44%),linear-gradient(105deg,#1b1d2a,#151827)}.hero h2{font-size:25px;margin:10px 0 5px}.hero p,.actions p{font-size:12px;color:#898da2}.orb{position:absolute;right:9%;top:25px;width:105px;height:105px;border:1px solid #b19aff2a;border-radius:50%;display:grid;place-items:center;color:#baa7ff;background:#865de622}.heroStatus{position:absolute;bottom:17px;right:23px;color:#a3a6b8;font-size:11px;display:flex;align-items:center;gap:8px}.tiles{display:grid;grid-template-columns:repeat(3,1fr);gap:13px;margin-top:14px}.tiles article{padding:16px 18px;min-height:110px}.tiles article span{display:flex;align-items:center;gap:8px;color:#9ca0b2;font-size:11px}.tiles article span svg{width:16px;color:#a992ff}.tiles article b{display:block;font-size:14px;margin:10px 0 4px}.tiles article small{color:#777b90;font-size:10px}.actions,.logs{margin-top:14px;padding:20px}.actions h3,.logs h3{font-size:14px;margin:0 0 5px}.buttons{display:flex;flex-wrap:wrap;gap:9px;margin-top:17px}.buttons button,.logs button{height:39px;padding:0 13px;border:1px solid #ffffff16;border-radius:11px;background:#1c2030;color:#d9d9e4;font:inherit;font-size:11px;display:flex;align-items:center;gap:8px;cursor:pointer}.buttons button.primary{background:linear-gradient(110deg,#7657df,#5944bd);color:white}.buttons button:disabled{opacity:.45;cursor:not-allowed}.notice{margin-top:13px;color:#b9adf8;font-size:12px}.logs pre{max-height:260px;overflow:auto;background:#0b0d14;border-radius:10px;padding:12px;color:#b5b7c5;white-space:pre-wrap;font:11px/1.5 ui-monospace,monospace}footer{padding:18px 2px;color:#74788c;font-size:10px}@media(max-width:800px){.tiles{grid-template-columns:1fr}.mcontent{margin-left:85px;padding:24px 16px}}@media(max-width:520px){.mrail{left:7px;top:8px;bottom:8px;width:54px}.mcontent{margin-left:68px;padding:17px 10px}.mhead{align-items:flex-start;gap:10px;flex-direction:column}.buttons button{flex:1 1 145px;justify-content:center}}
```

## Подтверждение действий

Каждая операция с последствиями должна сначала показать пользователю точное действие и параметры. До подтверждения backend не меняет состояние. Кнопка "Отмена" закрывает диалог без побочных эффектов. После подтверждения приложение выполняет описанное действие и сообщает результат только после ответа backend.

### `frontend/src/components/ConfirmAction.tsx`

```tsx
import { AlertTriangle, X } from 'lucide-react';

type Props = {
  title: string;
  details: string[];
  warning?: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export default function ConfirmAction({ title, details, warning, busy = false, onConfirm, onCancel }: Props) {
  return <div className="confirm-backdrop" role="presentation" onMouseDown={e => { if (e.target === e.currentTarget && !busy) onCancel(); }}>
    <section className="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
      <button className="confirm-close" aria-label="Отмена" disabled={busy} onClick={onCancel}><X size={18}/></button>
      <div className="confirm-icon"><AlertTriangle size={21}/></div>
      <p className="confirm-kicker">ПОДТВЕРЖДЕНИЕ ДЕЙСТВИЯ</p>
      <h2 id="confirm-title">{title}</h2>
      <div className="confirm-details">{details.map((line, i) => <p key={i}>{line}</p>)}</div>
      {warning && <div className="confirm-warning">{warning}</div>}
      <div className="confirm-buttons">
        <button className="confirm-cancel" disabled={busy} onClick={onCancel}>Отмена</button>
        <button className="confirm-accept" disabled={busy} onClick={onConfirm}>{busy ? 'Выполняется…' : 'Продолжить'}</button>
      </div>
    </section>
  </div>;
}
```

Пример вызова для запуска:

```tsx
askAction({
  command: 'start_app',
  title: 'Запустить Discord Bot Panel?',
  details: [
    'Будет запущен локальный backend и подключение Discord Gateway.',
    'Локальный API использует адрес 127.0.0.1:8787.',
    'Конфигурация читается из локального .env.'
  ],
  warning: 'Если настройки не заполнены, запуск завершится ошибкой. Настройки Windows не изменяются.'
});
```

Стили диалога:

```css
.confirm-backdrop{position:fixed;inset:0;z-index:50;display:grid;place-items:center;padding:18px;background:#05060bcc;backdrop-filter:blur(9px)}
.confirm-dialog{position:relative;width:min(480px,100%);padding:25px;border:1px solid #ffffff20;border-radius:20px;background:linear-gradient(145deg,#1d2030,#131622);box-shadow:0 25px 90px #0009;animation:confirm-in .18s ease-out}
.confirm-close{position:absolute;right:14px;top:14px;width:34px;height:34px;border:0;border-radius:10px;background:#ffffff0a;color:#aeb1c2;display:grid;place-items:center;cursor:pointer}
.confirm-icon{width:42px;height:42px;display:grid;place-items:center;border-radius:13px;color:#c2adff;background:#9a7bff20;margin-bottom:16px}
.confirm-kicker{font-size:9px;letter-spacing:1.4px;color:#969bb1;font-weight:700}
.confirm-dialog h2{margin:7px 35px 14px 0;font-size:19px;color:#f4f3fc}
.confirm-details p{margin:7px 0;color:#b2b5c5;font-size:12px;line-height:1.55}
.confirm-warning{margin-top:14px;padding:11px 12px;border:1px solid #e7ae4630;border-radius:11px;background:#e7ae460c;color:#e7c982;font-size:11px;line-height:1.5}
.confirm-buttons{display:flex;justify-content:flex-end;gap:9px;margin-top:22px}
.confirm-buttons button{height:38px;padding:0 15px;border:1px solid #ffffff1a;border-radius:10px;background:#202334;color:#d5d6e1;font:inherit;font-size:11px;cursor:pointer}
.confirm-buttons .confirm-accept{border-color:#9478ff70;background:linear-gradient(110deg,#7759df,#5945ba);color:white}
.confirm-buttons button:disabled{opacity:.55;cursor:wait}
@keyframes confirm-in{from{opacity:0;transform:translateY(7px) scale(.99)}to{opacity:1;transform:translateY(0) scale(1)}}
```

Показывайте реальные пути, настройки и последствия перед установкой, импортом профиля, запуском службы, остановкой и резервным копированием. Не показывайте секретные ключи. Для установки службы или изменения системной сети требуется отдельное подтверждение Windows. Не обходите UAC и не запускайте неизвестные скрипты молча.

## Единый BAT-файл

Сохраните как `Start all in one.bat` в корне проекта. Меню запускает выбранный пункт после ввода цифры.

```bat
@echo off
chcp 65001 >nul
setlocal EnableExtensions
cd /d "%~dp0"
if not exist "logs" mkdir "logs"
if not exist "data" mkdir "data"
if not exist "backups" mkdir "backups"

:menu
cls
echo ===========================================
echo       Discord Bot Panel - управление
echo ===========================================
echo 1. Установить зависимости
echo 2. Собрать приложение
echo 3. Запустить приложение
echo 4. Остановить приложение
echo 5. Проверить состояние
echo 6. Открыть приложение
echo 7. Просмотреть журнал
echo 8. Создать резервную копию
echo 9. Выход
echo.
set "choice="
set /p "choice=Введите номер и нажмите Enter: "
if "%choice%"=="1" goto install
if "%choice%"=="2" goto build
if "%choice%"=="3" goto run
if "%choice%"=="4" goto stop
if "%choice%"=="5" goto health
if "%choice%"=="6" goto open
if "%choice%"=="7" goto logs
if "%choice%"=="8" goto backup
if "%choice%"=="9" exit /b 0
echo Неверный номер.
pause
goto menu

:install
where cargo >nul 2>nul
if errorlevel 1 (
 echo Rust не найден. Открываю официальный сайт установки.
 start "" "https://www.rust-lang.org/tools/install"
 pause
 goto menu
)
where node >nul 2>nul
if errorlevel 1 (
 echo Node.js не найден. Открываю официальный сайт.
 start "" "https://nodejs.org/"
 pause
 goto menu
)
where npm >nul 2>nul
if errorlevel 1 (echo npm не найден.& pause & goto menu)
if not exist "frontend\package.json" (echo Не найден frontend\package.json.& pause & goto menu)
if not exist ".env" if exist ".env.example" copy /Y ".env.example" ".env" >nul
if exist "Cargo.toml" cargo fetch
if errorlevel 1 goto error
if exist "desktop\Cargo.toml" (
 pushd desktop
 cargo fetch
 if errorlevel 1 (popd & goto error)
 popd
)
pushd frontend
call npm install
if errorlevel 1 (popd & goto error)
popd
echo Зависимости установлены.
pause
goto menu

:build
pushd frontend
call npm run typecheck
if errorlevel 1 (popd & goto error)
call npm run build
if errorlevel 1 (popd & goto error)
popd
if exist "Cargo.toml" (
 cargo fmt --check
 if errorlevel 1 goto error
 cargo check
 if errorlevel 1 goto error
 cargo test
 if errorlevel 1 goto error
 cargo clippy --all-targets -- -D warnings
 if errorlevel 1 goto error
 cargo build --release
 if errorlevel 1 goto error
)
if exist "desktop\Cargo.toml" (
 pushd desktop
 cargo tauri build
 if errorlevel 1 (popd & goto error)
 popd
) else (echo Папка desktop отсутствует. Tauri-сборка пропущена.)
echo Сборка завершена.
pause
goto menu

:run
if not exist ".env" (
 if exist ".env.example" copy /Y ".env.example" ".env" >nul
 echo Проверьте и заполните .env перед запуском.
 start "" notepad.exe "%CD%\.env"
 pause
 goto menu
)
set "APP_EXE="
if exist "desktop\target\release\discord-bot-panel-desktop.exe" set "APP_EXE=desktop\target\release\discord-bot-panel-desktop.exe"
if exist "desktop\src-tauri\target\release\discord-bot-panel.exe" set "APP_EXE=desktop\src-tauri\target\release\discord-bot-panel.exe"
if exist "src-tauri\target\release\discord-bot-panel.exe" set "APP_EXE=src-tauri\target\release\discord-bot-panel.exe"
if not defined APP_EXE (echo Приложение не собрано. Выберите пункт 2.& pause & goto menu)
start "" "%CD%\%APP_EXE%"
echo Запуск передан Windows.
goto menu

:stop
powershell -NoProfile -ExecutionPolicy Bypass -Command "$root=(Resolve-Path '.').Path; $procs=Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root,[System.StringComparison]::OrdinalIgnoreCase) -and $_.Name -like '*discord-bot-panel*' }; if($procs){$procs | ForEach-Object { Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }; 'Команда остановки отправлена.'} else {'Процесс панели не найден.'}"
pause
goto menu

:health
powershell -NoProfile -Command "try {$r=Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:8787/api/status' -TimeoutSec 3; $r.Content} catch {'API не отвечает на 127.0.0.1:8787.'}"
pause
goto menu

:open
start "" "http://127.0.0.1:8787"
goto menu

:logs
if exist "logs\panel.log" (start "" notepad.exe "%CD%\logs\panel.log") else (echo Журнал пока не создан.& pause)
goto menu

:backup
if not exist "data\panel.db" (echo База данных пока не создана.& pause & goto menu)
for /f %%T in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd_HHmmss"') do set "STAMP=%%T"
copy /Y "data\panel.db" "backups\panel_%STAMP%.db" >nul
if errorlevel 1 goto error
echo Резервная копия создана.
pause
goto menu

:error
echo Операция завершилась ошибкой. Проверьте сообщения выше.
pause
goto menu
```

Пути к EXE, Tauri config и frontend следует привести к фактической структуре проекта. BAT-файл не был выполнен на Windows и не устанавливает Rust, Node.js или WebView2 автоматически.

## Установка и проверки

Требуются Windows 10/11 x64, Rust stable, Node.js LTS, npm и компоненты для сборки Tauri. В Discord Developer Portal создайте приложение и бота, настройте OAuth2 Redirect URI `http://127.0.0.1:8787/auth/callback`, задайте секреты в `.env` и пригласите бота на сервер с минимально необходимыми разрешениями.

В корне Rust-проекта выполните:

```text
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

В frontend выполните:

```text
npm run typecheck
npm run build
```

Затем соберите установщик Tauri и проверьте его на чистой Windows 10/11. Эти проверки пока не выполнялись.

## Ограничения и обязательные доработки

Материалы не являются полной реализацией исходного ТЗ. Discord slash-команды и модерация не реализованы. API списка серверов возвращает пустой список. Нет реальной статистики, полного аудита, управления приветствиями, настройками команд или ролями. OAuth flow получает профиль, но не обеспечивает безопасное хранение токена для списка guilds. WebSocket и защищённые маршруты нуждаются в полной проверке сессии и прав. Не завершены CSRF-защита, rate limiting, строгая CORS-политика, операции Discord API и тестовый набор.

Tauri-шаблон требует упаковки backend и frontend в ресурсы установщика, переноса `.env` и базы в app data, корректного завершения дочернего процесса и проверки путей. Компонент подтверждения нужно подключить ко всем операциям и проверить, что отмена не вызывает побочных эффектов. Не запускайте неизвестные скрипты и не меняйте системные сетевые параметры без прозрачного отдельного подтверждения и проверки происхождения файлов.

Не заявляйте, что приложение собрано, протестировано или соответствует критериям готовности до фактического выполнения проверок.
