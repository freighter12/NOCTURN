//! Read-only diagnostics: event log errors, drivers, services, pending reboot,
//! System Restore status. All via allow-listed fixed PowerShell scripts.

use serde::Serialize;
use serde_json::Value;

use crate::error::AppResult;
use crate::security::exec::{self, as_array, PsScript};

#[derive(Debug, Clone, Serialize)]
pub struct EventEntry {
    pub log: String,
    pub id: i64,
    pub provider: String,
    /// 1 = critical, 2 = error.
    pub level: i64,
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DriverEntry {
    pub device: String,
    pub class: Option<String>,
    pub provider: Option<String>,
    pub version: Option<String>,
    pub date: Option<String>,
    pub signed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceEntry {
    pub name: String,
    pub display_name: String,
    pub start_mode: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PendingReboot {
    pub servicing: bool,
    pub windows_update: bool,
    pub file_renames: bool,
    pub any: bool,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

pub fn recent_errors() -> AppResult<Vec<EventEntry>> {
    let rows = as_array(exec::powershell_json(PsScript::RecentErrorEvents)?);
    Ok(rows
        .iter()
        .map(|r| EventEntry {
            log: s(r, "Log").unwrap_or_default(),
            id: r.get("Id").and_then(|x| x.as_i64()).unwrap_or(0),
            provider: s(r, "ProviderName").unwrap_or_default(),
            level: r.get("Level").and_then(|x| x.as_i64()).unwrap_or(2),
            time: s(r, "Time").unwrap_or_default(),
            message: s(r, "Message").unwrap_or_default().replace("\r\n", " "),
        })
        .collect())
}

pub fn drivers() -> AppResult<Vec<DriverEntry>> {
    let rows = as_array(exec::powershell_json(PsScript::Drivers)?);
    Ok(rows
        .iter()
        .map(|r| DriverEntry {
            device: s(r, "DeviceName").unwrap_or_default(),
            class: s(r, "DeviceClass"),
            provider: s(r, "DriverProviderName"),
            version: s(r, "DriverVersion"),
            date: s(r, "DriverDate"),
            signed: r.get("IsSigned").and_then(|x| x.as_bool()),
        })
        .collect())
}

pub fn services() -> AppResult<Vec<ServiceEntry>> {
    let rows = as_array(exec::powershell_json(PsScript::Services)?);
    Ok(rows
        .iter()
        .map(|r| ServiceEntry {
            name: s(r, "Name").unwrap_or_default(),
            display_name: s(r, "DisplayName").unwrap_or_default(),
            start_mode: s(r, "StartMode").unwrap_or_default(),
            state: s(r, "State").unwrap_or_default(),
        })
        .collect())
}

pub fn pending_reboot() -> AppResult<PendingReboot> {
    let v = exec::powershell_json(PsScript::PendingReboot)?;
    let b = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let (servicing, windows_update, file_renames) = (b("cbs"), b("wu"), b("pfro"));
    Ok(PendingReboot {
        servicing,
        windows_update,
        file_renames,
        // File rename operations alone are very common and often stale; only
        // servicing / Windows Update reliably mean "restart needed".
        any: servicing || windows_update,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreStatus {
    pub queryable: bool,
    pub points: Vec<Value>,
}

/// Restore point listing needs admin rights on most systems; when not
/// elevated, `queryable` is false and the UI says so rather than guessing.
pub fn restore_status() -> AppResult<RestoreStatus> {
    let v = exec::powershell_json(PsScript::RestorePointStatus)?;
    Ok(RestoreStatus {
        queryable: v.get("queryable").and_then(|x| x.as_bool()).unwrap_or(false),
        points: v.get("points").cloned().map(as_array).unwrap_or_default(),
    })
}
