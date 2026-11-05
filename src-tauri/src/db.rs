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
