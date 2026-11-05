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
use crate::system::{diagnostics, health::HealthReport, info::SystemInfo, monitor, network, power, process_control, process_view, processes};
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
pub async fn network_test(count: Option<u32>) -> AppResult<network::StabilityReport> {
    blocking(move || Ok(network::stability_test(count.unwrap_or(10)))).await
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
        st.invalidate_health();
        r
    })
    .await
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
}

#[tauri::command]
pub async fn list_games(state: State<'_, Shared>, refresh: Option<bool>) -> AppResult<Vec<GameView>> {
    let st = state.inner().clone();
    blocking(move || {
        let cached = st.games.games.read().map(|g| g.clone()).unwrap_or_default();
        let games = if refresh.unwrap_or(false) || cached.is_empty() { crate::games::refresh(&st.db, &st.games) } else { cached };
        let procs = st.telemetry.read().map(|t| t.processes.clone()).unwrap_or_default();
        let running = crate::games::session::running_game(&games, &procs).map(|(g, _)| g.id.clone());
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
                game: g,
            })
            .collect())
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
