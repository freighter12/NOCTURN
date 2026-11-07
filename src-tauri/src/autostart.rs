//! "Start with Windows": a per-user Run entry that opens NOCTURN hidden in
//! the tray (`--tray`). The registry is the source of truth, so turning it
//! off in Task Manager's Startup tab shows up here too.

use serde::Serialize;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

use crate::error::{AppError, AppResult};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Command-line flag: start hidden in the tray.
pub const TRAY_FLAG: &str = "--tray";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Autostart {
    /// Starts with Windows (entry present, points at this copy, not disabled).
    pub enabled: bool,
    /// The entry exists but was turned off in Task Manager / Settings › Apps › Startup.
    pub disabled_by_windows: bool,
}

/// True when this launch should stay in the tray.
pub fn started_hidden() -> bool {
    std::env::args().any(|a| a == TRAY_FLAG)
}

fn command_line() -> AppResult<String> {
    let exe = std::env::current_exe()?;
    Ok(format!("\"{}\" {TRAY_FLAG}", exe.display()))
}

/// Pure: does a Run value launch this executable?
pub fn points_at(value: &str, exe: &str) -> bool {
    let v = value.trim().trim_start_matches('"').to_ascii_lowercase();
    v.starts_with(&exe.to_ascii_lowercase())
}

pub fn get(name: &str) -> Autostart {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let value: Option<String> = hkcu.open_subkey_with_flags(RUN, KEY_READ).ok().and_then(|k| k.get_value(name).ok());
    let exe = std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_default();
    let ours = value.as_deref().is_some_and(|v| points_at(v, &exe));
    let approved = hkcu.open_subkey_with_flags(APPROVED, KEY_READ).ok().and_then(|k| k.get_raw_value(name).ok()).map(|v| crate::tweaks::registry::RawValue { vtype: v.vtype as u32, bytes: v.bytes.to_vec() });
    let on = crate::startup::is_enabled(&approved);
    Autostart { enabled: ours && on, disabled_by_windows: ours && !on }
}

/// Turn starting with Windows on or off for this copy of NOCTURN.
pub fn set(name: &str, enabled: bool) -> AppResult<Autostart> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey_with_flags(RUN, KEY_READ | KEY_WRITE).map_err(|e| AppError::msg(format!("Couldn't open the startup list: {e}")))?;
    if enabled {
        run.set_value(name, &command_line()?).map_err(|e| AppError::msg(format!("Couldn't add NOCTURN to startup: {e}")))?;
    } else if run.get_raw_value(name).is_ok() {
        run.delete_value(name).map_err(|e| AppError::msg(format!("Couldn't remove NOCTURN from startup: {e}")))?;
    }
    // Clear any "disabled" mark Task Manager left, so turning it on here works.
    if let Ok(k) = hkcu.open_subkey_with_flags(APPROVED, KEY_READ | KEY_WRITE) {
        if k.get_raw_value(name).is_ok() {
            let _ = k.delete_value(name);
        }
    }
    let now = get(name);
    tracing::info!(enabled = now.enabled, "start with Windows changed");
    Ok(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_our_own_entry_only() {
        let exe = r"C:\Users\Me\AppData\Local\NOCTURN\nocturn.exe";
        assert!(points_at(r#""C:\Users\Me\AppData\Local\NOCTURN\nocturn.exe" --tray"#, exe));
        assert!(points_at(r#""c:\users\me\appdata\local\nocturn\NOCTURN.EXE" --tray"#, exe), "case doesn't matter");
        assert!(!points_at(r#""D:\Old\NOCTURN\nocturn.exe" --tray"#, exe), "an older install elsewhere isn't this copy");
    }
}
