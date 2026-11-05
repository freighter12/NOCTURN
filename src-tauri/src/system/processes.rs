//! Safe process management.

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::error::{AppError, AppResult};

/// Processes that must never be terminated from NOCTURN. Ending any of these
/// crashes Windows, signs the user out or breaks security.
const PROTECTED: &[&str] = &[
    "system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
    "lsass.exe", "lsaiso.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "memcompression",
    "msmpeng.exe", "nissrv.exe", "securityhealthservice.exe", "sgrmbroker.exe", "audiodg.exe",
    "spoolsv.exe", "conhost.exe", "sihost.exe", "ctfmon.exe", "nocturn.exe", "secure system",
    "wudfhost.exe", "taskhostw.exe", "runtimebroker.exe", "searchindexer.exe",
];

pub fn is_protected(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    PROTECTED.contains(&n.as_str())
}

/// End a process after verifying the PID still belongs to the expected name
/// (guards against PID reuse between listing and clicking).
pub fn end_process(pid: u32, expected_name: &str) -> AppResult<()> {
    if is_protected(expected_name) {
        return Err(AppError::Forbidden(format!(
            "{expected_name} is a core Windows process and can't be ended from NOCTURN"
        )));
    }
    if pid == std::process::id() || pid <= 4 {
        return Err(AppError::Forbidden("refusing to end this process".into()));
    }
    let mut sys = System::new().map_err(|e| AppError::msg(e.to_string()))?;
    let p = Pid::from_u32(pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[p]), true, ProcessRefreshKind::nothing());
    let proc_ = sys
        .process(p)
        .ok_or_else(|| AppError::NotFound(format!("process {pid} is no longer running")))?;
    let actual = proc_.name().to_string_lossy().to_string();
    if !actual.eq_ignore_ascii_case(expected_name) {
        return Err(AppError::Invalid(format!(
            "process {pid} is now {actual}, not {expected_name}; refresh the list"
        )));
    }
    if is_protected(&actual) {
        return Err(AppError::Forbidden(format!("{actual} is a protected process")));
    }
    if proc_.kill() {
        Ok(())
    } else {
        Err(AppError::msg(format!(
            "Windows refused to end {actual} — it may belong to another user or need administrator rights"
        )))
    }
}

/// Outcome of closing every process of one app.
#[derive(Debug, Clone, Serialize)]
pub struct CloseResult {
    pub name: String,
    pub ended: usize,
    pub failed: usize,
    pub error: Option<String>,
}

/// Resolve a user/AI-supplied app name ("Spotify", "spotify.exe") to the exact
/// process name in `running`, case-insensitively. `None` if not running.
pub fn resolve_name<'a>(wanted: &str, running: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let w = wanted.trim().to_ascii_lowercase();
    if w.is_empty() {
        return None;
    }
    let with_exe = if w.ends_with(".exe") { w.clone() } else { format!("{w}.exe") };
    running
        .into_iter()
        .find(|n| {
            let n = n.to_ascii_lowercase();
            n == w || n == with_exe
        })
        .map(String::from)
}

/// End every running process with this exact name. Protected Windows
/// processes and NOCTURN itself are refused; each PID is re-checked by
/// `end_process` so nothing else is hit if a PID was reused.
pub fn close_app(name: &str) -> CloseResult {
    let fail = |e: String| CloseResult { name: name.to_string(), ended: 0, failed: 0, error: Some(e) };
    if is_protected(name) {
        return fail(format!("{name} is a core Windows process and can't be closed from NOCTURN"));
    }
    let mut sys = match System::new() {
        Ok(s) => s,
        Err(e) => return fail(e.to_string()),
    };
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let me = std::process::id();
    let pids: Vec<u32> = sys
        .processes()
        .iter()
        .filter(|(pid, p)| pid.as_u32() != me && p.name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|(pid, _)| pid.as_u32())
        .collect();
    if pids.is_empty() {
        return fail("not running".into());
    }
    let (mut ended, mut failed, mut last) = (0, 0, None);
    for pid in pids {
        match end_process(pid, name) {
            Ok(()) => ended += 1,
            // A child process may exit by itself once its parent is ended.
            Err(AppError::NotFound(_)) => ended += 1,
            Err(e) => {
                failed += 1;
                last = Some(e.to_string());
            }
        }
    }
    CloseResult { name: name.to_string(), ended, failed, error: last }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_core_processes() {
        assert!(is_protected("csrss.exe"));
        assert!(is_protected("LSASS.EXE"));
        assert!(is_protected("MsMpEng.exe"));
        assert!(!is_protected("chrome.exe"));
        assert!(end_process(4, "System").is_err());
        assert!(end_process(1234, "winlogon.exe").is_err());
    }

    #[test]
    fn resolves_app_names_and_refuses_protected() {
        let running = ["Spotify.exe", "brave.exe", "svchost.exe"];
        assert_eq!(resolve_name("spotify", running).as_deref(), Some("Spotify.exe"));
        assert_eq!(resolve_name("BRAVE.EXE", running).as_deref(), Some("brave.exe"));
        assert_eq!(resolve_name("discord", running), None);
        assert_eq!(resolve_name("  ", running), None);
        let r = close_app("lsass.exe");
        assert_eq!(r.ended, 0);
        assert!(r.error.unwrap().contains("core Windows process"));
        assert_eq!(close_app("definitely-not-running-nocturn-test.exe").error.as_deref(), Some("not running"));
    }
}
