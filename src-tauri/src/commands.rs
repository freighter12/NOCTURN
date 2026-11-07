//! Tauri command handlers — the frontend's only way into the backend.
//! Blocking work runs on the blocking pool so the UI never stalls.

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::State;

use crate::ai::{self, chat, provider, store as conv_store};
use crate::error::{AppError, AppResult};
use crate::security::{elevation, secrets};
use crate::settings::{self, AppSettings};
use crate::state::Shared;
use crate::system::{diagnostics, dns, health::HealthReport, info::SystemInfo, monitor, network, power, process_control, process_view, processes};
use crate::tweaks::{catalog, engine, journal, ChangeOp, Takes};

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::msg(format!("background task failed: {e}")))?
}

// ------------------------------------------------------------------ app

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    data_dir: String,
    log_dir: String,
    is_elevated: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, Shared>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.data_dir.display().to_string(),
        log_dir: state.log_dir.display().to_string(),
        is_elevated: elevation::is_elevated(),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, Shared>) -> AppSettings {
    settings::load(&state.db)
}

#[tauri::command]
pub fn save_settings(state: State<'_, Shared>, value: AppSettings) -> AppResult<AppSettings> {
    let mut v = value;
    if !["anthropic", "openai_compatible"].contains(&v.ai.provider.as_str()) {
        return Err(AppError::Invalid("unknown AI provider".into()));
    }
    if v.ai.provider == "openai_compatible" {
        v.ai.base_url = provider::validate_base_url(&v.ai.base_url)?;
    }
    if v.ai.model.trim().is_empty() || v.ai.model.len() > 120 {
        return Err(AppError::Invalid("enter a model name".into()));
    }
    v.ai.model = v.ai.model.trim().to_string();
    v.ai.max_tokens = v.ai.max_tokens.clamp(1024, 128_000);
    v.ai.custom_instructions = crate::util::clamp(&v.ai.custom_instructions, 4000);
    v.monitor = v.monitor.sanitised();
    settings::save(&state.db, &v)?;
    state.db.audit("info", "user", "settings.saved", None);
    Ok(v)
}

// -------------------------------------------------------------- privacy

#[tauri::command]
pub fn privacy_overview(state: State<'_, Shared>) -> AppResult<crate::privacy::PrivacyOverview> {
    let ai = settings::load(&state.db).ai;
    let configured = secrets::exists(ai::secret_name(&ai.provider));
    let endpoint = configured.then(|| if ai.provider == "anthropic" { "Anthropic (api.anthropic.com)".to_string() } else { ai.base_url.clone() });
    crate::privacy::overview(&state.db, &state.data_dir, endpoint)
}

#[tauri::command]
pub fn privacy_clear(state: State<'_, Shared>, category: String) -> AppResult<()> {
    crate::privacy::clear(&state.db, &category)
}

// ---------------------------------------------------------------- music

#[tauri::command]
pub fn music_folders(state: State<'_, Shared>) -> AppResult<Vec<String>> {
    crate::music::folders(&state.db)
}

#[tauri::command]
pub fn music_add_folder(state: State<'_, Shared>, path: String) -> AppResult<String> {
    crate::music::add_folder(&state.db, &path)
}

#[tauri::command]
pub fn music_remove_folder(state: State<'_, Shared>, path: String) -> AppResult<()> {
    crate::music::remove_folder(&state.db, &path)
}

#[tauri::command]
pub async fn music_scan(state: State<'_, Shared>, on_progress: Channel<crate::music::ScanProgress>) -> AppResult<crate::music::ScanReport> {
    let st = state.inner().clone();
    blocking(move || {
        crate::music::scan(&st.db, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await
}

#[tauri::command]
pub async fn music_tracks(state: State<'_, Shared>) -> AppResult<Vec<crate::music::Track>> {
    let st = state.inner().clone();
    blocking(move || crate::music::tracks(&st.db)).await
}

#[tauri::command]
pub fn music_set_like(state: State<'_, Shared>, id: i64, liked: bool) -> AppResult<()> {
    crate::music::set_like(&state.db, id, liked)
}

#[tauri::command]
pub fn music_record_play(state: State<'_, Shared>, id: i64, ms: i64) -> AppResult<()> {
    crate::music::record_play(&state.db, id, ms)
}

#[tauri::command]
pub fn music_history(state: State<'_, Shared>, limit: Option<i64>) -> AppResult<Vec<crate::music::Play>> {
    crate::music::history(&state.db, limit.unwrap_or(200).clamp(1, 2000))
}

#[tauri::command]
pub fn music_clear_history(state: State<'_, Shared>) -> AppResult<()> {
    crate::music::clear_history(&state.db)
}

#[tauri::command]
pub async fn music_recommended(state: State<'_, Shared>) -> AppResult<Vec<i64>> {
    let st = state.inner().clone();
    blocking(move || Ok(crate::music::recommended(&crate::music::tracks(&st.db)?, 24))).await
}

#[tauri::command]
pub fn music_playlists(state: State<'_, Shared>) -> AppResult<Vec<crate::music::Playlist>> {
    crate::music::playlists(&state.db)
}

#[tauri::command]
pub fn music_save_playlist(state: State<'_, Shared>, playlist: crate::music::PlaylistInput) -> AppResult<crate::music::Playlist> {
    crate::music::save_playlist(&state.db, playlist)
}

// ---- playlist import

/// Read a playlist from a link (Spotify, YouTube, Deezer) or pasted text and match it to the library.
#[tauri::command]
pub async fn music_import_preview(state: State<'_, Shared>, link: Option<String>, text: Option<String>) -> AppResult<crate::music_import::ImportPreview> {
    use crate::music_import as mi;
    let list = match (link.filter(|l| !l.trim().is_empty()), text.filter(|t| !t.trim().is_empty())) {
        (Some(l), _) => {
            let parsed = if mi::is_short_link(&l) { mi::resolve_short(&l).await? } else { mi::parse_link(&l)? };
            mi::fetch(&parsed).await?
        }
        (None, Some(t)) => mi::parse_text(&t)?,
        (None, None) => return Err(AppError::Invalid("paste a playlist link or a list of songs".into())),
    };
    let st = state.inner().clone();
    blocking(move || mi::match_list(&st.db, list)).await
}

#[tauri::command]
pub fn music_import_save(state: State<'_, Shared>, import: crate::music_import::SaveImport) -> AppResult<crate::music::Playlist> {
    crate::music_import::save(&state.db, import)
}

#[tauri::command]
pub async fn music_import_info(state: State<'_, Shared>, playlist_id: String) -> AppResult<Option<crate::music_import::ImportInfo>> {
    let st = state.inner().clone();
    blocking(move || crate::music_import::info(&st.db, &playlist_id)).await
}

#[tauri::command]
pub fn music_import_forget(state: State<'_, Shared>, playlist_id: String) -> AppResult<()> {
    crate::music_import::forget(&state.db, &playlist_id)
}

#[tauri::command]
pub fn music_import_open(state: State<'_, Shared>, playlist_id: String) -> AppResult<()> {
    crate::music_import::open_original(&state.db, &playlist_id)
}

// ---- music streaming (unofficial, opt-in)

#[tauri::command]
pub fn music_stream_status(state: State<'_, Shared>) -> crate::music_stream_engine::Status {
    crate::music_stream_engine::status(&state.db, &state.data_dir, crate::settings::load(&state.db).music_streaming)
}

/// Download and verify the tools if needed, then turn streaming on.
#[tauri::command]
pub async fn music_stream_enable(state: State<'_, Shared>, on_progress: Channel<crate::music_stream_engine::InstallProgress>) -> AppResult<crate::music_stream_engine::Status> {
    let st = state.inner().clone();
    crate::music_stream_engine::install(&st.db, &st.data_dir, &|p| {
        let _ = on_progress.send(p);
    })
    .await?;
    let mut s = crate::settings::load(&st.db);
    s.music_streaming = true;
    crate::settings::save(&st.db, &s)?;
    Ok(crate::music_stream_engine::status(&st.db, &st.data_dir, true))
}

#[tauri::command]
pub fn music_stream_disable(state: State<'_, Shared>, remove_tools: bool) -> AppResult<crate::music_stream_engine::Status> {
    let mut s = crate::settings::load(&state.db);
    s.music_streaming = false;
    crate::settings::save(&state.db, &s)?;
    if remove_tools {
        crate::music_stream_engine::remove(&state.db, &state.data_dir)?;
    }
    state.db.audit("info", "user", "music.stream.disable", Some(serde_json::json!({ "removed_tools": remove_tools })));
    Ok(crate::music_stream_engine::status(&state.db, &state.data_dir, false))
}

#[tauri::command]
pub async fn music_stream_update(state: State<'_, Shared>) -> AppResult<bool> {
    let st = state.inner().clone();
    crate::music_stream_engine::update(&st.db, &st.data_dir).await
}

/// Find YouTube uploads for songs (e.g. the missing part of an import) and add them as streaming tracks.
#[tauri::command]
pub async fn music_stream_resolve(
    state: State<'_, Shared>,
    items: Vec<crate::music_import::RemoteTrack>,
    on_progress: Channel<crate::music_stream_engine::ResolveProgress>,
) -> AppResult<Vec<crate::music_stream_engine::Resolved>> {
    let st = state.inner().clone();
    if !crate::settings::load(&st.db).music_streaming {
        return Err(AppError::Invalid("turn on streaming in Music › Sources first".into()));
    }
    if items.len() > 2000 {
        return Err(AppError::Invalid("too many songs at once (2,000 max)".into()));
    }
    Ok(crate::music_stream_engine::resolve_many(&st.db, items, &|p| {
        let _ = on_progress.send(p);
    })
    .await)
}

/// Get the next song's stream link ready (runs in the background).
#[tauri::command]
pub fn music_stream_prepare(state: State<'_, Shared>, id: i64) {
    let st = state.inner().clone();
    if !crate::settings::load(&st.db).music_streaming {
        return;
    }
    std::thread::spawn(move || crate::music_stream_engine::prepare(&st.db, &st.data_dir, id));
}

/// "Wrong song": find a different upload for a streaming song.
#[tauri::command]
pub async fn music_stream_rematch(state: State<'_, Shared>, id: i64) -> AppResult<Option<String>> {
    let st = state.inner().clone();
    blocking(move || crate::music_stream_engine::rematch(&st.db, id)).await
}

/// Remove streaming songs that no playlist uses and that aren't liked.
#[tauri::command]
pub fn music_stream_prune(state: State<'_, Shared>) -> AppResult<usize> {
    // The user asked: no grace period (the automatic cleanup keeps its hour).
    crate::music::prune_streams(&state.db, chrono::Duration::zero())
}

#[tauri::command]
pub fn music_stream_error(id: i64) -> Option<String> {
    crate::music_stream_engine::last_error(id)
}

#[tauri::command]
pub fn music_delete_playlist(state: State<'_, Shared>, id: String) -> AppResult<()> {
    crate::music::delete_playlist(&state.db, &id)
}

#[tauri::command]
/// Lyrics: an .lrc file next to the song or lyrics stored in it first; then,
/// if `online` (the user's setting), LRCLIB. Network trouble shows as "none".
pub async fn music_lyrics(state: State<'_, Shared>, id: i64, online: Option<bool>) -> AppResult<Option<crate::music::Lyrics>> {
    let st = state.inner().clone();
    let db = st.db.clone();
    let mut found = blocking(move || crate::music::lyrics(&db, id)).await?;
    if found.is_none() && online.unwrap_or(false) {
        found = match crate::lyrics_online::lyrics(&st.db, id).await {
            Ok(l) => l,
            Err(e) => {
                tracing::info!("online lyrics: {e}");
                None
            }
        };
    }
    if let Some(l) = found.as_mut() {
        l.offset_ms = crate::music::lyrics_offset(&st.db, id);
    }
    Ok(found)
}

/// The user's timing nudge for a song's lyrics (ms, positive = earlier; ±15 s).
#[tauri::command]
pub fn music_set_lyrics_offset(state: State<'_, Shared>, id: i64, offset_ms: i64) -> AppResult<i64> {
    crate::music::set_lyrics_offset(&state.db, id, offset_ms)
}

#[tauri::command]
pub fn music_session(state: State<'_, Shared>) -> crate::music::Session {
    crate::music::session(&state.db)
}

#[tauri::command]
pub fn music_save_session(state: State<'_, Shared>, session: crate::music::Session) -> AppResult<()> {
    crate::music::save_session(&state.db, session)
}

// -------------------------------------------------------------- library

#[tauri::command]
pub fn library_list(state: State<'_, Shared>) -> AppResult<Vec<crate::library::Item>> {
    crate::library::list(&state.db)
}

#[tauri::command]
pub async fn library_start_apps(refresh: Option<bool>) -> AppResult<Vec<crate::library::StartApp>> {
    blocking(move || crate::library::start_apps(refresh.unwrap_or(false))).await
}

/// Real icons for Start-menu apps (only AppIDs in the current Start list).
#[tauri::command]
pub async fn library_icons(state: State<'_, Shared>, app_ids: Vec<String>) -> AppResult<std::collections::HashMap<String, Option<String>>> {
    let dir = state.data_dir.clone();
    blocking(move || crate::library_icons::icons(&dir, &app_ids)).await
}

#[derive(Serialize)]
pub struct ServiceView {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    web: &'static str,
    app_installed: bool,
    has_app: bool,
    deep_links: Vec<&'static str>,
}

#[tauri::command]
pub fn library_services() -> Vec<ServiceView> {
    crate::library::SERVICES
        .iter()
        .map(|s| ServiceView {
            id: s.id,
            name: s.name,
            category: s.category,
            description: s.description,
            web: s.web,
            app_installed: s.scheme.is_some_and(crate::library::scheme_installed),
            has_app: s.scheme.is_some(),
            deep_links: s.deep_links.iter().map(|(l, _)| *l).collect(),
        })
        .collect()
}

#[tauri::command]
pub async fn library_add(state: State<'_, Shared>, items: Vec<crate::library::NewItem>) -> AppResult<Vec<crate::library::Item>> {
    let st = state.inner().clone();
    blocking(move || crate::library::add(&st.db, &items)).await
}

#[tauri::command]
pub fn library_edit(state: State<'_, Shared>, edit: crate::library::Edit) -> AppResult<crate::library::Item> {
    crate::library::edit(&state.db, &edit)
}

#[tauri::command]
pub fn library_remove(state: State<'_, Shared>, id: String) -> AppResult<()> {
    crate::library::remove(&state.db, &id)
}

#[tauri::command]
pub async fn library_launch(state: State<'_, Shared>, id: String, deep_link: Option<String>) -> AppResult<crate::library::Launched> {
    let st = state.inner().clone();
    blocking(move || crate::library::launch(&st.db, &id, deep_link.as_deref())).await
}

/// The UI preference document (themes, layout, navigation, widgets, player
/// mode…). The frontend validates every field and falls back to defaults, so
/// the backend only enforces shape and size.
const PREFS_KEY: &str = "ui.prefs";
const PREFS_MAX_BYTES: usize = 256 * 1024;

#[tauri::command]
pub fn get_prefs(state: State<'_, Shared>) -> AppResult<Value> {
    Ok(state.db.get_setting::<Value>(PREFS_KEY)?.unwrap_or(Value::Null))
}

#[tauri::command]
pub fn save_prefs(state: State<'_, Shared>, prefs: Value) -> AppResult<()> {
    if !prefs.is_object() {
        return Err(AppError::Invalid("preferences must be an object".into()));
    }
    if prefs.to_string().len() > PREFS_MAX_BYTES {
        return Err(AppError::Invalid("preferences are too large (custom images belong in files, not preferences)".into()));
    }
    state.db.set_setting(PREFS_KEY, &prefs)
}

#[tauri::command]
pub fn list_audit(state: State<'_, Shared>, limit: Option<i64>) -> AppResult<Vec<Value>> {
    let conn = state.db.conn();
    let mut stmt = conn.prepare("SELECT ts, level, actor, event, detail FROM audit_log ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit.unwrap_or(200).clamp(1, 2000)], |r| {
        Ok(serde_json::json!({
            "ts": r.get::<_, String>(0)?,
            "level": r.get::<_, String>(1)?,
            "actor": r.get::<_, String>(2)?,
            "event": r.get::<_, String>(3)?,
            "detail": r.get::<_, Option<String>>(4)?.and_then(|d| serde_json::from_str::<Value>(&d).ok()),
        }))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// --------------------------------------------------------------- system

#[tauri::command]
pub async fn get_system_info(state: State<'_, Shared>, refresh: Option<bool>) -> AppResult<SystemInfo> {
    let st = state.inner().clone();
    blocking(move || Ok(st.system_info(refresh.unwrap_or(false)))).await
}

#[tauri::command]
pub fn get_telemetry_history(state: State<'_, Shared>) -> Vec<monitor::Sample> {
    state.telemetry.read().map(|t| t.history.iter().cloned().collect()).unwrap_or_default()
}

#[tauri::command]
pub fn get_processes(state: State<'_, Shared>) -> Vec<monitor::ProcessInfo> {
    state.telemetry.read().map(|t| t.processes.clone()).unwrap_or_default()
}

#[tauri::command]
pub async fn end_process(state: State<'_, Shared>, pid: u32, name: String) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let res = processes::end_process(pid, &name);
        st.db.audit(
            if res.is_ok() { "info" } else { "warn" },
            "user",
            "process.end",
            Some(serde_json::json!({ "pid": pid, "name": name, "ok": res.is_ok(), "error": res.as_ref().err().map(|e| e.to_string()) })),
        );
        res
    })
    .await
}

/// Close whole apps by process name (every instance). Used by approved AI
/// plans and bulk actions. Each name is checked against the protected list and
/// every closure is written to the audit log with who asked for it.
#[tauri::command]
pub async fn close_apps(
    state: State<'_, Shared>,
    names: Vec<String>,
    source: Option<String>,
) -> AppResult<Vec<processes::CloseResult>> {
    if names.is_empty() || names.len() > 20 {
        return Err(AppError::Invalid("between 1 and 20 app names are required".into()));
    }
    if names.iter().any(|n| n.trim().is_empty() || n.len() > 260 || n.contains(['\\', '/'])) {
        return Err(AppError::Invalid("app names must be plain process names like Spotify.exe".into()));
    }
    let actor = if source.as_deref() == Some("ai") { "ai" } else { "user" };
    let st = state.inner().clone();
    blocking(move || {
        let results: Vec<_> = names.iter().map(|n| processes::close_app(n.trim())).collect();
        for r in &results {
            st.db.audit(
                if r.failed == 0 && r.ended > 0 { "info" } else { "warn" },
                actor,
                "process.close_app",
                Some(serde_json::json!({ "name": r.name, "ended": r.ended, "failed": r.failed, "error": r.error })),
            );
        }
        Ok(results)
    })
    .await
}

// ------------------------------------------------------------- hardware

/// Per-minute history for the last `minutes` (max 7 days), ≤ 360 points.
#[tauri::command]
pub async fn get_telemetry_range(state: State<'_, Shared>, minutes: i64) -> AppResult<Vec<crate::system::history::HistoryPoint>> {
    let st = state.inner().clone();
    blocking(move || crate::system::history::range(&st.db, chrono::Utc::now().timestamp_millis(), minutes, 360)).await
}

#[tauri::command]
pub async fn get_storage_health() -> AppResult<crate::system::info::StorageHealth> {
    blocking(|| Ok(crate::system::info::storage_health())).await
}

// ------------------------------------------------------------ processes

#[tauri::command]
pub fn process_overview(state: State<'_, Shared>) -> process_view::ProcOverview {
    process_view::overview(&state)
}

#[tauri::command]
pub async fn process_details(state: State<'_, Shared>, name: String) -> AppResult<process_view::ProcDetails> {
    let st = state.inner().clone();
    blocking(move || process_view::details(&st, &name)).await
}

/// Runtime change for every running instance; lasts until the app exits.
#[tauri::command]
pub async fn set_process_priority(
    state: State<'_, Shared>,
    name: String,
    priority: process_control::Priority,
) -> AppResult<Vec<process_view::ChangeOutcome>> {
    let st = state.inner().clone();
    blocking(move || {
        let res = process_view::set_priority_all(&st, &name, priority);
        st.db.audit(
            if res.is_ok() { "info" } else { "warn" },
            "user",
            "process.priority",
            Some(serde_json::json!({ "name": name, "priority": priority, "result": res.as_ref().map(|r| r.iter().filter(|o| o.ok).count()).map_err(|e| e.to_string()) })),
        );
        res
    })
    .await
}

#[tauri::command]
pub async fn set_process_affinity(
    state: State<'_, Shared>,
    name: String,
    mask: String,
) -> AppResult<Vec<process_view::ChangeOutcome>> {
    let m = crate::system::rules::parse_mask(&mask).ok_or_else(|| AppError::Invalid(format!("'{mask}' is not a valid CPU mask")))?;
    let st = state.inner().clone();
    blocking(move || {
        let res = process_view::set_affinity_all(&st, &name, m);
        st.db.audit(
            if res.is_ok() { "info" } else { "warn" },
            "user",
            "process.affinity",
            Some(serde_json::json!({ "name": name, "mask": format!("0x{m:x}"), "result": res.as_ref().map(|r| r.iter().filter(|o| o.ok).count()).map_err(|e| e.to_string()) })),
        );
        res
    })
    .await
}

#[tauri::command]
pub fn list_process_rules(state: State<'_, Shared>) -> AppResult<Vec<crate::system::rules::ProcessRule>> {
    crate::system::rules::list(&state.db)
}

/// Saving a rule also applies it to instances already running.
#[tauri::command]
pub async fn save_process_rule(
    state: State<'_, Shared>,
    rule: crate::system::rules::ProcessRule,
) -> AppResult<crate::system::rules::ProcessRule> {
    let st = state.inner().clone();
    blocking(move || {
        let saved = crate::system::rules::save(&st.db, &st.rules, rule)?;
        if saved.enabled {
            let running: Vec<(u32, String)> = st
                .telemetry
                .read()
                .map(|t| t.processes.iter().filter(|p| p.name.eq_ignore_ascii_case(&saved.name)).map(|p| (p.pid, p.name.clone())).collect())
                .unwrap_or_default();
            crate::system::rules::apply_to_new(&st.db, &st.rules, &running);
        }
        Ok(crate::system::rules::get(&st.db, &saved.name)?.unwrap_or(saved))
    })
    .await
}

#[tauri::command]
pub fn delete_process_rule(state: State<'_, Shared>, name: String) -> AppResult<()> {
    crate::system::rules::delete(&state.db, &state.rules, &name)
}

#[tauri::command]
pub async fn get_health(state: State<'_, Shared>, force: Option<bool>) -> AppResult<HealthReport> {
    let st = state.inner().clone();
    blocking(move || Ok(st.health(force.unwrap_or(false)))).await
}

#[tauri::command]
pub async fn get_events(state: State<'_, Shared>) -> AppResult<Vec<diagnostics::EventEntry>> {
    let st = state.inner().clone();
    blocking(move || st.events(std::time::Duration::from_secs(60)).ok_or_else(|| AppError::msg("could not read the event log"))).await
}

#[tauri::command]
pub async fn get_crash_report() -> AppResult<crate::system::crash::CrashReport> {
    blocking(crate::system::crash::report).await
}

#[tauri::command]
pub async fn get_driver_centre(state: State<'_, Shared>) -> AppResult<crate::system::driver_centre::DriverReport> {
    let st = state.inner().clone();
    blocking(move || {
        let info = st.system_info(false);
        crate::system::driver_centre::report(
            info.board.board_manufacturer.as_deref().unwrap_or(""),
            info.board.system_manufacturer.as_deref().unwrap_or(""),
        )
    })
    .await
}

/// Opens one of the driver centre's fixed official support pages.
#[tauri::command]
pub fn open_support_link(state: State<'_, Shared>, key: String) -> AppResult<()> {
    crate::system::driver_centre::open(&state.db, &key)
}

#[tauri::command]
pub async fn get_drivers() -> AppResult<Vec<diagnostics::DriverEntry>> {
    blocking(diagnostics::drivers).await
}

#[tauri::command]
pub async fn get_services() -> AppResult<Vec<diagnostics::ServiceEntry>> {
    blocking(diagnostics::services).await
}

#[tauri::command]
pub async fn get_restore_status() -> AppResult<diagnostics::RestoreStatus> {
    blocking(diagnostics::restore_status).await
}

// -------------------------------------------------------------- network

#[tauri::command]
pub async fn network_overview() -> AppResult<network::NetworkOverview> {
    blocking(|| Ok(network::overview())).await
}

#[tauri::command]
pub async fn network_test(state: State<'_, Shared>, count: Option<u32>) -> AppResult<network::StabilityReport> {
    let st = state.inner().clone();
    blocking(move || {
        let r = network::stability_test(count.unwrap_or(10));
        if let Err(e) = network::save_report(&st.db, &r) {
            tracing::warn!("could not store network test: {e}");
        }
        Ok(r)
    })
    .await
}

#[tauri::command]
pub fn network_history(state: State<'_, Shared>, limit: Option<i64>) -> AppResult<Vec<network::StoredPing>> {
    network::history(&state.db, limit.unwrap_or(300).clamp(1, 3000))
}

#[tauri::command]
pub async fn traceroute(host: String, on_hop: Channel<network::Hop>) -> AppResult<network::TraceReport> {
    blocking(move || {
        network::traceroute(&host, &|h| {
            let _ = on_hop.send(h.clone());
        })
    })
    .await
}

#[tauri::command]
pub fn dns_providers() -> &'static [dns::DnsProvider] {
    dns::PROVIDERS
}

#[tauri::command]
pub async fn preview_dns(index: u32, alias: String, provider: String) -> AppResult<Vec<dns::DnsPreviewItem>> {
    blocking(move || dns::preview(index, &alias, &provider)).await
}

#[tauri::command]
pub async fn set_dns(
    state: State<'_, Shared>,
    index: u32,
    alias: String,
    provider: String,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::ApplyReport> {
    let st = state.inner().clone();
    blocking(move || {
        dns::apply(&st.db, &st.data_dir, index, &alias, &provider, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await
}

// ---------------------------------------------------------------- power

#[tauri::command]
pub async fn power_plans() -> AppResult<Vec<power::PowerScheme>> {
    blocking(power::list).await
}

#[tauri::command]
pub async fn set_power_plan(
    state: State<'_, Shared>,
    guid: String,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::ApplyReport> {
    let st = state.inner().clone();
    blocking(move || {
        let g = crate::security::exec::SchemeGuid::parse(&guid)?;
        let name = power::list()?
            .into_iter()
            .find(|s| s.guid == g.as_str())
            .map(|s| s.name)
            .ok_or_else(|| AppError::NotFound("that power plan isn't installed".into()))?;
        let plan = vec![engine::PlannedOp {
            tweak_id: "power".into(),
            title: format!("Switch power plan to {name}"),
            takes: Takes::Immediately,
            op: ChangeOp::PowerPlan { scheme: g.as_str().into() },
        }];
        let r = engine::run_plan(&st.db, &st.data_dir, plan, "user", &format!("Power plan: {name}"), false, &|p| {
            let _ = on_progress.send(p);
        });
        st.invalidate_health();
        r
    })
    .await
}

// --------------------------------------------------------------- tweaks

#[tauri::command]
pub async fn list_tweaks() -> AppResult<Vec<engine::TweakView>> {
    blocking(|| Ok(engine::list_views())).await
}

#[tauri::command]
pub fn list_profiles() -> Vec<catalog::Profile> {
    catalog::profiles()
}

#[tauri::command]
pub async fn preview_tweaks(ids: Vec<String>) -> AppResult<engine::Preview> {
    blocking(move || engine::preview(&ids)).await
}

#[tauri::command]
pub async fn apply_tweaks(
    state: State<'_, Shared>,
    request: engine::ApplyRequest,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::ApplyReport> {
    let st = state.inner().clone();
    blocking(move || {
        let r = engine::apply(&st.db, &st.data_dir, request, &|p| {
            let _ = on_progress.send(p);
        });
        st.invalidate_health();
        r
    })
    .await
}

#[tauri::command]
pub async fn rollback_transaction(
    state: State<'_, Shared>,
    tx_id: String,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::RollbackReport> {
    let st = state.inner().clone();
    blocking(move || {
        let r = engine::rollback(&st.db, &tx_id, &|p| {
            let _ = on_progress.send(p);
        });
        // Customise changes take effect without signing out once the shell is told.
        crate::customise::notify_shell();
        st.invalidate_health();
        r
    })
    .await
}

// ---------------------------------------------------------- app profiles

#[derive(Serialize)]
pub struct AppProfilesView {
    profiles: Vec<crate::system::app_profiles::AppProfile>,
    active: Option<crate::system::app_profiles::Active>,
    /// Programs with a window right now, as suggestions.
    running_apps: Vec<String>,
}

#[tauri::command]
pub fn list_app_profiles(state: State<'_, Shared>) -> AppProfilesView {
    let mut running: Vec<String> = state
        .telemetry
        .read()
        .map(|t| t.processes.iter().filter(|p| p.has_window && !crate::system::processes::is_protected(&p.name)).map(|p| p.name.clone()).collect())
        .unwrap_or_default();
    running.sort_by_key(|n| n.to_lowercase());
    running.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    AppProfilesView {
        profiles: crate::system::app_profiles::list(&state.db),
        active: crate::system::app_profiles::active(&state.db),
        running_apps: running,
    }
}

#[tauri::command]
pub async fn save_app_profile(state: State<'_, Shared>, profile: crate::system::app_profiles::AppProfile) -> AppResult<crate::system::app_profiles::AppProfile> {
    let st = state.inner().clone();
    blocking(move || crate::system::app_profiles::save(&st.db, &st.rules, profile)).await
}

#[tauri::command]
pub fn delete_app_profile(state: State<'_, Shared>, exe: String) -> AppResult<()> {
    crate::system::app_profiles::delete(&state.db, &state.rules, &exe)
}

// ----------------------------------------------------------- maintenance

#[derive(Serialize)]
pub struct MaintenanceOverview {
    tasks: Vec<crate::system::scheduler::TaskView>,
    history: Vec<crate::system::scheduler::RunRecord>,
}

#[tauri::command]
pub fn maintenance_overview(state: State<'_, Shared>) -> AppResult<MaintenanceOverview> {
    Ok(MaintenanceOverview {
        tasks: crate::system::scheduler::overview(&state.db),
        history: crate::system::scheduler::history(&state.db, 100)?,
    })
}

#[tauri::command]
pub fn set_maintenance_task(state: State<'_, Shared>, task: String, enabled: bool, interval_days: u32) -> AppResult<()> {
    let id = crate::system::scheduler::parse_task(&task)?;
    crate::system::scheduler::set_config(&state.db, id, crate::system::scheduler::TaskConfig { enabled, interval_days })
}

#[tauri::command]
pub async fn run_maintenance_task(app: tauri::AppHandle, state: State<'_, Shared>, task: String) -> AppResult<crate::system::scheduler::RunRecord> {
    let id = crate::system::scheduler::parse_task(&task)?;
    let st = state.inner().clone();
    blocking(move || crate::system::scheduler::run_now(Some(&app), &st, id, "manual")).await
}

// ------------------------------------------------------------ benchmarks

#[tauri::command]
pub async fn run_benchmark(
    state: State<'_, Shared>,
    suite: crate::system::bench::Suite,
    target: Option<String>,
    label: String,
    on_progress: Channel<crate::system::bench::BenchProgress>,
) -> AppResult<crate::system::bench::BenchRun> {
    let st = state.inner().clone();
    if st.games.live.lock().ok().is_some_and(|l| l.is_some()) {
        return Err(AppError::Invalid("a game is running — benchmark results would be wrong, and the game would stutter. Close it first.".into()));
    }
    blocking(move || {
        crate::system::bench::run(&st.db, suite, target, &label, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await
}

#[tauri::command]
pub fn cancel_benchmark() {
    crate::system::bench::cancel();
}

#[tauri::command]
pub fn list_benchmarks(state: State<'_, Shared>) -> AppResult<Vec<crate::system::bench::BenchRun>> {
    crate::system::bench::history(&state.db, 200)
}

#[tauri::command]
pub fn delete_benchmark(state: State<'_, Shared>, id: i64) -> AppResult<()> {
    crate::system::bench::delete(&state.db, id)
}

#[tauri::command]
pub fn rename_benchmark(state: State<'_, Shared>, id: i64, label: String) -> AppResult<()> {
    crate::system::bench::rename(&state.db, id, &label)
}

#[tauri::command]
pub async fn boot_times() -> AppResult<crate::system::bench::BootReport> {
    blocking(crate::system::bench::boot_report).await
}

// ------------------------------------------------------------- services

#[tauri::command]
pub async fn list_services() -> AppResult<Vec<crate::system::services::ServiceView>> {
    blocking(crate::system::services::list).await
}

#[tauri::command]
pub async fn preview_services(selections: Vec<crate::system::services::Selection>) -> AppResult<Vec<crate::system::services::PreviewItem>> {
    blocking(move || crate::system::services::preview(&selections)).await
}

#[tauri::command]
pub async fn apply_services(
    state: State<'_, Shared>,
    selections: Vec<crate::system::services::Selection>,
    source: Option<String>,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::ApplyReport> {
    let source = source.unwrap_or_else(|| "user".into());
    let st = state.inner().clone();
    blocking(move || {
        let r = crate::system::services::apply(&st.db, &st.data_dir, &selections, &source, &|p| {
            let _ = on_progress.send(p);
        });
        st.invalidate_health();
        r
    })
    .await
}

#[tauri::command]
pub fn list_service_changes(state: State<'_, Shared>) -> AppResult<Vec<crate::startup_insight::StartupChange>> {
    crate::startup_insight::recent_changes_for(&state.db, crate::system::services::JOURNAL_ID, 30)
}

// ------------------------------------------------------------ customise

#[tauri::command]
pub async fn list_customise() -> AppResult<Vec<crate::customise::SettingView>> {
    blocking(|| Ok(crate::customise::list())).await
}

/// `selections` picks choices; `reset` (one section) or `reset_all` instead
/// returns every setting in that scope to the Windows default.
fn customise_selection(
    selections: Option<Vec<crate::customise::Selection>>,
    reset: Option<crate::customise::Section>,
    reset_all: bool,
) -> AppResult<Vec<crate::customise::Selection>> {
    match (selections, reset, reset_all) {
        (Some(s), None, false) => Ok(s),
        (None, Some(sec), false) => Ok(crate::customise::reset_selection(Some(sec))),
        (None, None, true) => Ok(crate::customise::reset_selection(None)),
        _ => Err(AppError::Invalid("give either selections or a reset target".into())),
    }
}

#[tauri::command]
pub async fn preview_customise(
    selections: Option<Vec<crate::customise::Selection>>,
    reset: Option<crate::customise::Section>,
    reset_all: Option<bool>,
) -> AppResult<Vec<crate::customise::PreviewItem>> {
    blocking(move || crate::customise::preview(&customise_selection(selections, reset, reset_all.unwrap_or(false))?)).await
}

#[tauri::command]
pub async fn apply_customise(
    state: State<'_, Shared>,
    selections: Option<Vec<crate::customise::Selection>>,
    reset: Option<crate::customise::Section>,
    reset_all: Option<bool>,
    label: String,
    source: Option<String>,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::ApplyReport> {
    let source = source.unwrap_or_else(|| "user".into());
    let st = state.inner().clone();
    blocking(move || {
        let sel = customise_selection(selections, reset, reset_all.unwrap_or(false))?;
        crate::customise::apply(&st.db, &st.data_dir, &sel, &label, &source, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await
}

#[tauri::command]
pub async fn restart_explorer(state: State<'_, Shared>) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || crate::customise::restart_explorer(&st.db)).await
}

#[tauri::command]
pub fn list_history(state: State<'_, Shared>, limit: Option<i64>) -> AppResult<Vec<journal::TxSummary>> {
    journal::list_tx(&state.db, limit.unwrap_or(100).clamp(1, 1000))
}

#[derive(Serialize)]
pub struct TxDetail {
    summary: journal::TxSummary,
    changes: Vec<journal::ChangeRow>,
}

#[tauri::command]
pub fn get_transaction(state: State<'_, Shared>, tx_id: String) -> AppResult<TxDetail> {
    Ok(TxDetail { summary: journal::get_tx(&state.db, &tx_id)?, changes: journal::changes_for(&state.db, &tx_id)? })
}

// -------------------------------------------------------------- startup

#[tauri::command]
pub async fn list_startup() -> AppResult<Vec<crate::startup::StartupItem>> {
    blocking(|| Ok(crate::startup_insight::with_insights())).await
}

#[tauri::command]
pub fn list_startup_changes(state: State<'_, Shared>) -> AppResult<Vec<crate::startup_insight::StartupChange>> {
    crate::startup_insight::recent_changes(&state.db, 30)
}

#[tauri::command]
pub async fn set_startup_enabled(
    state: State<'_, Shared>,
    ids: Vec<String>,
    enabled: bool,
    source: Option<String>,
    on_progress: Channel<engine::Progress>,
) -> AppResult<engine::ApplyReport> {
    let st = state.inner().clone();
    let source = match source.as_deref() {
        Some("ai") => "ai",
        _ => "user",
    };
    blocking(move || {
        let r = crate::startup::set_enabled(&st.db, &st.data_dir, &ids, enabled, source, &|p| {
            let _ = on_progress.send(p);
        });
        st.invalidate_health();
        r
    })
    .await
}

// ------------------------------------------------------------------- AI

#[derive(Serialize)]
pub struct AiKeyStatus {
    anthropic: bool,
    openai_compatible: bool,
}

#[tauri::command]
pub fn ai_key_status() -> AiKeyStatus {
    AiKeyStatus {
        anthropic: secrets::exists(ai::SECRET_ANTHROPIC),
        openai_compatible: secrets::exists(ai::SECRET_OPENAI),
    }
}

#[tauri::command]
pub fn ai_set_key(state: State<'_, Shared>, provider: String, key: Option<String>) -> AppResult<()> {
    let name = ai::secret_name(&provider);
    match key.filter(|k| !k.trim().is_empty()) {
        Some(k) => secrets::set(name, &k)?,
        None => secrets::delete(name)?,
    }
    state.db.audit("info", "user", "ai.key_updated", Some(serde_json::json!({ "provider": provider })));
    Ok(())
}

#[tauri::command]
pub async fn ai_test_connection(state: State<'_, Shared>) -> AppResult<provider::ConnectionTest> {
    let s = settings::load(&state.db).ai;
    let key = secrets::get(ai::secret_name(&s.provider))?;
    provider::test_connection(&s, key).await
}

#[tauri::command]
pub async fn ai_send(
    state: State<'_, Shared>,
    conversation_id: Option<String>,
    text: String,
    meta: Option<Value>,
    on_event: Channel<chat::ChatEvent>,
) -> AppResult<String> {
    chat::send(state.inner().clone(), conversation_id, text, meta, on_event).await
}

#[tauri::command]
pub fn ai_cancel(state: State<'_, Shared>, conversation_id: String) -> bool {
    chat::cancel(state.inner(), &conversation_id)
}

#[tauri::command]
pub fn ai_list_conversations(state: State<'_, Shared>) -> AppResult<Vec<conv_store::ConversationSummary>> {
    conv_store::list(&state.db)
}

#[tauri::command]
pub fn ai_get_conversation(state: State<'_, Shared>, id: String) -> AppResult<Vec<conv_store::StoredMessage>> {
    conv_store::messages(&state.db, &id)
}

#[tauri::command]
pub fn ai_rename_conversation(state: State<'_, Shared>, id: String, title: String) -> AppResult<()> {
    conv_store::rename(&state.db, &id, &title)
}

#[tauri::command]
pub fn ai_delete_conversation(state: State<'_, Shared>, id: String) -> AppResult<()> {
    conv_store::delete(&state.db, &id)
}

// --------------------------------------------------------------- gaming

#[derive(serde::Serialize)]
pub struct GameView {
    #[serde(flatten)]
    pub game: crate::games::detect::Game,
    pub running: bool,
    pub profile: Option<crate::games::profile::GameProfile>,
    pub sessions: usize,
    pub not_game: bool,
}

#[tauri::command]
pub async fn list_games(state: State<'_, Shared>, refresh: Option<bool>) -> AppResult<Vec<GameView>> {
    let st = state.inner().clone();
    blocking(move || {
        let cached = st.games.games.read().map(|g| g.clone()).unwrap_or_default();
        let games = if refresh.unwrap_or(false) || cached.is_empty() { crate::games::refresh(&st.db, &st.games) } else { cached };
        let procs = st.telemetry.read().map(|t| t.processes.clone()).unwrap_or_default();
        let hidden = crate::games::not_games(&st.db);
        let playable: Vec<_> = games.iter().filter(|g| !hidden.contains(&g.id)).cloned().collect();
        let running = crate::games::session::running_game(&playable, &procs).map(|(g, _)| g.id.clone());
        let profiles = crate::games::profile::all(&st.db).unwrap_or_default();
        let counts: std::collections::HashMap<String, usize> = crate::games::session::list(&st.db, None, 1000)
            .unwrap_or_default()
            .into_iter()
            .fold(Default::default(), |mut m, s| {
                *m.entry(s.game_id).or_default() += 1;
                m
            });
        Ok(games
            .into_iter()
            .map(|g| GameView {
                running: running.as_deref() == Some(g.id.as_str()),
                profile: profiles.iter().find(|p| p.game_id == g.id).cloned(),
                sessions: counts.get(&g.id).copied().unwrap_or(0),
                not_game: hidden.contains(&g.id),
                game: g,
            })
            .collect())
    })
    .await
}

/// Windows 11 window material behind the app: "mica", "acrylic" or "solid".
/// Fails on systems without it (e.g. Windows 10) — the page then stays solid.
#[tauri::command]
pub fn set_window_backdrop(app: tauri::AppHandle, kind: String, dark: bool) -> AppResult<()> {
    use tauri::window::{Effect, EffectsBuilder};
    use tauri::Manager;
    let w = app.get_webview_window("main").ok_or_else(|| AppError::msg("no main window"))?;
    let effect = match kind.as_str() {
        "mica" => Some(if dark { Effect::MicaDark } else { Effect::MicaLight }),
        "acrylic" => Some(Effect::Acrylic),
        "solid" => None,
        _ => return Err(AppError::Invalid("unknown backdrop".into())),
    };
    match effect {
        Some(e) => w.set_effects(EffectsBuilder::new().effect(e).build()),
        None => w.set_effects(None),
    }
    .map_err(|e| AppError::msg(format!("Windows didn't apply the effect: {e}")))
}

/// Detected games (cached), refreshing once if nothing is known yet.
fn known_games(st: &Shared) -> Vec<crate::games::detect::Game> {
    let cached = st.games.games.read().map(|g| g.clone()).unwrap_or_default();
    if cached.is_empty() {
        crate::games::refresh(&st.db, &st.games)
    } else {
        cached
    }
}

/// Cover art for detected games (data URLs; None = use a placeholder).
#[tauri::command]
pub async fn games_covers(state: State<'_, Shared>, ids: Vec<String>) -> AppResult<std::collections::HashMap<String, Option<String>>> {
    let st = state.inner().clone();
    blocking(move || Ok(crate::games::art::covers(&st.data_dir, &known_games(&st), &ids))).await
}

/// Start a detected game through its own launcher.
#[tauri::command]
pub async fn games_launch(state: State<'_, Shared>, id: String) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let games = known_games(&st);
        let g = games.iter().find(|g| g.id == id).ok_or_else(|| AppError::NotFound("game".into()))?;
        crate::games::art::launch(g)?;
        st.db.audit("info", "user", "game.launch", Some(serde_json::json!({ "game": g.id, "via": g.launch_via })));
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn add_custom_game(state: State<'_, Shared>, exe: String) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        crate::games::add_custom(&st.db, exe.trim())?;
        crate::games::refresh(&st.db, &st.games);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_custom_game(state: State<'_, Shared>, exe: String) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        crate::games::remove_custom(&st.db, &exe)?;
        crate::games::refresh(&st.db, &st.games);
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn set_not_game(state: State<'_, Shared>, id: String, not_game: bool) -> AppResult<()> {
    crate::games::set_not_game(&state.db, &state.games, &id, not_game)
}

#[derive(serde::Serialize)]
pub struct GamingNow {
    pub live: Option<crate::games::session::SessionSummary>,
    pub game_cpu: Option<f32>,
    pub game_mem: Option<u64>,
    pub overlays: Vec<crate::games::session::Overlay>,
}

#[tauri::command]
pub fn gaming_now(state: State<'_, Shared>) -> GamingNow {
    let procs = state.telemetry.read().map(|t| t.processes.clone()).unwrap_or_default();
    let live = state.games.live.lock().ok().and_then(|l| l.clone());
    GamingNow {
        game_cpu: live.as_ref().map(|l| l.game_cpu),
        game_mem: live.as_ref().map(|l| l.game_mem),
        live: live.map(|l| l.summary()),
        overlays: crate::games::session::running_overlays(&procs),
    }
}

#[tauri::command]
pub fn list_game_sessions(state: State<'_, Shared>, game_id: Option<String>) -> AppResult<Vec<crate::games::session::SessionSummary>> {
    crate::games::session::list(&state.db, game_id.as_deref(), 50)
}

#[tauri::command]
pub async fn preview_game_profile(
    state: State<'_, Shared>,
    profile: crate::games::profile::GameProfile,
) -> AppResult<Vec<crate::games::profile::ProfilePreviewItem>> {
    let st = state.inner().clone();
    blocking(move || crate::games::profile::preview(&st.db, &profile)).await
}

#[tauri::command]
pub async fn apply_game_profile(
    state: State<'_, Shared>,
    profile: crate::games::profile::GameProfile,
    game_name: String,
    on_progress: Channel<engine::Progress>,
) -> AppResult<crate::games::profile::ProfileOutcome> {
    let st = state.inner().clone();
    blocking(move || {
        crate::games::profile::apply(&st.db, &st.data_dir, &st.rules, profile, &game_name, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await
}

// -------------------------------------------------------------- storage

#[derive(Serialize)]
pub struct StorageRoot {
    pub path: String,
    pub label: String,
    pub total: Option<u64>,
    pub free: Option<u64>,
}

#[tauri::command]
pub fn storage_roots() -> Vec<StorageRoot> {
    let disks = sysinfo::Disks::new_with_refreshed_list().ok();
    let mut out: Vec<StorageRoot> = disks
        .iter()
        .flat_map(|d| d.iter())
        .map(|d| {
            let path = d.mount_point().display().to_string();
            let name = d.name().to_string_lossy().to_string();
            StorageRoot {
                label: if name.is_empty() { format!("Drive {path}") } else { format!("{name} ({})", path.trim_end_matches('\\')) },
                path,
                total: Some(d.total_space()),
                free: Some(d.available_space()),
            }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    if let Ok(home) = std::env::var("USERPROFILE") {
        out.insert(0, StorageRoot { label: "Your user folder".into(), path: home, total: None, free: None });
    }
    out
}

#[tauri::command]
pub async fn storage_scan(
    state: State<'_, Shared>,
    root: String,
    on_progress: Channel<crate::storage::scan::ScanProgress>,
) -> AppResult<crate::storage::ScanSummary> {
    let st = state.inner().clone();
    blocking(move || crate::storage::run_scan(&st.storage, &root, &|p| {
        let _ = on_progress.send(p);
    }))
    .await
}

#[tauri::command]
pub fn storage_cancel(state: State<'_, Shared>) {
    state.storage.cancel();
}

#[tauri::command]
pub fn storage_status(state: State<'_, Shared>) -> Option<crate::storage::ScanSummary> {
    crate::storage::last_summary(&state.storage)
}

#[tauri::command]
pub fn storage_folder(state: State<'_, Shared>, id: Option<u32>) -> AppResult<crate::storage::FolderView> {
    crate::storage::folder(&state.storage, id)
}

#[tauri::command]
pub fn storage_large_files(state: State<'_, Shared>) -> AppResult<Vec<crate::storage::FileView>> {
    crate::storage::large_files(&state.storage)
}

#[tauri::command]
pub async fn storage_find_duplicates(
    state: State<'_, Shared>,
    on_progress: Channel<crate::storage::dupes::DupProgress>,
) -> AppResult<Vec<crate::storage::DupGroupView>> {
    let st = state.inner().clone();
    blocking(move || crate::storage::find_duplicates(&st.storage, &|p| {
        let _ = on_progress.send(p);
    }))
    .await
}

#[tauri::command]
pub fn storage_duplicates(state: State<'_, Shared>) -> Option<Vec<crate::storage::DupGroupView>> {
    crate::storage::last_duplicates(&state.storage)
}

#[tauri::command]
pub async fn storage_recycle(state: State<'_, Shared>, paths: Vec<String>) -> AppResult<crate::storage::RecycleReport> {
    let st = state.inner().clone();
    blocking(move || crate::storage::recycle_files(&st.storage, &st.db, &paths)).await
}

#[tauri::command]
pub async fn recycle_bin_info() -> AppResult<crate::storage::recycle::RecycleInfo> {
    blocking(crate::storage::recycle::query).await
}

#[tauri::command]
pub async fn empty_recycle_bin(state: State<'_, Shared>) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let before = crate::storage::recycle::query().ok();
        crate::storage::recycle::empty()?;
        st.db.audit("info", "user", "storage.recycle_bin_emptied", Some(serde_json::json!({ "bytes": before.map(|b| b.size) })));
        Ok(())
    })
    .await
}
