//! Least-privilege elevation.
//!
//! NOCTURN runs as a normal user. Only changes under `HKEY_LOCAL_MACHINE` (and
//! restore point creation) need administrator rights. For those, the app
//! relaunches *itself* through UAC with a hidden worker flag, hands it a JSON
//! request file, and waits for a JSON response file.
//!
//! The worker never executes arbitrary instructions: requests reference
//! catalogue tweak IDs only, and every operation is re-validated against the
//! compiled-in catalogue inside the elevated process (see
//! `tweaks::elevated::handle`). HKCU changes are *never* performed by the
//! worker, because with over-the-shoulder UAC credentials its HKCU would be a
//! different user's hive.

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::error::{AppError, AppResult};

pub const WORKER_FLAG: &str = "--nocturn-elevated-worker";

/// Whether the current process token is elevated.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

fn exchange_dir() -> AppResult<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("NOCTURN").join("elevated").join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Run `request` in an elevated copy of NOCTURN and return its response.
/// Blocks until the worker exits (the UAC prompt is shown to the user).
pub fn run_elevated(request: &serde_json::Value) -> AppResult<serde_json::Value> {
    let dir = exchange_dir()?;
    let req_path = dir.join("request.json");
    let resp_path = dir.join("response.json");
    std::fs::write(&req_path, serde_json::to_vec(request)?)?;

    let exe = std::env::current_exe()?;
    let params = format!("{WORKER_FLAG} \"{}\" \"{}\"", req_path.display(), resp_path.display());

    let verb = HSTRING::from("runas");
    let file = HSTRING::from(exe.as_os_str());
    let params_h = HSTRING::from(params);

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params_h.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    let launched = unsafe { ShellExecuteExW(&mut info) };
    if let Err(e) = launched {
        let _ = std::fs::remove_dir_all(&dir);
        if e.code() == ERROR_CANCELLED.to_hresult() {
            return Err(AppError::Elevation("the administrator prompt was cancelled".into()));
        }
        return Err(AppError::Elevation(e.to_string()));
    }
    if info.hProcess.is_invalid() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(AppError::Elevation("no worker process handle".into()));
    }

    let exit_code = unsafe {
        // Five minutes covers restore point creation on slow disks.
        let wait = WaitForSingleObject(info.hProcess, 5 * 60 * 1000);
        let mut code = 1u32;
        if wait == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
        if wait != WAIT_OBJECT_0 {
            None
        } else {
            Some(code)
        }
    };

    let response = read_response(&resp_path);
    let _ = std::fs::remove_dir_all(&dir);

    match (exit_code, response) {
        (_, Ok(v)) => Ok(v),
        (None, _) => Err(AppError::Elevation("the elevated worker timed out".into())),
        (Some(c), Err(e)) => {
            Err(AppError::Elevation(format!("worker exited with code {c} and no result ({e})")))
        }
    }
}

fn read_response(path: &Path) -> AppResult<serde_json::Value> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Entry point used by `main` before Tauri starts. Returns `Some(exit_code)`
/// when this process was launched as the elevated worker.
pub fn maybe_run_worker() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == WORKER_FLAG)?;
    let (Some(req), Some(resp)) = (args.get(pos + 1), args.get(pos + 2)) else {
        return Some(2);
    };
    if !is_elevated() {
        return Some(3);
    }
    let response = match std::fs::read(req)
        .map_err(AppError::from)
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(AppError::from))
    {
        Ok(request) => crate::tweaks::elevated::handle(request),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    };
    match std::fs::write(resp, serde_json::to_vec(&response).unwrap_or_default()) {
        Ok(()) => Some(0),
        Err(_) => Some(4),
    }
}
