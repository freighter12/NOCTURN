//! Rule-based health analysis. Every finding carries the measured evidence it
//! is based on, so the UI and the AI can distinguish *confirmed* observations
//! from guesses.

use std::collections::HashMap;

use serde::Serialize;

use super::diagnostics::{EventEntry, PendingReboot};
use super::info::SystemInfo;
use super::monitor::{ProcessInfo, Sample};
use super::power::PowerScheme;
use crate::startup::StartupItem;
use crate::tweaks::engine::{State, TweakView};
use crate::util::fmt_bytes;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FindingAction {
    Tweaks { ids: Vec<String>, label: String },
    Page { page: String, label: String },
    Ask { prompt: String, label: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub evidence: String,
    pub action: Option<FindingAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub score: u32,
    pub condition: &'static str,
    pub findings: Vec<Finding>,
    pub checked_at: String,
    /// Checks that couldn't run (e.g. event log unavailable).
    pub skipped: Vec<String>,
}

pub struct Inputs<'a> {
    pub info: &'a SystemInfo,
    pub sample: Option<&'a Sample>,
    pub processes: &'a [ProcessInfo],
    pub startup: &'a [StartupItem],
    pub tweaks: &'a [TweakView],
    pub events: Option<&'a [EventEntry]>,
    pub pending: Option<&'a PendingReboot>,
    pub power: Option<&'a [PowerScheme]>,
}

const POWER_SAVER: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";

fn f(id: &str, sev: Severity, title: impl Into<String>, detail: impl Into<String>, evidence: impl Into<String>, action: Option<FindingAction>) -> Finding {
    Finding { id: id.into(), severity: sev, title: title.into(), detail: detail.into(), evidence: evidence.into(), action }
}

fn tweak_state(tweaks: &[TweakView], id: &str) -> Option<State> {
    tweaks.iter().find(|t| t.tweak.id == id).map(|t| t.status.state)
}

pub fn analyse(i: &Inputs) -> HealthReport {
    let mut out: Vec<Finding> = Vec::new();
    let mut skipped = Vec::new();

    // ---- storage
    if let Some(s) = i.sample {
        let sys_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_ascii_uppercase();
        for d in s.disks.iter().filter(|d| !d.removable && d.total > 0) {
            let pct_free = d.available as f64 * 100.0 / d.total as f64;
            let is_sys = d.mount.to_ascii_uppercase().starts_with(&sys_drive);
            let gb_free = d.available as f64 / 1024f64.powi(3);
            let evidence = format!("{} free of {} ({pct_free:.0}%) on {}", fmt_bytes(d.available), fmt_bytes(d.total), d.mount);
            if is_sys && (pct_free < 5.0 || gb_free < 5.0) {
                out.push(f("storage.system_critical", Severity::Critical, "System drive is almost full",
                    "Windows needs free space for updates, the page file and temporary files. When the system drive is this full, apps slow down, updates fail and games can crash while loading.",
                    evidence, Some(FindingAction::Tweaks { ids: vec!["performance.clean_temp".into()], label: "Clean temporary files".into() })));
            } else if is_sys && (pct_free < 10.0 || gb_free < 15.0) {
                out.push(f("storage.system_low", Severity::Warning, "System drive is running low on space",
                    "Keeping at least 10–15% free helps Windows Update and SSD performance.",
                    evidence, Some(FindingAction::Tweaks { ids: vec!["performance.clean_temp".into()], label: "Clean temporary files".into() })));
            } else if !is_sys && pct_free < 5.0 {
                out.push(f(&format!("storage.low.{}", d.mount), Severity::Info, format!("Drive {} is nearly full", d.mount),
                    "Games installed on a nearly full drive can fail to update.", evidence, None));
            }
        }

        // ---- memory
        if s.mem_total > 0 {
            let pct = s.mem_used as f64 * 100.0 / s.mem_total as f64;
            let evidence = format!("{} of {} in use ({pct:.0}%)", fmt_bytes(s.mem_used), fmt_bytes(s.mem_total));
            if pct >= 90.0 {
                out.push(f("memory.high", Severity::Warning, "Memory is almost fully used",
                    "When RAM runs out, Windows moves data to disk, which causes stutter and slow app switching.",
                    evidence, Some(FindingAction::Ask { prompt: "Find what's using my RAM.".into(), label: "Find what's using RAM".into() })));
            } else if pct >= 80.0 {
                out.push(f("memory.elevated", Severity::Info, "Memory use is high", "Close unused apps before gaming to leave headroom.", evidence,
                    Some(FindingAction::Ask { prompt: "Find what's using my RAM.".into(), label: "Find what's using RAM".into() })));
            }
            if s.mem_total < 8 * 1024 * 1024 * 1024 {
                out.push(f("memory.small", Severity::Info, "Less than 8 GB of RAM installed",
                    "Many current games recommend 16 GB. This is a hardware limit software tweaks can't fix.",
                    format!("{} installed", fmt_bytes(s.mem_total)), None));
            }
        }

        // ---- uptime
        let days = s.uptime_secs / 86_400;
        if days >= 7 {
            out.push(f("system.uptime", Severity::Info, "PC hasn't restarted in a while",
                "Fast Startup means \"Shut down\" doesn't fully restart Windows. A real restart clears leaked memory and finishes pending updates.",
                format!("Up for {days} days"), None));
        }

        // ---- CPU right now
        if s.cpu_usage >= 85.0 {
            let mut groups: HashMap<String, f32> = HashMap::new();
            for p in i.processes {
                *groups.entry(p.name.clone()).or_default() += p.cpu;
            }
            let mut top: Vec<_> = groups.into_iter().collect();
            top.sort_by(|a, b| b.1.total_cmp(&a.1));
            let names: Vec<String> = top.iter().take(3).map(|(n, c)| format!("{n} {c:.0}%")).collect();
            out.push(f("cpu.busy", Severity::Info, "CPU is very busy right now",
                "This is a snapshot, not a trend. If it stays high while idle, something in the background needs attention.",
                format!("{:.0}% total. Top: {}", s.cpu_usage, names.join(", ")),
                Some(FindingAction::Ask { prompt: "My CPU usage is high. What's causing it?".into(), label: "Investigate".into() })));
        }
    } else {
        skipped.push("Live telemetry not available yet".into());
    }

    // ---- pending reboot
    match i.pending {
        Some(p) if p.any => out.push(f("system.pending_reboot", Severity::Warning, "Windows is waiting for a restart",
            "Updates or system components are half-installed until you restart. This can cause odd behaviour and failed installs.",
            [p.servicing.then_some("component servicing"), p.windows_update.then_some("Windows Update")].into_iter().flatten().collect::<Vec<_>>().join(", "),
            None)),
        Some(_) => {}
        None => skipped.push("Pending restart check".into()),
    }

    // ---- startup
    let enabled: Vec<&StartupItem> = i.startup.iter().filter(|s| s.enabled).collect();
    if enabled.len() >= 12 {
        out.push(f("startup.many", Severity::Warning, "Lots of apps start with Windows",
            "Each startup app adds to boot time and keeps running in the background.",
            format!("{} startup apps enabled", enabled.len()),
            Some(FindingAction::Page { page: "startup".into(), label: "Review startup apps".into() })));
    } else if enabled.len() >= 8 {
        out.push(f("startup.some", Severity::Info, "Several apps start with Windows",
            "Disabling ones you don't need right away will speed up sign-in.",
            format!("{} startup apps enabled", enabled.len()),
            Some(FindingAction::Page { page: "startup".into(), label: "Review startup apps".into() })));
    }
    let broken: Vec<&str> = enabled.iter().filter(|s| s.file_exists == Some(false)).map(|s| s.name.as_str()).collect();
    if !broken.is_empty() {
        out.push(f("startup.broken", Severity::Info, "Startup entries point to missing programs",
            "These are left over from uninstalled apps. Disabling them is harmless.",
            broken.join(", "), Some(FindingAction::Page { page: "startup".into(), label: "Review startup apps".into() })));
    }

    // ---- power plan
    if let Some(list) = i.power {
        if let Some(active) = list.iter().find(|p| p.active) {
            if active.guid == POWER_SAVER && !i.info.is_laptop {
                out.push(f("power.saver", Severity::Warning, "Power saver plan is active on a desktop",
                    "Power saver limits CPU speed. On a desktop there's no battery to save.",
                    format!("Active plan: {}", active.name),
                    Some(FindingAction::Tweaks { ids: vec!["gaming.power_high_performance".into()], label: "Use High performance".into() })));
            }
        }
    }

    // ---- gaming defaults
    if tweak_state(i.tweaks, "gaming.game_mode") == Some(State::NotApplied) {
        out.push(f("gaming.game_mode_off", Severity::Info, "Game Mode is turned off",
            "Game Mode stops Windows Update installing drivers and showing restart prompts mid-game.",
            "AutoGameModeEnabled is not 1",
            Some(FindingAction::Tweaks { ids: vec!["gaming.game_mode".into()], label: "Enable Game Mode".into() })));
    }
    if tweak_state(i.tweaks, "gaming.disable_background_recording") == Some(State::NotApplied) {
        out.push(f("gaming.background_recording", Severity::Info, "Background game recording may be on",
            "Xbox Game Bar can record in the background, using the GPU's video encoder while you play.",
            "GameDVR_Enabled / AppCaptureEnabled are not 0",
            Some(FindingAction::Tweaks { ids: vec!["gaming.disable_background_recording".into()], label: "Turn off".into() })));
    }

    // ---- hardware & drivers
    for d in &i.info.disks {
        if let Some(h) = &d.health {
            if h != "Healthy" {
                out.push(f(&format!("disk.health.{}", d.model), Severity::Critical, format!("{} reports a health problem", d.model),
                    "Back up important files now. A drive reporting Warning/Unhealthy can fail without further notice.",
                    format!("Health status: {h}"), None));
            }
        }
    }
    let os_on_hdd = i.info.disks.len() == 1 && i.info.disks[0].media_type.as_deref() == Some("HDD");
    if os_on_hdd {
        out.push(f("disk.hdd", Severity::Info, "Windows is running from a hard drive",
            "A hard drive is the most common cause of slow boot and long loading times. An SSD is the single biggest upgrade.",
            format!("{} ({})", i.info.disks[0].model, i.info.disks[0].media_type.clone().unwrap_or_default()), None));
    }
    for g in &i.info.gpus {
        if let Some(date) = g.driver_date.as_deref().and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()) {
            let age = chrono::Utc::now().signed_duration_since(date.with_timezone(&chrono::Utc)).num_days();
            let vendor = g.vendor.clone().unwrap_or_default().to_ascii_lowercase();
            let discrete = vendor.contains("nvidia") || vendor.contains("amd") || vendor.contains("advanced micro");
            if discrete && age > 365 {
                out.push(f(&format!("gpu.driver_old.{}", g.name), Severity::Info, format!("{} driver is over a year old", g.name),
                    "New GPU drivers often include fixes and optimisations for recent games. Get them from NVIDIA/AMD, not third-party updaters.",
                    format!("Driver {} from {}", g.driver_version.clone().unwrap_or_default(), date.format("%Y-%m-%d")), None));
            }
        }
    }

    // ---- event log
    match i.events {
        Some(events) => {
            let count = |pred: &dyn Fn(&EventEntry) -> bool| events.iter().filter(|e| pred(e)).count();
            let kp = count(&|e| e.provider.contains("Kernel-Power") && e.id == 41);
            if kp > 0 {
                out.push(f("events.unexpected_shutdown", Severity::Warning, "Unexpected shutdowns in the last 7 days",
                    "Windows restarted without shutting down cleanly. Common causes: power cuts, overheating, unstable overclocks, failing PSU, or holding the power button.",
                    format!("{kp} × Kernel-Power 41"), Some(FindingAction::Ask { prompt: "My PC has been shutting down or restarting unexpectedly. Why?".into(), label: "Investigate".into() })));
            }
            let whea = count(&|e| e.provider.contains("WHEA"));
            if whea > 0 {
                out.push(f("events.whea", Severity::Warning, "Hardware errors were reported",
                    "WHEA errors come from the CPU, memory or PCIe devices. They often point to unstable overclocks/XMP or failing hardware.",
                    format!("{whea} × WHEA-Logger"), Some(FindingAction::Ask { prompt: "Windows logged WHEA hardware errors. What do they mean for my PC?".into(), label: "Investigate".into() })));
            }
            let tdr = count(&|e| (e.provider.eq_ignore_ascii_case("Display") && e.id == 4101) || e.provider.contains("nvlddmkm") || e.provider.contains("amdkmdag"));
            if tdr > 0 {
                out.push(f("events.gpu_reset", Severity::Warning, "The graphics driver crashed and recovered",
                    "This causes black screens or game crashes. Typical causes: driver bugs, GPU overclocks, overheating or insufficient power.",
                    format!("{tdr} display driver event(s)"), Some(FindingAction::Ask { prompt: "My graphics driver keeps crashing and recovering. How do I fix it?".into(), label: "Investigate".into() })));
            }
            let disk = count(&|e| (e.provider.eq_ignore_ascii_case("disk") && [7, 51, 153].contains(&e.id)) || (e.provider.eq_ignore_ascii_case("Ntfs") && e.id == 55));
            if disk > 0 {
                out.push(f("events.disk", Severity::Warning, "Disk errors were logged",
                    "Bad blocks, timeouts or file system corruption were reported. Back up important data.",
                    format!("{disk} disk/NTFS error(s)"), None));
            }
            let mut crashes: HashMap<String, usize> = HashMap::new();
            for e in events.iter().filter(|e| e.provider == "Application Error" && e.id == 1000) {
                let app = e.message.split_whitespace().find(|w| w.to_ascii_lowercase().ends_with(".exe,") || w.to_ascii_lowercase().ends_with(".exe"))
                    .map(|w| w.trim_end_matches(',').to_string()).unwrap_or_else(|| "an application".into());
                *crashes.entry(app).or_default() += 1;
            }
            if !crashes.is_empty() {
                let mut v: Vec<_> = crashes.into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1));
                out.push(f("events.app_crashes", Severity::Info, "Apps crashed recently",
                    "Repeated crashes of the same app usually point to that app, its mods/overlays, or its drivers.",
                    v.iter().take(4).map(|(a, n)| format!("{a} ×{n}")).collect::<Vec<_>>().join(", "),
                    Some(FindingAction::Ask { prompt: "Some of my apps have been crashing. Can you look into it?".into(), label: "Investigate".into() })));
            }
        }
        None => skipped.push("Event log scan".into()),
    }

    out.sort_by(|a, b| b.severity.cmp(&a.severity));
    let penalty: u32 = out
        .iter()
        .map(|f| match f.severity {
            Severity::Critical => 25,
            Severity::Warning => 10,
            Severity::Info => 3,
        })
        .sum();
    let score = 100u32.saturating_sub(penalty);
    let condition = if out.iter().any(|f| f.severity == Severity::Critical) || score < 60 {
        "Needs attention"
    } else if score < 85 {
        "Fair"
    } else {
        "Good"
    };
    HealthReport { score, condition, findings: out, checked_at: crate::db::now(), skipped }
}
