//! Startup Manager: enumerate Run keys and Startup folders, and enable or
//! disable entries through their `StartupApproved` flag (non-destructive —
//! exactly what Task Manager does; entries are never deleted).

use std::path::PathBuf;

use serde::Serialize;
use winreg::enums::*;
use winreg::RegKey;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::tweaks::engine::{self, ApplyReport, PlannedOp, Progress};
use crate::tweaks::registry::{self, RawValue};
use crate::tweaks::{ChangeOp, StartupLocation, Takes};

#[derive(Debug, Clone, Serialize)]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub location: StartupLocation,
    pub location_label: &'static str,
    pub enabled: bool,
    pub file_path: Option<String>,
    pub file_exists: Option<bool>,
    pub requires_admin: bool,
    /// Publisher, signature and review flags (see `startup_insight`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insight: Option<crate::startup_insight::StartupInsight>,
}

fn run_key(loc: StartupLocation) -> Option<(RegKey, &'static str, u32)> {
    match loc {
        StartupLocation::RunUser => Some((
            RegKey::predef(HKEY_CURRENT_USER),
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_WOW64_64KEY,
        )),
        StartupLocation::RunMachine => Some((
            RegKey::predef(HKEY_LOCAL_MACHINE),
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            KEY_WOW64_64KEY,
        )),
        StartupLocation::RunMachine32 => Some((
            RegKey::predef(HKEY_LOCAL_MACHINE),
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            KEY_WOW64_32KEY,
        )),
        _ => None,
    }
}

fn startup_folder(loc: StartupLocation) -> Option<PathBuf> {
    match loc {
        StartupLocation::FolderUser => std::env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs\Startup")),
        StartupLocation::FolderCommon => std::env::var_os("ProgramData")
            .map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs\StartUp")),
        _ => None,
    }
}

/// `StartupApproved` encoding: 12 bytes; first byte even = enabled, odd =
/// disabled; bytes 4..12 hold the FILETIME the entry was disabled.
pub fn approval_bytes(enabled: bool) -> RawValue {
    let mut bytes = vec![0u8; 12];
    if enabled {
        bytes[0] = 0x02;
    } else {
        bytes[0] = 0x03;
        // FILETIME = 100ns intervals since 1601-01-01.
        let unix_100ns = chrono::Utc::now().timestamp_micros().max(0) as u64 * 10;
        let ft = unix_100ns + 116_444_736_000_000_000;
        bytes[4..12].copy_from_slice(&ft.to_le_bytes());
    }
    RawValue { vtype: REG_BINARY as u32, bytes }
}

pub fn is_enabled(raw: &Option<RawValue>) -> bool {
    match raw {
        None => true,
        Some(v) => v.bytes.first().map(|b| b & 1 == 0).unwrap_or(true),
    }
}

fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) => {
                let var = &after[..b];
                match std::env::var(var) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &after[b + 1..];
            }
            None => {
                out.push_str(&rest[a..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Best-effort extraction of the executable path from a Run command line.
pub fn exe_from_command(cmd: &str) -> Option<String> {
    let cmd = expand_env(cmd.trim());
    if let Some(rest) = cmd.strip_prefix('"') {
        return rest.split('"').next().map(String::from).filter(|s| !s.is_empty());
    }
    let lower = cmd.to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(i) = lower.find(ext) {
            return Some(cmd[..i + ext.len()].to_string());
        }
    }
    cmd.split_whitespace().next().map(String::from)
}

fn item_id(loc: StartupLocation, name: &str) -> String {
    format!("{}:{name}", serde_json::to_value(loc).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default())
}

pub fn list() -> Vec<StartupItem> {
    let mut items = Vec::new();
    for loc in StartupLocation::ALL {
        let (ahive, apath) = loc.approved_key();
        if let Some((root, path, view)) = run_key(loc) {
            let Ok(key) = root.open_subkey_with_flags(path, KEY_READ | view) else { continue };
            for (name, value) in key.enum_values().flatten() {
                if name.is_empty() {
                    continue;
                }
                let raw = RawValue { vtype: value.vtype.clone() as u32, bytes: value.bytes.to_vec() };
                let command = raw.as_string().unwrap_or_default();
                let file = exe_from_command(&command);
                let approved = registry::read(ahive, &apath, &name).ok().flatten();
                items.push(StartupItem {
                    id: item_id(loc, &name),
                    file_exists: file.as_ref().map(|f| std::path::Path::new(f).exists()),
                    file_path: file,
                    name,
                    command,
                    location: loc,
                    location_label: loc.label(),
                    enabled: is_enabled(&approved),
                    requires_admin: loc.is_machine(),
                    insight: None,
                });
            }
        } else if let Some(dir) = startup_folder(loc) {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let fname = entry.file_name().to_string_lossy().into_owned();
                if fname.eq_ignore_ascii_case("desktop.ini") || entry.path().is_dir() {
                    continue;
                }
                let approved = registry::read(ahive, &apath, &fname).ok().flatten();
                let path = entry.path().display().to_string();
                items.push(StartupItem {
                    id: item_id(loc, &fname),
                    name: fname
                        .rsplit_once('.')
                        .map(|(n, _)| n.to_string())
                        .unwrap_or_else(|| fname.clone()),
                    command: path.clone(),
                    file_path: Some(path),
                    file_exists: Some(true),
                    location: loc,
                    location_label: loc.label(),
                    enabled: is_enabled(&approved),
                    requires_admin: loc.is_machine(),
                    insight: None,
                });
            }
        }
    }
    items.sort_by_key(|a| a.name.to_lowercase());
    items
}

/// Does an entry with this approval-key name exist at this location?
pub fn entry_exists(loc: StartupLocation, name: &str) -> bool {
    if let Some((root, path, view)) = run_key(loc) {
        return root
            .open_subkey_with_flags(path, KEY_READ | view)
            .and_then(|k| k.get_raw_value(name))
            .is_ok();
    }
    startup_folder(loc).map(|d| d.join(name).is_file()).unwrap_or(false)
}

/// Resolve an item ID to the approval-key value name.
fn resolve(id: &str) -> AppResult<(StartupLocation, String)> {
    let (loc_s, name) = id
        .split_once(':')
        .ok_or_else(|| AppError::Invalid(format!("bad startup item id '{id}'")))?;
    let loc: StartupLocation = serde_json::from_value(serde_json::Value::String(loc_s.into()))
        .map_err(|_| AppError::Invalid(format!("unknown startup location '{loc_s}'")))?;
    if name.is_empty() || name.len() > 260 {
        return Err(AppError::Invalid("bad startup entry name".into()));
    }
    if !entry_exists(loc, name) {
        return Err(AppError::NotFound(format!("startup entry '{name}' no longer exists")));
    }
    Ok((loc, name.to_string()))
}

pub fn set_enabled(
    db: &Db,
    data_dir: &std::path::Path,
    ids: &[String],
    enabled: bool,
    source: &str,
    progress: &dyn Fn(Progress),
) -> AppResult<ApplyReport> {
    if ids.is_empty() {
        return Err(AppError::Invalid("no startup entries selected".into()));
    }
    let mut plan = Vec::new();
    for id in ids {
        let (location, name) = resolve(id)?;
        plan.push(PlannedOp {
            tweak_id: "startup".into(),
            title: format!("{} {}", if enabled { "Enable" } else { "Disable" }, name),
            takes: Takes::SignOut,
            op: ChangeOp::StartupApproval { location, name, enabled },
        });
    }
    let label = if ids.len() == 1 {
        plan[0].title.clone()
    } else {
        format!("{} {} startup apps", if enabled { "Enable" } else { "Disable" }, ids.len())
    };
    engine::run_plan(db, data_dir, plan, source, &label, false, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_encoding() {
        assert!(is_enabled(&None));
        assert!(is_enabled(&Some(approval_bytes(true))));
        assert!(!is_enabled(&Some(approval_bytes(false))));
        assert_eq!(approval_bytes(false).bytes.len(), 12);
        assert!(is_enabled(&Some(RawValue { vtype: 3, bytes: vec![6, 0, 0, 0] })));
        assert!(!is_enabled(&Some(RawValue { vtype: 3, bytes: vec![7, 0, 0, 0] })));
    }

    #[test]
    fn exe_parsing() {
        assert_eq!(exe_from_command("\"C:\\Program Files\\App\\app.exe\" --min").as_deref(), Some("C:\\Program Files\\App\\app.exe"));
        assert_eq!(exe_from_command("C:\\Tools\\thing.exe /background").as_deref(), Some("C:\\Tools\\thing.exe"));
    }

    #[test]
    fn list_does_not_panic() {
        let items = list();
        for i in items {
            assert!(!i.id.is_empty());
        }
    }
}
