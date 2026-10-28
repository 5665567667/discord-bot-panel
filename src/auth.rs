use crate::{crypto, state::AppState};
use axum::http::{HeaderMap, header};
use rand::{Rng, distributions::Alphanumeric};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use subtle::ConstantTimeEq;

#[derive(Clone, Serialize)]
pub struct UserView { pub id: String, pub username: String, pub avatar: Option<String> }
#[derive(Clone)]
pub struct Session { pub user: UserView, pub access_token: String, pub refresh_token: String, pub access_expires_at: i64 }
#[derive(Deserialize)]
pub struct DiscordUser { pub id: String, pub username: String, pub avatar: Option<String> }
#[derive(Deserialize)]
pub struct TokenReply { pub access_token: String, pub refresh_token: String, pub expires_in: i64, pub token_type: String }

pub fn random_token(size: usize) -> String { rand::thread_rng().sample_iter(&Alphanumeric).take(size).map(char::from).collect() }
pub fn digest(value: &str) -> String { format!("{:x}", Sha256::digest(value.as_bytes())) }
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then(|| value.to_owned())
    })
}
pub fn csrf_matches(headers: &HeaderMap) -> bool {
    let Some(cookie) = cookie_value(headers, "csrf") else { return false; };
    let Some(header) = headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) else { return false; };
    cookie.as_bytes().ct_eq(header.as_bytes()).into()
}

pub async fn load_session(state: &AppState, headers: &HeaderMap) -> anyhow::Result<Option<Session>> {
    let Some(raw) = cookie_value(headers, "session") else { return Ok(None); };
    let hash = digest(&raw);
    let row = sqlx::query("SELECT s.user_id,s.access_token_enc,s.refresh_token_enc,s.access_expires_at,s.session_expires_at,u.username,u.avatar FROM sessions s JOIN users u ON u.discord_id=s.user_id WHERE s.token_hash=?")
        .bind(hash).fetch_optional(&state.db).await?;
    let Some(row) = row else { return Ok(None); };
    let session_expiry: i64 = row.try_get("session_expires_at")?;
    if session_expiry <= chrono::Utc::now().timestamp() { return Ok(None); }
    let user = UserView { id: row.try_get("user_id")?, username: row.try_get("username")?, avatar: row.try_get("avatar")? };
    let mut access: String = crypto::open(&state.config.encryption_key, &row.try_get::<String, _>("access_token_enc")?)?;
    let mut refresh: String = crypto::open(&state.config.encryption_key, &row.try_get::<String, _>("refresh_token_enc")?)?;
    let mut access_expiry: i64 = row.try_get("access_expires_at")?;
    if access_expiry <= chrono::Utc::now().timestamp() + 45 {
        let response = state.http.post("https://discord.com/api/v10/oauth2/token").form(&[
            ("client_id", state.config.client_id.as_str()),
            ("client_secret", state.config.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ]).send().await?.error_for_status()?;
        let token: TokenReply = response.json().await?;
        access = token.access_token;
        refresh = token.refresh_token;
        access_expiry = chrono::Utc::now().timestamp() + token.expires_in;
        let access_enc = crypto::seal(&state.config.encryption_key, &access)?;
        let refresh_enc = crypto::seal(&state.config.encryption_key, &refresh)?;
        sqlx::query("UPDATE sessions SET access_token_enc=?,refresh_token_enc=?,access_expires_at=? WHERE token_hash=?")
            .bind(access_enc).bind(refresh_enc).bind(access_expiry).bind(digest(&raw)).execute(&state.db).await?;
    }
    Ok(Some(Session { user, access_token: access, refresh_token: refresh, access_expires_at: access_expiry }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_cookie_parser_handles_other_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "theme=dark; session=abc123; csrf=xyz".parse().unwrap());
        assert_eq!(cookie_value(&headers, "session").as_deref(), Some("abc123"));
        assert_eq!(cookie_value(&headers, "missing"), None);
    }
    #[test]
    fn csrf_requires_matching_cookie_and_header() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "csrf=token".parse().unwrap());
        headers.insert("x-csrf-token", "token".parse().unwrap());
        assert!(csrf_matches(&headers));
        headers.insert("x-csrf-token", "wrong".parse().unwrap());
        assert!(!csrf_matches(&headers));
    }
}
