//! Process-manager view model: processes grouped by app with session stats and
//! evidence-based flags, per-instance details, and group-wide priority /
//! affinity changes.

use std::collections::HashMap;

use serde::Serialize;

use super::monitor::ProcessInfo;
use super::process_control::{self, Priority};
use super::processes::is_protected;
use super::rules::{self, ProcessRule};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ProcGroup {
    pub name: String,
    pub instances: usize,
    pub cpu: f32,
    pub memory: u64,
    pub disk_bps: u64,
    pub has_window: bool,
    pub exe: Option<String>,
    pub protected: bool,
    /// Session statistics (since NOCTURN started).
    pub cpu_avg: f32,
    pub cpu_peak: f32,
    pub mem_peak: u64,
    pub first_seen: Option<i64>,
    /// "high_cpu" | "high_memory" | "disk_heavy" — each backed by a measured threshold.
    pub flags: Vec<&'static str>,
    pub rule: Option<ProcessRule>,
}

#[derive(Debug, Serialize)]
pub struct ProcOverview {
    pub groups: Vec<ProcGroup>,
    pub total_processes: usize,
    pub logical_cpus: usize,
    pub mem_total: u64,
    pub sampled_at: Option<i64>,
}

/// Thresholds for the heavy-process flags. Sustained CPU uses the session
/// average so a one-off spike doesn't flag an app.
const CPU_NOW: f32 = 15.0;
const CPU_AVG: f32 = 8.0;
const MEM_MIN: u64 = 1536 * 1024 * 1024;
const MEM_FRACTION: f64 = 0.10;
const DISK_BPS: u64 = 25 * 1024 * 1024;

pub fn flags_for(cpu: f32, cpu_avg: f32, memory: u64, mem_total: u64, disk: u64) -> Vec<&'static str> {
    let mut f = Vec::new();
    if cpu >= CPU_NOW && cpu_avg >= CPU_AVG {
        f.push("high_cpu");
    }
    if memory >= MEM_MIN.max((mem_total as f64 * MEM_FRACTION) as u64) {
        f.push("high_memory");
    }
    if disk >= DISK_BPS {
        f.push("disk_heavy");
    }
    f
}

pub fn overview(state: &AppState) -> ProcOverview {
    let (procs, stats, latest, at) = match state.telemetry.read() {
        Ok(t) => (t.processes.clone(), t.stats.clone(), t.latest.clone(), t.processes_at),
        Err(_) => Default::default(),
    };
    let rules: Vec<ProcessRule> = state.rules.read().map(|r| r.clone()).unwrap_or_default();
    let mem_total = latest.as_ref().map(|s| s.mem_total).unwrap_or(0);
    let logical_cpus = latest.as_ref().map(|s| s.cpu_per_core.len()).unwrap_or(0);

    let mut groups: HashMap<String, Vec<&ProcessInfo>> = HashMap::new();
    for p in procs.iter().filter(|p| p.pid > 4) {
        groups.entry(p.name.to_ascii_lowercase()).or_default().push(p);
    }
    let mut out: Vec<ProcGroup> = groups
        .into_iter()
        .map(|(key, ps)| {
            let cpu: f32 = ps.iter().map(|p| p.cpu).sum();
            let memory: u64 = ps.iter().map(|p| p.memory).sum();
            let disk: u64 = ps.iter().map(|p| p.disk_read_bps + p.disk_write_bps).sum();
            let st = stats.get(&key);
            let cpu_avg = st.map(|s| s.cpu_avg()).unwrap_or(cpu);
            let name = ps[0].name.clone();
            ProcGroup {
                instances: ps.len(),
                cpu,
                memory,
                disk_bps: disk,
                has_window: ps.iter().any(|p| p.has_window),
                exe: ps.iter().find_map(|p| p.exe.clone()),
                protected: is_protected(&name),
                cpu_avg,
                cpu_peak: st.map(|s| s.cpu_peak).unwrap_or(cpu),
                mem_peak: st.map(|s| s.mem_peak).unwrap_or(memory),
                first_seen: st.map(|s| s.first_seen),
                flags: flags_for(cpu, cpu_avg, memory, mem_total, disk),
                rule: rules.iter().find(|r| r.name.eq_ignore_ascii_case(&name)).cloned(),
                name,
            }
        })
        .collect();
    out.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
    ProcOverview { total_processes: procs.len(), groups: out, logical_cpus, mem_total, sampled_at: at }
}

#[derive(Debug, Serialize)]
pub struct Instance {
    pub pid: u32,
    pub cpu: f32,
    pub memory: u64,
    pub exe: Option<String>,
    pub has_window: bool,
    pub run_time_secs: u64,
    pub priority: Option<String>,
    pub affinity: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProcDetails {
    pub name: String,
    pub protected: bool,
    pub instances: Vec<Instance>,
    pub system_mask: String,
    pub logical_cpus: u32,
    pub rule: Option<ProcessRule>,
}

fn instances_of(state: &AppState, name: &str) -> Vec<ProcessInfo> {
    state
        .telemetry
        .read()
        .map(|t| t.processes.iter().filter(|p| p.name.eq_ignore_ascii_case(name)).cloned().collect())
        .unwrap_or_default()
}

pub fn details(state: &AppState, name: &str) -> AppResult<ProcDetails> {
    let ps = instances_of(state, name);
    if ps.is_empty() {
        return Err(AppError::NotFound(format!("{name} is not running")));
    }
    let sys = process_control::system_mask();
    let instances = ps
        .iter()
        .map(|p| Instance {
            pid: p.pid,
            cpu: p.cpu,
            memory: p.memory,
            exe: p.exe.clone(),
            has_window: p.has_window,
            run_time_secs: p.run_time_secs,
            priority: process_control::priority_label(p.pid),
            affinity: process_control::affinity(p.pid).map(|(m, _)| format!("0x{m:x}")),
        })
        .collect();
    Ok(ProcDetails {
        name: ps[0].name.clone(),
        protected: is_protected(name),
        instances,
        system_mask: format!("0x{sys:x}"),
        logical_cpus: sys.count_ones(),
        rule: rules::get(&state.db, name).ok().flatten(),
    })
}

#[derive(Debug, Serialize)]
pub struct ChangeOutcome {
    pub pid: u32,
    pub ok: bool,
    pub error: Option<String>,
}

fn for_each(state: &AppState, name: &str, f: impl Fn(u32) -> AppResult<()>) -> AppResult<Vec<ChangeOutcome>> {
    if is_protected(name) {
        return Err(AppError::Forbidden(format!("{name} is a core Windows process and can't be changed from NOCTURN")));
    }
    let ps = instances_of(state, name);
    if ps.is_empty() {
        return Err(AppError::NotFound(format!("{name} is not running")));
    }
    Ok(ps
        .iter()
        .map(|p| match f(p.pid) {
            Ok(()) => ChangeOutcome { pid: p.pid, ok: true, error: None },
            Err(e) => ChangeOutcome { pid: p.pid, ok: false, error: Some(e.to_string()) },
        })
        .collect())
}

pub fn set_priority_all(state: &AppState, name: &str, p: Priority) -> AppResult<Vec<ChangeOutcome>> {
    for_each(state, name, |pid| process_control::set_priority(pid, name, p))
}

pub fn set_affinity_all(state: &AppState, name: &str, mask: u64) -> AppResult<Vec<ChangeOutcome>> {
    process_control::validate_mask(mask, process_control::system_mask())?;
    for_each(state, name, |pid| process_control::set_affinity(pid, name, mask))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_need_sustained_or_large_usage() {
        let gb = 1024 * 1024 * 1024u64;
        assert!(flags_for(40.0, 2.0, 0, 32 * gb, 0).is_empty(), "a spike alone is not 'high CPU'");
        assert_eq!(flags_for(40.0, 20.0, 0, 32 * gb, 0), vec!["high_cpu"]);
        assert!(flags_for(0.0, 0.0, 2 * gb, 32 * gb, 0).is_empty(), "2 GB of 32 GB is under 10%");
        assert_eq!(flags_for(0.0, 0.0, 4 * gb, 32 * gb, 0), vec!["high_memory"]);
        assert_eq!(flags_for(0.0, 0.0, 2 * gb, 8 * gb, 0), vec!["high_memory"]);
        assert_eq!(flags_for(0.0, 0.0, 0, 8 * gb, 30 * 1024 * 1024), vec!["disk_heavy"]);
    }
}
