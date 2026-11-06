//! Recycle Bin: move files into it, report its size, empty it.

use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHFileOperationW, SHQueryRecycleBinW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FO_DELETE, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHFILEOPSTRUCTW, SHQUERYRBINFO,
};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
pub struct RecycleInfo {
    pub size: u64,
    pub items: u64,
}

pub fn query() -> AppResult<RecycleInfo> {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.map_err(|e| AppError::msg(format!("couldn't read the Recycle Bin: {e}")))?;
    Ok(RecycleInfo { size: info.i64Size.max(0) as u64, items: info.i64NumItems.max(0) as u64 })
}

/// Permanently empties the Recycle Bin on every drive.
pub fn empty() -> AppResult<()> {
    let r = unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) };
    match r {
        Ok(()) => Ok(()),
        // E_UNEXPECTED is returned when the bin is already empty.
        Err(e) if e.code().0 as u32 == 0x8000_FFFF => Ok(()),
        Err(e) => Err(AppError::msg(format!("couldn't empty the Recycle Bin: {e}"))),
    }
}

/// Move one file to the Recycle Bin. If the file can't be recycled (too big
/// for the bin, or a drive without one), Windows asks the user before
/// deleting it permanently (`FOF_WANTNUKEWARNING`).
pub fn recycle(path: &str) -> AppResult<()> {
    // pFrom is a double-NUL-terminated list.
    let wide: Vec<u16> = path.encode_utf16().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(wide.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let rc = unsafe { SHFileOperationW(&mut op) };
    if op.fAnyOperationsAborted.as_bool() {
        return Err(AppError::msg("cancelled — the file was kept"));
    }
    if rc != 0 {
        return Err(AppError::msg(match rc {
            0x20 => "the file is open in another program".to_string(),
            0x05 | 0x78 => "access denied".to_string(),
            n => format!("Windows couldn't move it to the Recycle Bin (code {n:#x})"),
        }));
    }
    if std::path::Path::new(path).exists() {
        return Err(AppError::msg("the file is still there after the move"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_works() {
        query().unwrap();
    }

    /// Leaves a small file in the real Recycle Bin, so it only runs on request:
    /// `cargo test -- --ignored recycles_a_file`.
    #[test]
    #[ignore]
    fn recycles_a_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nocturn-recycle-test.txt");
        std::fs::write(&p, b"recycle me").unwrap();
        recycle(&p.display().to_string()).unwrap();
        assert!(!p.exists());
        assert!(recycle(&p.display().to_string()).is_err(), "missing file");
    }
}
