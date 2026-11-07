//! Running quietly: while the window is hidden in the tray (or minimised)
//! NOCTURN tells WebView2 the page isn't on screen — it stops painting,
//! slows its timers and gives memory back — and the monitor samples less
//! often. Everything that works from the tray (scheduled checks, app
//! profiles, alerts, music) keeps going.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager, Runtime};

static HIDDEN: AtomicBool = AtomicBool::new(false);

/// True while no one can see the window.
pub fn is_hidden() -> bool {
    HIDDEN.load(Ordering::Relaxed)
}

/// Record that the window went out of (or back into) view, and tell WebView2.
pub fn set_hidden<R: Runtime>(app: &AppHandle<R>, hidden: bool) {
    if HIDDEN.swap(hidden, Ordering::SeqCst) == hidden {
        return;
    }
    tracing::info!(hidden, "window {}", if hidden { "hidden: running quietly" } else { "shown" });
    if let Some(w) = app.get_webview_window("main") {
        let res = w.with_webview(move |pw| {
            #[cfg(windows)]
            webview_level(&pw.controller(), hidden);
        });
        if let Err(e) = res {
            tracing::debug!("webview visibility not changed: {e}");
        }
    }
    let _ = app.emit("background", hidden);
}

/// Hidden: stop painting, throttle timers and aim for low memory. Pages
/// playing audio keep running normally (Chromium never throttles those), so
/// music carries on from the tray.
#[cfg(windows)]
fn webview_level(controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller, hidden: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL};
    use windows::core::Interface;
    unsafe {
        if let Err(e) = controller.SetIsVisible(!hidden) {
            tracing::debug!("SetIsVisible failed: {e}");
        }
        let level = if hidden { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
        // Needs a WebView2 runtime from 2023 or later; older ones just skip it.
        if let Ok(wv) = controller.CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_19>()) {
            if let Err(e) = wv.SetMemoryUsageTargetLevel(level) {
                tracing::debug!("SetMemoryUsageTargetLevel failed: {e}");
            }
        }
    }
}
