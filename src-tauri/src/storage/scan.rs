//! One-pass folder scan: per-folder sizes, the largest files, and candidates
//! for duplicate detection.
//!
//! * Junctions, symlinks and mount points are never followed (no loops, no
//!   double counting).
//! * Cloud-only placeholders (OneDrive "online-only" files) are counted
//!   separately and never opened — reading them would download them.
//! * Unreadable folders are counted, not fatal.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

const ATTR_REPARSE_POINT: u32 = 0x400;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_OPEN: u32 = 0x40000;
const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
const CLOUD_ONLY: u32 = ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA_ACCESS;

/// Files smaller than this are ignored for duplicate detection.
pub const DUP_MIN_BYTES: u64 = 1 << 20;
pub const LARGE_KEEP: usize = 200;

#[derive(Debug, Clone)]
pub struct Dir {
    pub name: String,
    pub parent: Option<u32>,
    /// Total including subfolders (after the scan finishes).
    pub size: u64,
    pub files: u64,
    /// Files directly in this folder.
    pub own_size: u64,
    pub own_files: u64,
    pub children: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileRec {
    pub size: u64,
    pub path: String,
    /// Last write time, Unix ms.
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub current: String,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub root: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub dirs: Vec<Dir>,
    /// Largest files, biggest first.
    pub large: Vec<FileRec>,
    pub dup_candidates: Vec<FileRec>,
    pub cloud_only: u64,
    pub unreadable: u64,
    pub skipped_links: u64,
    pub cancelled: bool,
}

impl ScanResult {
    pub fn total_size(&self) -> u64 {
        self.dirs.first().map(|d| d.size).unwrap_or(0)
    }
    pub fn total_files(&self) -> u64 {
        self.dirs.first().map(|d| d.files).unwrap_or(0)
    }
    pub fn path_of(&self, id: u32) -> PathBuf {
        let mut names = Vec::new();
        let mut cur = Some(id);
        while let Some(i) = cur {
            let d = &self.dirs[i as usize];
            if d.parent.is_some() {
                names.push(d.name.as_str());
            }
            cur = d.parent;
        }
        let mut p = PathBuf::from(&self.root);
        for n in names.iter().rev() {
            p.push(n);
        }
        p
    }
}

/// FILETIME (100 ns since 1601) → Unix ms.
fn filetime_ms(t: u64) -> Option<i64> {
    (t > 0).then(|| (t / 10_000) as i64 - 11_644_473_600_000)
}

pub fn scan(root: &Path, cancel: &AtomicBool, progress: &dyn Fn(ScanProgress)) -> ScanResult {
    let started = Instant::now();
    let mut r = ScanResult { root: root.display().to_string(), started_at: crate::db::now(), ..Default::default() };
    r.dirs.push(Dir { name: r.root.clone(), parent: None, size: 0, files: 0, own_size: 0, own_files: 0, children: vec![] });
    let mut heap: BinaryHeap<Reverse<FileRec>> = BinaryHeap::new();
    let mut stack: Vec<(PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut last_report = Instant::now();

    while let Some((dir, id)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            r.cancelled = true;
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => {
                r.unreadable += 1;
                continue;
            }
        };
        for entry in entries.flatten() {
            // On Windows this comes from the directory listing itself — no extra I/O.
            let Ok(md) = entry.metadata() else { continue };
            let attrs = md.file_attributes();
            if md.is_dir() {
                if attrs & ATTR_REPARSE_POINT != 0 {
                    r.skipped_links += 1;
                    continue;
                }
                let child = r.dirs.len() as u32;
                r.dirs.push(Dir {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    parent: Some(id),
                    size: 0,
                    files: 0,
                    own_size: 0,
                    own_files: 0,
                    children: vec![],
                });
                r.dirs[id as usize].children.push(child);
                stack.push((entry.path(), child));
                continue;
            }
            if attrs & CLOUD_ONLY != 0 {
                r.cloud_only += 1;
                continue;
            }
            if md.file_type().is_symlink() {
                r.skipped_links += 1;
                continue;
            }
            let size = md.len();
            let d = &mut r.dirs[id as usize];
            d.own_size += size;
            d.own_files += 1;
            files += 1;
            bytes += size;
            let needs_path = size >= DUP_MIN_BYTES || heap.len() < LARGE_KEEP || heap.peek().is_some_and(|m| size > m.0.size);
            if needs_path {
                let rec = FileRec { size, path: entry.path().display().to_string(), modified: filetime_ms(md.last_write_time()) };
                if size >= DUP_MIN_BYTES {
                    r.dup_candidates.push(rec.clone());
                }
                heap.push(Reverse(rec));
                if heap.len() > LARGE_KEEP {
                    heap.pop();
                }
            }
        }
        if last_report.elapsed() >= Duration::from_millis(250) {
            last_report = Instant::now();
            progress(ScanProgress { files, dirs: r.dirs.len() as u64, bytes, current: dir.display().to_string() });
        }
    }

    // Children always come after their parent, so one reverse pass totals the tree.
    for d in r.dirs.iter_mut() {
        d.size = d.own_size;
        d.files = d.own_files;
    }
    for i in (1..r.dirs.len()).rev() {
        let (size, n, parent) = (r.dirs[i].size, r.dirs[i].files, r.dirs[i].parent.unwrap_or(0) as usize);
        r.dirs[parent].size += size;
        r.dirs[parent].files += n;
    }
    r.large = heap.into_sorted_vec().into_iter().map(|Reverse(f)| f).collect();
    r.duration_ms = started.elapsed().as_millis() as u64;
    progress(ScanProgress { files, dirs: r.dirs.len() as u64, bytes, current: String::new() });
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_tree_and_largest_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(r"a\b")).unwrap();
        std::fs::write(d.path().join("top.bin"), vec![0u8; 10]).unwrap();
        std::fs::write(d.path().join(r"a\one.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(d.path().join(r"a\b\big.bin"), vec![0u8; 2 << 20]).unwrap();
        let r = scan(d.path(), &AtomicBool::new(false), &|_| {});
        assert!(!r.cancelled);
        assert_eq!(r.total_files(), 3);
        assert_eq!(r.total_size(), 10 + 100 + (2 << 20));
        assert_eq!(r.large[0].size, 2 << 20, "biggest first");
        assert!(r.large[0].path.ends_with("big.bin"));
        assert_eq!(r.dup_candidates.len(), 1, "only files >= 1 MiB");
        let a = r.dirs[0].children.iter().map(|&c| &r.dirs[c as usize]).find(|x| x.name == "a").unwrap();
        assert_eq!(a.size, 100 + (2 << 20));
        assert_eq!(a.own_size, 100);
        let b = a.children[0];
        assert_eq!(r.path_of(b), d.path().join(r"a\b"));
    }

    #[test]
    fn does_not_follow_junctions() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("real")).unwrap();
        std::fs::write(d.path().join(r"real\f.bin"), vec![0u8; 50]).unwrap();
        // A junction back to the root would loop forever if followed.
        let link = d.path().join(r"real\loop");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &link.display().to_string(), &d.path().display().to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let r = scan(d.path(), &AtomicBool::new(false), &|_| {});
        assert_eq!(r.total_size(), 50);
        if ok {
            assert_eq!(r.skipped_links, 1);
        }
    }

    #[test]
    fn cancel_stops_early() {
        let d = tempfile::tempdir().unwrap();
        let r = scan(d.path(), &AtomicBool::new(true), &|_| {});
        assert!(r.cancelled);
    }
}
