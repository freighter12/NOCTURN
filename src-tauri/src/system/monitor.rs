//! Live telemetry.
//!
//! A background thread samples CPU, memory, GPU, network and storage once per
//! second and emits a `telemetry` event to the UI. It also keeps a short
//! history (for charts) and a process snapshot (for the process view and the
//! AI) refreshed every 3 seconds.

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{
    CpuRefreshKind, Disks, Gpus, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
    RefreshKind, System, UpdateKind,
};
use tauri::{AppHandle, Emitter};

use crate::security::exec::{self, SafeCommand};

const HISTORY: usize = 120;

#[derive(Debug, Clone, Serialize, Default)]
pub struct GpuSample {
    pub name: String,
    /// `None` until Windows has two samples to compute a delta from.
    pub usage: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// Only available from a vendor tool (currently nvidia-smi).
    pub temperature_c: Option<f32>,
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
    pub cpu_usage: f32,
    pub cpu_per_core: Vec<f32>,
    pub cpu_freq_mhz: u64,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub gpus: Vec<GpuSample>,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub disks: Vec<DiskSample>,
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
}

#[derive(Default)]
pub struct TelemetryStore {
    pub latest: Option<Sample>,
    pub history: VecDeque<Sample>,
    pub processes: Vec<ProcessInfo>,
    pub processes_at: Option<i64>,
}

pub type SharedTelemetry = Arc<RwLock<TelemetryStore>>;

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

fn nvidia_temps() -> Vec<(String, f32)> {
    if exec::nvidia_smi_path().is_none() {
        return vec![];
    }
    let Ok(out) = exec::run_with_timeout(&SafeCommand::NvidiaSmiQuery, Duration::from_secs(5)) else {
        return vec![];
    };
    if !out.success() {
        return vec![];
    }
    out.stdout
        .lines()
        .filter_map(|l| {
            let mut parts = l.split(',').map(str::trim);
            let name = parts.next()?.to_string();
            let temp: f32 = parts.next()?.parse().ok()?;
            Some((name, temp))
        })
        .collect()
}

pub fn start(app: AppHandle, store: SharedTelemetry) {
    std::thread::Builder::new()
        .name("nocturn-telemetry".into())
        .spawn(move || run(app, store))
        .expect("spawn telemetry thread");
}

fn run(app: AppHandle, store: SharedTelemetry) {
    let Ok(mut sys) = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    ) else {
        tracing::error!("telemetry: could not initialise system sampler");
        return;
    };
    let mut proc_sys = System::new().ok();
    let mut gpus = Gpus::new_with_refreshed_list().ok();
    let mut nets = Networks::new_with_refreshed_list().ok();
    let mut disks = Disks::new_with_refreshed_list().ok();
    let logical = sys.cpus().len().max(1) as f32;

    let mut last_tick = Instant::now();
    let mut tick: u64 = 0;
    let mut disk_cache = disks.as_ref().map(disk_samples).unwrap_or_default();
    let mut temps: Vec<(String, f32)> = nvidia_temps();
    let proc_kind = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_disk_usage()
        .with_exe(UpdateKind::OnlyIfNotSet);

    loop {
        std::thread::sleep(Duration::from_millis(1000));
        let elapsed = last_tick.elapsed().as_secs_f64().max(0.001);
        last_tick = Instant::now();
        tick += 1;

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

        if tick % 15 == 0 {
            if let Some(d) = disks.as_mut() {
                d.refresh(true);
                disk_cache = disk_samples(d);
            }
        }
        if tick % 5 == 0 {
            temps = nvidia_temps();
        }

        let gpu_samples = gpus
            .as_mut()
            .map(|g| {
                g.refresh(true);
                g.iter()
                    .map(|gpu| {
                        let name = gpu.model().unwrap_or("GPU").to_string();
                        let temperature_c = temps
                            .iter()
                            .find(|(n, _)| name.contains(n.as_str()) || n.contains(name.as_str()))
                            .map(|(_, t)| *t);
                        GpuSample {
                            usage: gpu.usage(),
                            vram_used: gpu.used_memory(),
                            vram_total: gpu.total_memory(),
                            temperature_c,
                            name,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Process snapshot every 3 s.
        let mut process_count = store.read().map(|s| s.processes.len()).unwrap_or(0);
        if tick % 3 == 1 {
            if let Some(ps) = proc_sys.as_mut() {
                ps.refresh_processes_specifics(ProcessesToUpdate::All, true, proc_kind);
                let window = 3.0f64;
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
                            cpu: (p.cpu_usage() / logical).clamp(0.0, 100.0),
                            memory: p.memory(),
                            disk_read_bps: (du.read_bytes as f64 / window) as u64,
                            disk_write_bps: (du.written_bytes as f64 / window) as u64,
                            run_time_secs: p.run_time(),
                        }
                    })
                    .collect();
                process_count = list.len();
                if let Ok(mut s) = store.write() {
                    s.processes = list;
                    s.processes_at = Some(chrono::Utc::now().timestamp_millis());
                }
            }
        }

        let sample = Sample {
            ts: chrono::Utc::now().timestamp_millis(),
            cpu_usage: sys.global_cpu_usage(),
            cpu_per_core: sys.cpus().iter().map(|c| c.usage()).collect(),
            cpu_freq_mhz: sys.cpus().first().map(|c| c.frequency()).unwrap_or(0),
            mem_total: sys.total_memory(),
            mem_used: sys.used_memory(),
            mem_available: sys.available_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            gpus: gpu_samples,
            net_rx_bps: (rx as f64 / elapsed) as u64,
            net_tx_bps: (tx as f64 / elapsed) as u64,
            disks: disk_cache.clone(),
            process_count,
            uptime_secs: System::uptime().unwrap_or(0),
        };

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
