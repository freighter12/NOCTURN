//! One-shot maintenance actions.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::security::exec::{self, SafeCommand};

const MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize, Default)]
pub struct TempScan {
    pub path: String,
    pub eligible_files: u64,
    pub eligible_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CleanResult {
    pub deleted_files: u64,
    pub freed_bytes: u64,
    pub skipped_in_use: u64,
    pub removed_dirs: u64,
}

/// The user's temp directory, refusing anything that doesn't look like one.
pub fn temp_dir() -> AppResult<PathBuf> {
    let dir = std::env::temp_dir();
    let canon = dunce_canonical(&dir)?;
    let lower = canon.to_string_lossy().to_ascii_lowercase();
    // Defence in depth: only ever operate on a folder literally named Temp/Tmp
    // that is not a drive root or a profile root.
    let name_ok = canon
        .file_name()
        .map(|n| matches!(n.to_string_lossy().to_ascii_lowercase().as_str(), "temp" | "tmp"))
        .unwrap_or(false)
        // Per-session temp dirs look like ...\Temp\2
        || canon
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().eq_ignore_ascii_case("temp"))
            .unwrap_or(false);
    if !name_ok || canon.parent().is_none() || lower.ends_with(":\\") {
        return Err(AppError::Forbidden(format!(
            "temp directory {} does not look like a temporary folder; refusing to clean it",
            canon.display()
        )));
    }
    Ok(canon)
}

fn dunce_canonical(p: &Path) -> AppResult<PathBuf> {
    let c = std::fs::canonicalize(p)?;
    // Strip the \\?\ verbatim prefix for display and comparisons.
    let s = c.to_string_lossy();
    Ok(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()))
}

fn is_old(meta: &std::fs::Metadata, now: SystemTime) -> bool {
    meta.modified()
        .ok()
        .and_then(|m| now.duration_since(m).ok())
        .map(|age| age >= MIN_AGE)
        .unwrap_or(false)
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path, &std::fs::Metadata)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        // Never follow symlinks/junctions out of the temp folder.
        if meta.file_type().is_symlink() {
            continue;
        }
        let path = entry.path();
        if meta.is_dir() {
            walk(&path, f);
        } else {
            f(&path, &meta);
        }
    }
}

pub fn scan_temp() -> AppResult<TempScan> {
    let dir = temp_dir()?;
    let now = SystemTime::now();
    let mut scan = TempScan { path: dir.display().to_string(), ..Default::default() };
    walk(&dir, &mut |_, meta| {
        scan.total_bytes += meta.len();
        if is_old(meta, now) {
            scan.eligible_files += 1;
            scan.eligible_bytes += meta.len();
        }
    });
    Ok(scan)
}

pub fn clean_temp() -> AppResult<CleanResult> {
    let dir = temp_dir()?;
    let now = SystemTime::now();
    let mut res = CleanResult::default();
    walk(&dir, &mut |path, meta| {
        if !is_old(meta, now) {
            return;
        }
        match std::fs::remove_file(path) {
            Ok(()) => {
                res.deleted_files += 1;
                res.freed_bytes += meta.len();
            }
            Err(_) => res.skipped_in_use += 1,
        }
    });
    // Remove now-empty subdirectories (never the temp root itself).
    remove_empty_dirs(&dir, &dir, &mut res.removed_dirs);
    Ok(res)
}

fn remove_empty_dirs(root: &Path, dir: &Path, count: &mut u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() && !ft.is_symlink() {
            let p = entry.path();
            remove_empty_dirs(root, &p, count);
            if p != root && std::fs::remove_dir(&p).is_ok() {
                *count += 1;
            }
        }
    }
}

pub fn flush_dns() -> AppResult<String> {
    let out = exec::run(&SafeCommand::IpconfigFlushDns)?;
    if out.success() {
        Ok("DNS resolver cache flushed".into())
    } else {
        Err(AppError::msg(format!(
            "ipconfig /flushdns failed: {}",
            if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() }
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_is_accepted_and_scannable() {
        let d = temp_dir().expect("user temp dir should be accepted");
        assert!(d.is_absolute());
        let s = scan_temp().unwrap();
        assert!(s.eligible_bytes <= s.total_bytes);
    }
}
