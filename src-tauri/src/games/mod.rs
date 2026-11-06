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
    /// IDs the user marked as "not a game" (e.g. Steam utilities). They stay
    /// listed so they can be restored, but never start sessions.
    pub not_games: RwLock<Vec<String>>,
}

const NOT_GAMES_KEY: &str = "games.not_games";

pub fn not_games(db: &Db) -> Vec<String> {
    db.get_setting::<Vec<String>>(NOT_GAMES_KEY).ok().flatten().unwrap_or_default()
}

/// Mark or unmark a detected entry as "not a game".
pub fn set_not_game(db: &Db, gs: &GameState, id: &str, not_game: bool) -> AppResult<()> {
    if id.is_empty() || id.len() > 600 || id.chars().any(|c| c.is_control()) {
        return Err(crate::error::AppError::Invalid("invalid game id".into()));
    }
    let mut list = not_games(db);
    list.retain(|x| x != id);
    if not_game {
        list.push(id.to_string());
    }
    db.set_setting(NOT_GAMES_KEY, &list)?;
    if let Ok(mut n) = gs.not_games.write() {
        *n = list;
    }
    // A session for something that isn't a game is dropped, not recorded.
    if not_game {
        if let Ok(mut live) = gs.live.lock() {
            if live.as_ref().is_some_and(|l| l.summary().game_id == id) {
                *live = None;
            }
        }
    }
    db.audit("info", "user", if not_game { "game.not_game" } else { "game.restored" }, Some(serde_json::json!({ "id": id })));
    Ok(())
}

/// Games that can start sessions (detected minus "not a game").
pub fn playable(gs: &GameState) -> Vec<Game> {
    let hidden = gs.not_games.read().map(|n| n.clone()).unwrap_or_default();
    gs.games.read().map(|g| g.iter().filter(|x| !hidden.contains(&x.id)).cloned().collect()).unwrap_or_default()
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
    if let Ok(mut n) = gs.not_games.write() {
        *n = not_games(db);
    }
    if let Ok(mut g) = gs.games.write() {
        *g = games.clone();
    }
    games
}

/// Called by the telemetry thread for every sample.
pub fn observe(db: &Db, gs: &GameState, procs: &[crate::system::monitor::ProcessInfo], s: &Sample) {
    let games = playable(gs);
    if games.is_empty() {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str) -> Game {
        Game { id: id.into(), name: id.into(), launcher: "Steam", install_dir: None, exes: vec![] }
    }

    #[test]
    fn not_a_game_is_excluded_and_restorable() {
        let db = Db::open_in_memory().unwrap();
        let gs = GameState::default();
        *gs.games.write().unwrap() = vec![game("steam:388080"), game("steam:730")];
        set_not_game(&db, &gs, "steam:388080", true).unwrap();
        assert_eq!(playable(&gs).iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["steam:730"]);
        assert_eq!(not_games(&db), vec!["steam:388080".to_string()], "persisted");
        set_not_game(&db, &gs, "steam:388080", true).unwrap();
        assert_eq!(not_games(&db).len(), 1, "no duplicates");
        set_not_game(&db, &gs, "steam:388080", false).unwrap();
        assert_eq!(playable(&gs).len(), 2);
        assert!(set_not_game(&db, &gs, "", true).is_err());
    }
}
