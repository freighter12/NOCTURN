//! Gaming optimiser: game detection, per-game profiles and session monitoring.

pub mod detect;
pub mod profile;
pub mod session;

use std::sync::{Mutex, RwLock};

use rusqlite::params;

use crate::db::{now, Db};
use crate::error::AppResult;
use crate::system::monitor::Sample;
use detect::Game;
use session::LiveSession;

#[derive(Default)]
pub struct GameState {
    pub games: RwLock<Vec<Game>>,
    pub live: Mutex<Option<LiveSession>>,
}

pub fn custom_exes(db: &Db) -> AppResult<Vec<String>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT exe FROM custom_games ORDER BY exe")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn add_custom(db: &Db, exe: &str) -> AppResult<()> {
    profile::validate_exe(exe)?;
    db.conn().execute("INSERT OR IGNORE INTO custom_games(exe, added_at) VALUES (?1, ?2)", params![exe, now()])?;
    db.audit("info", "user", "game.added", Some(serde_json::json!({ "exe": exe })));
    Ok(())
}

pub fn remove_custom(db: &Db, exe: &str) -> AppResult<()> {
    db.conn().execute("DELETE FROM custom_games WHERE exe = ?1", [exe])?;
    Ok(())
}

/// Re-detect installed games and update the cache.
pub fn refresh(db: &Db, gs: &GameState) -> Vec<Game> {
    let games = detect::detect(&custom_exes(db).unwrap_or_default());
    if let Ok(mut g) = gs.games.write() {
        *g = games.clone();
    }
    games
}

/// Called by the telemetry thread for every sample.
pub fn observe(db: &Db, gs: &GameState, procs: &[crate::system::monitor::ProcessInfo], s: &Sample) {
    let games = match gs.games.read() {
        Ok(g) if !g.is_empty() => g.clone(),
        _ => return,
    };
    let finished = match gs.live.lock() {
        Ok(mut live) => session::observe(&mut live, &games, procs, s),
        Err(_) => None,
    };
    if let Some(done) = finished {
        let profile_json = profile::get(db, &done.game_id).ok().flatten().and_then(|p| serde_json::to_string(&p).ok());
        match session::store(db, &done, profile_json) {
            Ok(_) => tracing::info!(game = %done.game_name, mins = (done.ended_at - done.started_at) / 60_000, "game session recorded"),
            Err(e) => tracing::warn!("could not store game session: {e}"),
        }
    }
}
