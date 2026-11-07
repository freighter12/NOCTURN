//! "Why is my PC slow right now?" — watch for a few seconds, then explain in
//! plain words what is using the processor, memory, disks and network, with a
//! fix for each finding that goes through NOCTURN's usual safe paths (closing
//! an app that isn't a protected Windows process, opening a page, or a change
//! shown in the approval dialog). Windows' own background work (updates,
//! antivirus scans, search indexing) is named and explained rather than closed.

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;

use crate::state::AppState;
use crate::system::monitor::{ProcessInfo, Sample};
use crate::system::processes;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Close every instance of an app (a non-protected process name).
    CloseApp { name: String, label: String },
    /// Open a NOCTURN page.
    OpenPage { page: String, label: String },
    /// Preview and apply an optimisation through the approval dialog.
    ApplyTweak { id: String, label: String },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    /// "high" | "medium" | "info"
    pub severity: &'static str,
    pub title: String,
    pub detail: String,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Summary {
    pub seconds: f32,
    pub cpu_pct: f32,
    pub memory_pct: f32,
    pub disk_busy_pct: f32,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub gpu_pct: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub uptime_days: f32,
    pub game: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlowReport {
    pub summary: Summary,
    pub findings: Vec<Finding>,
    /// One line for the top of the results.
    pub verdict: String,
}

/// Extra facts that don't come from the samples.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub power_plan: Option<String>,
    pub running_game: Option<String>,
    /// System drive (C:) free and total bytes, and whether it's a hard disk.
    pub system_drive: Option<(u64, u64, bool)>,
}

/// Friendly names for processes people see in Task Manager.
fn friendly(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    let known: &[(&str, &str)] = &[
        ("chrome.exe", "Google Chrome"),
        ("msedge.exe", "Microsoft Edge"),
        ("firefox.exe", "Firefox"),
        ("brave.exe", "Brave"),
        ("opera.exe", "Opera"),
        ("opera_gx.exe", "Opera GX"),
        ("discord.exe", "Discord"),
        ("spotify.exe", "Spotify"),
        ("steam.exe", "Steam"),
        ("steamwebhelper.exe", "Steam"),
        ("epicgameslauncher.exe", "Epic Games Launcher"),
        ("onedrive.exe", "OneDrive"),
        ("code.exe", "VS Code"),
        ("teams.exe", "Microsoft Teams"),
        ("ms-teams.exe", "Microsoft Teams"),
        ("msmpeng.exe", "Windows Security antivirus"),
        ("tiworker.exe", "Windows Update"),
        ("trustedinstaller.exe", "Windows Update"),
        ("mousocoreworker.exe", "Windows Update"),
        ("wuauclt.exe", "Windows Update"),
        ("searchindexer.exe", "Windows Search indexing"),
        ("searchprotocolhost.exe", "Windows Search indexing"),
        ("searchfilterhost.exe", "Windows Search indexing"),
        ("explorer.exe", "File Explorer"),
        ("dwm.exe", "Windows desktop (DWM)"),
        ("memcompression", "Windows memory compression"),
        ("system", "Windows (System)"),
        ("nocturn.exe", "NOCTURN"),
        ("msedgewebview2.exe", "WebView2 (used by apps like NOCTURN, Teams, Widgets)"),
    ];
    known.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string()).unwrap_or_else(|| name.trim_end_matches(".exe").trim_end_matches(".EXE").to_string())
}

/// Windows' own background work: explained, never closed.
fn background_task(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "tiworker.exe" | "trustedinstaller.exe" | "mousocoreworker.exe" | "wuauclt.exe" => Some("Windows Update is installing or preparing updates. It finishes on its own — avoid restarting in the middle of it."),
        "msmpeng.exe" => Some("Windows Security is scanning for malware. Scans usually take a few minutes and run when the PC seems idle."),
        "searchindexer.exe" | "searchprotocolhost.exe" | "searchfilterhost.exe" => Some("Windows is indexing files so search is fast. It slows down by itself when you use the PC."),
        "memcompression" => Some("Windows is compressing memory because RAM is getting full — a sign that apps need more memory than is free."),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
struct Group {
    name: String,
    cpu: f32,
    memory: u64,
    disk_bps: u64,
    count: usize,
    has_window: bool,
}

/// Processes grouped by name (a browser's many processes count as one app).
fn groups(procs: &[ProcessInfo]) -> Vec<Group> {
    let mut m: HashMap<String, Group> = HashMap::new();
    for p in procs {
        let key = friendly(&p.name);
        let g = m.entry(key.clone()).or_insert_with(|| Group { name: p.name.clone(), ..Default::default() });
        g.cpu += p.cpu;
        g.memory += p.memory;
        g.disk_bps += p.disk_read_bps + p.disk_write_bps;
        g.count += 1;
        g.has_window |= p.has_window;
        // Keep the name of the "main" process (the one with a window, or the biggest).
        if p.has_window && !g.name.eq_ignore_ascii_case(&p.name) {
            g.name = p.name.clone();
        }
    }
    m.into_values().collect()
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1_073_741_824.0)
}

/// Disk throughput: "45 MB/s", "1.2 GB/s".
fn disk_rate(bps: u64) -> String {
    let mb = bps as f64 / 1_048_576.0;
    if mb >= 1024.0 { format!("{:.1} GB/s", mb / 1024.0) } else { format!("{mb:.0} MB/s") }
}

fn rate(bps: u64) -> String {
    let mbit = bps as f64 * 8.0 / 1_000_000.0;
    if mbit >= 1.0 {
        format!("{mbit:.0} Mbit/s")
    } else {
        format!("{:.0} kbit/s", bps as f64 * 8.0 / 1000.0)
    }
}

fn close_action(g: &Group) -> Option<Action> {
    (!processes::is_protected(&g.name) && background_task(&g.name).is_none() && !g.name.eq_ignore_ascii_case("explorer.exe") && !g.name.eq_ignore_ascii_case("nocturn.exe")).then(|| Action::CloseApp { name: g.name.clone(), label: format!("Close {}", friendly(&g.name)) })
}

/// "Your processor is busy" → "your processor is busy", but app and drive
/// names ("C:", "Google Chrome") keep their capitals.
fn lower_first(title: &str) -> String {
    let first = title.split(' ').next().unwrap_or("");
    if ["Your", "The", "Memory", "Something", "Not"].contains(&first) {
        let mut c = title.chars();
        c.next().map(|f| f.to_lowercase().chain(c).collect()).unwrap_or_default()
    } else {
        title.to_string()
    }
}

fn avg(v: impl Iterator<Item = f32>) -> f32 {
    let (s, n) = v.fold((0.0, 0), |(s, n), x| (s + x, n + 1));
    if n == 0 { 0.0 } else { s / n as f32 }
}

/// Turn a few seconds of samples into findings. Pure, so it can be tested.
pub fn analyse(samples: &[Sample], procs: &[ProcessInfo], ctx: &Context) -> SlowReport {
    let mut f: Vec<Finding> = Vec::new();
    let last = samples.last();
    let cpu = avg(samples.iter().map(|s| s.cpu_usage));
    let max_core = samples.iter().flat_map(|s| s.cpu_per_core.iter().copied()).fold(0.0f32, f32::max);
    let mem_pct = last.map(|s| if s.mem_total > 0 { (s.mem_total - s.mem_available.min(s.mem_total)) as f32 * 100.0 / s.mem_total as f32 } else { 0.0 }).unwrap_or(0.0);
    let commit = last.and_then(|s| Some(s.commit_bytes? as f32 / s.commit_limit?.max(1) as f32));
    let paging = avg(samples.iter().filter_map(|s| s.pages_per_sec));
    // Busiest disk (average across the window, per disk).
    let mut disk_busy: HashMap<String, Vec<f32>> = HashMap::new();
    for s in samples {
        for d in &s.disk_io {
            disk_busy.entry(d.disk.clone()).or_default().push(d.active_pct);
        }
    }
    let (busy_disk, busy_pct) = disk_busy.into_iter().map(|(k, v)| (k, avg(v.into_iter()))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or_default();
    let down = samples.iter().map(|s| s.net_rx_bps).sum::<u64>() / samples.len().max(1) as u64;
    let up = samples.iter().map(|s| s.net_tx_bps).sum::<u64>() / samples.len().max(1) as u64;
    let gpu = last.and_then(|s| s.gpus.first());
    let gpu_pct = gpu.and_then(|g| g.usage);
    let gpu_temp = gpu.and_then(|g| g.temperature_c);
    let uptime_days = last.map(|s| s.uptime_secs as f32 / 86_400.0).unwrap_or(0.0);
    let gaming = ctx.running_game.clone();
    let secs = (samples.iter().map(|s| s.interval_ms as f32).sum::<f32>() / 1000.0).max(1.0);

    let mut gs = groups(procs);

    // --- processor
    gs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let top_cpu: Vec<&Group> = gs.iter().filter(|g| g.cpu >= 8.0 && !g.name.eq_ignore_ascii_case("system idle process")).take(3).collect();
    if cpu >= 60.0 {
        let severity = if gaming.is_some() { "info" } else if cpu >= 85.0 { "high" } else { "medium" };
        let who = if top_cpu.is_empty() { "many small background tasks together".to_string() } else { top_cpu.iter().map(|g| format!("{} ({:.0}%)", friendly(&g.name), g.cpu)).collect::<Vec<_>>().join(", ") };
        let mut detail = format!("The processor averaged {cpu:.0}% over {secs:.0} seconds. Using it most: {who}.");
        if let Some(g) = &gaming {
            detail.push_str(&format!(" {g} is running, so high use is expected while you play."));
        }
        for g in &top_cpu {
            if let Some(why) = background_task(&g.name) {
                detail.push_str(&format!(" {why}"));
                break;
            }
        }
        f.push(Finding { severity, title: if cpu >= 85.0 { "Your processor is maxed out".into() } else { "Your processor is busy".into() }, detail, actions: top_cpu.iter().filter_map(|g| close_action(g)).take(2).collect() });
    } else if gaming.is_none() {
        // One app keeping a whole core busy: on a many-core PC that's a small
        // share of the total, so judge it against one core's worth. Windows
        // spreads a busy thread across cores, so the per-core peak alone misses it.
        let cores = last.map(|s| s.cpu_per_core.len()).unwrap_or(0).max(1) as f32;
        let one_core = 100.0 / cores;
        let hog = gs.iter().find(|g| g.cpu >= (one_core * 0.85).max(4.0) && !g.name.eq_ignore_ascii_case("system idle process") && background_task(&g.name).is_none());
        if let Some(g) = hog.or(if max_core >= 97.0 { top_cpu.first().copied() } else { None }) {
            f.push(Finding {
                severity: "medium",
                title: format!("{} is keeping a processor core fully busy", friendly(&g.name)),
                detail: format!("Overall use looks fine ({cpu:.0}%), but {} is using about one whole core. If you aren't using it right now it may be stuck; apps that run on one core (many games, older programs) also feel slow when that core is full.", friendly(&g.name)),
                actions: close_action(g).into_iter().collect(),
            });
        }
    }
    for g in &top_cpu {
        if let Some(why) = background_task(&g.name) {
            if cpu < 60.0 && g.cpu >= 10.0 {
                f.push(Finding { severity: "info", title: format!("{} is working in the background", friendly(&g.name)), detail: why.to_string(), actions: vec![] });
            }
        }
    }

    // --- memory
    gs.sort_by_key(|g| std::cmp::Reverse(g.memory));
    let top_mem: Vec<&Group> = gs.iter().filter(|g| g.memory >= 500 * 1_048_576 && background_task(&g.name).is_none()).take(3).collect();
    let mem_hot = mem_pct >= 85.0 || commit.is_some_and(|c| c >= 0.9) || paging >= 800.0;
    if mem_hot {
        let severity = if mem_pct >= 92.0 || commit.is_some_and(|c| c >= 0.95) || paging >= 2000.0 { "high" } else { "medium" };
        let who = top_mem
            .iter()
            .map(|g| if g.count > 3 { format!("{} {} across {} processes", friendly(&g.name), gb(g.memory), g.count) } else { format!("{} {}", friendly(&g.name), gb(g.memory)) })
            .collect::<Vec<_>>()
            .join(", ");
        let mut detail = format!("{mem_pct:.0}% of your memory is in use");
        if !who.is_empty() {
            detail.push_str(&format!(" — most by {who}"));
        }
        detail.push('.');
        if paging >= 800.0 {
            detail.push_str(" Windows is moving memory to the disk to make room, which makes everything feel sluggish.");
        }
        if top_mem.iter().any(|g| g.count > 8 && ["Google Chrome", "Microsoft Edge", "Firefox", "Brave", "Opera", "Opera GX"].contains(&friendly(&g.name).as_str())) {
            detail.push_str(" Browsers use a process per tab and extension; closing tabs you don't need frees the most.");
        }
        f.push(Finding { severity, title: "Memory is nearly full".into(), detail, actions: top_mem.iter().filter_map(|g| close_action(g)).take(2).collect() });
    }

    // --- disks
    if busy_pct >= 85.0 {
        gs.sort_by_key(|g| std::cmp::Reverse(g.disk_bps));
        let who = gs.iter().find(|g| g.disk_bps >= 1_048_576);
        let drive = busy_disk.split_whitespace().last().unwrap_or("A drive").to_string();
        let hdd = ctx.system_drive.is_some_and(|(_, _, hdd)| hdd) && drive.eq_ignore_ascii_case("C:");
        let mut detail = format!("{drive} was busy {busy_pct:.0}% of the time, so programs wait for it to load or save anything.");
        if let Some(g) = who {
            detail.push_str(&format!(" Reading/writing most: {} ({}).", friendly(&g.name), disk_rate(g.disk_bps)));
            if let Some(why) = background_task(&g.name) {
                detail.push_str(&format!(" {why}"));
            }
        }
        if hdd {
            detail.push_str(" This is a hard disk; moving Windows to an SSD makes this kind of slowdown mostly disappear.");
        }
        f.push(Finding { severity: if busy_pct >= 95.0 { "high" } else { "medium" }, title: format!("{drive} is working flat out"), detail, actions: who.and_then(close_action).into_iter().collect() });
    }

    // --- system drive space
    if let Some((free, total, _)) = ctx.system_drive {
        if total > 0 && (free < 10 * 1_073_741_824 || (free as f64) < total as f64 * 0.08) {
            f.push(Finding {
                severity: if free < 4 * 1_073_741_824 { "high" } else { "medium" },
                title: "C: is nearly full".into(),
                detail: format!("Only {} free of {}. Windows needs free space for updates, temporary files and memory it moves to disk; when it runs low everything slows down.", gb(free), gb(total)),
                actions: vec![Action::OpenPage { page: "storage".into(), label: "See what's using space".into() }, Action::ApplyTweak { id: "performance.clean_temp".into(), label: "Clean temporary files".into() }],
            });
        }
    }

    // --- heat
    if let Some(t) = gpu_temp {
        if t >= 87.0 {
            f.push(Finding {
                severity: "high",
                title: format!("Your graphics card is hot ({t:.0} °C)"),
                detail: "Above about 85 °C graphics cards slow themselves down to cool off, which shows up as stutter. Check the fans are spinning, the case vents aren't blocked and dust isn't built up.".into(),
                actions: vec![Action::OpenPage { page: "hardware".into(), label: "Open Hardware".into() }],
            });
        }
    }

    // --- network
    if down >= 4_000_000 || up >= 2_000_000 {
        let names: Vec<String> = procs
            .iter()
            .filter_map(|p| match p.name.to_ascii_lowercase().as_str() {
                "steam.exe" | "steamwebhelper.exe" if p.disk_write_bps > 1_000_000 => Some("Steam".to_string()),
                "epicgameslauncher.exe" if p.disk_write_bps > 1_000_000 => Some("Epic Games".to_string()),
                "onedrive.exe" => Some("OneDrive".to_string()),
                "tiworker.exe" | "mousocoreworker.exe" => Some("Windows Update".to_string()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut detail = format!("Your connection is busy: {} down, {} up. That can make online games lag and pages load slowly.", rate(down), rate(up));
        if !names.is_empty() {
            detail.push_str(&format!(" Likely downloading or syncing: {}.", names.join(", ")));
        }
        f.push(Finding { severity: if gaming.is_some() { "medium" } else { "info" }, title: "Something is using your internet heavily".into(), detail, actions: vec![Action::OpenPage { page: "network".into(), label: "Open Network".into() }] });
    }

    // --- power plan
    if let Some(p) = &ctx.power_plan {
        if p.to_ascii_lowercase().contains("saver") {
            f.push(Finding {
                severity: "medium",
                title: "The Power saver plan is holding your PC back".into(),
                detail: format!("Windows is on \"{p}\", which lowers processor speed to save energy. Fine on battery; on a desktop or when plugged in it makes everything slower."),
                actions: vec![Action::ApplyTweak { id: "gaming.power_high_performance".into(), label: "Switch to High performance".into() }],
            });
        }
    }

    // --- uptime
    if uptime_days >= 7.0 {
        f.push(Finding {
            severity: "info",
            title: format!("Not restarted for {} days", uptime_days.floor()),
            detail: "A restart clears memory that apps have slowly piled up and finishes pending updates. \"Shut down\" with Fast Startup on doesn't count — use Restart.".into(),
            actions: vec![],
        });
    }

    let order = |s: &str| match s {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    };
    f.sort_by_key(|x| order(x.severity));
    let verdict = match f.first() {
        None => format!("Nothing is slowing your PC down right now — processor {cpu:.0}%, memory {mem_pct:.0}%, disks {busy_pct:.0}% busy."),
        Some(x) if x.severity == "info" => "Nothing major right now — a few things worth knowing below.".into(),
        Some(x) => format!("Main reason: {}.", lower_first(x.title.trim_end_matches('.'))),
    };
    SlowReport {
        summary: Summary {
            seconds: secs,
            cpu_pct: cpu,
            memory_pct: mem_pct,
            disk_busy_pct: busy_pct,
            download_bps: down,
            upload_bps: up,
            gpu_pct,
            gpu_temp_c: gpu_temp,
            uptime_days,
            game: gaming,
        },
        findings: f,
        verdict,
    }
}

/// Watch for `seconds`, then analyse.
pub fn run(st: &AppState, seconds: u64) -> SlowReport {
    let start = chrono::Utc::now().timestamp_millis();
    std::thread::sleep(Duration::from_secs(seconds.clamp(2, 15)));
    let (samples, procs) = st
        .telemetry
        .read()
        .map(|t| (t.history.iter().filter(|s| s.ts >= start).cloned().collect::<Vec<_>>(), t.processes.clone()))
        .unwrap_or_default();
    // If the monitor is paused, fall back to the latest sample.
    let samples = if samples.is_empty() { st.telemetry.read().ok().and_then(|t| t.latest.clone()).into_iter().collect() } else { samples };
    let games = st.games.games.read().map(|g| g.clone()).unwrap_or_default();
    let hidden = crate::games::not_games(&st.db);
    let playable: Vec<_> = games.into_iter().filter(|g| !hidden.contains(&g.id)).collect();
    // Only counts as "playing" when the game is actually using the PC (not a
    // launcher or tool idling in the background).
    let running_game = crate::games::session::running_game(&playable, &procs).filter(|(_, p)| p.cpu >= 2.0 || p.memory >= 400 * 1_048_576).map(|(g, _)| g.name.clone());
    let power_plan = crate::system::power::list().ok().and_then(|l| l.into_iter().find(|p| p.active).map(|p| p.name));
    let system_drive = samples.last().and_then(|s| s.disks.iter().find(|d| d.mount.to_ascii_uppercase().starts_with("C:")).map(|d| (d.available, d.total, d.kind.eq_ignore_ascii_case("hdd"))));
    let report = analyse(&samples, &procs, &Context { power_plan, running_game, system_drive });
    st.db.audit("info", "user", "slowcheck.run", Some(serde_json::json!({ "findings": report.findings.iter().map(|f| f.title.clone()).collect::<Vec<_>>() })));
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(cpu: f32, used_gb: u64, disk_busy: f32, rx: u64) -> Sample {
        Sample {
            interval_ms: 1000,
            cpu_usage: cpu,
            cpu_per_core: vec![cpu, cpu],
            mem_total: 16 * 1_073_741_824,
            mem_used: used_gb * 1_073_741_824,
            mem_available: (16 - used_gb) * 1_073_741_824,
            net_rx_bps: rx,
            disk_io: vec![crate::system::perf::DiskIo { disk: "0 C:".into(), read_bps: 0, write_bps: 0, active_pct: disk_busy }],
            process_count: 100,
            uptime_secs: 3600,
            ..Default::default()
        }
    }

    fn proc(name: &str, cpu: f32, mem_mb: u64, window: bool) -> ProcessInfo {
        ProcessInfo { pid: 1, parent: None, name: name.into(), exe: None, cpu, memory: mem_mb * 1_048_576, disk_read_bps: 0, disk_write_bps: 0, run_time_secs: 10, has_window: window }
    }

    #[test]
    fn quiet_pc_says_so() {
        let r = analyse(&vec![sample(8.0, 6, 3.0, 1000); 5], &[proc("chrome.exe", 2.0, 400, true)], &Context::default());
        assert!(r.findings.is_empty());
        assert!(r.verdict.starts_with("Nothing is slowing your PC down"));
    }

    #[test]
    fn cpu_hog_is_named_and_closable_but_windows_work_is_explained() {
        let procs = vec![proc("render.exe", 70.0, 900, true), proc("MsMpEng.exe", 15.0, 200, false)];
        let r = analyse(&vec![sample(92.0, 6, 5.0, 0); 5], &procs, &Context::default());
        let f = &r.findings[0];
        assert_eq!((f.severity, f.title.as_str()), ("high", "Your processor is maxed out"));
        assert!(f.detail.contains("render (70%)") && f.detail.contains("Windows Security antivirus (15%)"), "{}", f.detail);
        assert!(f.detail.contains("Windows Security is scanning"));
        assert_eq!(f.actions, vec![Action::CloseApp { name: "render.exe".into(), label: "Close render".into() }], "protected antivirus is never offered for closing");
        assert!(r.verdict.contains("processor is maxed out"));
    }

    #[test]
    fn one_app_holding_a_whole_core_on_a_many_core_pc() {
        let mut s = sample(9.0, 6, 3.0, 0);
        s.cpu_per_core = vec![9.0; 16];
        let procs = vec![proc("stuck.exe", 6.2, 50, false), proc("chrome.exe", 2.0, 400, true), proc("nocturn.exe", 1.0, 150, true)];
        let r = analyse(&vec![s.clone(); 5], &procs, &Context::default());
        assert_eq!(r.findings.len(), 1, "{:?}", r.findings);
        assert_eq!(r.findings[0].title, "stuck is keeping a processor core fully busy");
        assert_eq!(r.findings[0].actions, vec![Action::CloseApp { name: "stuck.exe".into(), label: "Close stuck".into() }]);
        // NOCTURN itself is reported honestly but never offered for closing.
        let r = analyse(&vec![s; 5], &[proc("nocturn.exe", 6.5, 150, true)], &Context::default());
        assert!(r.findings[0].actions.is_empty());
    }

    #[test]
    fn browser_tabs_are_grouped_for_memory() {
        let mut procs: Vec<ProcessInfo> = (0..14).map(|i| proc("chrome.exe", 0.5, 600, i == 0)).collect();
        procs.push(proc("game.exe", 1.0, 2000, true));
        let r = analyse(&vec![sample(20.0, 15, 5.0, 0); 5], &procs, &Context::default());
        let m = r.findings.iter().find(|f| f.title == "Memory is nearly full").expect("memory finding");
        assert!(m.detail.contains("Google Chrome 8.2 GB across 14 processes"), "{}", m.detail);
        assert!(m.detail.contains("closing tabs"));
        assert!(m.actions.iter().any(|a| matches!(a, Action::CloseApp { name, .. } if name == "chrome.exe")));
    }

    #[test]
    fn game_running_makes_high_use_expected() {
        let r = analyse(&vec![sample(95.0, 6, 5.0, 0); 5], &[proc("eldenring.exe", 80.0, 3000, true)], &Context { running_game: Some("ELDEN RING".into()), ..Default::default() });
        assert_eq!(r.findings[0].severity, "info");
        assert!(r.findings[0].detail.contains("ELDEN RING is running"));
    }

    #[test]
    fn disk_space_power_saver_and_downloads() {
        let ctx = Context { power_plan: Some("Power saver".into()), system_drive: Some((3 * 1_073_741_824, 240 * 1_073_741_824, false)), ..Default::default() };
        let r = analyse(&vec![sample(10.0, 6, 97.0, 12_000_000); 5], &[], &ctx);
        let titles: Vec<&str> = r.findings.iter().map(|f| f.title.as_str()).collect();
        assert!(titles.contains(&"C: is nearly full"), "{titles:?}");
        assert!(r.verdict.contains("C: is"), "drive letter keeps its capital: {}", r.verdict);
        assert!(titles.contains(&"C: is working flat out"));
        assert!(titles.contains(&"The Power saver plan is holding your PC back"));
        assert!(titles.contains(&"Something is using your internet heavily"));
        assert_eq!(r.findings[0].severity, "high", "most serious first");
        let ps = r.findings.iter().find(|f| f.title.starts_with("The Power saver")).unwrap();
        assert_eq!(ps.actions, vec![Action::ApplyTweak { id: "gaming.power_high_performance".into(), label: "Switch to High performance".into() }]);
    }
}
