//! Nocturn Music for iPhone.
//!
//! Rust side: YouTube Music browsing (yt.rs), synced lyrics (lyrics.rs) and
//! the on-phone library file. Playback, the lock screen and the Liquid Glass
//! tab bar are native Swift (plugins/nocturn).

pub mod ai;
pub mod import;
pub mod lyrics;
pub mod textmatch;
pub mod yt;

use std::path::PathBuf;

use tauri::Manager;

#[tauri::command]
fn app_info() -> String {
    format!("v{} · {} · {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)
}

#[tauri::command]
async fn yt_search(query: String) -> Result<yt::SearchResults, String> {
    yt::search(&query).await
}

#[tauri::command]
async fn yt_suggest(input: String) -> Result<Vec<String>, String> {
    yt::suggestions(&input).await
}

#[tauri::command]
async fn yt_home() -> Result<Vec<yt::Shelf>, String> {
    yt::home().await
}

#[tauri::command]
async fn yt_collection(id: String) -> Result<yt::Collection, String> {
    yt::collection(&id).await
}

#[tauri::command]
async fn yt_artist(id: String) -> Result<yt::ArtistPage, String> {
    yt::artist(&id).await
}

#[tauri::command]
async fn yt_radio(video_id: String) -> Result<Vec<yt::Track>, String> {
    yt::radio(&video_id).await
}

/// Audio link for the computer-only stand-in player (the iPhone resolves links natively).
#[tauri::command]
async fn yt_stream(video_id: String) -> Result<yt::Stream, String> {
    yt::stream(&video_id).await
}

/// Lyrics are saved on the phone once found, so they also show offline
/// (downloaded songs) and load instantly the next time.
#[tauri::command]
async fn lyrics_get(app: tauri::AppHandle, artist: String, title: String, album: Option<String>, duration_ms: Option<i64>) -> Result<Option<lyrics::Lyrics>, String> {
    let path = lyrics_cache_path(&app, &artist, &title, duration_ms);
    if let Some(hit) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str::<lyrics::Lyrics>(&s).ok()) {
        return Ok(Some(hit));
    }
    let found = lyrics::get(&artist, &title, album.as_deref(), duration_ms).await?;
    if let (Some(p), Some(l)) = (&path, &found) {
        if let Ok(json) = serde_json::to_string(l) {
            let _ = std::fs::write(p, json);
        }
    }
    Ok(found)
}

fn lyrics_cache_path(app: &tauri::AppHandle, artist: &str, title: &str, duration_ms: Option<i64>) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let dir = app.path().app_data_dir().ok()?.join("lyrics");
    std::fs::create_dir_all(&dir).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (artist.to_lowercase(), title.to_lowercase(), duration_ms.map(|d| d / 2000)).hash(&mut h);
    Some(dir.join(format!("{:016x}.json", h.finish())))
}

#[tauri::command]
async fn import_playlist(app: tauri::AppHandle, link: String) -> Result<import::Imported, String> {
    import::import(&app, &link).await
}

#[tauri::command]
async fn ai_shuffle(input: ai::Input) -> Result<ai::Plan, String> {
    ai::shuffle(input).await
}

// ---- library file (liked songs, saved albums/playlists, your playlists, history)

fn library_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("library.json"))
}

#[tauri::command]
fn library_load(app: tauri::AppHandle) -> Result<String, String> {
    let p = library_path(&app)?;
    Ok(std::fs::read_to_string(p).unwrap_or_else(|_| "{}".into()))
}

#[tauri::command]
fn library_save(app: tauri::AppHandle, json: String) -> Result<(), String> {
    if json.len() > 20 * 1024 * 1024 {
        return Err("library is too large".into());
    }
    serde_json::from_str::<serde_json::Value>(&json).map_err(|_| "library data is not valid".to_string())?;
    let p = library_path(&app)?;
    // write-then-rename so a crash never leaves half a file
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_nocturn::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            yt_search,
            yt_suggest,
            yt_home,
            yt_collection,
            yt_artist,
            yt_radio,
            yt_stream,
            lyrics_get,
            import_playlist,
            ai_shuffle,
            library_load,
            library_save
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nocturn Music");
}
