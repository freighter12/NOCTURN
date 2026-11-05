//! Game sessions and overlay detection.
//!
//! While a detected game is running, every telemetry sample is classified:
//! - **GPU-limited** — GPU busy ≥ 95%: the graphics card sets the frame rate.
//! - **CPU-limited** — GPU < 80% while the busiest CPU thread is ≥ 90%: the
//!   game is waiting on one or more CPU threads.
//! - otherwise neither is saturated (frame cap, V-Sync, engine or I/O limit).
//!
//! These are the same signals PresentMon-style tools use for a first guess;
//! they're reported as "likely", not as measured frame-time causes. Sessions
//! shorter than a minute aren't stored.

use std::collections::HashSet;


use rusqlite::params;
use serde::Serialize;

use super::detect::Game;
use crate::db::Db;
use crate::error::AppResult;
use crate::system::monitor::{ProcessInfo, Sample};

const GPU_BOUND: f32 = 95.0;
const GPU_NOT_BUSY: f32 = 80.0;
const CORE_BUSY: f32 = 90.0;
const MIN_SESSION_MS: i64 = 60_000;

/// Known overlay / capture processes that hook into games.
pub const OVERLAYS: &[(&str, &str)] = &[
    ("gameoverlayui.exe", "Steam overlay"),
    ("eosoverlayrenderer-win64-shipping.exe", "Epic Games overlay"),
    ("nvidia overlay.exe", "NVIDIA overlay"),
    ("nvidia share.exe", "NVIDIA ShadowPlay / overlay"),
    ("gamebar.exe", "Xbox Game Bar"),
    ("gamebarftserver.exe", "Xbox Game Bar"),
    ("rtss.exe", "RivaTuner Statistics Server"),
    ("msiafterburner.exe", "MSI Afterburner"),
    ("overwolf.exe", "Overwolf"),
    ("medal.exe", "Medal"),
    ("obs64.exe", "OBS Studio"),
    ("discord.exe", "Discord (in-game overlay if enabled)"),
    ("radeonsoftware.exe", "AMD Radeon overlay"),
    ("amdrsserv.exe", "AMD Radeon overlay"),
    ("ubisoftconnect.exe", "Ubisoft Connect overlay"),
    ("eadesktop.exe", "EA app overlay"),
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Overlay {
    pub process: String,
    pub label: &'static str,
}

pub fn running_overlays(procs: &[ProcessInfo]) -> Vec<Overlay> {
    let names: HashSet<String> = procs.iter().map(|p| p.name.to_ascii_lowercase()).collect();
    let mut seen = HashSet::new();
    OVERLAYS
        .iter()
        .filter(|(p, _)| names.contains(*p))
        .filter(|(_, l)| seen.insert(*l))
        .map(|(p, l)| Overlay { process: procs.iter().find(|x| x.name.eq_ignore_ascii_case(p)).map(|x| x.name.clone()).unwrap_or_else(|| p.to_string()), label: l })
        .collect()
}

/// The running process that belongs to a detected game, if any (the one
/// using the most memory when a game runs several processes).
pub fn running_game<'a>(games: &'a [Game], procs: &'a [ProcessInfo]) -> Option<(&'a Game, &'a ProcessInfo)> {
    let mut best: Option<(&Game, &ProcessInfo)> = None;
    for p in procs {
        let Some(exe) = p.exe.as_deref() else { continue };
        let exe_l = exe.to_ascii_lowercase();
        let hit = games.iter().find(|g| {
            g.exes.iter().any(|e| e.eq_ignore_ascii_case(exe))
                || g.install_dir.as_ref().is_some_and(|d| {
                    let d = d.to_ascii_lowercase();
                    exe_l.starts_with(&d) && exe_l.as_bytes().get(d.len()) == Some(&b'\\')
                })
        });
        if let Some(g) = hit {
            if best.is_none_or(|(_, b)| p.memory > b.memory) {
                best = Some((g, p));
            }
        }
    }
    best
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LiveSession {
    pub game_id: String,
    pub game_name: String,
    pub exe: String,
    pub started_at: i64,
    pub last_seen: i64,
    pub samples: u32,
    pub cpu_sum: f64,
    pub max_core_sum: f64,
    pub gpu_sum: f64,
    pub gpu_n: u32,
    pub gpu_temp_peak: Option<f32>,
    pub mem_pct_sum: f64,
    pub game_mem_peak: u64,
    pub gpu_bound: u32,
    pub cpu_bound: u32,
    /// Latest reading for the live view.
    pub game_cpu: f32,
    pub game_mem: u64,
    #[serde(skip)]
    pub missing: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionSummary {
    pub id: Option<i64>,
    pub game_id: String,
    pub game_name: String,
    pub exe: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub samples: u32,
    pub avg_cpu: f32,
    pub avg_max_core: f32,
    pub avg_gpu: Option<f32>,
    pub peak_gpu_temp: Option<f32>,
    pub avg_mem_pct: f32,
    pub game_mem_peak: u64,
    pub gpu_bound_pct: f32,
    pub cpu_bound_pct: f32,
    pub verdict: String,
}

pub fn verdict(samples: u32, gpu_bound_pct: f32, cpu_bound_pct: f32, has_gpu: bool) -> String {
    if samples < 10 {
        return "Not enough data yet — play for at least a minute.".into();
    }
    if !has_gpu {
        return "No GPU usage readings on this PC, so the bottleneck can't be estimated.".into();
    }
    if gpu_bound_pct >= 60.0 {
        "Likely GPU-limited most of the time. Lower resolution or graphics settings (or enable DLSS/FSR) to raise FPS; Windows and CPU tweaks won't change much.".into()
    } else if cpu_bound_pct >= 30.0 {
        "Often CPU-limited: the GPU waited on the processor. Closing background apps, a higher game priority or lower CPU-heavy settings (view distance, crowds, physics) may help.".into()
    } else {
        "Neither the CPU nor the GPU was saturated — a frame-rate cap, V-Sync or the game engine itself is probably setting the pace.".into()
    }
}

impl LiveSession {
    fn new(g: &Game, p: &ProcessInfo, now: i64) -> Self {
        LiveSession { game_id: g.id.clone(), game_name: g.name.clone(), exe: p.exe.clone().unwrap_or_default(), started_at: now, last_seen: now, ..Default::default() }
    }

    fn add(&mut self, s: &Sample, p: &ProcessInfo) {
        let max_core = s.cpu_per_core.iter().cloned().fold(0.0f32, f32::max);
        let gpu = s.gpus.iter().filter_map(|g| g.usage).reduce(f32::max);
        self.samples += 1;
        self.cpu_sum += s.cpu_usage as f64;
        self.max_core_sum += max_core as f64;
        if let Some(g) = gpu {
            self.gpu_sum += g as f64;
            self.gpu_n += 1;
            if g >= GPU_BOUND {
                self.gpu_bound += 1;
            } else if g < GPU_NOT_BUSY && max_core >= CORE_BUSY {
                self.cpu_bound += 1;
            }
        }
        if let Some(t) = s.gpus.iter().filter_map(|g| g.temperature_c).reduce(f32::max) {
            self.gpu_temp_peak = Some(self.gpu_temp_peak.map_or(t, |m| m.max(t)));
        }
        self.mem_pct_sum += if s.mem_total > 0 { s.mem_used as f64 / s.mem_total as f64 * 100.0 } else { 0.0 };
        self.game_mem_peak = self.game_mem_peak.max(p.memory);
        self.game_cpu = p.cpu;
        self.game_mem = p.memory;
        self.last_seen = s.ts;
        self.missing = 0;
    }

    pub fn summary(&self) -> SessionSummary {
        let n = self.samples.max(1) as f32;
        let gn = self.gpu_n.max(1) as f32;
        let gpu_bound_pct = self.gpu_bound as f32 / gn * 100.0;
        let cpu_bound_pct = self.cpu_bound as f32 / gn * 100.0;
        SessionSummary {
            id: None,
            game_id: self.game_id.clone(),
            game_name: self.game_name.clone(),
            exe: self.exe.clone(),
            started_at: self.started_at,
            ended_at: self.last_seen,
            samples: self.samples,
            avg_cpu: (self.cpu_sum / n as f64) as f32,
            avg_max_core: (self.max_core_sum / n as f64) as f32,
            avg_gpu: (self.gpu_n > 0).then(|| (self.gpu_sum / gn as f64) as f32),
            peak_gpu_temp: self.gpu_temp_peak,
            avg_mem_pct: (self.mem_pct_sum / n as f64) as f32,
            game_mem_peak: self.game_mem_peak,
            gpu_bound_pct,
            cpu_bound_pct,
            verdict: verdict(self.samples, gpu_bound_pct, cpu_bound_pct, self.gpu_n > 0),
        }
    }
}

/// Feed one telemetry sample. Returns a finished session when a game exits
/// (after two consecutive samples without it, to ride out brief restarts).
pub fn observe(live: &mut Option<LiveSession>, games: &[Game], procs: &[ProcessInfo], s: &Sample) -> Option<SessionSummary> {
    match (running_game(games, procs), live.as_mut()) {
        (Some((g, p)), Some(cur)) if cur.game_id == g.id => {
            cur.add(s, p);
            None
        }
        (Some((g, p)), _) => {
            let finished = live.take().map(|c| c.summary());
            let mut fresh = LiveSession::new(g, p, s.ts);
            fresh.add(s, p);
            *live = Some(fresh);
            finished.filter(|f| f.ended_at - f.started_at >= MIN_SESSION_MS)
        }
        (None, Some(cur)) => {
            cur.missing += 1;
            if cur.missing >= 2 {
                let f = live.take().map(|c| c.summary());
                return f.filter(|f| f.ended_at - f.started_at >= MIN_SESSION_MS);
            }
            None
        }
        (None, None) => None,
    }
}

pub fn store(db: &Db, s: &SessionSummary, profile_json: Option<String>) -> AppResult<i64> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO game_sessions(game_id, game_name, exe, started_at, ended_at, samples, avg_cpu, avg_max_core, avg_gpu, peak_gpu_temp, avg_mem_pct, game_mem_peak, gpu_bound_pct, cpu_bound_pct, profile_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![s.game_id, s.game_name, s.exe, s.started_at, s.ended_at, s.samples, s.avg_cpu, s.avg_max_core, s.avg_gpu, s.peak_gpu_temp, s.avg_mem_pct, s.game_mem_peak as i64, s.gpu_bound_pct, s.cpu_bound_pct, profile_json],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(db: &Db, game_id: Option<&str>, limit: i64) -> AppResult<Vec<SessionSummary>> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT id, game_id, game_name, exe, started_at, ended_at, samples, avg_cpu, avg_max_core, avg_gpu, peak_gpu_temp, avg_mem_pct, game_mem_peak, gpu_bound_pct, cpu_bound_pct
         FROM game_sessions WHERE (?1 IS NULL OR game_id = ?1) ORDER BY started_at DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![game_id, limit], |r| {
            let (samples, gpu, gb, cb): (u32, Option<f32>, f32, f32) = (r.get(6)?, r.get(9)?, r.get(13)?, r.get(14)?);
            Ok(SessionSummary {
                id: r.get(0)?,
                game_id: r.get(1)?,
                game_name: r.get(2)?,
                exe: r.get(3)?,
                started_at: r.get(4)?,
                ended_at: r.get(5)?,
                samples,
                avg_cpu: r.get(7)?,
                avg_max_core: r.get(8)?,
                avg_gpu: gpu,
                peak_gpu_temp: r.get(10)?,
                avg_mem_pct: r.get(11)?,
                game_mem_peak: r.get::<_, i64>(12)? as u64,
                gpu_bound_pct: gb,
                cpu_bound_pct: cb,
                verdict: verdict(samples, gb, cb, gpu.is_some()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::monitor::GpuSample;

    fn game() -> Game {
        Game { id: "steam:1".into(), name: "Test".into(), launcher: "Steam", install_dir: Some(r"D:\Games\Test".into()), exes: vec![] }
    }
    fn proc_(exe: &str, mem: u64) -> ProcessInfo {
        ProcessInfo { pid: 1, parent: None, name: "x.exe".into(), exe: Some(exe.into()), cpu: 10.0, memory: mem, disk_read_bps: 0, disk_write_bps: 0, run_time_secs: 1, has_window: true }
    }
    fn sample(ts: i64, gpu: f32, core: f32) -> Sample {
        Sample { ts, cpu_usage: 20.0, cpu_per_core: vec![core, 10.0], mem_total: 100, mem_used: 50, gpus: vec![GpuSample { name: "g".into(), usage: Some(gpu), temperature_c: Some(70.0), ..Default::default() }], ..Default::default() }
    }

    #[test]
    fn matches_processes_inside_the_install_folder_only() {
        let games = vec![game()];
        assert!(running_game(&games, &[proc_(r"D:\Games\Test\bin\test.exe", 1)]).is_some());
        assert!(running_game(&games, &[proc_(r"D:\Games\TestOther\test.exe", 1)]).is_none(), "prefix must end at a folder boundary");
        let two = [proc_(r"D:\Games\Test\a.exe", 5), proc_(r"D:\Games\Test\b.exe", 50)];
        let (_, p) = running_game(&games, &two).unwrap();
        assert_eq!(p.memory, 50, "picks the main (largest) process");
    }

    #[test]
    fn session_lifecycle_and_classification() {
        let games = vec![game()];
        let running = vec![proc_(r"D:\Games\Test\t.exe", 100)];
        let mut live = None;
        for i in 0..70 {
            // 50 GPU-bound samples, 20 CPU-bound samples, one per second.
            let s = if i < 50 { sample(i * 1000, 99.0, 50.0) } else { sample(i * 1000, 60.0, 97.0) };
            assert!(observe(&mut live, &games, &running, &s).is_none());
        }
        assert!(observe(&mut live, &games, &[], &sample(71_000, 0.0, 0.0)).is_none(), "one missing sample is tolerated");
        let done = observe(&mut live, &games, &[], &sample(72_000, 0.0, 0.0)).expect("session ends");
        assert!(live.is_none());
        assert_eq!(done.samples, 70);
        assert!((done.gpu_bound_pct - 71.4).abs() < 0.2, "{}", done.gpu_bound_pct);
        assert!((done.cpu_bound_pct - 28.6).abs() < 0.2);
        assert!(done.verdict.starts_with("Likely GPU-limited"));
        assert_eq!(done.peak_gpu_temp, Some(70.0));
    }

    #[test]
    fn short_sessions_are_dropped_and_storage_roundtrips() {
        let games = vec![game()];
        let running = vec![proc_(r"D:\Games\Test\t.exe", 1)];
        let mut live = None;
        observe(&mut live, &games, &running, &sample(0, 50.0, 50.0));
        observe(&mut live, &games, &[], &sample(1000, 0.0, 0.0));
        assert!(observe(&mut live, &games, &[], &sample(2000, 0.0, 0.0)).is_none(), "a 1-second session isn't stored");

        let db = Db::open_in_memory().unwrap();
        let mut l = LiveSession::new(&game(), &running[0], 0);
        for i in 0..20 {
            l.add(&sample(i * 5000, 50.0, 95.0), &running[0]);
        }
        let id = store(&db, &l.summary(), None).unwrap();
        let back = list(&db, Some("steam:1"), 10).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, Some(id));
        assert!(back[0].verdict.starts_with("Often CPU-limited"));
    }

    #[test]
    fn overlays_are_deduplicated_by_label() {
        let mut a = proc_("x", 1);
        a.name = "GameBar.exe".into();
        let mut b = proc_("x", 1);
        b.name = "GameBarFTServer.exe".into();
        let o = running_overlays(&[a, b]);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].label, "Xbox Game Bar");
    }
}
