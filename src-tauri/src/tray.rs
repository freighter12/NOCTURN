//! System tray, close-to-tray and desktop notifications.
//!
//! The tray menu only *asks* the window to do things: music and workspace
//! actions are carried out by the frontend (which owns playback and
//! preferences), and anything that changes the system — "Clean temporary
//! files…" — opens the usual approval dialog. Nothing runs silently from here.

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Window, WindowEvent};
use tauri_plugin_notification::NotificationExt;

use crate::error::AppResult;
use crate::settings;
use crate::state::AppState;

const TRAY_ID: &str = "main";
const HINT_KEY: &str = "tray.hint_shown";

/// What the frontend tells the tray about (music and workspaces).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct TrayState {
    /// "Title — Artist" of the current track, if any.
    pub track: Option<String>,
    pub playing: bool,
    pub workspaces: Vec<TrayWorkspace>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TrayWorkspace {
    pub id: String,
    pub name: String,
    pub active: bool,
}

static LAST: Mutex<Option<TrayState>> = Mutex::new(None);

/// Windows menus treat `&` as a mnemonic marker; keep text short and literal.
fn label(s: &str, max: usize) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    let s = if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>()) } else { s };
    s.replace('&', "&&")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Keep only what the menu can safely show.
fn sanitise(mut s: TrayState) -> TrayState {
    s.track = s.track.filter(|t| !t.trim().is_empty());
    s.workspaces.retain(|w| valid_id(&w.id) && !w.name.trim().is_empty());
    s.workspaces.truncate(20);
    s
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, s: &TrayState) -> tauri::Result<tauri::menu::Menu<R>> {
    let mut ws = SubmenuBuilder::new(app, "Workspaces");
    if s.workspaces.is_empty() {
        ws = ws.item(&MenuItem::with_id(app, "info:no-ws", "No workspaces yet", false, None::<&str>)?);
    }
    for w in &s.workspaces {
        ws = ws.item(&CheckMenuItem::with_id(app, format!("ws:{}", w.id), label(&w.name, 40), true, w.active, None::<&str>)?);
    }
    if s.workspaces.iter().any(|w| w.active) {
        ws = ws.separator().text("ws:leave", "Leave workspace");
    }
    let ws = ws.build()?;

    let mut m = MenuBuilder::new(app).text("open", "Open NOCTURN").text("search", "Search…").text("slow", "Why is my PC slow?").separator();
    if let Some(t) = &s.track {
        m = m
            .item(&MenuItem::with_id(app, "info:track", format!("♪ {}", label(t, 48)), false, None::<&str>)?)
            .text("music:toggle", if s.playing { "Pause" } else { "Play" })
            .text("music:next", "Next track")
            .text("music:prev", "Previous track")
            .separator();
    }
    m.item(&ws).text("clean", "Clean temporary files…").separator().text("report", "Report a problem…").text("quit", "Quit NOCTURN").build()
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    crate::background::set_hidden(app, false);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "open" => show_main(app),
        "quit" => {
            tracing::info!("quit from tray");
            app.exit(0);
        }
        // Playback works with the window hidden.
        "music:toggle" | "music:next" | "music:prev" => {
            let _ = app.emit("tray", id);
        }
        _ if id == "search" || id == "clean" || id == "report" || id == "slow" || id == "ws:leave" || id.starts_with("ws:") => {
            show_main(app);
            let _ = app.emit("tray", id);
        }
        _ => {}
    }
}

/// Create the tray icon. Called once from setup.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app, &TrayState::default())?;
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("NOCTURN")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| on_menu(app, e.id().as_ref()))
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

/// The frontend reports music and workspace state; the menu is rebuilt only when it changed.
#[tauri::command]
pub fn tray_update(app: AppHandle, state: TrayState) -> AppResult<()> {
    let s = sanitise(state);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(&s) {
            return Ok(());
        }
        *last = Some(s.clone());
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let menu = build_menu(&app, &s).map_err(|e| crate::error::AppError::msg(e.to_string()))?;
        let _ = tray.set_menu(Some(menu));
        let tip = match (&s.track, s.playing) {
            (Some(t), true) => format!("NOCTURN — ♪ {}", label(t, 60).replace("&&", "&")),
            _ => "NOCTURN".to_string(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
    Ok(())
}

/// Close button: hide to the tray (default) or quit, per Settings.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    // Minimised counts as out of view; coming back (taskbar, Alt+Tab, restore)
    // wakes the page up again.
    // (Windows sends focus/resize events while a hidden window is created, so
    // only trust them when the window is really on screen.)
    if matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(true)) && window.is_visible().unwrap_or(false) {
        let minimised = window.is_minimized().unwrap_or(false);
        tracing::debug!(?event, minimised, "window event");
        crate::background::set_hidden(window.app_handle(), minimised);
    }
    let WindowEvent::CloseRequested { api, .. } = event else { return };
    let app = window.app_handle();
    let Some(st) = app.try_state::<Arc<AppState>>() else { return };
    if !settings::load(&st.db).close_to_tray || app.tray_by_id(TRAY_ID).is_none() {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    crate::background::set_hidden(app, true);
    // Say once where the window went.
    if !st.db.get_setting::<bool>(HINT_KEY).ok().flatten().unwrap_or(false) {
        let _ = st.db.set_setting(HINT_KEY, &true);
        show_notification(app, "NOCTURN is still running", "It's in the system tray, keeping scheduled checks and app profiles going. Right-click the moon for quick actions, or choose Quit there. You can change this in Settings.");
    }
}

fn show_notification<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification failed: {e}");
    }
}

/// A desktop notification for something that needs attention — only when
/// notifications are on and the window isn't already in front of the user
/// (then the in-app toast is enough).
pub fn notify<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    let Some(st) = app.try_state::<Arc<AppState>>() else { return };
    if !settings::load(&st.db).desktop_notifications {
        return;
    }
    let in_front = app
        .get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) && w.is_focused().unwrap_or(false))
        .unwrap_or(false);
    if !in_front {
        show_notification(app, title, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_escape_ampersands_and_truncate() {
        assert_eq!(label("Rock & Roll", 40), "Rock && Roll");
        assert_eq!(label("abcdefghij", 5), "abcd…");
        assert_eq!(label("a\u{7}b", 10), "ab");
    }

    #[test]
    fn sanitise_drops_bad_workspaces_and_empty_tracks() {
        let s = sanitise(TrayState {
            track: Some("  ".into()),
            playing: true,
            workspaces: vec![
                TrayWorkspace { id: "ws-1".into(), name: "Study".into(), active: true },
                TrayWorkspace { id: "bad id;".into(), name: "X".into(), active: false },
                TrayWorkspace { id: "ws-2".into(), name: " ".into(), active: false },
            ],
        });
        assert_eq!(s.track, None);
        assert_eq!(s.workspaces.len(), 1);
        assert_eq!(s.workspaces[0].id, "ws-1");
    }
}
