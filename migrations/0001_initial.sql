PRAGMA foreign_keys = ON;

CREATE TABLE users (
    discord_id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    avatar TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE oauth_states (
    state_hash TEXT PRIMARY KEY,
    expires_at INTEGER NOT NULL
);
CREATE INDEX oauth_states_expiry ON oauth_states(expires_at);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(discord_id) ON DELETE CASCADE,
    access_token_enc TEXT NOT NULL,
    refresh_token_enc TEXT NOT NULL,
    access_expires_at INTEGER NOT NULL,
    session_expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX sessions_user_id ON sessions(user_id);
CREATE INDEX sessions_expiry ON sessions(session_expires_at);

CREATE TABLE guild_settings (
    guild_id TEXT PRIMARY KEY,
    welcome_channel_id TEXT,
    welcome_message TEXT NOT NULL DEFAULT 'Добро пожаловать, {user}! ',
    mod_log_channel_id TEXT,
    updated_at INTEGER NOT NULL
);

CREATE TABLE warnings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    moderator_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX warnings_target ON warnings(guild_id, target_id, created_at DESC);

CREATE TABLE action_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    target_id TEXT,
    details TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);
CREATE INDEX action_log_guild ON action_log(guild_id, created_at DESC);

CREATE TABLE command_settings (
    guild_id TEXT NOT NULL,
    command TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    PRIMARY KEY (guild_id, command)
);

CREATE TABLE automation_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    config TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE punishments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    moderator_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
