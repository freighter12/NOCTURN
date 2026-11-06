//! Benchmarks: CPU (integer + floating point, one core and all cores),
//! memory (bandwidth and latency), storage (sequential and 4K random, real
//! disk I/O with the Windows cache bypassed) and boot time from Windows' own
//! records. Results are stored so runs can be compared before/after a change.
//!
//! Benchmarks only read and write NOCTURN's own temporary file, which is
//! always deleted afterwards.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::exec::{self, PsScript};

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suite {
    Cpu,
    Memory,
    Storage,
    Boot,
}

impl Suite {
    fn as_str(self) -> &'static str {
        match self {
            Suite::Cpu => "cpu",
            Suite::Memory => "memory",
            Suite::Storage => "storage",
            Suite::Boot => "boot",
        }
    }
    fn parse(s: &str) -> Option<Suite> {
        Some(match s {
            "cpu" => Suite::Cpu,
            "memory" => Suite::Memory,
            "storage" => Suite::Storage,
            "boot" => Suite::Boot,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Metric {
    pub key: String,
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub higher_is_better: bool,
}

fn m(key: &str, label: &str, value: f64, unit: &str, higher_is_better: bool) -> Metric {
    Metric { key: key.into(), label: label.into(), value, unit: unit.into(), higher_is_better }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Context {
    pub power_plan: Option<String>,
    pub threads: usize,
    pub cpu: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchRun {
    pub id: i64,
    pub started_at: String,
    pub suite: Suite,
    pub target: Option<String>,
    pub label: String,
    pub duration_ms: i64,
    pub metrics: Vec<Metric>,
    pub context: Context,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchProgress {
    pub stage: String,
    pub pct: f64,
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Relaxed);
    }
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

fn cancelled() -> AppResult<()> {
    if CANCEL.load(Ordering::Relaxed) {
        Err(AppError::msg("benchmark cancelled"))
    } else {
        Ok(())
    }
}

// ------------------------------------------------------------------- CPU

fn run_cpu(progress: &dyn Fn(BenchProgress)) -> AppResult<Vec<Metric>> {
    let t = threads();
    let d = Duration::from_secs(3);
    let stop = &CANCEL;
    progress(BenchProgress { stage: "Integer, one core".into(), pct: 0.0 });
    let h1 = nocturn_bench::cpu_hash(1, d, stop);
    cancelled()?;
    progress(BenchProgress { stage: format!("Integer, all {t} threads"), pct: 25.0 });
    let hn = nocturn_bench::cpu_hash(t, d, stop);
    cancelled()?;
    progress(BenchProgress { stage: "Floating point, one core".into(), pct: 50.0 });
    let f1 = nocturn_bench::cpu_matmul(1, d, stop);
    cancelled()?;
    progress(BenchProgress { stage: format!("Floating point, all {t} threads"), pct: 75.0 });
    let fnn = nocturn_bench::cpu_matmul(t, d, stop);
    cancelled()?;
    Ok(vec![
        m("hash_1t", "Integer (SHA-256), one core", h1, "MB/s", true),
        m("hash_mt", "Integer (SHA-256), all cores", hn, "MB/s", true),
        m("flops_1t", "Floating point, one core", f1, "GFLOPS", true),
        m("flops_mt", "Floating point, all cores", fnn, "GFLOPS", true),
        m("scaling", "Multi-core scaling", hn / h1.max(1e-9), "×", true),
    ])
}

// ---------------------------------------------------------------- memory

fn run_memory(progress: &dyn Fn(BenchProgress)) -> AppResult<Vec<Metric>> {
    let avail = sysinfo::System::new()
        .map(|mut sys| {
            sys.refresh_memory();
            sys.available_memory() as usize
        })
        .unwrap_or(2 << 30);
    // Two buffers of this size are used; stay well inside free memory.
    let bytes = (256usize << 20).min(avail / 8).max(32 << 20);
    progress(BenchProgress { stage: "Bandwidth (read, write, copy)".into(), pct: 0.0 });
    let (r, w, c) = nocturn_bench::mem_bandwidth(bytes, threads(), Duration::from_secs(3), &CANCEL);
    cancelled()?;
    progress(BenchProgress { stage: "Latency (random access)".into(), pct: 60.0 });
    let lat = nocturn_bench::mem_latency((128usize << 20).min(avail / 8), Duration::from_secs(3), &CANCEL);
    cancelled()?;
    Ok(vec![
        m("read", "Read bandwidth", r, "GB/s", true),
        m("write", "Write bandwidth", w, "GB/s", true),
        m("copy", "Copy bandwidth", c, "GB/s", true),
        m("latency", "Random-access latency", lat, "ns", false),
    ])
}

// --------------------------------------------------------------- storage

const NO_BUFFERING: u32 = 0x2000_0000;
const WRITE_THROUGH: u32 = 0x8000_0000;

/// A 4 KiB-aligned buffer, as unbuffered I/O requires.
struct Aligned {
    ptr: *mut u8,
    len: usize,
    layout: std::alloc::Layout,
}
impl Aligned {
    fn new(len: usize) -> Self {
        let layout = std::alloc::Layout::from_size_align(len, 4096).expect("layout");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "out of memory");
        Aligned { ptr, len, layout }
    }
    fn slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}
impl Drop for Aligned {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.ptr, self.layout) }
    }
}

/// Deletes the test file and its folder however the run ends.
struct TempFile {
    dir: PathBuf,
    file: PathBuf,
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

/// Where the test file goes on `drive` (e.g. `C:\`), validated against the
/// mounted fixed drives.
fn test_dir(drive: &str) -> AppResult<(PathBuf, u64)> {
    let disks = sysinfo::Disks::new_with_refreshed_list().map_err(|e| AppError::msg(e.to_string()))?;
    let want = drive.trim_end_matches('\\').to_ascii_uppercase();
    let d = disks
        .iter()
        .find(|d| d.mount_point().to_string_lossy().trim_end_matches('\\').to_ascii_uppercase() == want)
        .ok_or_else(|| AppError::Invalid(format!("{drive} isn't a mounted drive")))?;
    let free = d.available_space();
    let sysdrive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_ascii_uppercase();
    let dir = if want == sysdrive {
        std::env::temp_dir().join("NOCTURN-benchmark")
    } else {
        PathBuf::from(format!("{want}\\NOCTURN-benchmark"))
    };
    Ok((dir, free))
}

fn run_storage(drive: &str, progress: &dyn Fn(BenchProgress)) -> AppResult<Vec<Metric>> {
    let (dir, free) = test_dir(drive)?;
    const GIB: u64 = 1 << 30;
    let size: u64 = if free > 8 * GIB { 1 << 30 } else if free > 3 * GIB { 512 << 20 } else {
        return Err(AppError::Invalid(format!("{drive} needs at least 3 GB free for the storage test")));
    };
    std::fs::create_dir_all(&dir).map_err(|e| AppError::msg(format!("couldn't create the test folder on {drive}: {e}")))?;
    let tmp = TempFile { file: dir.join(format!("test-{}.bin", uuid::Uuid::new_v4())), dir };
    let chunk = 1usize << 20;
    let mut buf = Aligned::new(chunk);
    let mut seed = 0x1234_5678_9ABC_DEF0u64;
    // Incompressible data so drives that compress can't cheat.
    for w in buf.slice().chunks_exact_mut(8) {
        w.copy_from_slice(&xorshift(&mut seed).to_le_bytes());
    }

    // Sequential write (also creates the file for the read tests).
    progress(BenchProgress { stage: "Sequential write".into(), pct: 0.0 });
    let mut f = OpenOptions::new().write(true).create_new(true).custom_flags(NO_BUFFERING | WRITE_THROUGH).open(&tmp.file)?;
    let t0 = Instant::now();
    let mut written = 0u64;
    while written < size {
        f.write_all(buf.slice())?;
        written += chunk as u64;
        if written % (64 << 20) == 0 {
            cancelled()?;
            progress(BenchProgress { stage: "Sequential write".into(), pct: 25.0 * written as f64 / size as f64 });
        }
    }
    let seq_write = written as f64 / 1e6 / t0.elapsed().as_secs_f64();
    drop(f);

    // Sequential read, bypassing the Windows file cache.
    progress(BenchProgress { stage: "Sequential read".into(), pct: 25.0 });
    let mut f = OpenOptions::new().read(true).custom_flags(NO_BUFFERING).open(&tmp.file)?;
    let t0 = Instant::now();
    let mut read = 0u64;
    while read < size {
        let n = f.read(buf.slice())?;
        if n == 0 {
            break;
        }
        read += n as u64;
        if read % (64 << 20) == 0 {
            cancelled()?;
        }
    }
    let seq_read = read as f64 / 1e6 / t0.elapsed().as_secs_f64();

    // 4K random read, one request at a time (how most apps and games load).
    progress(BenchProgress { stage: "4K random read".into(), pct: 50.0 });
    let blocks = size / 4096;
    let mut small = Aligned::new(4096);
    let (rr_iops, rr_lat) = {
        let t0 = Instant::now();
        let mut n = 0u64;
        while t0.elapsed() < Duration::from_secs(4) {
            let off = (xorshift(&mut seed) % blocks) * 4096;
            f.seek(SeekFrom::Start(off))?;
            f.read_exact(small.slice())?;
            n += 1;
            if n % 1024 == 0 {
                cancelled()?;
            }
        }
        let s = t0.elapsed().as_secs_f64();
        (n as f64 / s, s * 1e6 / n.max(1) as f64)
    };
    drop(f);

    // 4K random write, write-through.
    progress(BenchProgress { stage: "4K random write".into(), pct: 75.0 });
    let mut f = OpenOptions::new().write(true).custom_flags(NO_BUFFERING | WRITE_THROUGH).open(&tmp.file)?;
    let rw_iops = {
        let t0 = Instant::now();
        let mut n = 0u64;
        while t0.elapsed() < Duration::from_secs(3) {
            let off = (xorshift(&mut seed) % blocks) * 4096;
            f.seek(SeekFrom::Start(off))?;
            f.write_all(small.slice())?;
            n += 1;
            if n % 256 == 0 {
                cancelled()?;
            }
        }
        n as f64 / t0.elapsed().as_secs_f64()
    };
    drop(f);
    drop(tmp);

    Ok(vec![
        m("seq_read", "Sequential read", seq_read, "MB/s", true),
        m("seq_write", "Sequential write", seq_write, "MB/s", true),
        m("rand_read_iops", "4K random read", rr_iops, "IOPS", true),
        m("rand_read_lat", "4K random read latency", rr_lat, "µs", false),
        m("rand_write_iops", "4K random write", rw_iops, "IOPS", true),
    ])
}

// ------------------------------------------------------------------ boot

#[derive(Debug, Clone, Serialize)]
pub struct Boot {
    pub start: DateTime<Utc>,
    pub logon: Option<DateTime<Utc>>,
    /// Kernel start → sign-in, seconds. Includes time spent at the sign-in screen.
    pub to_sign_in: Option<f64>,
    /// 0 cold boot, 1 fast startup, 2 resume from hibernation.
    pub boot_type: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootReport {
    pub boots: Vec<Boot>,
    /// Firmware (BIOS/UEFI) time for the most recent boot, seconds.
    pub firmware_s: Option<f64>,
    pub median_to_sign_in: Option<f64>,
    pub fast_startup: Option<bool>,
}

pub fn parse_boots(rows: &[Value]) -> Vec<Boot> {
    let t = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc));
    let mut out: Vec<Boot> = rows
        .iter()
        .filter_map(|r| {
            let start = t(r, "start")?;
            let logon = t(r, "logon");
            let to_sign_in = logon.map(|l| (l - start).num_milliseconds() as f64 / 1000.0).filter(|s| *s >= 0.0 && *s < 1800.0);
            let boot_type = r.get("boot_type").and_then(|b| b.as_str()).and_then(|s| s.parse().ok());
            Some(Boot { start, logon, to_sign_in, boot_type })
        })
        .collect();
    out.sort_by(|a, b| b.start.cmp(&a.start));
    // A sign-in can only belong to the latest boot before it.
    for i in 1..out.len() {
        if let (Some(l), newer) = (out[i].logon, out[i - 1].start) {
            if l > newer {
                out[i].logon = None;
                out[i].to_sign_in = None;
            }
        }
    }
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(v[v.len() / 2])
}

pub fn boot_report() -> AppResult<BootReport> {
    let rows = exec::as_array(exec::powershell_json(PsScript::BootTimes)?);
    let boots = parse_boots(&rows);
    let power = r"SYSTEM\CurrentControlSet\Control\Session Manager\Power";
    let reg = |name: &str| {
        crate::tweaks::registry::read(crate::tweaks::Hive::Hklm, power, name).ok().flatten().and_then(|v| v.as_dword())
    };
    let median_to_sign_in = median(boots.iter().filter(|b| b.boot_type == Some(0) || b.boot_type.is_none()).filter_map(|b| b.to_sign_in).collect());
    Ok(BootReport { firmware_s: reg("FwPOSTTime").filter(|v| *v > 0).map(|ms| ms as f64 / 1000.0), median_to_sign_in, fast_startup: reg("HiberbootEnabled").map(|v| v == 1), boots })
}

fn run_boot() -> AppResult<Vec<Metric>> {
    let r = boot_report()?;
    let mut out = Vec::new();
    if let Some(f) = r.firmware_s {
        out.push(m("firmware", "Firmware (BIOS/UEFI) start", f, "s", false));
    }
    if let Some(s) = r.boots.first().and_then(|b| b.to_sign_in) {
        out.push(m("last_to_sign_in", "Last boot: Windows start to sign-in", s, "s", false));
    }
    if let Some(s) = r.median_to_sign_in {
        out.push(m("median_to_sign_in", "Typical (median) Windows start to sign-in", s, "s", false));
    }
    if out.is_empty() {
        return Err(AppError::msg("Windows hasn't recorded any boot timings yet"));
    }
    Ok(out)
}

// ------------------------------------------------------------- run/store

fn context() -> Context {
    let cpu = sysinfo::System::new().ok().and_then(|mut sys| {
        sys.refresh_cpu_all();
        sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty())
    });
    Context {
        power_plan: crate::system::power::list().ok().and_then(|l| l.into_iter().find(|s| s.active).map(|s| s.name)),
        threads: threads(),
        cpu,
        notes: vec![],
    }
}

pub fn run(db: &Db, suite: Suite, target: Option<String>, label: &str, progress: &dyn Fn(BenchProgress)) -> AppResult<BenchRun> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::msg("a benchmark is already running"));
    }
    let _guard = Guard;
    CANCEL.store(false, Ordering::SeqCst);
    let started_at = crate::db::now();
    let t0 = Instant::now();
    let mut ctx = context();
    let target = match suite {
        Suite::Storage => Some(target.ok_or_else(|| AppError::Invalid("choose a drive".into()))?),
        _ => None,
    };
    let metrics = match suite {
        Suite::Cpu => run_cpu(progress)?,
        Suite::Memory => run_memory(progress)?,
        Suite::Storage => run_storage(target.as_deref().unwrap_or(""), progress)?,
        Suite::Boot => run_boot()?,
    };
    if matches!(suite, Suite::Cpu | Suite::Memory) {
        ctx.notes.push("Close other apps for repeatable numbers; differences under about 5% are normal run-to-run variation.".into());
    }
    progress(BenchProgress { stage: "Done".into(), pct: 100.0 });
    let label: String = label.chars().filter(|c| !c.is_control()).take(80).collect();
    let label = if label.trim().is_empty() { "Benchmark".to_string() } else { label };
    let duration_ms = t0.elapsed().as_millis() as i64;
    let id = {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO bench_runs(started_at, suite, target, label, duration_ms, metrics_json, context_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![started_at, suite.as_str(), target, label, duration_ms, serde_json::to_string(&metrics)?, serde_json::to_string(&ctx)?],
        )?;
        conn.last_insert_rowid()
    };
    db.audit("info", "user", "bench.run", Some(serde_json::json!({ "suite": suite.as_str(), "target": target, "id": id })));
    Ok(BenchRun { id, started_at, suite, target, label, duration_ms, metrics, context: ctx })
}

pub fn history(db: &Db, limit: i64) -> AppResult<Vec<BenchRun>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, started_at, suite, target, label, duration_ms, metrics_json, context_json FROM bench_runs ORDER BY started_at DESC, id DESC LIMIT ?1")?;
    let rows = st
        .query_map([limit], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, started_at, suite, target, label, duration_ms, mj, cj)| {
            Some(BenchRun {
                id,
                started_at,
                suite: Suite::parse(&suite)?,
                target,
                label,
                duration_ms,
                metrics: serde_json::from_str(&mj).ok()?,
                context: serde_json::from_str(&cj).unwrap_or_default(),
            })
        })
        .collect())
}

pub fn delete(db: &Db, id: i64) -> AppResult<()> {
    db.conn().execute("DELETE FROM bench_runs WHERE id = ?1", [id])?;
    Ok(())
}

pub fn rename(db: &Db, id: i64, label: &str) -> AppResult<()> {
    let label: String = label.chars().filter(|c| !c.is_control()).take(80).collect();
    db.conn().execute("UPDATE bench_runs SET label = ?1 WHERE id = ?2", rusqlite::params![label, id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Tests that touch the global RUNNING flag must not overlap.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn boots_pair_with_their_own_sign_in() {
        let rows = vec![
            json!({"start":"2026-11-06T13:38:27Z","logon":"2026-11-06T13:38:43Z","boot_type":"0"}),
            // This boot's next sign-in is after the newer boot: not its own.
            json!({"start":"2026-11-05T10:00:00Z","logon":"2026-11-06T13:38:43Z","boot_type":"0"}),
            json!({"start":"2026-11-04T10:00:00Z","logon":"2026-11-04T10:00:30Z","boot_type":"1"}),
        ];
        let b = parse_boots(&rows);
        assert_eq!(b[0].to_sign_in, Some(16.0));
        assert_eq!(b[1].to_sign_in, None);
        assert_eq!(b[2].boot_type, Some(1));
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn storage_rejects_unknown_drive() {
        assert!(test_dir("Q:\\nope").is_err());
    }

    #[test]
    fn runs_are_stored_and_listed() {
        let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let db = Db::open_in_memory().unwrap();
        let r = run(&db, Suite::Memory, None, "test", &|_| {}).unwrap();
        assert!(r.metrics.iter().any(|m| m.key == "latency" && m.value > 0.0));
        let h = history(&db, 10).unwrap();
        assert_eq!(h.len(), 1);
        // JSON round-trips f64 to within an ulp, not always bit-exactly.
        assert_eq!(h[0].metrics.len(), r.metrics.len());
        for (a, b) in h[0].metrics.iter().zip(&r.metrics) {
            assert_eq!(a.key, b.key);
            assert!((a.value - b.value).abs() <= b.value.abs() * 1e-12, "{} {} vs {}", a.key, a.value, b.value);
        }
        rename(&db, r.id, "after").unwrap();
        assert_eq!(history(&db, 10).unwrap()[0].label, "after");
        delete(&db, r.id).unwrap();
        assert!(history(&db, 10).unwrap().is_empty());
        assert!(!running());
    }

    #[test]
    fn one_run_at_a_time() {
        let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        RUNNING.store(true, Ordering::SeqCst);
        let db = Db::open_in_memory().unwrap();
        let e = run(&db, Suite::Boot, None, "x", &|_| {});
        RUNNING.store(false, Ordering::SeqCst);
        assert!(e.is_err());
    }

    /// Real disk I/O on the system drive (~1 GB written, then deleted).
    /// Run explicitly: `cargo test -- --ignored storage_benchmark`.
    #[test]
    #[ignore]
    fn storage_benchmark() {
        let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let db = Db::open_in_memory().unwrap();
        let drive = format!("{}\\", std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()));
        let r = run(&db, Suite::Storage, Some(drive), "test", &|_| {}).unwrap();
        println!("{:#?}", r.metrics);
        assert!(r.metrics.iter().all(|m| m.value > 0.0));
        assert!(!std::env::temp_dir().join("NOCTURN-benchmark").exists(), "test file cleaned up");
    }
}
