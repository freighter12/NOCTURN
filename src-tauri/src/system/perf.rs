//! Windows performance counters (PDH) for the numbers sysinfo doesn't give:
//! per-disk throughput and busy time, memory commit and paging.
//!
//! English counter paths (`PdhAddEnglishCounterW`) work on every display
//! language. The query lives on the telemetry thread and is collected once
//! per tick; rate counters need two collections, so the first tick reads 0.

use serde::Serialize;
use windows::core::w;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

#[derive(Debug, Clone, Serialize, Default)]
pub struct DiskIo {
    /// PDH instance name, e.g. "0 C:" or "1 D:".
    pub disk: String,
    pub read_bps: u64,
    pub write_bps: u64,
    /// Percent of time the disk was busy (100 − % idle).
    pub active_pct: f32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PerfSample {
    pub disks: Vec<DiskIo>,
    pub commit_bytes: Option<u64>,
    pub commit_limit: Option<u64>,
    /// Hard page faults resolved from disk per second (memory pressure signal).
    pub pages_per_sec: Option<f32>,
}

pub struct Perf {
    query: PDH_HQUERY,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
    idle: PDH_HCOUNTER,
    commit: PDH_HCOUNTER,
    limit: PDH_HCOUNTER,
    pages: PDH_HCOUNTER,
}

impl Drop for Perf {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

impl Perf {
    pub fn new() -> Option<Self> {
        unsafe {
            let mut q = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut q) != 0 {
                return None;
            }
            let add = |path: windows::core::PCWSTR| {
                let mut c = PDH_HCOUNTER::default();
                (PdhAddEnglishCounterW(q, path, 0, &mut c) == 0).then_some(c)
            };
            let perf = Perf {
                read: add(w!("\\PhysicalDisk(*)\\Disk Read Bytes/sec")).unwrap_or_default(),
                write: add(w!("\\PhysicalDisk(*)\\Disk Write Bytes/sec")).unwrap_or_default(),
                idle: add(w!("\\PhysicalDisk(*)\\% Idle Time")).unwrap_or_default(),
                commit: add(w!("\\Memory\\Committed Bytes")).unwrap_or_default(),
                limit: add(w!("\\Memory\\Commit Limit")).unwrap_or_default(),
                pages: add(w!("\\Memory\\Pages/sec")).unwrap_or_default(),
                query: q,
            };
            PdhCollectQueryData(q);
            Some(perf)
        }
    }

    pub fn sample(&mut self) -> PerfSample {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return PerfSample::default();
            }
        }
        let reads = array(self.read);
        let writes = array(self.write);
        let idles = array(self.idle);
        let mut disks: Vec<DiskIo> = reads
            .iter()
            .filter(|(n, _)| n != "_Total")
            .map(|(n, r)| DiskIo {
                disk: n.clone(),
                read_bps: r.max(0.0) as u64,
                write_bps: writes.iter().find(|(m, _)| m == n).map(|(_, v)| v.max(0.0) as u64).unwrap_or(0),
                active_pct: idles.iter().find(|(m, _)| m == n).map(|(_, v)| (100.0 - v).clamp(0.0, 100.0) as f32).unwrap_or(0.0),
            })
            .collect();
        disks.sort_by(|a, b| a.disk.cmp(&b.disk));
        PerfSample {
            disks,
            commit_bytes: single(self.commit).map(|v| v as u64),
            commit_limit: single(self.limit).map(|v| v as u64),
            pages_per_sec: single(self.pages).map(|v| v as f32),
        }
    }
}

fn single(c: PDH_HCOUNTER) -> Option<f64> {
    if c.is_invalid() {
        return None;
    }
    let mut v = PDH_FMT_COUNTERVALUE::default();
    unsafe {
        if PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) != 0 || v.CStatus != 0 {
            return None;
        }
        Some(v.Anonymous.doubleValue)
    }
}

fn array(c: PDH_HCOUNTER) -> Vec<(String, f64)> {
    if c.is_invalid() {
        return vec![];
    }
    unsafe {
        let (mut size, mut count) = (0u32, 0u32);
        if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA || size == 0 {
            return vec![];
        }
        // The buffer holds the item array followed by the instance-name strings.
        let mut buf: Vec<u64> = vec![0; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
            return vec![];
        }
        (0..count as usize)
            .filter_map(|i| {
                let it = &*items.add(i);
                if it.FmtValue.CStatus != 0 {
                    return None;
                }
                Some((it.szName.to_string().ok()?, it.FmtValue.Anonymous.doubleValue))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_real_counters() {
        let mut p = Perf::new().expect("open PDH query");
        std::thread::sleep(std::time::Duration::from_millis(400));
        let s = p.sample();
        assert!(!s.disks.is_empty(), "at least one physical disk instance");
        assert!(s.disks.iter().all(|d| (0.0..=100.0).contains(&d.active_pct)));
        let (c, l) = (s.commit_bytes.unwrap(), s.commit_limit.unwrap());
        assert!(c > 0 && l >= c, "commit {c} within limit {l}");
        assert!(s.pages_per_sec.is_some());
    }
}
