//! Process priority and CPU affinity, plus app-window detection.
//!
//! Safety rules enforced here (not just in the UI):
//! - Realtime priority is never offered: it can starve input, audio and the
//!   kernel's own worker threads and freeze the machine.
//! - Protected Windows processes (see `processes::PROTECTED`) are refused.
//! - Every PID is re-checked against the expected process name right before
//!   it is changed, so a reused PID can't redirect the change.
//! - Affinity masks must be non-empty and within the system mask.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessAffinityMask, OpenProcess, SetPriorityClass, SetProcessAffinityMask,
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_INFORMATION, REALTIME_PRIORITY_CLASS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
    WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use super::processes::is_protected;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

impl Priority {
    fn class(self) -> PROCESS_CREATION_FLAGS {
        match self {
            Priority::Idle => IDLE_PRIORITY_CLASS,
            Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Priority::High => HIGH_PRIORITY_CLASS,
        }
    }

    /// `None` for Realtime or unknown classes — those are reported as-is but
    /// never set by NOCTURN.
    fn from_class(c: u32) -> Option<Self> {
        [Priority::Idle, Priority::BelowNormal, Priority::Normal, Priority::AboveNormal, Priority::High]
            .into_iter()
            .find(|p| p.class().0 == c)
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::Idle => "Low",
            Priority::BelowNormal => "Below normal",
            Priority::Normal => "Normal",
            Priority::AboveNormal => "Above normal",
            Priority::High => "High",
        }
    }
}

/// Owned process handle, closed on drop.
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> AppResult<Handle> {
    unsafe { OpenProcess(access, false, pid) }
        .map(Handle)
        .map_err(|e| AppError::Forbidden(format!("Windows refused access to process {pid} ({e}). It may belong to another user or need administrator rights.")))
}

/// Current priority as a label ("Normal", "Realtime", …), or `None` if the
/// process can't be queried.
pub fn priority_label(pid: u32) -> Option<String> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    let c = unsafe { GetPriorityClass(h.0) };
    if c == 0 {
        return None;
    }
    if c == REALTIME_PRIORITY_CLASS.0 {
        return Some("Realtime".into());
    }
    Some(Priority::from_class(c).map(|p| p.label().to_string()).unwrap_or_else(|| format!("0x{c:x}")))
}

/// (process mask, system mask) for a process.
pub fn affinity(pid: u32) -> Option<(u64, u64)> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    let (mut p, mut s) = (0usize, 0usize);
    unsafe { GetProcessAffinityMask(h.0, &mut p, &mut s) }.ok()?;
    Some((p as u64, s as u64))
}

/// The system affinity mask (all logical processors in this process's group).
pub fn system_mask() -> u64 {
    affinity(std::process::id()).map(|(_, s)| s).unwrap_or(0)
}

pub fn validate_mask(mask: u64, system: u64) -> AppResult<()> {
    if mask == 0 {
        return Err(AppError::Invalid("select at least one CPU".into()));
    }
    if system != 0 && mask & !system != 0 {
        return Err(AppError::Invalid("the selection includes CPUs this PC doesn't have".into()));
    }
    Ok(())
}

/// Re-check that `pid` is still `expected` before touching it.
fn verify(pid: u32, expected: &str) -> AppResult<()> {
    if is_protected(expected) {
        return Err(AppError::Forbidden(format!("{expected} is a core Windows process and can't be changed from NOCTURN")));
    }
    if pid == std::process::id() || pid <= 4 {
        return Err(AppError::Forbidden("refusing to change this process".into()));
    }
    let mut sys = System::new().map_err(|e| AppError::msg(e.to_string()))?;
    let p = Pid::from_u32(pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[p]), true, ProcessRefreshKind::nothing());
    let actual = sys
        .process(p)
        .ok_or_else(|| AppError::NotFound(format!("process {pid} is no longer running")))?
        .name()
        .to_string_lossy()
        .to_string();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(AppError::Invalid(format!("process {pid} is now {actual}, not {expected}")));
    }
    Ok(())
}

pub fn set_priority(pid: u32, expected: &str, p: Priority) -> AppResult<()> {
    verify(pid, expected)?;
    let h = open(pid, PROCESS_SET_INFORMATION)?;
    unsafe { SetPriorityClass(h.0, p.class()) }.map_err(|e| AppError::msg(format!("couldn't set priority: {e}")))
}

pub fn set_affinity(pid: u32, expected: &str, mask: u64) -> AppResult<()> {
    validate_mask(mask, system_mask())?;
    verify(pid, expected)?;
    let h = open(pid, PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION)?;
    unsafe { SetProcessAffinityMask(h.0, mask as usize) }.map_err(|e| AppError::msg(format!("couldn't set CPU affinity: {e}")))
}

/// PIDs of processes that own a visible, top-level, non-tool window — the
/// same heuristic Task Manager uses to separate "Apps" from background work.
pub fn window_pids() -> HashSet<u32> {
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let set = &mut *(lparam.0 as *mut HashSet<u32>);
        if IsWindowVisible(hwnd).as_bool()
            && GetWindow(hwnd, GW_OWNER).map(|o| o.is_invalid()).unwrap_or(true)
            && (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0) == 0
        {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                set.insert(pid);
            }
        }
        BOOL(1)
    }
    let mut set: HashSet<u32> = HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut set as *mut _ as isize));
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_is_not_a_settable_priority() {
        assert_eq!(Priority::from_class(REALTIME_PRIORITY_CLASS.0), None);
        let json: Result<Priority, _> = serde_json::from_str("\"realtime\"");
        assert!(json.is_err());
        assert_eq!(serde_json::from_str::<Priority>("\"above_normal\"").unwrap(), Priority::AboveNormal);
    }

    #[test]
    fn masks_are_validated() {
        assert!(validate_mask(0, 0xFF).is_err());
        assert!(validate_mask(0x100, 0xFF).is_err());
        assert!(validate_mask(0x0F, 0xFF).is_ok());
    }

    #[test]
    fn reads_own_process() {
        let me = std::process::id();
        assert_eq!(priority_label(me).as_deref(), Some("Normal"));
        let (p, s) = affinity(me).unwrap();
        assert!(s != 0 && p & s == p);
    }

    #[test]
    fn refuses_protected_and_self() {
        assert!(set_priority(1234, "csrss.exe", Priority::High).is_err());
        assert!(set_priority(std::process::id(), "nocturn.exe", Priority::High).is_err());
    }

    #[test]
    fn changes_a_child_process_priority_and_affinity() {
        // A real round trip on a process we own.
        let mut child = std::process::Command::new(r"C:\Windows\System32\ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");
        let pid = child.id();
        std::thread::sleep(std::time::Duration::from_millis(300));
        set_priority(pid, "PING.EXE", Priority::BelowNormal).unwrap();
        assert_eq!(priority_label(pid).as_deref(), Some("Below normal"));
        let sys = system_mask();
        let first = sys & sys.wrapping_neg(); // lowest available CPU
        set_affinity(pid, "PING.EXE", first).unwrap();
        assert_eq!(affinity(pid).unwrap().0, first);
        assert!(set_affinity(pid, "PING.EXE", 0).is_err());
        assert!(set_priority(pid, "notepad.exe", Priority::High).is_err(), "name mismatch must be refused");
        let _ = child.kill();
    }
}
