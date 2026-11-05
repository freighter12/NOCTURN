//! Live telemetry.
//!
//! A background thread samples CPU, memory, GPU, disk I/O, network and
//! storage at the user's chosen interval (1/2/5 s) and emits a `telemetry`
//! event to the UI. It keeps a short in-memory history for live charts,
//! writes one averaged row per minute for the 7-day history, raises threshold
//! alerts, and refreshes the process snapshot (~every 3 s) that the process
//! manager, saved rules and the AI use. Monitoring can be paused; saved
//! process rules keep working at a slow rate while it is.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{
    CpuRefreshKind, Disks, Gpus, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
    RefreshKind, System, UpdateKind,
};
use tauri::{AppHandle, Emitter};

use super::gpu::Nvidia;
use super::history::{self, Alerts, MinuteAcc};
use super::perf::{DiskIo, Perf};
use super::process_control;
use crate::state::Shared;

const HISTORY: usize = 120;

#[derive(Debug, Clone, Serialize, Default)]
pub struct GpuSample {
    pub name: String,
    /// `None` until Windows has two samples to compute a delta from.
    pub usage: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// The fields below come from the vendor library (NVIDIA NVML) only.
    pub temperature_c: Option<f32>,
    pub core_clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub max_core_clock_mhz: Option<u32>,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub fan_pct: Option<u32>,
    /// "nvml" when vendor sensors are available, otherwise "windows".
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DiskSample {
    pub mount: String,
    pub label: String,
    pub file_system: String,
    pub kind: String,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Sample {
    pub ts: i64,
    pub interval_ms: u32,
    pub cpu_usage: f32,
    pub cpu_per_core: Vec<f32>,
    pub cpu_freq_mhz: u64,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Memory pressure: committed virtual memory vs the commit limit, and
    /// hard page faults per second.
    pub commit_bytes: Option<u64>,
    pub commit_limit: Option<u64>,
    pub pages_per_sec: Option<f32>,
    pub gpus: Vec<GpuSample>,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub disks: Vec<DiskSample>,
    pub disk_io: Vec<DiskIo>,
    pub process_count: usize,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub exe: Option<String>,
    /// Percent of *total* CPU capacity (0–100), like Task Manager.
    pub cpu: f32,
    pub memory: u64,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub run_time_secs: u64,
    /// Owns a visible top-level window (an "app" rather than background work).
    #[serde(default)]
    pub has_window: bool,
}

/// Per-app usage since NOCTURN started (grouped by process name).
#[derive(Debug, Clone, Serialize, Default)]
pub struct ProcStat {
    pub name: String,
    pub samples: u32,
    pub cpu_sum: f64,
    pub cpu_peak: f32,
    pub mem_peak: u64,
    pub first_seen: i64,
    pub last_seen: i64,
}

impl ProcStat {
    pub fn cpu_avg(&self) -> f32 {
        if self.samples == 0 { 0.0 } else { (self.cpu_sum / self.samples as f64) as f32 }
    }
}

const MAX_STATS: usize = 800;

#[derive(Default)]
pub struct TelemetryStore {
    pub latest: Option<Sample>,
    pub history: VecDeque<Sample>,
    pub processes: Vec<ProcessInfo>,
    pub processes_at: Option<i64>,
    pub stats: HashMap<String, ProcStat>,
    pub paused: bool,
}

pub type SharedTelemetry = Arc<RwLock<TelemetryStore>>;

fn update_stats(stats: &mut HashMap<String, ProcStat>, list: &[ProcessInfo], now: i64) {
    let mut groups: HashMap<String, (f32, u64, String)> = HashMap::new();
    for p in list {
        let g = groups.entry(p.name.to_ascii_lowercase()).or_insert((0.0, 0, p.name.clone()));
        g.0 += p.cpu;
        g.1 += p.memory;
    }
    for (key, (cpu, mem, name)) in groups {
        let s = stats.entry(key).or_insert_with(|| ProcStat { name, first_seen: now, ..Default::default() });
        s.samples += 1;
        s.cpu_sum += cpu as f64;
        s.cpu_peak = s.cpu_peak.max(cpu);
        s.mem_peak = s.mem_peak.max(mem);
        s.last_seen = now;
    }
    if stats.len() > MAX_STATS {
        // Forget apps not seen for the longest time.
        let mut v: Vec<(String, i64)> = stats.iter().map(|(k, s)| (k.clone(), s.last_seen)).collect();
        v.sort_by_key(|(_, t)| *t);
        for (k, _) in v.into_iter().take(stats.len() - MAX_STATS) {
            stats.remove(&k);
        }
    }
}

fn disk_samples(disks: &Disks) -> Vec<DiskSample> {
    disks
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| DiskSample {
            mount: d.mount_point().display().to_string(),
            label: d.name().to_string_lossy().into_owned(),
            file_system: d.file_system().to_string_lossy().into_owned(),
            kind: format!("{:?}", d.kind()),
            total: d.total_space(),
            available: d.available_space(),
            removable: d.is_removable(),
        })
        .collect()
}

pub fn start(app: AppHandle, state: Shared) {
    std::thread::Builder::new()
        .name("nocturn-telemetry".into())
        .spawn(move || run(app, state))
        .expect("spawn telemetry thread");
}

/// Process snapshot + saved-rule application. Returns the process count.
struct ProcScanner {
    sys: Option<System>,
    known: HashSet<u32>,
    last: Instant,
    logical: f32,
}

impl ProcScanner {
    fn scan(&mut self, state: &Shared) -> usize {
        let Some(ps) = self.sys.as_mut() else { return 0 };
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage().with_exe(UpdateKind::OnlyIfNotSet);
        ps.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        let window = self.last.elapsed().as_secs_f64().max(0.5);
        self.last = Instant::now();
        let windows = process_control::window_pids();
        let list: Vec<ProcessInfo> = ps
            .processes()
            .values()
            .map(|p| {
                let du = p.disk_usage();
                ProcessInfo {
                    pid: p.pid().as_u32(),
                    parent: p.parent().map(|x| x.as_u32()),
                    name: p.name().to_string_lossy().into_owned(),
                    exe: p.exe().map(|e| e.display().to_string()),
                    cpu: (p.cpu_usage() / self.logical).clamp(0.0, 100.0),
                    memory: p.memory(),
                    disk_read_bps: (du.read_bytes as f64 / window) as u64,
                    disk_write_bps: (du.written_bytes as f64 / window) as u64,
                    run_time_secs: p.run_time(),
                    has_window: windows.contains(&p.pid().as_u32()),
                }
            })
            .collect();
        // Saved rules apply once to each process that appeared since the last
        // scan (on the first scan: everything already running).
        let fresh: Vec<(u32, String)> = list.iter().filter(|p| !self.known.contains(&p.pid)).map(|p| (p.pid, p.name.clone())).collect();
        self.known = list.iter().map(|p| p.pid).collect();
        crate::system::rules::apply_to_new(&state.db, &state.rules, &fresh);
        let n = list.len();
        let now = chrono::Utc::now().timestamp_millis();
        if let Ok(mut s) = state.telemetry.write() {
            update_stats(&mut s.stats, &list, now);
            s.processes = list;
            s.processes_at = Some(now);
        }
        n
    }
}

fn run(app: AppHandle, state: Shared) {
    let store = state.telemetry.clone();
    let Ok(mut sys) = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    ) else {
        tracing::error!("telemetry: could not initialise system sampler");
        return;
    };
    let logical = sys.cpus().len().max(1) as f32;
    let mut procs = ProcScanner { sys: System::new().ok(), known: HashSet::new(), last: Instant::now(), logical };
    let mut gpus = Gpus::new_with_refreshed_list().ok();
    let mut nets = Networks::new_with_refreshed_list().ok();
    let mut disks = Disks::new_with_refreshed_list().ok();
    let nvidia = Nvidia::new();
    let mut perf = Perf::new();
    if perf.is_none() {
        tracing::warn!("telemetry: performance counters unavailable; disk I/O and memory pressure disabled");
    }

    let mut cfg = crate::settings::load(&state.db).monitor;
    let mut disk_cache = disks.as_ref().map(disk_samples).unwrap_or_default();
    let mut minute = MinuteAcc::default();
    let mut alerts = Alerts::default();
    let mut last_tick = Instant::now();
    let mut last_disks = Instant::now();
    let mut last_settings = Instant::now();
    let mut last_prune: Option<Instant> = None;
    let mut process_count = 0usize;

    loop {
        std::thread::sleep(Duration::from_millis(if cfg.paused { 1000 } else { cfg.interval_ms as u64 }));
        if last_settings.elapsed() >= Duration::from_secs(3) {
            cfg = crate::settings::load(&state.db).monitor;
            last_settings = Instant::now();
            if let Ok(mut s) = store.write() {
                s.paused = cfg.paused;
            }
        }

        // Process scan roughly every 3 s (every 10 s while paused, for rules).
        let proc_every = if cfg.paused { Duration::from_secs(10) } else { Duration::from_millis(2900) };
        if procs.last.elapsed() >= proc_every || procs.known.is_empty() {
            process_count = procs.scan(&state);
        }
        if cfg.paused {
            last_tick = Instant::now();
            continue;
        }

        let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
        last_tick = Instant::now();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let (mut rx, mut tx) = (0u64, 0u64);
        if let Some(n) = nets.as_mut() {
            n.refresh(true);
            for (name, data) in n.iter() {
                if name.to_string_lossy().to_ascii_lowercase().contains("loopback") {
                    continue;
                }
                rx += data.received();
                tx += data.transmitted();
            }
        }
        if last_disks.elapsed() >= Duration::from_secs(15) {
            if let Some(d) = disks.as_mut() {
                d.refresh(true);
                disk_cache = disk_samples(d);
            }
            last_disks = Instant::now();
        }
        let pf = perf.as_mut().map(|p| p.sample()).unwrap_or_default();

        // Windows (sysinfo) gives usage/VRAM for every vendor; NVML adds
        // temperature, clocks, power and fan for NVIDIA cards.
        let nv = nvidia.sample();
        let mut gpu_samples: Vec<GpuSample> = gpus
            .as_mut()
            .map(|g| {
                g.refresh(true);
                g.iter()
                    .map(|gpu| GpuSample {
                        name: gpu.model().unwrap_or("GPU").to_string(),
                        usage: gpu.usage(),
                        vram_used: gpu.used_memory(),
                        vram_total: gpu.total_memory(),
                        source: "windows".into(),
                        ..Default::default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        for n in nv {
            let target = gpu_samples.iter_mut().find(|g| g.name.contains(&n.name) || n.name.contains(&g.name));
            let g = match target {
                Some(g) => g,
                None => {
                    gpu_samples.push(GpuSample { name: n.name.clone(), ..Default::default() });
                    gpu_samples.last_mut().unwrap()
                }
            };
            g.usage = n.usage.or(g.usage);
            g.vram_used = n.vram_used.or(g.vram_used);
            g.vram_total = n.vram_total.or(g.vram_total);
            g.temperature_c = n.temperature_c;
            g.core_clock_mhz = n.core_clock_mhz;
            g.mem_clock_mhz = n.mem_clock_mhz;
            g.max_core_clock_mhz = n.max_core_clock_mhz;
            g.power_w = n.power_w;
            g.power_limit_w = n.power_limit_w;
            g.fan_pct = n.fan_pct;
            g.source = "nvml".into();
        }

        let sample = Sample {
            ts: chrono::Utc::now().timestamp_millis(),
            interval_ms: cfg.interval_ms,
            cpu_usage: sys.global_cpu_usage(),
            cpu_per_core: sys.cpus().iter().map(|c| c.usage()).collect(),
            cpu_freq_mhz: sys.cpus().first().map(|c| c.frequency()).unwrap_or(0),
            mem_total: sys.total_memory(),
            mem_used: sys.used_memory(),
            mem_available: sys.available_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            commit_bytes: pf.commit_bytes,
            commit_limit: pf.commit_limit,
            pages_per_sec: pf.pages_per_sec,
            gpus: gpu_samples,
            net_rx_bps: (rx as f64 / elapsed) as u64,
            net_tx_bps: (tx as f64 / elapsed) as u64,
            disks: disk_cache.clone(),
            disk_io: pf.disks,
            process_count,
            uptime_secs: System::uptime().unwrap_or(0),
        };

        if let Some(done) = minute.add(&sample) {
            if let Err(e) = history::store(&state.db, &done) {
                tracing::warn!("telemetry history write failed: {e}");
            }
        }
        if last_prune.is_none_or(|t| t.elapsed() >= Duration::from_secs(3600)) {
            let _ = history::prune(&state.db, sample.ts);
            last_prune = Some(Instant::now());
        }
        if state.games.games.read().map(|g| !g.is_empty()).unwrap_or(false) {
            let procs_now = store.read().map(|s| s.processes.clone()).unwrap_or_default();
            crate::games::observe(&state.db, &state.games, &procs_now, &sample);
        }
        for a in alerts.check(&sample, &cfg, Instant::now()) {
            let _ = app.emit("alert", &a);
        }

        if let Ok(mut s) = store.write() {
            s.history.push_back(sample.clone());
            while s.history.len() > HISTORY {
                s.history.pop_front();
            }
            s.latest = Some(sample.clone());
        }
        if let Err(e) = app.emit("telemetry", &sample) {
            tracing::warn!("telemetry emit failed: {e}");
        }
    }
}
