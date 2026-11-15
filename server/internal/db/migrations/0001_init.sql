-- Game Save: initial schema.

CREATE TABLE IF NOT EXISTS users (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    username     TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL DEFAULT '',
    disabled     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT    NOT NULL
);

-- One row per API key. Keys are stored only as a SHA-256 hash; the plaintext
-- is shown once at creation time and can never be recovered.
CREATE TABLE IF NOT EXISTS api_keys (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label        TEXT    NOT NULL DEFAULT '',
    key_prefix   TEXT    NOT NULL,
    key_hash     TEXT    NOT NULL UNIQUE,
    created_at   TEXT    NOT NULL,
    last_used_at TEXT,
    revoked_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);

-- A game is one sync namespace: the cloud counterpart of a game's save folder.
CREATE TABLE IF NOT EXISTS games (
    id         TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    slug       TEXT    NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT    NOT NULL,
    updated_at TEXT    NOT NULL,
    UNIQUE(user_id, slug)
);
CREATE INDEX IF NOT EXISTS idx_games_user ON games(user_id);

-- Current state of every file in a game namespace.
CREATE TABLE IF NOT EXISTS game_files (
    game_id    TEXT    NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    path       TEXT    NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT    NOT NULL,
    mtime_ns   INTEGER NOT NULL DEFAULT 0,
    version    INTEGER NOT NULL,
    device     TEXT    NOT NULL DEFAULT '',
    updated_at TEXT    NOT NULL,
    PRIMARY KEY (game_id, path)
);
CREATE INDEX IF NOT EXISTS idx_game_files_game ON game_files(game_id);

-- Full history, so a conflicting upload is never lost.
CREATE TABLE IF NOT EXISTS game_file_versions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id    TEXT    NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    path       TEXT    NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT    NOT NULL,
    version    INTEGER NOT NULL,
    device     TEXT    NOT NULL DEFAULT '',
    created_at TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_gfv_game_path ON game_file_versions(game_id, path, version DESC);

-- Content addressed blob store index. Blobs are never removed automatically;
-- 'gamesave gc' collects the ones no longer referenced.
CREATE TABLE IF NOT EXISTS blobs (
    sha256     TEXT    PRIMARY KEY,
    size       INTEGER NOT NULL,
    created_at TEXT    NOT NULL
);
