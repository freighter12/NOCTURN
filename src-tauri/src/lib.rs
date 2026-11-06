//! NOCTURN — Windows optimisation engine and AI desktop assistant.
//!
//! Module map:
//! * `system`   — read-only inspection: hardware, telemetry, network, diagnostics, health
//! * `tweaks`   — NOCTURN TWEAK: catalogue, preview, journaled apply, rollback
//! * `startup`  — Startup Manager (reversible enable/disable)
//! * `ai`       — NOCTURN AI: providers, tools, agent loop, conversations
//! * `security` — allow-listed execution, secrets, elevation
//! * `db`, `settings`, `logging`, `state`, `commands`

mod ai;
mod commands;
mod customise;
mod db;
mod games;
mod library;
mod music;
mod error;
mod logging;
mod security;
mod settings;
mod startup;
mod storage;
mod startup_insight;
mod state;
mod system;
mod tweaks;
mod util;

use std::sync::Arc;

use tauri::Manager;

pub fn run() {
    // When relaunched through UAC as the narrowly scoped elevated worker, do
    // that job and exit without starting the UI.
    if let Some(code) = security::elevation::maybe_run_worker() {
        std::process::exit(code);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        // Nocturn Music playback and artwork: serves only indexed tracks,
        // with byte ranges so seeking works (http://media.localhost/...).
        .register_asynchronous_uri_scheme_protocol("media", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let range = request.headers().get("range").and_then(|v| v.to_str().ok()).map(String::from);
                let served = match app.try_state::<Arc<state::AppState>>() {
                    Some(st) => music::serve(&st.db, request.uri().path(), range.as_deref()),
                    None => music::Served { status: 503, mime: "text/plain".into(), body: b"starting".to_vec(), range: None },
                };
                let mut b = tauri::http::Response::builder()
                    .status(served.status)
                    .header("Content-Type", served.mime)
                    .header("Accept-Ranges", "bytes")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "no-store");
                if let Some((start, end, total)) = served.range {
                    b = b.header("Content-Range", format!("bytes {start}-{end}/{total}"));
                }
                responder.respond(b.body(served.body).unwrap_or_default());
            });
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let log_dir = app.path().app_log_dir().unwrap_or_else(|_| data_dir.join("logs"));
            std::fs::create_dir_all(&data_dir)?;
            if let Some(guard) = logging::init(&log_dir) {
                // Keep the log writer alive for the whole process.
                Box::leak(Box::new(guard));
            }
            tracing::info!(version = env!("CARGO_PKG_VERSION"), elevated = security::elevation::is_elevated(), "NOCTURN starting");

            let db = db::Db::open(&data_dir.join("nocturn.db"))
                .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
            match tweaks::journal::recover_interrupted(&db) {
                Ok(0) => {}
                Ok(n) => tracing::warn!("{n} change(s) were interrupted by an earlier shutdown; they can be rolled back from History"),
                Err(e) => tracing::error!("journal recovery failed: {e}"),
            }
            db.audit("info", "system", "app.start", None);

            let state = Arc::new(state::AppState::new(db, data_dir, log_dir));
            system::monitor::start(app.handle().clone(), state.clone());
            system::scheduler::start(app.handle().clone(), state.clone());
            system::app_profiles::start(app.handle().clone(), state.clone());

            // Warm the hardware inventory in the background.
            let warm = state.clone();
            std::thread::spawn(move || {
                let _ = warm.system_info(false);
                let n = games::refresh(&warm.db, &warm.games).len();
                tracing::info!("detected {n} installed game(s)");
            });

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::save_settings,
            commands::list_audit,
            commands::get_prefs,
            commands::music_folders,
            commands::music_add_folder,
            commands::music_remove_folder,
            commands::music_scan,
            commands::music_tracks,
            commands::music_set_like,
            commands::music_record_play,
            commands::music_history,
            commands::music_clear_history,
            commands::music_recommended,
            commands::music_playlists,
            commands::music_save_playlist,
            commands::music_delete_playlist,
            commands::music_lyrics,
            commands::music_session,
            commands::music_save_session,
            commands::library_list,
            commands::library_start_apps,
            commands::library_services,
            commands::library_add,
            commands::library_edit,
            commands::library_remove,
            commands::library_launch,
            commands::save_prefs,
            commands::get_system_info,
            commands::get_telemetry_history,
            commands::get_telemetry_range,
            commands::get_storage_health,
            commands::get_processes,
            commands::end_process,
            commands::close_apps,
            commands::process_overview,
            commands::process_details,
            commands::set_process_priority,
            commands::set_process_affinity,
            commands::list_process_rules,
            commands::save_process_rule,
            commands::delete_process_rule,
            commands::get_health,
            commands::get_events,
            commands::get_crash_report,
            commands::get_driver_centre,
            commands::open_support_link,
            commands::get_drivers,
            commands::get_services,
            commands::get_restore_status,
            commands::network_overview,
            commands::network_test,
            commands::network_history,
            commands::traceroute,
            commands::dns_providers,
            commands::preview_dns,
            commands::set_dns,
            commands::storage_roots,
            commands::storage_scan,
            commands::storage_cancel,
            commands::storage_status,
            commands::storage_folder,
            commands::storage_large_files,
            commands::storage_find_duplicates,
            commands::storage_duplicates,
            commands::storage_recycle,
            commands::recycle_bin_info,
            commands::empty_recycle_bin,
            commands::power_plans,
            commands::set_power_plan,
            commands::list_tweaks,
            commands::list_profiles,
            commands::preview_tweaks,
            commands::apply_tweaks,
            commands::rollback_transaction,
            commands::list_app_profiles,
            commands::save_app_profile,
            commands::delete_app_profile,
            commands::maintenance_overview,
            commands::set_maintenance_task,
            commands::run_maintenance_task,
            commands::run_benchmark,
            commands::cancel_benchmark,
            commands::list_benchmarks,
            commands::delete_benchmark,
            commands::rename_benchmark,
            commands::boot_times,
            commands::list_services,
            commands::preview_services,
            commands::apply_services,
            commands::list_service_changes,
            commands::list_customise,
            commands::preview_customise,
            commands::apply_customise,
            commands::restart_explorer,
            commands::list_history,
            commands::get_transaction,
            commands::list_startup,
            commands::list_startup_changes,
            commands::set_startup_enabled,
            commands::list_games,
            commands::add_custom_game,
            commands::remove_custom_game,
            commands::set_not_game,
            commands::gaming_now,
            commands::list_game_sessions,
            commands::preview_game_profile,
            commands::apply_game_profile,
            commands::ai_key_status,
            commands::ai_set_key,
            commands::ai_test_connection,
            commands::ai_send,
            commands::ai_cancel,
            commands::ai_list_conversations,
            commands::ai_get_conversation,
            commands::ai_rename_conversation,
            commands::ai_delete_conversation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NOCTURN");
}
