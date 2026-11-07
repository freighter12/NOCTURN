//! SQLite persistence: settings, the change journal (transactions + changes),
//! the audit log and AI conversations.
//!
//! The journal is write-ahead: a change row containing the *original* value is
//! committed before the system is modified, so a crash mid-apply never loses
//! the information needed to roll back.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::{AppError, AppResult};

const MIGRATIONS: &[&str] = &[
    // v1 — initial schema
    r#"
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE transactions (
        id            TEXT PRIMARY KEY,
        created_at    TEXT NOT NULL,
        finished_at   TEXT,
        source        TEXT NOT NULL,
        label         TEXT NOT NULL,
        status        TEXT NOT NULL,
        restore_point TEXT,
        backup_path   TEXT
    );

    CREATE TABLE changes (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        tx_id          TEXT NOT NULL REFERENCES transactions(id),
        tweak_id       TEXT NOT NULL,
        target         TEXT NOT NULL,
        op_json        TEXT NOT NULL,
        original_json  TEXT,
        status         TEXT NOT NULL,
        error          TEXT,
        applied_at     TEXT,
        rolled_back_at TEXT
    );
    CREATE INDEX idx_changes_tx ON changes(tx_id);

    CREATE TABLE audit_log (
        id     INTEGER PRIMARY KEY AUTOINCREMENT,
        ts     TEXT NOT NULL,
        level  TEXT NOT NULL,
        actor  TEXT NOT NULL,
        event  TEXT NOT NULL,
        detail TEXT
    );
    CREATE INDEX idx_audit_ts ON audit_log(ts);

    CREATE TABLE conversations (
        id         TEXT PRIMARY KEY,
        title      TEXT NOT NULL,
        provider   TEXT NOT NULL,
        model      TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE messages (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        role            TEXT NOT NULL,
        content_json    TEXT NOT NULL,
        created_at      TEXT NOT NULL
    );
    CREATE INDEX idx_messages_conv ON messages(conversation_id);
    "#,
    // 2: per-app process rules (priority / affinity applied at launch).
    r#"
    CREATE TABLE process_rules (
        name            TEXT PRIMARY KEY COLLATE NOCASE,
        priority        TEXT,
        affinity        TEXT,
        enabled         INTEGER NOT NULL DEFAULT 1,
        note            TEXT NOT NULL DEFAULT '',
        created_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL,
        applied_count   INTEGER NOT NULL DEFAULT 0,
        last_applied_at TEXT,
        last_error      TEXT
    );
    "#,
    // 3: per-minute hardware history (kept 7 days).
    r#"
    CREATE TABLE telemetry_minute (
        ts          INTEGER PRIMARY KEY,
        cpu         REAL NOT NULL,
        cpu_max_core REAL NOT NULL,
        mem_pct     REAL NOT NULL,
        gpu         REAL,
        gpu_temp    REAL,
        gpu_power   REAL,
        disk_read   INTEGER NOT NULL,
        disk_write  INTEGER NOT NULL,
        net_rx      INTEGER NOT NULL,
        net_tx      INTEGER NOT NULL
    );
    "#,
    // 4: gaming — per-game profiles, games added by hand, play sessions.
    r#"
    CREATE TABLE game_profiles (
        game_id    TEXT PRIMARY KEY,
        json       TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE custom_games (
        exe      TEXT PRIMARY KEY COLLATE NOCASE,
        added_at TEXT NOT NULL
    );
    CREATE TABLE game_sessions (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        game_id       TEXT NOT NULL,
        game_name     TEXT NOT NULL,
        exe           TEXT NOT NULL,
        started_at    INTEGER NOT NULL,
        ended_at      INTEGER NOT NULL,
        samples       INTEGER NOT NULL,
        avg_cpu       REAL NOT NULL,
        avg_max_core  REAL NOT NULL,
        avg_gpu       REAL,
        peak_gpu_temp REAL,
        avg_mem_pct   REAL NOT NULL,
        game_mem_peak INTEGER NOT NULL,
        gpu_bound_pct REAL NOT NULL,
        cpu_bound_pct REAL NOT NULL,
        profile_json  TEXT
    );
    CREATE INDEX idx_sessions_game ON game_sessions(game_id, started_at);
    "#,
    // 5: network stability test results (one row per ping target per test).
    r#"
    CREATE TABLE network_tests (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        tested_at TEXT NOT NULL,
        label     TEXT NOT NULL,
        target    TEXT NOT NULL,
        avg_ms    REAL,
        jitter_ms REAL,
        loss_pct  REAL NOT NULL
    );
    CREATE INDEX idx_network_tests_at ON network_tests(tested_at);
    "#,
    // 6: benchmark runs (one row per run; metrics and context as JSON).
    r#"
    CREATE TABLE bench_runs (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        started_at   TEXT NOT NULL,
        suite        TEXT NOT NULL,
        target       TEXT,
        label        TEXT NOT NULL,
        duration_ms  INTEGER NOT NULL,
        metrics_json TEXT NOT NULL,
        context_json TEXT NOT NULL
    );
    CREATE INDEX idx_bench_runs_suite ON bench_runs(suite, started_at);
    "#,
    // 7: scheduled maintenance history.
    r#"
    CREATE TABLE maintenance_runs (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        task        TEXT NOT NULL,
        started_at  TEXT NOT NULL,
        finished_at TEXT NOT NULL,
        status      TEXT NOT NULL,
        trigger     TEXT NOT NULL,
        summary     TEXT NOT NULL
    );
    CREATE INDEX idx_maintenance_task ON maintenance_runs(task, started_at);
    "#,
    // 8: App Library (installed apps, connected services, websites).
    r#"
    CREATE TABLE library_items (
        id             TEXT PRIMARY KEY,
        kind           TEXT NOT NULL,
        name           TEXT NOT NULL,
        target         TEXT NOT NULL,
        category       TEXT NOT NULL DEFAULT '',
        favourite      INTEGER NOT NULL DEFAULT 0,
        pinned         INTEGER NOT NULL DEFAULT 0,
        icon_json      TEXT NOT NULL DEFAULT '{}',
        added_at       TEXT NOT NULL,
        last_opened_at TEXT,
        open_count     INTEGER NOT NULL DEFAULT 0
    );
    "#,
    // 9: Nocturn Music (local catalogue source, playlists, likes, plays).
    r#"
    CREATE TABLE music_folders (
        path     TEXT PRIMARY KEY,
        added_at TEXT NOT NULL
    );
    CREATE TABLE music_tracks (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        source       TEXT NOT NULL DEFAULT 'local',
        path         TEXT NOT NULL UNIQUE,
        folder       TEXT NOT NULL,
        title        TEXT NOT NULL,
        artist       TEXT NOT NULL,
        album        TEXT NOT NULL,
        album_artist TEXT NOT NULL,
        track_no     INTEGER,
        disc_no      INTEGER,
        year         INTEGER,
        genre        TEXT NOT NULL DEFAULT '',
        duration_ms  INTEGER NOT NULL,
        has_art      INTEGER NOT NULL DEFAULT 0,
        size         INTEGER NOT NULL,
        mtime        INTEGER NOT NULL,
        added_at     TEXT NOT NULL
    );
    CREATE INDEX idx_music_tracks_album ON music_tracks(album_artist, album);
    CREATE TABLE music_likes (
        track_id INTEGER PRIMARY KEY,
        liked_at TEXT NOT NULL
    );
    CREATE TABLE music_plays (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        track_id  INTEGER NOT NULL,
        played_at TEXT NOT NULL,
        ms_played INTEGER NOT NULL
    );
    CREATE INDEX idx_music_plays_at ON music_plays(played_at);
    CREATE TABLE music_playlists (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        cover_json  TEXT NOT NULL DEFAULT '{}',
        created_at  TEXT NOT NULL,
        updated_at  TEXT NOT NULL
    );
    CREATE TABLE music_playlist_tracks (
        playlist_id TEXT NOT NULL,
        position    INTEGER NOT NULL,
        track_id    INTEGER NOT NULL,
        PRIMARY KEY (playlist_id, position)
    );
    "#,
    // Playlist imports: the original list from another service or a pasted list,
    // kept so songs missing from the library can be matched later.
    r#"
    CREATE TABLE music_playlist_imports (
        playlist_id TEXT PRIMARY KEY,
        source      TEXT NOT NULL,
        url         TEXT,
        items_json  TEXT NOT NULL,
        imported_at TEXT NOT NULL
    );
    "#,
    // Lyrics found online (LRCLIB), and "not found" answers, so each song is asked once.
    r#"
    CREATE TABLE music_lyrics_cache (
        key          TEXT PRIMARY KEY,
        found        INTEGER NOT NULL,
        synced       TEXT,
        plain        TEXT,
        instrumental INTEGER NOT NULL DEFAULT 0,
        fetched_at   TEXT NOT NULL
    );
    "#,
];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// In-memory database, used by unit tests.
    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;",
        )?;
        migrate(&conn)?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        // A poisoned mutex only means another thread panicked while holding the
        // lock; SQLite itself is still consistent, so keep going.
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---- settings -------------------------------------------------------

    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> AppResult<Option<T>> {
        let raw: Option<String> = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        match raw {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> AppResult<()> {
        let s = serde_json::to_string(value)?;
        self.conn().execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, s],
        )?;
        Ok(())
    }

    // ---- audit ----------------------------------------------------------

    pub fn audit(&self, level: &str, actor: &str, event: &str, detail: Option<serde_json::Value>) {
        let detail = detail.map(|d| d.to_string());
        let res = self.conn().execute(
            "INSERT INTO audit_log(ts, level, actor, event, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), level, actor, event, detail],
        );
        if let Err(e) = res {
            tracing::error!("failed to write audit log: {e}");
        }
        tracing::info!(target: "audit", actor, event, "{level}");
    }
}

fn migrate(conn: &Connection) -> AppResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i64;
        if version < target {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {target}; COMMIT;"))
                .map_err(|e| AppError::msg(format!("database migration {target} failed: {e}")))?;
        }
    }
    Ok(())
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_settings_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting::<String>("x").unwrap(), None);
        db.set_setting("x", &"hello").unwrap();
        db.set_setting("x", &"world").unwrap();
        assert_eq!(db.get_setting::<String>("x").unwrap().as_deref(), Some("world"));
        let v: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
    }
}
