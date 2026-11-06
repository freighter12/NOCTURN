//! Duplicate detection: same size → same first 64 KiB → same SHA-256.
//! Only files that pass every stage are reported, so a group always holds
//! byte-identical files (modulo SHA-256 collisions).

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::scan::FileRec;

const HEAD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct DupGroup {
    pub size: u64,
    /// Space that deleting all but one copy would free.
    pub wasted: u64,
    pub files: Vec<FileRec>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DupProgress {
    pub hashed_bytes: u64,
    pub total_bytes: u64,
    pub groups: usize,
}

fn hash(path: &str, limit: Option<usize>, cancel: &AtomicBool) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut left = limit.unwrap_or(usize::MAX);
    while left > 0 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let want = buf.len().min(left);
        let n = f.read(&mut buf[..want]).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        left -= n;
    }
    Some(h.finalize().into())
}

fn regroup(groups: Vec<Vec<FileRec>>, limit: Option<usize>, cancel: &AtomicBool, on_file: &mut dyn FnMut(u64)) -> Vec<Vec<FileRec>> {
    let mut out = Vec::new();
    for g in groups {
        let mut by: HashMap<[u8; 32], Vec<FileRec>> = HashMap::new();
        for f in g {
            if cancel.load(Ordering::Relaxed) {
                return out;
            }
            let n = limit.map(|l| (l as u64).min(f.size)).unwrap_or(f.size);
            if let Some(k) = hash(&f.path, limit, cancel) {
                by.entry(k).or_default().push(f);
            }
            on_file(n);
        }
        out.extend(by.into_values().filter(|v| v.len() > 1));
    }
    out
}

pub fn find(candidates: &[FileRec], cancel: &AtomicBool, progress: &dyn Fn(DupProgress)) -> Vec<DupGroup> {
    let mut by_size: HashMap<u64, Vec<FileRec>> = HashMap::new();
    for f in candidates {
        by_size.entry(f.size).or_default().push(f.clone());
    }
    let groups: Vec<Vec<FileRec>> = by_size.into_values().filter(|v| v.len() > 1).collect();

    // Progress counts the head pass plus a worst-case full pass.
    let head_total: u64 = groups.iter().flatten().map(|f| f.size.min(HEAD_BYTES as u64)).sum();
    let mut total = head_total + groups.iter().flatten().map(|f| f.size).sum::<u64>();
    let mut hashed = 0u64;
    let mut last = Instant::now();
    let mut tick = |n: u64, total: u64, groups: usize| {
        hashed += n;
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            progress(DupProgress { hashed_bytes: hashed, total_bytes: total, groups });
        }
    };

    let heads = regroup(groups, Some(HEAD_BYTES), cancel, &mut |n| tick(n, total, 0));
    // Files ruled out by the head pass won't be read in full: shrink the estimate.
    let full_total: u64 = heads.iter().flatten().map(|f| f.size).sum();
    total = head_total + full_total;
    let n_heads = heads.len();
    let full = regroup(heads, None, cancel, &mut |n| tick(n, total, n_heads));

    let mut out: Vec<DupGroup> = full
        .into_iter()
        .map(|mut files| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            let size = files[0].size;
            DupGroup { size, wasted: size * (files.len() as u64 - 1), files }
        })
        .collect();
    out.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    progress(DupProgress { hashed_bytes: total, total_bytes: total, groups: out.len() });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(p: &std::path::Path) -> FileRec {
        FileRec { size: std::fs::metadata(p).unwrap().len(), path: p.display().to_string(), modified: None }
    }

    #[test]
    fn finds_only_byte_identical_files() {
        let d = tempfile::tempdir().unwrap();
        let mut a = vec![7u8; 200_000];
        let w = |n: &str, data: &[u8]| {
            let p = d.path().join(n);
            std::fs::write(&p, data).unwrap();
            rec(&p)
        };
        let f1 = w("one.bin", &a);
        let f2 = w("two.bin", &a);
        // Same size and same first 64 KiB, different tail: must not match.
        a[150_000] = 1;
        let f3 = w("three.bin", &a);
        // Different size.
        let f4 = w("four.bin", &a[..100_000]);
        let groups = find(&[f1, f2, f3, f4], &AtomicBool::new(false), &|_| {});
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].files.len(), 2);
        assert!(groups[0].files.iter().all(|f| f.path.ends_with("one.bin") || f.path.ends_with("two.bin")));
        assert_eq!(groups[0].wasted, 200_000);
    }

    #[test]
    fn unreadable_files_are_dropped() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.bin");
        std::fs::write(&p, [1u8; 10]).unwrap();
        let ghost = FileRec { size: 10, path: d.path().join("gone.bin").display().to_string(), modified: None };
        assert!(find(&[rec(&p), ghost], &AtomicBool::new(false), &|_| {}).is_empty());
    }
}
