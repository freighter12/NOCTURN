//! Safe process management.

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
}
