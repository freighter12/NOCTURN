//! Privacy overview: what Nocturn stores (all locally), what leaves the PC,
//! and controls to clear data the user may not want kept.

use serde::Serialize;

use crate::db::Db;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
pub struct Stored {
    pub key: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub count: i64,
    /// Whether the user can clear it here.
    pub clearable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outbound {
    pub what: &'static str,
    pub when: &'static str,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrivacyOverview {
    pub data_dir: String,
    pub database_bytes: u64,
    pub stored: Vec<Stored>,
    pub outbound: Vec<Outbound>,
}

/// (key, label, detail, count SQL, clear SQL)
const CATEGORIES: &[(&str, &str, &str, &str, Option<&str>)] = &[
    ("change_journal", "Change history", "Every change Nocturn made and the original values, so it can be undone. Kept so rollback always works.", "SELECT COUNT(*) FROM changes", None),
    ("audit_log", "Audit log", "Security-relevant events (elevations, applied plans, launches). Kept for your protection.", "SELECT COUNT(*) FROM audit_log", None),
    ("ai_conversations", "AI conversations", "Your chats with Nocturn AI.", "SELECT COUNT(*) FROM conversations", Some("DELETE FROM messages; DELETE FROM conversations;")),
    ("telemetry", "Hardware history", "Per-minute CPU, GPU, memory and disk readings for the last 7 days.", "SELECT COUNT(*) FROM telemetry_minute", Some("DELETE FROM telemetry_minute;")),
    ("game_sessions", "Game sessions", "When you played and how hard your PC worked.", "SELECT COUNT(*) FROM game_sessions", Some("DELETE FROM game_sessions;")),
    ("network_tests", "Network tests", "Latency and packet-loss results.", "SELECT COUNT(*) FROM network_tests", Some("DELETE FROM network_tests;")),
    ("benchmarks", "Benchmark results", "Saved benchmark runs.", "SELECT COUNT(*) FROM bench_runs", Some("DELETE FROM bench_runs;")),
    ("maintenance", "Maintenance history", "When scheduled checks ran and what they found.", "SELECT COUNT(*) FROM maintenance_runs", Some("DELETE FROM maintenance_runs;")),
    ("library", "App Library", "Apps, services and websites you added, and when you last opened them.", "SELECT COUNT(*) FROM library_items", Some("UPDATE library_items SET last_opened_at = NULL, open_count = 0;")),
    ("music_index", "Music index", "Titles, artists and albums read from your music folders (the files themselves stay where they are).", "SELECT COUNT(*) FROM music_tracks", None),
    ("music_imports", "Imported playlist lists", "The song lists of playlists you imported, kept so missing songs can be matched when you add them.", "SELECT COUNT(*) FROM music_playlist_imports", None),
    ("music_lyrics", "Lyrics found online", "Lyrics NOCTURN looked up for songs that had none, so each song is asked only once.", "SELECT COUNT(*) FROM music_lyrics_cache", Some("DELETE FROM music_lyrics_cache;")),
    ("music_history", "Listening history", "What you played and when; used for Recently played and recommendations.", "SELECT COUNT(*) FROM music_plays", Some("DELETE FROM music_plays;")),
];

pub fn overview(db: &Db, data_dir: &std::path::Path, ai_endpoint: Option<String>) -> AppResult<PrivacyOverview> {
    let conn = db.conn();
    let stored = CATEGORIES
        .iter()
        .map(|(key, label, detail, count_sql, clear)| Stored {
            key,
            label,
            detail,
            count: conn.query_row(count_sql, [], |r| r.get(0)).unwrap_or(0),
            clearable: clear.is_some(),
        })
        .collect();
    drop(conn);
    let database_bytes = std::fs::metadata(data_dir.join("nocturn.db")).map(|m| m.len()).unwrap_or(0);
    let mut outbound = vec![
        Outbound { what: "Network tests", when: "Only when you (or a maintenance task you turned on) run one", to: "Your router, Cloudflare (1.1.1.1) and Google (8.8.8.8) — pings and DNS lookups only".into() },
        Outbound { what: "Playlist imports", when: "Only when you paste a playlist link and click Find songs (or Update from link)", to: "Spotify, YouTube or Deezer — a request for that playlist's page; nothing about your PC or library".into() },
        Outbound { what: "Music streaming (if you turn it on)", when: "When you play a song that isn't in your library, find songs for an imported playlist, or once a day to keep yt-dlp current", to: "YouTube and YouTube Music — searches for song titles and the audio stream; GitHub — yt-dlp and Deno downloads. Nothing about your PC".into() },
        Outbound { what: "AI shuffle (if you turn it on)", when: "When the play queue runs low", to: "YouTube Music — the title and artist of the last songs you played, to get similar songs. Nothing about your PC".into() },
        Outbound { what: "Set up AI for me (if you use it)", when: "Only while setting up the free local AI", to: "GitHub — the Ollama installer and its checksum list; Ollama — the AI model download. After that the AI runs entirely on this PC".into() },
        Outbound { what: "Lyrics lookups (if on)", when: "When you open lyrics for a song that has none of its own — once per song", to: "lrclib.net — the song's artist, title, album and length; nothing about your PC (Customise › Music to turn off)".into() },
        Outbound { what: "Game cover art", when: "Once per game, the first time the Library shows it", to: "Steam's public store and image server (game names not on Steam: one title search) and the Microsoft Store catalogue (Xbox games, by package name). Steam games use Steam's own local copy first. Nothing about your PC".into() },
        Outbound { what: "Update checks", when: "A minute after start-up and once a day (Settings › Updates), or when you click Check", to: "github.com — a request for the latest version number; nothing about your PC. Downloads only when you click Install".into() },
        Outbound { what: "Support pages and websites", when: "Only when you click them", to: "Opened in your browser; Nocturn sends nothing itself".into() },
    ];
    if let Some(ep) = ai_endpoint {
        outbound.insert(0, Outbound { what: "Nocturn AI messages", when: "Only when you send a message", to: format!("{ep} — your messages, plus PC details from tools if system access is on (Settings › NOCTURN AI)") });
    }
    Ok(PrivacyOverview { data_dir: data_dir.display().to_string(), database_bytes, stored, outbound })
}

pub fn clear(db: &Db, key: &str) -> AppResult<()> {
    let (_, label, _, _, sql) = CATEGORIES.iter().find(|c| c.0 == key).ok_or_else(|| AppError::Invalid(format!("unknown data category '{key}'")))?;
    let sql = sql.ok_or_else(|| AppError::Forbidden(format!("{label} can't be cleared from here")))?;
    db.conn().execute_batch(&format!("BEGIN; {sql} COMMIT;"))?;
    db.audit("info", "user", "privacy.clear", Some(serde_json::json!({ "category": key })));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_clears_only_allowed_categories() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let o = overview(&db, dir.path(), Some("api.anthropic.com".into())).unwrap();
        assert!(o.stored.iter().any(|s| s.key == "change_journal" && !s.clearable));
        assert!(o.outbound[0].to.contains("anthropic"));
        db.conn().execute("INSERT INTO music_plays(track_id, played_at, ms_played) VALUES (1, 'x', 1)", []).unwrap();
        clear(&db, "music_history").unwrap();
        assert_eq!(overview(&db, dir.path(), None).unwrap().stored.iter().find(|s| s.key == "music_history").unwrap().count, 0);
        assert!(clear(&db, "change_journal").is_err(), "undo history is protected");
        assert!(clear(&db, "audit_log").is_err());
        assert!(clear(&db, "everything; DROP TABLE x").is_err());
    }
}
