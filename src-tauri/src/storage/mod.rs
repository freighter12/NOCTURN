//! Storage intelligence: folder usage map, largest files, duplicate files,
//! and review-then-recycle.
//!
//! Safety rules for removing files:
//! * Only files the last scan (or duplicate search) reported can be removed,
//!   and only while their size is unchanged since then.
//! * Files under Windows, Program Files and ProgramData, page/hibernation
//!   files, System Restore data and the Recycle Bin itself are refused.
//! * Files go to the Recycle Bin (Windows asks before anything would be
//!   deleted permanently), and every removal is written to the audit log.
//! * At least one copy of every duplicate group is always kept.

pub mod dupes;
pub mod recycle;
pub mod scan;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::Serialize;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use dupes::{DupGroup, DupProgress};
use scan::{FileRec, ScanProgress, ScanResult};

#[derive(Default)]
pub struct StorageState {
    busy: AtomicBool,
    cancel: AtomicBool,
    last: RwLock<Option<Arc<ScanResult>>>,
    dupes: RwLock<Option<Arc<Vec<DupGroup>>>>,
    /// Lower-cased paths recycled since the last scan.
    removed: RwLock<HashSet<String>>,
}

/// Clears the busy flag however the operation ends.
struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl StorageState {
    fn begin(&self) -> AppResult<BusyGuard<'_>> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(AppError::msg("a scan is already running"));
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(BusyGuard(&self.busy))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn last(&self) -> AppResult<Arc<ScanResult>> {
        self.last.read().ok().and_then(|g| g.clone()).ok_or_else(|| AppError::Invalid("scan a drive or folder first".into()))
    }

    fn is_removed(&self, path: &str) -> bool {
        self.removed.read().map(|r| r.contains(&path.to_ascii_lowercase())).unwrap_or(false)
    }

    /// Duplicate groups minus recycled files (groups left with one copy drop out).
    fn live_groups(&self) -> Option<Vec<DupGroup>> {
        let groups = self.dupes.read().ok().and_then(|g| g.clone())?;
        Some(
            groups
                .iter()
                .filter_map(|grp| {
                    let files: Vec<FileRec> = grp.files.iter().filter(|f| !self.is_removed(&f.path)).cloned().collect();
                    (files.len() > 1).then(|| DupGroup { size: grp.size, wasted: grp.size * (files.len() as u64 - 1), files })
                })
                .collect(),
        )
    }
}

// ------------------------------------------------------------------ views

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub root: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub total_size: u64,
    pub total_files: u64,
    pub folders: u64,
    pub cloud_only: u64,
    pub unreadable: u64,
    pub skipped_links: u64,
    pub cancelled: bool,
    pub dup_candidates: usize,
}

fn summary(r: &ScanResult) -> ScanSummary {
    ScanSummary {
        root: r.root.clone(),
        started_at: r.started_at.clone(),
        duration_ms: r.duration_ms,
        total_size: r.total_size(),
        total_files: r.total_files(),
        folders: r.dirs.len() as u64,
        cloud_only: r.cloud_only,
        unreadable: r.unreadable,
        skipped_links: r.skipped_links,
        cancelled: r.cancelled,
        dup_candidates: r.dup_candidates.len(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderChild {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub files: u64,
    pub has_children: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderView {
    pub id: u32,
    pub path: String,
    pub size: u64,
    pub files: u64,
    /// Files directly in this folder (not in a subfolder).
    pub own_size: u64,
    pub own_files: u64,
    /// Root first, this folder last.
    pub crumbs: Vec<(u32, String)>,
    pub children: Vec<FolderChild>,
    pub more_children: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileView {
    pub path: String,
    pub size: u64,
    pub modified: Option<i64>,
    /// Why this file can't be removed by NOCTURN, if it can't.
    pub protected: Option<&'static str>,
    /// Inside an app's data folder: removing it may reset or break that app.
    pub app_data: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DupGroupView {
    pub size: u64,
    pub wasted: u64,
    pub files: Vec<FileView>,
}

fn env_dir(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty()).map(|v| format!("{}\\", v.trim_end_matches('\\').to_ascii_lowercase()))
}

/// Why NOCTURN refuses to remove `path`, if it does.
pub fn protection(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    let under = |var: &str| env_dir(var).is_some_and(|d| lower.starts_with(&d));
    if under("WINDIR") {
        return Some("Part of Windows");
    }
    if under("ProgramFiles") || under("ProgramFiles(x86)") || under("ProgramW6432") {
        return Some("Part of an installed program — uninstall it from Settings instead");
    }
    if under("ProgramData") {
        return Some("Shared program data");
    }
    if lower.contains("\\$recycle.bin\\") {
        return Some("Already in the Recycle Bin");
    }
    if lower.contains("\\system volume information\\") {
        return Some("System Restore data");
    }
    let p = Path::new(&lower);
    let at_drive_root = p.parent().is_some_and(|d| d.parent().is_none());
    if at_drive_root && matches!(p.file_name().and_then(|f| f.to_str()), Some("pagefile.sys" | "hiberfil.sys" | "swapfile.sys" | "dumpstack.log" | "dumpstack.log.tmp")) {
        return Some("Windows system file — change its size in Windows settings instead");
    }
    None
}

fn file_view(f: &FileRec) -> FileView {
    FileView {
        protected: protection(&f.path),
        app_data: f.path.to_ascii_lowercase().contains("\\appdata\\"),
        path: f.path.clone(),
        size: f.size,
        modified: f.modified,
    }
}

const MAX_CHILDREN: usize = 300;

pub fn folder(st: &StorageState, id: Option<u32>) -> AppResult<FolderView> {
    let r = st.last()?;
    let id = id.unwrap_or(0);
    let d = r.dirs.get(id as usize).ok_or_else(|| AppError::NotFound(format!("folder {id}")))?;
    let mut crumbs = Vec::new();
    let mut cur = Some(id);
    while let Some(i) = cur {
        crumbs.push((i, r.dirs[i as usize].name.clone()));
        cur = r.dirs[i as usize].parent;
    }
    crumbs.reverse();
    let mut children: Vec<FolderChild> = d
        .children
        .iter()
        .map(|&c| {
            let x = &r.dirs[c as usize];
            FolderChild { id: c, name: x.name.clone(), size: x.size, files: x.files, has_children: !x.children.is_empty() }
        })
        .collect();
    children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    let more = children.len().saturating_sub(MAX_CHILDREN);
    children.truncate(MAX_CHILDREN);
    Ok(FolderView {
        id,
        path: r.path_of(id).display().to_string(),
        size: d.size,
        files: d.files,
        own_size: d.own_size,
        own_files: d.own_files,
        crumbs,
        children,
        more_children: more,
    })
}

pub fn large_files(st: &StorageState) -> AppResult<Vec<FileView>> {
    Ok(st.last()?.large.iter().filter(|f| !st.is_removed(&f.path)).map(file_view).collect())
}

fn group_views(groups: &[DupGroup]) -> Vec<DupGroupView> {
    groups.iter().map(|g| DupGroupView { size: g.size, wasted: g.wasted, files: g.files.iter().map(file_view).collect() }).collect()
}

pub fn last_summary(st: &StorageState) -> Option<ScanSummary> {
    st.last.read().ok().and_then(|g| g.as_deref().map(summary))
}

pub fn last_duplicates(st: &StorageState) -> Option<Vec<DupGroupView>> {
    st.live_groups().map(|g| group_views(&g))
}

// -------------------------------------------------------------- actions

pub fn validate_root(root: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(root.trim());
    if !p.is_absolute() || !p.is_dir() {
        return Err(AppError::Invalid("choose a drive or folder that exists".into()));
    }
    Ok(p)
}

pub fn run_scan(st: &StorageState, root: &str, progress: &dyn Fn(ScanProgress)) -> AppResult<ScanSummary> {
    let root = validate_root(root)?;
    let _busy = st.begin()?;
    let r = scan::scan(&root, &st.cancel, progress);
    let s = summary(&r);
    if let Ok(mut g) = st.last.write() {
        *g = Some(Arc::new(r));
    }
    if let Ok(mut g) = st.dupes.write() {
        *g = None;
    }
    if let Ok(mut g) = st.removed.write() {
        g.clear();
    }
    tracing::info!(root = %s.root, files = s.total_files, ms = s.duration_ms, "storage scan finished");
    Ok(s)
}

pub fn find_duplicates(st: &StorageState, progress: &dyn Fn(DupProgress)) -> AppResult<Vec<DupGroupView>> {
    let r = st.last()?;
    let _busy = st.begin()?;
    let groups = dupes::find(&r.dup_candidates, &st.cancel, progress);
    if st.cancel.load(Ordering::SeqCst) {
        return Err(AppError::msg("duplicate search cancelled"));
    }
    let views = group_views(&groups);
    if let Ok(mut g) = st.dupes.write() {
        *g = Some(Arc::new(groups));
    }
    Ok(views)
}

#[derive(Debug, Clone, Serialize)]
pub struct RecycleResult {
    pub path: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecycleReport {
    pub results: Vec<RecycleResult>,
    pub freed: u64,
}

/// Check every requested path before touching anything. Returns the
/// accepted paths with their recorded sizes, or the first reason to refuse.
fn check_request(scan: Option<&ScanResult>, groups: Option<&[DupGroup]>, paths: &[String]) -> AppResult<Vec<(String, u64)>> {
    if paths.is_empty() || paths.len() > 500 {
        return Err(AppError::Invalid("choose between 1 and 500 files".into()));
    }
    let mut known: HashMap<String, u64> = HashMap::new();
    for f in scan.map(|s| s.large.as_slice()).unwrap_or_default().iter().chain(groups.unwrap_or_default().iter().flat_map(|g| g.files.iter())) {
        known.insert(f.path.to_ascii_lowercase(), f.size);
    }
    let wanted: HashSet<String> = paths.iter().map(|p| p.to_ascii_lowercase()).collect();
    for g in groups.unwrap_or_default() {
        if g.files.iter().all(|f| wanted.contains(&f.path.to_ascii_lowercase())) {
            return Err(AppError::Forbidden(format!("keep at least one copy of {}", Path::new(&g.files[0].path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())));
        }
    }
    let mut out = Vec::new();
    for p in paths {
        let size = *known.get(&p.to_ascii_lowercase()).ok_or_else(|| AppError::Forbidden(format!("{p} wasn't in the last scan — scan again first")))?;
        if let Some(why) = protection(p) {
            return Err(AppError::Forbidden(format!("{p}: {why}")));
        }
        let md = std::fs::symlink_metadata(p).map_err(|_| AppError::NotFound(format!("{p} no longer exists")))?;
        if !md.is_file() {
            return Err(AppError::Forbidden(format!("{p} isn't a regular file")));
        }
        if md.len() != size {
            return Err(AppError::Invalid(format!("{p} changed since the scan — scan again first")));
        }
        out.push((p.clone(), size));
    }
    Ok(out)
}

pub fn recycle_files(st: &StorageState, db: &Db, paths: &[String]) -> AppResult<RecycleReport> {
    let scan = st.last()?;
    // Groups minus files already recycled, so "keep one copy" counts what's left.
    let groups = st.live_groups();
    let mut seen = HashSet::new();
    let paths: Vec<String> = paths.iter().filter(|p| seen.insert(p.to_ascii_lowercase())).cloned().collect();
    let accepted = check_request(Some(&scan), groups.as_deref(), &paths)?;

    let mut results = Vec::new();
    let mut freed = 0;
    for (p, size) in &accepted {
        match recycle::recycle(p) {
            Ok(()) => {
                freed += size;
                if let Ok(mut r) = st.removed.write() {
                    r.insert(p.to_ascii_lowercase());
                }
                results.push(RecycleResult { path: p.clone(), ok: true, error: None });
            }
            Err(e) => results.push(RecycleResult { path: p.clone(), ok: false, error: Some(e.to_string()) }),
        }
    }
    db.audit(
        "info",
        "user",
        "storage.recycled",
        Some(serde_json::json!({
            "files": results.iter().filter(|r| r.ok).map(|r| &r.path).collect::<Vec<_>>(),
            "bytes": freed,
            "failed": results.iter().filter(|r| !r.ok).count(),
        })),
    );
    Ok(RecycleReport { results, freed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_locations() {
        let win = std::env::var("WINDIR").unwrap();
        assert!(protection(&format!(r"{win}\System32\kernel32.dll")).is_some());
        assert!(protection(r"C:\pagefile.sys").is_some());
        assert!(protection(r"D:\hiberfil.sys").is_some());
        assert!(protection(r"C:\$Recycle.Bin\S-1-5\$R123.bin").is_some());
        assert!(protection(r"C:\Users\me\Videos\pagefile.sys").is_none(), "only at a drive root");
        assert!(protection(r"D:\Downloads\movie.mkv").is_none());
        let pf = std::env::var("ProgramFiles").unwrap();
        assert!(protection(&format!(r"{pf}\App\big.dat")).is_some());
    }

    fn rec(p: &Path) -> FileRec {
        FileRec { size: std::fs::metadata(p).unwrap().len(), path: p.display().to_string(), modified: None }
    }

    #[test]
    fn request_checks() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.bin");
        let b = d.path().join("b.bin");
        let other = d.path().join("other.bin");
        for p in [&a, &b, &other] {
            std::fs::write(p, [1u8; 64]).unwrap();
        }
        let scan = ScanResult { large: vec![rec(&a), rec(&b)], ..Default::default() };
        let groups = vec![DupGroup { size: 64, wasted: 64, files: vec![rec(&a), rec(&b)] }];
        let s = |p: &Path| p.display().to_string();

        assert!(check_request(Some(&scan), Some(&groups), &[s(&a)]).is_ok());
        assert!(check_request(Some(&scan), Some(&groups), &[s(&a), s(&b)]).is_err(), "never every copy");
        assert!(check_request(Some(&scan), None, &[s(&a), s(&b)]).is_ok(), "not duplicates: fine");
        assert!(check_request(Some(&scan), Some(&groups), &[s(&other)]).is_err(), "not in the scan");
        assert!(check_request(None, None, &[s(&a)]).is_err(), "no scan at all");
        assert!(check_request(Some(&scan), None, &[]).is_err());
        std::fs::write(&a, [1u8; 65]).unwrap();
        assert!(check_request(Some(&scan), None, &[s(&a)]).is_err(), "changed since the scan");
        std::fs::remove_file(&b).unwrap();
        assert!(check_request(Some(&scan), None, &[s(&b)]).is_err(), "gone");
    }

    #[test]
    fn scan_folder_and_views() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join(r"sub\x.bin"), [0u8; 300]).unwrap();
        std::fs::write(d.path().join("y.bin"), [0u8; 100]).unwrap();
        let st = StorageState::default();
        assert!(folder(&st, None).is_err(), "nothing scanned yet");
        let s = run_scan(&st, &d.path().display().to_string(), &|_| {}).unwrap();
        assert_eq!(s.total_size, 400);
        let root = folder(&st, None).unwrap();
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.own_size, 100);
        let sub = folder(&st, Some(root.children[0].id)).unwrap();
        assert_eq!(sub.crumbs.len(), 2);
        assert!(sub.path.ends_with("sub"));
        assert_eq!(large_files(&st).unwrap()[0].size, 300);
        assert!(run_scan(&st, r"Z:\definitely\missing", &|_| {}).is_err());
    }
}
