//! Windows performance counters (PDH) for the numbers sysinfo doesn't give:
//! per-disk throughput and busy time, memory commit and paging, and GPU
//! usage per graphics card measured the way Task Manager does it.
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
    /// GPU usage per adapter LUID ("0x00000000_0x0000d1e7"): the busiest
    /// engine, 0–100. Empty until the second collection.
    pub gpu_by_luid: std::collections::HashMap<String, f32>,
}

pub struct Perf {
    query: PDH_HQUERY,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
    idle: PDH_HCOUNTER,
    commit: PDH_HCOUNTER,
    limit: PDH_HCOUNTER,
    pages: PDH_HCOUNTER,
    gpu: PDH_HCOUNTER,
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
                gpu: add(w!("\\GPU Engine(*)\\Utilization Percentage")).unwrap_or_default(),
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
            gpu_by_luid: gpu_usage_by_adapter(&array(self.gpu)),
        }
    }
}

/// Windows reports one "GPU Engine" instance per process per engine, named
/// like `pid_1234_luid_0x00000000_0x0000D1E7_phys_0_eng_3_engtype_3D`.
/// Adding every instance together (what sysinfo does) counts the 3D, copy,
/// video and compute engines on top of each other and goes past 100%. Task
/// Manager sums the processes per engine and shows the busiest engine; so do we.
pub fn gpu_usage_by_adapter(items: &[(String, f64)]) -> std::collections::HashMap<String, f32> {
    use std::collections::HashMap;
    let mut per_engine: HashMap<(String, String), f64> = HashMap::new();
    for (name, v) in items {
        let lower = name.to_ascii_lowercase();
        let Some(start) = lower.find("luid_") else { continue };
        let rest = &lower[start + 5..];
        // "0xHIGH_0xLOW_phys_N_eng_M_engtype_X"
        let mut parts = rest.split('_');
        let (Some(hi), Some(lo)) = (parts.next(), parts.next()) else { continue };
        let luid = format!("{hi}_{lo}");
        let engine = match (rest.find("phys_"), rest.find("_engtype")) {
            (Some(a), Some(b)) if b > a => rest[a..b].to_string(),
            (Some(a), _) => rest[a..].to_string(),
            _ => continue,
        };
        if v.is_finite() && *v > 0.0 {
            *per_engine.entry((luid, engine)).or_insert(0.0) += v;
        } else {
            per_engine.entry((luid, engine)).or_insert(0.0);
        }
    }
    let mut out: HashMap<String, f32> = HashMap::new();
    for ((luid, _), v) in per_engine {
        let e = out.entry(luid).or_insert(0.0);
        *e = e.max(v.clamp(0.0, 100.0) as f32);
    }
    out
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
    fn gpu_usage_is_busiest_engine_not_sum() {
        let items: Vec<(String, f64)> = [
            ("pid_100_luid_0x00000000_0x0000D1E7_phys_0_eng_0_engtype_3D", 60.0),
            ("pid_200_luid_0x00000000_0x0000D1E7_phys_0_eng_0_engtype_3D", 25.0),
            ("pid_100_luid_0x00000000_0x0000D1E7_phys_0_eng_3_engtype_VideoDecode", 70.0),
            ("pid_300_luid_0x00000000_0x0000D1E7_phys_0_eng_5_engtype_Copy", 40.0),
            ("pid_100_luid_0x00000000_0x0000AAAA_phys_0_eng_0_engtype_3D", 12.0),
            // Two processes saturating one engine still reads 100, not 130.
            ("pid_1_luid_0x00000000_0x0000BBBB_phys_0_eng_0_engtype_3D", 80.0),
            ("pid_2_luid_0x00000000_0x0000BBBB_phys_0_eng_0_engtype_3D", 50.0),
            ("garbage", 99.0),
        ]
        .iter()
        .map(|(n, v)| (n.to_string(), *v))
        .collect();
        let m = gpu_usage_by_adapter(&items);
        assert_eq!(m["0x00000000_0x0000d1e7"], 85.0); // 3D 60+25 beats decode 70 and copy 40
        assert_eq!(m["0x00000000_0x0000aaaa"], 12.0);
        assert_eq!(m["0x00000000_0x0000bbbb"], 100.0);
        assert_eq!(m.len(), 3);
    }

    /// Old (sum of all engines) vs new (busiest engine) on this PC.
    #[test]
    #[ignore]
    fn live_gpu_usage_compare() {
        let mut p = Perf::new().unwrap();
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            unsafe { PdhCollectQueryData(p.query) };
            let items = array(p.gpu);
            let sum: f64 = items.iter().map(|(_, v)| v.max(0.0)).sum();
            let new = gpu_usage_by_adapter(&items);
            eprintln!("{} instances, old sum {sum:.1}%, new {new:?}", items.len());
            assert!(new.values().all(|v| (0.0..=100.0).contains(v)));
        }
        let _ = p.sample();
    }

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
