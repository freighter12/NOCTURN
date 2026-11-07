//! Auto-updates through the Tauri updater.
//!
//! NOCTURN checks for a newer signed release (at start-up after a minute, then
//! daily, if allowed in Settings) and *tells* the user; it never installs on
//! its own. Installing is a button the user presses. Every download is
//! verified against the public key built into the app before it runs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdError, Update, UpdaterExt};

use crate::error::{AppError, AppResult};
use crate::settings;
use crate::state::AppState;

const NOTIFIED_KEY: &str = "updates.notified_version";
const FIRST_CHECK: Duration = Duration::from_secs(60);
const INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// The update found by the last check, kept so "Install" installs exactly what was shown.
static PENDING: Mutex<Option<Update>> = Mutex::new(None);
/// Only one install at a time.
static INSTALLING: Mutex<bool> = Mutex::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
    /// Set when the check itself didn't work; `available` is false then.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub done: bool,
}

fn friendly(e: &UpdError) -> String {
    match e {
        UpdError::ReleaseNotFound => "No update has been published yet, or the update server couldn't be reached.".into(),
        UpdError::Reqwest(_) => "Couldn't reach the update server. Check your internet connection.".into(),
        UpdError::Network(_) => "Couldn't reach the update server. Check your internet connection.".into(),
        UpdError::EmptyEndpoints => "This build has no update server configured.".into(),
        other => format!("Update check failed: {other}"),
    }
}

async fn check(app: &AppHandle) -> UpdateInfo {
    let current = app.package_info().version.to_string();
    let base = UpdateInfo { current: current.clone(), available: false, version: None, notes: None, date: None, problem: None };
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => return UpdateInfo { problem: Some(friendly(&e)), ..base },
    };
    match updater.check().await {
        Ok(Some(u)) => {
            let info = UpdateInfo {
                available: true,
                version: Some(u.version.clone()),
                notes: u.body.clone().map(|b| b.chars().take(4000).collect()),
                date: u.date.map(|d| d.to_string()),
                ..base
            };
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(u);
            info
        }
        Ok(None) => {
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
            base
        }
        Err(e) => {
            tracing::info!("update check: {e}");
            UpdateInfo { problem: Some(friendly(&e)), ..base }
        }
    }
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> AppResult<UpdateInfo> {
    let info = check(&app).await;
    if let Some(st) = app.try_state::<Arc<AppState>>() {
        st.db.audit("info", "user", "update.check", Some(serde_json::json!({ "available": info.version, "problem": info.problem })));
    }
    Ok(info)
}

/// Download, verify and run the update the last check found. On Windows the
/// installer closes NOCTURN and starts the new version.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> AppResult<()> {
    {
        let mut busy = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
        if *busy {
            return Err(AppError::msg("an update is already being installed"));
        }
        *busy = true;
    }
    let result = install(&app).await;
    *INSTALLING.lock().unwrap_or_else(|e| e.into_inner()) = false;
    result
}

async fn install(app: &AppHandle) -> AppResult<()> {
    let update = PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| AppError::Invalid("check for updates first".into()))?;
    if let Some(st) = app.try_state::<Arc<AppState>>() {
        st.db.audit("info", "user", "update.install", Some(serde_json::json!({ "from": app.package_info().version.to_string(), "to": update.version })));
    }
    tracing::info!(to = %update.version, "installing update");
    let mut downloaded: u64 = 0;
    let a = app.clone();
    let b = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = a.emit("update-progress", Progress { downloaded, total, done: false });
            },
            move || {
                let _ = b.emit("update-progress", Progress { downloaded: 0, total: None, done: true });
            },
        )
        .await
        .map_err(|e| AppError::msg(format!("the update couldn't be installed: {e}")))?;
    // Windows normally exits during install; restart covers everything else.
    app.restart();
}

/// Background checks: tell the user once per new version, never install.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let allowed = app.try_state::<Arc<AppState>>().map(|st| settings::load(&st.db).auto_update_check).unwrap_or(false);
            if allowed && !cfg!(debug_assertions) {
                let info = check(&app).await;
                if let (true, Some(v), Some(st)) = (info.available, info.version.clone(), app.try_state::<Arc<AppState>>()) {
                    let told = st.db.get_setting::<String>(NOTIFIED_KEY).ok().flatten();
                    let _ = app.emit("update-available", &info);
                    if told.as_deref() != Some(v.as_str()) {
                        let _ = st.db.set_setting(NOTIFIED_KEY, &v);
                        crate::tray::notify(&app, &format!("NOCTURN {v} is available"), "Open Settings › Updates to see what's new and install it.");
                    }
                }
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}
