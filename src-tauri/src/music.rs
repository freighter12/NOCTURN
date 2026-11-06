//! Nocturn Music — catalogue, library, playlists, likes and listening history.
//!
//! Catalogue sources: tracks carry a `source`. The only source today is
//! `local` — music files in folders the user chose, read in place and never
//! uploaded. The schema and the playback protocol are source-agnostic so a
//! licensed Nocturn catalogue can be added as another source later.
//!
//! Playback goes through the `media` URI scheme (see `serve`), which only
//! serves tracks that are in the index — never arbitrary paths.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use lofty::prelude::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

pub const AUDIO_EXTS: &[&str] = &["mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav", "wma"];
const MAX_FILES: usize = 100_000;
static SCANNING: AtomicBool = AtomicBool::new(false);

fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

// ------------------------------------------------------------------ folders

pub fn folders(db: &Db) -> AppResult<Vec<String>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT path FROM music_folders ORDER BY path COLLATE NOCASE")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn add_folder(db: &Db, path: &str) -> AppResult<String> {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    if !p.is_absolute() || !p.is_dir() {
        return Err(AppError::Invalid("choose an existing folder (full path, e.g. C:\\Users\\you\\Music)".into()));
    }
    let canon = dunce_canon(&p);
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    if canon.to_ascii_lowercase().starts_with(&windir) {
        return Err(AppError::Invalid("the Windows folder can't be a music folder".into()));
    }
    db.conn().execute("INSERT OR IGNORE INTO music_folders(path, added_at) VALUES (?1, ?2)", params![canon, now()])?;
    Ok(canon)
}

/// Remove a folder and its tracks from the index (files are untouched).
pub fn remove_folder(db: &Db, path: &str) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM music_folders WHERE path = ?1", [path])?;
    conn.execute("DELETE FROM music_tracks WHERE folder = ?1", [path])?;
    conn.execute("DELETE FROM music_playlist_tracks WHERE track_id NOT IN (SELECT id FROM music_tracks)", [])?;
    conn.execute("DELETE FROM music_likes WHERE track_id NOT IN (SELECT id FROM music_tracks)", [])?;
    Ok(())
}

/// Absolute path without the `\\?\` prefix `canonicalize` adds on Windows.
fn dunce_canon(p: &Path) -> String {
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = c.display().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

// --------------------------------------------------------------------- scan

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub folders: usize,
    pub files: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unreadable: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub folder: String,
    pub files: usize,
}

struct Meta {
    title: String,
    artist: String,
    album: String,
    album_artist: String,
    track_no: Option<u32>,
    disc_no: Option<u32>,
    year: Option<i64>,
    genre: String,
    duration_ms: i64,
    has_art: bool,
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Unknown".into())
}

/// Read tags; falls back to the file name for missing fields.
fn read_meta(p: &Path) -> Option<Meta> {
    let f = lofty::read_from_path(p).ok()?;
    let duration_ms = f.properties().duration().as_millis() as i64;
    let tag = f.primary_tag().or_else(|| f.first_tag());
    let s = |v: Option<std::borrow::Cow<'_, str>>| v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    let title = tag.and_then(|t| s(t.title())).unwrap_or_else(|| stem(p));
    let artist = tag.and_then(|t| s(t.artist())).unwrap_or_else(|| "Unknown artist".into());
    let album = tag.and_then(|t| s(t.album())).unwrap_or_else(|| p.parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().to_string()).unwrap_or_else(|| "Unknown album".into()));
    let album_artist = tag.and_then(|t| t.get_string(ItemKey::AlbumArtist).map(|x| x.trim().to_string())).filter(|x| !x.is_empty()).unwrap_or_else(|| artist.clone());
    let has_art = tag.is_some_and(|t| !t.pictures().is_empty()) || folder_art(p).is_some();
    Some(Meta {
        title,
        artist,
        album,
        album_artist,
        track_no: tag.and_then(|t| t.track()),
        disc_no: tag.and_then(|t| t.disk()),
        year: tag.and_then(|t| t.date()).map(|d| d.year as i64),
        genre: tag.and_then(|t| s(t.genre())).unwrap_or_default(),
        duration_ms,
        has_art,
    })
}

fn folder_art(p: &Path) -> Option<PathBuf> {
    let dir = p.parent()?;
    ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "album.jpg"].iter().map(|n| dir.join(n)).find(|c| c.is_file())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue; // never follow links or junctions out of the folder
        }
        let p = e.path();
        if ft.is_dir() {
            walk(&p, out);
        } else if is_audio(&p) {
            out.push(p);
        }
    }
}

pub fn scan(db: &Db, progress: &dyn Fn(ScanProgress)) -> AppResult<ScanReport> {
    if SCANNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::msg("a music scan is already running"));
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            SCANNING.store(false, Ordering::SeqCst);
        }
    }
    let _d = Done;
    let folders = folders(db)?;
    let mut rep = ScanReport { folders: folders.len(), files: 0, added: 0, updated: 0, removed: 0, unreadable: 0 };
    for folder in &folders {
        let mut files = Vec::new();
        walk(Path::new(folder), &mut files);
        rep.files += files.len();
        // Known files in this folder: path -> (size, mtime).
        let known: HashMap<String, (i64, i64)> = {
            let conn = db.conn();
            let mut st = conn.prepare("SELECT path, size, mtime FROM music_tracks WHERE folder = ?1")?;
            let rows = st.query_map([folder], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?.collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().collect()
        };
        let mut seen = std::collections::HashSet::new();
        for (n, f) in files.iter().enumerate() {
            let path = f.display().to_string();
            seen.insert(path.clone());
            let Ok(md) = std::fs::metadata(f) else { continue };
            let size = md.len() as i64;
            let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            if known.get(&path) == Some(&(size, mtime)) {
                continue;
            }
            let Some(m) = read_meta(f) else {
                rep.unreadable += 1;
                continue;
            };
            let conn = db.conn();
            let exists = known.contains_key(&path);
            conn.execute(
                "INSERT INTO music_tracks(path, folder, title, artist, album, album_artist, track_no, disc_no, year, genre, duration_ms, has_art, size, mtime, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(path) DO UPDATE SET title = excluded.title, artist = excluded.artist, album = excluded.album, album_artist = excluded.album_artist,
                   track_no = excluded.track_no, disc_no = excluded.disc_no, year = excluded.year, genre = excluded.genre, duration_ms = excluded.duration_ms,
                   has_art = excluded.has_art, size = excluded.size, mtime = excluded.mtime",
                params![path, folder, m.title, m.artist, m.album, m.album_artist, m.track_no, m.disc_no, m.year, m.genre, m.duration_ms, m.has_art as i64, size, mtime, now()],
            )?;
            if exists { rep.updated += 1 } else { rep.added += 1 }
            if n % 50 == 0 {
                progress(ScanProgress { folder: folder.clone(), files: n });
            }
        }
        // Files that disappeared.
        let gone: Vec<&String> = known.keys().filter(|k| !seen.contains(*k)).collect();
        for g in &gone {
            db.conn().execute("DELETE FROM music_tracks WHERE path = ?1", [g])?;
        }
        rep.removed += gone.len();
    }
    let conn = db.conn();
    conn.execute("DELETE FROM music_playlist_tracks WHERE track_id NOT IN (SELECT id FROM music_tracks)", [])?;
    conn.execute("DELETE FROM music_likes WHERE track_id NOT IN (SELECT id FROM music_tracks)", [])?;
    drop(conn);
    db.audit("info", "user", "music.scan", Some(serde_json::json!({ "folders": rep.folders, "files": rep.files, "added": rep.added, "removed": rep.removed })));
    Ok(rep)
}

// ------------------------------------------------------------------- tracks

#[derive(Debug, Clone, Serialize)]
pub struct Track {
    pub id: i64,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: String,
    pub duration_ms: i64,
    pub has_art: bool,
    pub liked: bool,
    pub added_at: String,
    pub plays: i64,
    pub last_played: Option<String>,
}

pub fn tracks(db: &Db) -> AppResult<Vec<Track>> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT t.id, t.source, t.title, t.artist, t.album, t.album_artist, t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, t.has_art, t.added_at,
                (l.track_id IS NOT NULL), COALESCE(p.n, 0), p.last
         FROM music_tracks t
         LEFT JOIN music_likes l ON l.track_id = t.id
         LEFT JOIN (SELECT track_id, COUNT(*) n, MAX(played_at) last FROM music_plays GROUP BY track_id) p ON p.track_id = t.id
         ORDER BY t.album_artist COLLATE NOCASE, t.album COLLATE NOCASE, t.disc_no, t.track_no, t.title COLLATE NOCASE",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok(Track {
                id: r.get(0)?,
                source: r.get(1)?,
                title: r.get(2)?,
                artist: r.get(3)?,
                album: r.get(4)?,
                album_artist: r.get(5)?,
                track_no: r.get(6)?,
                disc_no: r.get(7)?,
                year: r.get(8)?,
                genre: r.get(9)?,
                duration_ms: r.get(10)?,
                has_art: r.get::<_, i64>(11)? != 0,
                added_at: r.get(12)?,
                liked: r.get::<_, i64>(13)? != 0,
                plays: r.get(14)?,
                last_played: r.get(15)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn track_path(db: &Db, id: i64) -> AppResult<Option<PathBuf>> {
    Ok(db.conn().query_row("SELECT path FROM music_tracks WHERE id = ?1", [id], |r| r.get::<_, String>(0)).optional()?.map(PathBuf::from))
}

pub fn set_like(db: &Db, id: i64, liked: bool) -> AppResult<()> {
    let conn = db.conn();
    if liked {
        conn.execute("INSERT OR IGNORE INTO music_likes(track_id, liked_at) SELECT id, ?2 FROM music_tracks WHERE id = ?1", params![id, now()])?;
    } else {
        conn.execute("DELETE FROM music_likes WHERE track_id = ?1", [id])?;
    }
    Ok(())
}

/// Count a play (the player calls this after 30 s or half the track).
pub fn record_play(db: &Db, id: i64, ms: i64) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("INSERT INTO music_plays(track_id, played_at, ms_played) SELECT id, ?2, ?3 FROM music_tracks WHERE id = ?1", params![id, now(), ms.max(0)])?;
    conn.execute("DELETE FROM music_plays WHERE played_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-730 days')", [])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Play {
    pub track_id: i64,
    pub played_at: String,
}

pub fn history(db: &Db, limit: i64) -> AppResult<Vec<Play>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT track_id, played_at FROM music_plays ORDER BY id DESC LIMIT ?1")?;
    let rows = st.query_map([limit], |r| Ok(Play { track_id: r.get(0)?, played_at: r.get(1)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn clear_history(db: &Db) -> AppResult<()> {
    db.conn().execute("DELETE FROM music_plays", [])?;
    Ok(())
}

// ------------------------------------------------------------- recommended

/// Picks from the user's own listening only: tracks by the artists they play
/// most that they haven't heard lately, then liked tracks not played in a
/// while. No network, no profile leaves the PC.
pub fn recommended(all: &[Track], limit: usize) -> Vec<i64> {
    let mut artist_plays: HashMap<&str, i64> = HashMap::new();
    for t in all {
        *artist_plays.entry(t.artist.as_str()).or_default() += t.plays + if t.liked { 3 } else { 0 };
    }
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(14)).to_rfc3339();
    let stale = |t: &Track| t.last_played.as_deref().is_none_or(|l| l < cutoff.as_str());
    let mut picks: Vec<&Track> = all.iter().filter(|t| stale(t) && artist_plays.get(t.artist.as_str()).copied().unwrap_or(0) > 0).collect();
    picks.sort_by_key(|t| (std::cmp::Reverse(artist_plays.get(t.artist.as_str()).copied().unwrap_or(0)), t.plays));
    let mut out: Vec<i64> = picks.iter().map(|t| t.id).collect();
    for t in all.iter().filter(|t| t.liked && stale(t)) {
        if !out.contains(&t.id) {
            out.push(t.id);
        }
    }
    out.truncate(limit);
    out
}

// ---------------------------------------------------------------- playlists

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Cover {
    /// Use this track's artwork.
    #[serde(default)]
    pub track_id: Option<i64>,
    #[serde(default)]
    pub glyph: Option<String>,
    #[serde(default)]
    pub colour: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub cover: Cover,
    pub tracks: Vec<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaylistInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub cover: Cover,
    pub tracks: Vec<i64>,
}

pub fn playlists(db: &Db) -> AppResult<Vec<Playlist>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, name, description, cover_json, created_at, updated_at FROM music_playlists ORDER BY name COLLATE NOCASE")?;
    let mut out: Vec<Playlist> = st
        .query_map([], |r| {
            let cover: String = r.get(3)?;
            Ok(Playlist { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, cover: serde_json::from_str(&cover).unwrap_or_default(), tracks: vec![], created_at: r.get(4)?, updated_at: r.get(5)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut st = conn.prepare("SELECT playlist_id, track_id FROM music_playlist_tracks ORDER BY playlist_id, position")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    for (pid, tid) in rows {
        if let Some(p) = out.iter_mut().find(|p| p.id == pid) {
            p.tracks.push(tid);
        }
    }
    Ok(out)
}

pub fn save_playlist(db: &Db, p: PlaylistInput) -> AppResult<Playlist> {
    let name: String = p.name.chars().filter(|c| !c.is_control()).take(80).collect::<String>().trim().to_string();
    if name.is_empty() {
        return Err(AppError::Invalid("give the playlist a name".into()));
    }
    if p.tracks.len() > 10_000 {
        return Err(AppError::Invalid("a playlist can hold up to 10,000 tracks".into()));
    }
    let description: String = p.description.chars().filter(|c| !c.is_control() || *c == '\n').take(300).collect();
    let cover = Cover {
        track_id: p.cover.track_id,
        glyph: p.cover.glyph.map(|g| g.trim().chars().take(2).collect::<String>()).filter(|g| !g.is_empty()),
        colour: p.cover.colour.filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())),
    };
    let id = match p.id {
        Some(id) if id.len() <= 64 && db.conn().query_row("SELECT 1 FROM music_playlists WHERE id = ?1", [&id], |_| Ok(())).optional()?.is_some() => id,
        Some(_) => return Err(AppError::NotFound("playlist".into())),
        None => uuid::Uuid::new_v4().to_string(),
    };
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    let ts = now();
    tx.execute(
        "INSERT INTO music_playlists(id, name, description, cover_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, cover_json = excluded.cover_json, updated_at = excluded.updated_at",
        params![id, name, description, serde_json::to_string(&cover)?, ts],
    )?;
    tx.execute("DELETE FROM music_playlist_tracks WHERE playlist_id = ?1", [&id])?;
    {
        let mut ins = tx.prepare("INSERT INTO music_playlist_tracks(playlist_id, position, track_id) SELECT ?1, ?2, id FROM music_tracks WHERE id = ?3")?;
        let mut pos = 0i64;
        for t in &p.tracks {
            if ins.execute(params![id, pos, t])? > 0 {
                pos += 1;
            }
        }
    }
    tx.commit()?;
    drop(conn);
    playlists(db)?.into_iter().find(|x| x.id == id).ok_or_else(|| AppError::msg("playlist vanished"))
}

pub fn delete_playlist(db: &Db, id: &str) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM music_playlist_tracks WHERE playlist_id = ?1", [id])?;
    conn.execute("DELETE FROM music_playlists WHERE id = ?1", [id])?;
    Ok(())
}

// ------------------------------------------------------------------- lyrics

#[derive(Debug, Clone, Serialize)]
pub struct Lyrics {
    pub text: String,
    /// Synced lines (ms, text) from an .lrc file.
    pub synced: Vec<(i64, String)>,
    pub source: &'static str,
}

/// Parse LRC `[mm:ss.xx] text` lines.
pub fn parse_lrc(s: &str) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    for line in s.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(end) = rest.strip_prefix('[').and_then(|r| r.find(']').map(|i| (i, r))) {
            let (i, r) = end;
            let tag = &r[..i];
            if let Some((m, sec)) = tag.split_once(':') {
                if let (Ok(m), Ok(sec)) = (m.parse::<i64>(), sec.parse::<f64>()) {
                    stamps.push(m * 60_000 + (sec * 1000.0) as i64);
                }
            }
            rest = r[i + 1..].trim_start();
        }
        for t in stamps {
            out.push((t, rest.to_string()));
        }
    }
    out.sort_by_key(|x| x.0);
    out
}

pub fn lyrics(db: &Db, id: i64) -> AppResult<Option<Lyrics>> {
    let Some(p) = track_path(db, id)? else { return Ok(None) };
    let lrc = p.with_extension("lrc");
    if let Ok(s) = std::fs::read_to_string(&lrc) {
        let synced = parse_lrc(&s);
        let text = if synced.is_empty() { s.clone() } else { synced.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join("\n") };
        return Ok(Some(Lyrics { text, synced, source: "lrc file" }));
    }
    let f = lofty::read_from_path(&p).ok();
    let tag = f.as_ref().and_then(|f| f.primary_tag().or_else(|| f.first_tag()));
    let text = tag.and_then(|t| t.get_string(ItemKey::Lyrics).or_else(|| t.get_string(ItemKey::UnsyncLyrics)).map(String::from));
    Ok(text.filter(|t| !t.trim().is_empty()).map(|text| Lyrics { synced: parse_lrc(&text), text, source: "embedded" }))
}

// --------------------------------------------------------- playback session

const SESSION_KEY: &str = "music.session";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub queue: Vec<i64>,
    pub index: usize,
    pub position_ms: i64,
    pub shuffle: bool,
    /// off, all, one
    pub repeat: String,
}

pub fn session(db: &Db) -> Session {
    db.get_setting(SESSION_KEY).ok().flatten().unwrap_or_default()
}

pub fn save_session(db: &Db, mut s: Session) -> AppResult<()> {
    s.queue.truncate(5000);
    if !["off", "all", "one"].contains(&s.repeat.as_str()) {
        s.repeat = "off".into();
    }
    db.set_setting(SESSION_KEY, &s)
}

// ---------------------------------------------------------------- serving

pub struct Served {
    pub status: u16,
    pub mime: String,
    pub body: Vec<u8>,
    /// (start, end, total) for partial responses.
    pub range: Option<(u64, u64, u64)>,
}

fn mime_of(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("m4a") | Some("aac") => "audio/mp4",
        Some("ogg") | Some("oga") => "audio/ogg",
        Some("opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("wma") => "audio/x-ms-wma",
        _ => "application/octet-stream",
    }
}

/// `bytes=start-end` → (start, end inclusive), clamped to `total`.
pub fn parse_range(h: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let spec = h?.trim().strip_prefix("bytes=")?;
    let (a, b) = spec.split(',').next()?.split_once('-')?;
    if total == 0 {
        return None;
    }
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (total.saturating_sub(n), total - 1)
        }
        (s, "") => (s.parse().ok()?, total - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(total - 1)),
    };
    (start <= end && start < total).then_some((start, end))
}

const CHUNK: u64 = 2 * 1024 * 1024;

/// Serve `/track/<id>` (audio, ranged) or `/art/<id>` (cover image).
pub fn serve(db: &Db, path: &str, range: Option<&str>) -> Served {
    let not_found = || Served { status: 404, mime: "text/plain".into(), body: b"not found".to_vec(), range: None };
    let mut parts = path.trim_matches('/').splitn(2, '/');
    let (kind, id) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let Ok(id) = id.parse::<i64>() else { return not_found() };
    let Ok(Some(file)) = track_path(db, id) else { return not_found() };
    match kind {
        "track" => {
            let Ok(mut f) = std::fs::File::open(&file) else { return not_found() };
            let total = f.metadata().map(|m| m.len()).unwrap_or(0);
            let (start, end) = parse_range(range, total).unwrap_or((0, total.saturating_sub(1)));
            // Serve at most CHUNK per request; the player asks for the rest.
            let end = end.min(start + CHUNK - 1);
            let mut buf = vec![0u8; (end - start + 1) as usize];
            if f.seek(SeekFrom::Start(start)).is_err() || f.read_exact(&mut buf).is_err() {
                return not_found();
            }
            Served { status: 206, mime: mime_of(&file).into(), body: buf, range: Some((start, end, total)) }
        }
        "art" => {
            if let Ok(tf) = lofty::read_from_path(&file) {
                if let Some(pic) = tf.primary_tag().or_else(|| tf.first_tag()).and_then(|t| t.pictures().first()) {
                    let mime = pic.mime_type().map(|m| m.as_str().to_string()).unwrap_or_else(|| "image/jpeg".into());
                    return Served { status: 200, mime, body: pic.data().to_vec(), range: None };
                }
            }
            match folder_art(&file).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, body)) => Served { status: 200, mime: if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) { "image/png".into() } else { "image/jpeg".into() }, body, range: None },
                None => not_found(),
            }
        }
        _ => not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny valid WAV file with `ms` of silence.
    pub fn wav(ms: u32) -> Vec<u8> {
        let rate = 8000u32;
        let samples = rate * ms / 1000;
        let data_len = samples * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 2).to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        v.resize(v.len() + data_len as usize, 0);
        v
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Some((10, 19)));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Some((90, 99)));
        assert_eq!(parse_range(Some("bytes=50-500"), 100), Some((50, 99)));
        assert_eq!(parse_range(Some("bytes=200-"), 100), None);
        assert_eq!(parse_range(Some("nonsense"), 100), None);
        assert_eq!(parse_range(None, 100), None);
    }

    #[test]
    fn lrc() {
        let l = parse_lrc("[ti:Song]\n[00:01.50]Hello\n[00:00.00][00:03.00]Twice\n");
        assert_eq!(l, vec![(0, "Twice".into()), (1500, "Hello".into()), (3000, "Twice".into())]);
    }

    #[test]
    fn scan_library_and_serve() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("Artist").join("Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01 First.wav"), wav(1500)).unwrap();
        std::fs::write(album.join("02 Second.wav"), wav(800)).unwrap();
        std::fs::write(album.join("notes.txt"), "not music").unwrap();
        std::fs::write(album.join("02 Second.lrc"), "[00:00.40]line one\n").unwrap();
        let db = Db::open_in_memory().unwrap();
        assert!(add_folder(&db, r"C:\Windows\Media").is_err());
        let folder = add_folder(&db, &dir.path().display().to_string()).unwrap();
        let r = scan(&db, &|_| {}).unwrap();
        assert_eq!((r.files, r.added), (2, 2));
        let t = tracks(&db).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].title, "01 First", "falls back to the file name");
        assert_eq!(t[0].album, "Album", "folder name as album");
        assert!((1400..=1600).contains(&t[0].duration_ms), "{}", t[0].duration_ms);

        // Rescan: unchanged files are skipped; a deleted file is removed.
        assert_eq!(scan(&db, &|_| {}).unwrap().added, 0);
        std::fs::remove_file(album.join("01 First.wav")).unwrap();
        assert_eq!(scan(&db, &|_| {}).unwrap().removed, 1);
        let t = tracks(&db).unwrap();
        assert_eq!(t.len(), 1);

        // Serving: only indexed IDs, ranges honoured.
        let id = t[0].id;
        let s = serve(&db, &format!("/track/{id}"), Some("bytes=0-9"));
        assert_eq!((s.status, s.body.len(), &s.body[..4]), (206, 10, &b"RIFF"[..]));
        assert_eq!(serve(&db, "/track/99999", None).status, 404);
        assert_eq!(serve(&db, "/track/../../etc/passwd", None).status, 404);
        assert_eq!(serve(&db, "/art/abc", None).status, 404);

        // Likes, plays, playlists, lyrics.
        set_like(&db, id, true).unwrap();
        record_play(&db, id, 30_000).unwrap();
        let t = tracks(&db).unwrap();
        assert!(t[0].liked && t[0].plays == 1);
        let p = save_playlist(&db, PlaylistInput { id: None, name: "Focus".into(), description: "deep work".into(), cover: Cover { track_id: Some(id), glyph: None, colour: Some("bad".into()) }, tracks: vec![id, 4242, id] }).unwrap();
        assert_eq!(p.tracks, vec![id, id], "unknown tracks dropped, duplicates allowed");
        assert_eq!(p.cover.colour, None);
        let p2 = save_playlist(&db, PlaylistInput { id: Some(p.id.clone()), name: "Focus 2".into(), description: String::new(), cover: Cover::default(), tracks: vec![] }).unwrap();
        assert_eq!((p2.name.as_str(), p2.tracks.len()), ("Focus 2", 0));
        assert!(save_playlist(&db, PlaylistInput { id: Some("nope".into()), name: "x".into(), description: String::new(), cover: Cover::default(), tracks: vec![] }).is_err());
        let l = lyrics(&db, id).unwrap().unwrap();
        assert_eq!(l.synced, vec![(400, "line one".to_string())]);

        // Removing the folder removes its tracks from the index only.
        remove_folder(&db, &folder).unwrap();
        assert!(tracks(&db).unwrap().is_empty());
        assert!(album.join("02 Second.wav").exists(), "files are never deleted");
        assert!(playlists(&db).unwrap()[0].tracks.is_empty());
    }

    #[test]
    fn recommendations_come_from_listening() {
        let mk = |id: i64, artist: &str, plays: i64, liked: bool| Track { id, source: "local".into(), title: format!("t{id}"), artist: artist.into(), album: "a".into(), album_artist: artist.into(), track_no: None, disc_no: None, year: None, genre: String::new(), duration_ms: 1, has_art: false, liked, added_at: String::new(), plays, last_played: None };
        let all = vec![mk(1, "A", 10, false), mk(2, "A", 0, false), mk(3, "B", 0, false), mk(4, "C", 0, true)];
        let r = recommended(&all, 10);
        assert_eq!(r[0], 2, "unplayed track by most-played artist first");
        assert!(r.contains(&4) && !r.contains(&3));
    }
}
