//! Smart playlists: listening statistics for the built-in mixes, and the
//! user's own rule-based lists. Rules are evaluated by the frontend against
//! the library it already holds; here they're only stored.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Default)]
pub struct Stats {
    /// (track, plays) in the last 30 days.
    pub recent: Vec<(i64, i64)>,
    /// (track, local hour 0–23, plays) over the last year.
    pub by_hour: Vec<(i64, i64, i64)>,
}

pub fn stats(db: &Db) -> AppResult<Stats> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT track_id, COUNT(*) FROM music_plays WHERE played_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-30 days') GROUP BY track_id",
    )?;
    let recent = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    let mut st = conn.prepare(
        "SELECT track_id, CAST(strftime('%H', played_at, 'localtime') AS INTEGER) h, COUNT(*) FROM music_plays
         WHERE played_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-365 days') GROUP BY track_id, h",
    )?;
    let by_hour = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(Stats { recent, by_hour })
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartPlaylist {
    pub id: String,
    pub name: String,
    pub rules: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmartInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub rules: serde_json::Value,
}

pub fn list(db: &Db) -> AppResult<Vec<SmartPlaylist>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, name, rules_json, created_at, updated_at FROM music_smart_playlists ORDER BY name COLLATE NOCASE")?;
    let rows = st
        .query_map([], |r| {
            let rules: String = r.get(2)?;
            Ok(SmartPlaylist { id: r.get(0)?, name: r.get(1)?, rules: serde_json::from_str(&rules).unwrap_or(serde_json::Value::Null), created_at: r.get(3)?, updated_at: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn save(db: &Db, p: SmartInput) -> AppResult<SmartPlaylist> {
    let name: String = p.name.chars().filter(|c| !c.is_control()).take(80).collect::<String>().trim().to_string();
    if name.is_empty() {
        return Err(AppError::Invalid("give the smart playlist a name".into()));
    }
    if !p.rules.is_object() {
        return Err(AppError::Invalid("the rules aren't valid".into()));
    }
    let rules = serde_json::to_string(&p.rules)?;
    if rules.len() > 16 * 1024 {
        return Err(AppError::Invalid("too many rules".into()));
    }
    let id = match p.id {
        Some(id) if id.len() <= 64 && db.conn().query_row("SELECT 1 FROM music_smart_playlists WHERE id = ?1", [&id], |_| Ok(())).optional()?.is_some() => id,
        Some(_) => return Err(AppError::NotFound("smart playlist".into())),
        None => format!("smart-{}", uuid::Uuid::new_v4()),
    };
    let ts = now();
    db.conn().execute(
        "INSERT INTO music_smart_playlists(id, name, rules_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, rules_json = excluded.rules_json, updated_at = excluded.updated_at",
        params![id, name, rules, ts],
    )?;
    list(db)?.into_iter().find(|x| x.id == id).ok_or_else(|| AppError::msg("smart playlist vanished"))
}

pub fn delete(db: &Db, id: &str) -> AppResult<()> {
    db.conn().execute("DELETE FROM music_smart_playlists WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_playlists_round_trip_and_validate() {
        let db = Db::open_in_memory().unwrap();
        assert!(save(&db, SmartInput { id: None, name: "  ".into(), rules: serde_json::json!({}) }).is_err());
        assert!(save(&db, SmartInput { id: None, name: "x".into(), rules: serde_json::json!([1]) }).is_err());
        let p = save(&db, SmartInput { id: None, name: "Chill".into(), rules: serde_json::json!({ "match": "all", "rules": [] }) }).unwrap();
        assert!(p.id.starts_with("smart-"));
        let p2 = save(&db, SmartInput { id: Some(p.id.clone()), name: "Chill 2".into(), rules: p.rules.clone() }).unwrap();
        assert_eq!(p2.id, p.id);
        assert_eq!(list(&db).unwrap().len(), 1);
        assert!(save(&db, SmartInput { id: Some("nope".into()), name: "x".into(), rules: serde_json::json!({}) }).is_err());
        delete(&db, &p.id).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn stats_count_recent_plays_and_hours() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute("INSERT INTO music_tracks(path, folder, title, artist, album, album_artist, duration_ms, size, mtime, added_at) VALUES ('p', 'f', 't', 'a', 'al', 'a', 1, 1, 1, ?1)", [now()])
            .unwrap();
        crate::music::record_play(&db, 1, 1000).unwrap();
        crate::music::record_play(&db, 1, 1000).unwrap();
        let s = stats(&db).unwrap();
        assert_eq!(s.recent, vec![(1, 2)]);
        assert_eq!(s.by_hour.iter().map(|x| x.2).sum::<i64>(), 2);
    }
}
