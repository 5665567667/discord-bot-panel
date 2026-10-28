use crate::{auth::{self, DiscordUser, TokenReply, UserView}, crypto, state::AppState};
use axum::{extract::{Query, State}, http::{HeaderMap, HeaderValue, StatusCode, header}, response::{IntoResponse, Redirect}, Json};
use serde::Deserialize;
use sqlx::Row;

#[derive(Deserialize)] pub struct OAuthCallback { code: String, state: String, error: Option<String> }
fn set_cookie(headers: &mut HeaderMap, value: String) { if let Ok(v) = HeaderValue::from_str(&value) { headers.append(header::SET_COOKIE, v); } }
fn cookie(name: &str, value: &str, max_age: i64, http_only: bool, secure: bool) -> String {
    format!("{name}={value}; Path=/; SameSite=Lax; Max-Age={max_age}{}{}", if http_only { "; HttpOnly" } else { "" }, if secure { "; Secure" } else { "" })
}
pub async fn login(State(state): State<AppState>) -> impl IntoResponse {
    let raw = auth::random_token(48);
    let exp = chrono::Utc::now().timestamp() + 600;
    if sqlx::query("INSERT INTO oauth_states(state_hash,expires_at) VALUES(?,?)").bind(auth::digest(&raw)).bind(exp).execute(&state.db).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Не удалось создать OAuth-состояние").into_response();
    }
    let url = format!("https://discord.com/oauth2/authorize?client_id={}&response_type=code&redirect_uri={}&scope=identify%20guilds&state={}", urlencoding::encode(&state.config.client_id), urlencoding::encode(&state.config.redirect_uri), raw);
    let mut headers = HeaderMap::new();
    set_cookie(&mut headers, cookie("oauth_state", &raw, 600, true, state.config.cookie_secure));
    (headers, Redirect::temporary(&url)).into_response()
}
pub async fn callback(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<OAuthCallback>) -> impl IntoResponse {
    if query.error.is_some() { return (StatusCode::BAD_REQUEST, "Авторизация Discord отменена").into_response(); }
    let Some(cookie_state) = auth::cookie_value(&headers, "oauth_state") else { return (StatusCode::BAD_REQUEST, "Отсутствует OAuth state cookie").into_response(); };
    if !bool::from(cookie_state.as_bytes().ct_eq(query.state.as_bytes())) { return (StatusCode::BAD_REQUEST, "Проверка OAuth state не пройдена").into_response(); }
    let state_hash = auth::digest(&query.state);
    let now = chrono::Utc::now().timestamp();
    let found: Option<i64> = sqlx::query_scalar("SELECT expires_at FROM oauth_states WHERE state_hash=?").bind(&state_hash).fetch_optional(&state.db).await.ok().flatten();
    if found.is_none_or(|expires| expires < now) { return (StatusCode::BAD_REQUEST, "OAuth state истёк или уже использован").into_response(); }
    let _ = sqlx::query("DELETE FROM oauth_states WHERE state_hash=?").bind(state_hash).execute(&state.db).await;

    let form = [("client_id", state.config.client_id.as_str()), ("client_secret", state.config.client_secret.as_str()), ("grant_type", "authorization_code"), ("code", query.code.as_str()), ("redirect_uri", state.config.redirect_uri.as_str())];
    let response = match state.http.post("https://discord.com/api/v10/oauth2/token").form(&form).send().await {
        Ok(r) => match r.error_for_status() { Ok(r) => r, Err(_) => return (StatusCode::BAD_GATEWAY, "Discord отклонил код авторизации").into_response() },
        Err(_) => return (StatusCode::BAD_GATEWAY, "Не удалось связаться с Discord").into_response(),
    };
    let token: TokenReply = match response.json().await { Ok(t) => t, Err(_) => return (StatusCode::BAD_GATEWAY, "Ответ OAuth Discord некорректен").into_response() };
    let user_response = match state.http.get("https://discord.com/api/v10/users/@me").bearer_auth(&token.access_token).send().await {
        Ok(r) => match r.error_for_status() { Ok(r) => r, Err(_) => return (StatusCode::BAD_GATEWAY, "Не удалось получить профиль Discord").into_response() },
        Err(_) => return (StatusCode::BAD_GATEWAY, "Не удалось связаться с Discord").into_response(),
    };
    let user: DiscordUser = match user_response.json().await { Ok(u) => u, Err(_) => return (StatusCode::BAD_GATEWAY, "Профиль Discord имеет неожиданный формат").into_response() };
    let now = chrono::Utc::now().timestamp();
    if sqlx::query("INSERT INTO users(discord_id,username,avatar,updated_at) VALUES(?,?,?,?) ON CONFLICT(discord_id) DO UPDATE SET username=excluded.username,avatar=excluded.avatar,updated_at=excluded.updated_at")
        .bind(&user.id).bind(&user.username).bind(&user.avatar).bind(now).execute(&state.db).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Не удалось сохранить профиль пользователя").into_response();
    }
    let raw_session = auth::random_token(64);
    let raw_csrf = auth::random_token(48);
    let access_enc = match crypto::seal(&state.config.encryption_key, &token.access_token) { Ok(v) => v, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Не удалось защитить OAuth-токен").into_response() };
    let refresh_enc = match crypto::seal(&state.config.encryption_key, &token.refresh_token) { Ok(v) => v, Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Не удалось защитить OAuth-токен").into_response() };
    let session_expiry = now + 7 * 24 * 60 * 60;
    if sqlx::query("INSERT INTO sessions(token_hash,user_id,access_token_enc,refresh_token_enc,access_expires_at,session_expires_at,created_at) VALUES(?,?,?,?,?,?,?)")
        .bind(auth::digest(&raw_session)).bind(&user.id).bind(access_enc).bind(refresh_enc).bind(now + token.expires_in).bind(session_expiry).bind(now).execute(&state.db).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Не удалось создать сессию").into_response();
    }
    let mut response_headers = HeaderMap::new();
    set_cookie(&mut response_headers, cookie("session", &raw_session, 7 * 24 * 60 * 60, true, state.config.cookie_secure));
    set_cookie(&mut response_headers, cookie("csrf", &raw_csrf, 7 * 24 * 60 * 60, false, state.config.cookie_secure));
    set_cookie(&mut response_headers, cookie("oauth_state", "", 0, true, state.config.cookie_secure));
    (response_headers, Redirect::to("/")).into_response()
}
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match auth::load_session(&state, &headers).await {
        Ok(Some(session)) => Json(session.user).into_response(),
        Ok(None) => (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"Требуется вход через Discord"}))).into_response(),
        Err(error) => { tracing::warn!(error=%error, "Session validation failed"); (StatusCode::UNAUTHORIZED, "Сессия недействительна. Войдите снова.").into_response() }
    }
}
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !auth::csrf_matches(&headers) { return (StatusCode::FORBIDDEN, "CSRF-проверка не пройдена").into_response(); }
    if let Some(raw) = auth::cookie_value(&headers, "session") { let _ = sqlx::query("DELETE FROM sessions WHERE token_hash=?").bind(auth::digest(&raw)).execute(&state.db).await; }
    let mut response_headers = HeaderMap::new();
    set_cookie(&mut response_headers, cookie("session", "", 0, true, state.config.cookie_secure));
    set_cookie(&mut response_headers, cookie("csrf", "", 0, false, state.config.cookie_secure));
    (response_headers, Json(serde_json::json!({"ok":true}))).into_response()
}
