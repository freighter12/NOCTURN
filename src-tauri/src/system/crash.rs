//! Crash analyser: blue screens, unexpected shutdowns, app crashes and hangs,
//! and hardware (WHEA) errors from the last 90 days, explained in plain
//! language with the evidence behind every conclusion. Read-only.
//!
//! One crash usually leaves several records (Kernel-Power 41 + WER 1001 +
//! EventLog 6008); they are merged so each incident is counted once.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::error::AppResult;
use crate::security::exec::{self, PsScript};

pub const WINDOW_DAYS: i64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Graphics,
    Driver,
    Memory,
    Storage,
    Hardware,
    Power,
    Overlay,
    App,
    Windows,
    Unknown,
}

impl Area {
    pub fn label(self) -> &'static str {
        match self {
            Area::Graphics => "Graphics driver or GPU",
            Area::Driver => "A device driver",
            Area::Memory => "Memory (RAM) or a driver corrupting it",
            Area::Storage => "Storage drive or its driver",
            Area::Hardware => "Hardware (CPU, RAM, motherboard) or overclock",
            Area::Power => "Power loss, power supply or overheating",
            Area::Overlay => "An overlay or capture tool injected into the app",
            Area::App => "The app itself",
            Area::Windows => "A Windows component",
            Area::Unknown => "Not enough information",
        }
    }
}

// ------------------------------------------------------------ knowledge

pub struct StopCode {
    pub code: u32,
    pub name: &'static str,
    pub area: Area,
    pub meaning: &'static str,
}

/// Common stop codes and what they usually point at.
pub const STOP_CODES: &[StopCode] = &[
    StopCode { code: 0x0A, name: "IRQL_NOT_LESS_OR_EQUAL", area: Area::Driver, meaning: "A driver accessed memory it wasn't allowed to. Usually a faulty or outdated driver; unstable RAM or overclocks can cause it too." },
    StopCode { code: 0x19, name: "BAD_POOL_HEADER", area: Area::Memory, meaning: "Kernel memory bookkeeping was corrupted, usually by a driver; occasionally faulty RAM." },
    StopCode { code: 0x1A, name: "MEMORY_MANAGEMENT", area: Area::Memory, meaning: "Windows found its memory structures corrupted. Often unstable RAM (including XMP/EXPO profiles), sometimes a driver." },
    StopCode { code: 0x1E, name: "KMODE_EXCEPTION_NOT_HANDLED", area: Area::Driver, meaning: "A kernel-mode program (usually a driver) hit an error it didn't handle." },
    StopCode { code: 0x24, name: "NTFS_FILE_SYSTEM", area: Area::Storage, meaning: "A problem in the NTFS file system driver, usually from disk errors or a failing drive." },
    StopCode { code: 0x3B, name: "SYSTEM_SERVICE_EXCEPTION", area: Area::Driver, meaning: "An error while running a system service call — often a graphics, antivirus or VPN driver." },
    StopCode { code: 0x50, name: "PAGE_FAULT_IN_NONPAGED_AREA", area: Area::Memory, meaning: "Invalid memory was referenced. A faulty driver, antivirus filter, or RAM." },
    StopCode { code: 0x7A, name: "KERNEL_DATA_INPAGE_ERROR", area: Area::Storage, meaning: "Windows couldn't read data back from the page file — typically a failing drive, cable or storage driver." },
    StopCode { code: 0x7E, name: "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED", area: Area::Driver, meaning: "A system thread crashed; the driver named in the dump is the usual suspect." },
    StopCode { code: 0x7F, name: "UNEXPECTED_KERNEL_MODE_TRAP", area: Area::Hardware, meaning: "The CPU raised a trap the kernel couldn't handle — hardware faults, overclocking or a bad driver." },
    StopCode { code: 0x9F, name: "DRIVER_POWER_STATE_FAILURE", area: Area::Driver, meaning: "A driver didn't respond when the PC was going to sleep, waking, or shutting down." },
    StopCode { code: 0xA0, name: "INTERNAL_POWER_ERROR", area: Area::Power, meaning: "Windows' power management hit a fatal error, often during sleep or hibernate." },
    StopCode { code: 0xC2, name: "BAD_POOL_CALLER", area: Area::Driver, meaning: "A driver made a bad memory request." },
    StopCode { code: 0xD1, name: "DRIVER_IRQL_NOT_LESS_OR_EQUAL", area: Area::Driver, meaning: "A driver accessed invalid memory — very commonly network, graphics or audio drivers." },
    StopCode { code: 0xEF, name: "CRITICAL_PROCESS_DIED", area: Area::Windows, meaning: "A process Windows can't run without stopped. Can follow disk corruption or a failed update." },
    StopCode { code: 0xF4, name: "CRITICAL_OBJECT_TERMINATION", area: Area::Storage, meaning: "A critical process ended, frequently because the system drive stopped responding." },
    StopCode { code: 0x101, name: "CLOCK_WATCHDOG_TIMEOUT", area: Area::Hardware, meaning: "A CPU core stopped responding. Typical of CPU overclock/undervolt instability or BIOS issues." },
    StopCode { code: 0x113, name: "VIDEO_DXGKRNL_FATAL_ERROR", area: Area::Graphics, meaning: "The DirectX graphics kernel hit a fatal error — graphics driver or GPU." },
    StopCode { code: 0x116, name: "VIDEO_TDR_FAILURE", area: Area::Graphics, meaning: "The graphics card stopped responding and the driver couldn't recover it. Driver issues, GPU overclock, overheating, or power delivery." },
    StopCode { code: 0x117, name: "VIDEO_TDR_TIMEOUT_DETECTED", area: Area::Graphics, meaning: "The graphics driver stopped responding and was restarted (the screen goes black for a moment)." },
    StopCode { code: 0x119, name: "VIDEO_SCHEDULER_INTERNAL_ERROR", area: Area::Graphics, meaning: "The GPU scheduler found a fatal error — usually the graphics driver." },
    StopCode { code: 0x124, name: "WHEA_UNCORRECTABLE_ERROR", area: Area::Hardware, meaning: "The hardware itself reported a fatal error. Usually CPU/RAM overclocks or undervolts, overheating, or a failing component." },
    StopCode { code: 0x133, name: "DPC_WATCHDOG_VIOLATION", area: Area::Driver, meaning: "A driver took too long. Common with storage (SSD firmware/SATA drivers) and some network drivers." },
    StopCode { code: 0x139, name: "KERNEL_SECURITY_CHECK_FAILURE", area: Area::Driver, meaning: "A driver corrupted a critical data structure. Often an incompatible driver; sometimes RAM." },
    StopCode { code: 0x13A, name: "KERNEL_MODE_HEAP_CORRUPTION", area: Area::Driver, meaning: "A driver corrupted kernel memory — graphics drivers are a frequent cause." },
    StopCode { code: 0x154, name: "UNEXPECTED_STORE_EXCEPTION", area: Area::Storage, meaning: "The memory compression store hit an error — often storage drivers or a failing drive, sometimes RAM." },
    StopCode { code: 0x1CA, name: "SYNTHETIC_WATCHDOG_TIMEOUT", area: Area::Hardware, meaning: "The system stopped responding for too long; often a hardware hang." },
    StopCode { code: 0xC000021A, name: "STATUS_SYSTEM_PROCESS_TERMINATED", area: Area::Windows, meaning: "A core user-mode process failed, often after a bad update or corrupted system files." },
];

pub fn stop_code(code: u32) -> Option<&'static StopCode> {
    STOP_CODES.iter().find(|s| s.code == code)
}

/// Common application exception codes.
pub fn exception_name(code: u32) -> Option<(&'static str, &'static str)> {
    Some(match code {
        0xC000_0005 => ("Access violation", "The app read or wrote memory it doesn't own — a bug in the app, a mod/plugin, or something injected into it."),
        0xC000_0409 => ("Fail-fast / buffer overrun", "The app deliberately stopped itself after detecting a serious internal problem."),
        0xC000_0374 => ("Heap corruption", "The app's memory was corrupted, by its own code or a loaded add-on. Unstable RAM can also cause it."),
        0xC000_00FD => ("Stack overflow", "The app ran out of stack space, usually endless recursion in its code."),
        0xC000_001D => ("Illegal instruction", "The CPU was asked to run an instruction it doesn't support or the code was corrupted. Can also mean CPU instability."),
        0xC000_0135 => ("Missing DLL", "A file the app needs wasn't found. Reinstalling the app or its runtime usually fixes it."),
        0xC000_0142 => ("DLL failed to start", "A library failed to initialise, often from a missing runtime or a conflicting add-on."),
        0xC000_0094 => ("Division by zero", "A bug in the app."),
        0xC000_0006 => ("In-page error", "The app's code couldn't be read from disk — drive or network-share problems."),
        0xE043_4352 => (".NET exception", "An unhandled error in a .NET app; the .NET Runtime event has the details."),
        0x8000_0003 => ("Breakpoint", "The app stopped itself on purpose (an assertion or crash handler). Game engines do this when they hit an internal error."),
        0xC000_0602 => ("Unknown error (fail-fast)", "The app raised a fail-fast error to stop safely."),
        0x4000_0015 => ("Abort", "The app called abort() after an internal error."),
        _ => return None,
    })
}

const GPU_MODULES: &[&str] = &[
    "nvwgf2umx.dll", "nvwgf2um.dll", "nvd3dumx.dll", "nvd3dum.dll", "nvoglv64.dll", "nvoglv32.dll", "nvgpucomp64.dll", "nvapi64.dll",
    "nvcuda64.dll", "nvlddmkm.sys", "atidxx64.dll", "atiumd64.dll", "atiuxp64.dll", "amdxc64.dll", "amdvlk64.dll", "atio6axx.dll",
    "amdxx64.dll", "igd10iumd64.dll", "igd12umd64.dll", "igxelpicd64.dll", "igc64.dll", "igdusc64.dll", "d3d12core.dll", "dxgi.dll",
    "d3d11.dll", "vulkan-1.dll",
];
const OVERLAY_MODULES: &[&str] = &[
    "gameoverlayrenderer64.dll", "gameoverlayrenderer.dll", "discordhook64.dll", "discordhook.dll", "rtsshooks64.dll", "rtsshooks.dll",
    "nvspcap64.dll", "graphics-hook64.dll", "graphics-hook32.dll", "overlay64.dll", "mangohud.dll", "reshade64.dll", "dxgi_reshade.dll",
    "gameoverlay64.dll", "medalhook64.dll",
];
const GENERIC_MODULES: &[&str] = &[
    "ntdll.dll", "kernelbase.dll", "kernel32.dll", "ucrtbase.dll", "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "combase.dll",
    "msvcrt.dll", "user32.dll", "win32u.dll", "clr.dll", "coreclr.dll", "unknown", "",
];

/// Who most likely caused an app crash, from the faulting module.
pub fn module_area(app: &str, app_path: &str, module: &str, module_path: &str) -> (Area, String) {
    let m = module.to_ascii_lowercase();
    if GPU_MODULES.contains(&m.as_str()) {
        return (Area::Graphics, format!("It failed inside {module}, part of the graphics driver or DirectX."));
    }
    if OVERLAY_MODULES.contains(&m.as_str()) {
        return (Area::Overlay, format!("It failed inside {module}, an overlay/capture hook loaded into the app."));
    }
    if GENERIC_MODULES.contains(&m.as_str()) {
        return (Area::App, format!("It failed in {}, a general Windows library. That's where the error surfaced, not usually its cause — the app or an add-on it loaded is the likely source.", if module.is_empty() { "an unknown module" } else { module }));
    }
    let app_dir = std::path::Path::new(app_path).parent().map(|p| p.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mp = module_path.to_ascii_lowercase();
    if m == app.to_ascii_lowercase() || (!app_dir.is_empty() && mp.starts_with(&app_dir)) {
        return (Area::App, format!("It failed in {module}, which ships with the app itself."));
    }
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    if mp.starts_with(&windir) {
        return (Area::Windows, format!("It failed in {module}, a Windows component."));
    }
    (Area::Unknown, format!("It failed in {module}, loaded from outside the app's folder (a plug-in, add-on or another program's hook)."))
}

// ------------------------------------------------------------------ model

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemKind {
    BlueScreen,
    UnexpectedShutdown,
    HardwareError,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemIncident {
    pub time: DateTime<Utc>,
    pub kind: SystemKind,
    /// Hex stop code, e.g. `0x00000116`.
    pub code: Option<String>,
    pub code_name: Option<String>,
    pub area: Area,
    pub area_label: String,
    pub explanation: String,
    pub dump: Option<String>,
    /// Which event-log records this incident was built from.
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Counted {
    pub name: String,
    pub count: usize,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppGroup {
    pub app: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub crashes: usize,
    pub hangs: usize,
    pub last: DateTime<Utc>,
    pub modules: Vec<Counted>,
    pub codes: Vec<Counted>,
    pub area: Area,
    pub area_label: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// good, info, warning, critical
    pub severity: &'static str,
    pub title: String,
    pub detail: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Day {
    pub day: String,
    pub blue_screens: usize,
    pub shutdowns: usize,
    pub app_crashes: usize,
    pub hangs: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DumpFile {
    pub path: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dumps {
    /// `None` when the folder couldn't be read (needs administrator rights).
    pub minidumps: Option<Vec<DumpFile>>,
    pub memory_dmp: Option<DumpFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashReport {
    pub window_days: i64,
    pub generated_at: DateTime<Utc>,
    pub blue_screens: usize,
    pub unexpected_shutdowns: usize,
    pub hardware_errors: usize,
    pub app_crashes: usize,
    pub app_hangs: usize,
    pub clean_boots: usize,
    pub last_system_crash: Option<DateTime<Utc>>,
    pub findings: Vec<Finding>,
    pub incidents: Vec<SystemIncident>,
    pub apps: Vec<AppGroup>,
    pub daily: Vec<Day>,
    /// Daily stability index (1–10) from Reliability Monitor, oldest first.
    pub stability: Vec<(String, f64)>,
    pub dumps: Dumps,
}

// ---------------------------------------------------------------- parsing

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim()
}

fn time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s(v, "time")).ok().map(|t| t.with_timezone(&Utc))
}

/// `"0x0000001a (0x..., ...)"`, `"26"`, `"0x1a"` → 0x1a.
pub fn parse_code(text: &str) -> Option<u32> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(i) = t.to_ascii_lowercase().find("0x") {
        let hex: String = t[i + 2..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        return u64::from_str_radix(&hex, 16).ok().map(|v| v as u32);
    }
    let dec: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    dec.parse::<u64>().ok().map(|v| v as u32)
}

fn hex(code: u32) -> String {
    format!("0x{code:08X}")
}

fn bluescreen(time: DateTime<Utc>, code: Option<u32>, dump: Option<String>, evidence: Vec<String>) -> SystemIncident {
    let known = code.and_then(stop_code);
    let area = known.map(|k| k.area).unwrap_or(Area::Unknown);
    SystemIncident {
        time,
        kind: SystemKind::BlueScreen,
        code: code.map(hex),
        code_name: known.map(|k| k.name.to_string()),
        area,
        area_label: area.label().into(),
        explanation: known
            .map(|k| k.meaning.to_string())
            .unwrap_or_else(|| "Windows stopped with an error. The stop code isn't on NOCTURN's list; search Microsoft's bug check reference for it.".into()),
        dump,
        evidence,
    }
}

/// Merge the raw records into incidents and app groups.
pub fn analyse(events: &[Value], stability_raw: &[Value], dumps: Dumps, now: DateTime<Utc>) -> CrashReport {
    let mut incidents: Vec<SystemIncident> = Vec::new();
    let mut whea_corrected = 0usize;
    let near = |a: DateTime<Utc>, b: DateTime<Utc>, mins: i64| (a - b).num_minutes().abs() <= mins;

    // 1. WER bugcheck records are the most precise (code + dump path).
    for e in events.iter().filter(|e| s(e, "kind") == "bugcheck") {
        let Some(t) = time(e) else { continue };
        let dump = Some(s(e, "dump").to_string()).filter(|d| !d.is_empty());
        incidents.push(bluescreen(t, parse_code(s(e, "text")), dump, vec!["Windows Error Reporting 1001 (bug check)".into()]));
    }
    // 2. Kernel-Power 41: a bugcheck without a WER record, or a shutdown.
    for e in events.iter().filter(|e| s(e, "kind") == "unexpected_shutdown") {
        let Some(t) = time(e) else { continue };
        let code = parse_code(s(e, "code")).filter(|c| *c != 0);
        if let Some(c) = code {
            if let Some(i) = incidents.iter_mut().find(|i| i.kind == SystemKind::BlueScreen && near(i.time, t, 30)) {
                i.evidence.push("Kernel-Power 41".into());
                if i.code.is_none() {
                    *i = bluescreen(i.time, Some(c), i.dump.clone(), i.evidence.clone());
                }
                continue;
            }
            incidents.push(bluescreen(t, Some(c), None, vec!["Kernel-Power 41 (stop code recorded, no crash report)".into()]));
            continue;
        }
        let forced = s(e, "power_button") != "0" && !s(e, "power_button").is_empty() || s(e, "long_press").eq_ignore_ascii_case("true");
        let sleeping = s(e, "sleep") == "1";
        let explanation = if forced {
            "The power button was held down to force the PC off — usually because it had frozen.".to_string()
        } else if sleeping {
            "Power was lost while the PC was going to sleep or waking up. Often a driver that doesn't handle sleep properly.".to_string()
        } else {
            "Windows didn't shut down cleanly and recorded no error: power was cut, the power supply tripped, the PC overheated, or it froze so hard nothing could be logged.".to_string()
        };
        incidents.push(SystemIncident {
            time: t,
            kind: SystemKind::UnexpectedShutdown,
            code: None,
            code_name: None,
            area: Area::Power,
            area_label: if forced { "Forced power-off after a freeze".into() } else { Area::Power.label().into() },
            explanation,
            dump: None,
            evidence: vec!["Kernel-Power 41".into()],
        });
    }
    // 3. EventLog 6008 only when nothing else recorded that shutdown.
    for e in events.iter().filter(|e| s(e, "kind") == "eventlog_6008") {
        let Some(t) = time(e) else { continue };
        if incidents.iter().any(|i| near(i.time, t, 20)) {
            if let Some(i) = incidents.iter_mut().find(|i| near(i.time, t, 20)) {
                i.evidence.push("EventLog 6008".into());
            }
            continue;
        }
        incidents.push(SystemIncident {
            time: t,
            kind: SystemKind::UnexpectedShutdown,
            code: None,
            code_name: None,
            area: Area::Power,
            area_label: Area::Power.label().into(),
            explanation: "Windows noted at the next start that the previous shutdown was unexpected.".into(),
            dump: None,
            evidence: vec!["EventLog 6008".into()],
        });
    }
    // 4. WHEA: fatal errors are incidents; corrected ones only counted.
    for e in events.iter().filter(|e| s(e, "kind") == "whea") {
        let Some(t) = time(e) else { continue };
        let level = e.get("level").and_then(|v| v.as_i64()).unwrap_or(3);
        if level > 2 {
            whea_corrected += 1;
            continue;
        }
        incidents.push(SystemIncident {
            time: t,
            kind: SystemKind::HardwareError,
            code: e.get("id").and_then(|v| v.as_i64()).map(|i| format!("WHEA {i}")),
            code_name: None,
            area: Area::Hardware,
            area_label: Area::Hardware.label().into(),
            explanation: {
                let txt = s(e, "text");
                if txt.is_empty() { "The hardware reported an uncorrectable error.".into() } else { txt.split(['\r', '\n']).next().unwrap_or(txt).to_string() }
            },
            dump: None,
            evidence: vec!["WHEA-Logger".into()],
        });
    }
    incidents.sort_by(|a, b| b.time.cmp(&a.time));

    // ---- apps
    let mut groups: HashMap<String, (AppGroup, HashMap<String, usize>, HashMap<u32, usize>, Vec<(Area, String)>)> = HashMap::new();
    for e in events.iter().filter(|e| matches!(s(e, "kind"), "app_crash" | "app_hang")) {
        let Some(t) = time(e) else { continue };
        let app = s(e, "app");
        if app.is_empty() {
            continue;
        }
        let key = app.to_ascii_lowercase();
        let entry = groups.entry(key).or_insert_with(|| {
            (
                AppGroup {
                    app: app.to_string(),
                    path: Some(s(e, "app_path").to_string()).filter(|p| !p.is_empty()),
                    version: Some(s(e, "app_version").to_string()).filter(|p| !p.is_empty()),
                    crashes: 0,
                    hangs: 0,
                    last: t,
                    modules: vec![],
                    codes: vec![],
                    area: Area::Unknown,
                    area_label: String::new(),
                    explanation: String::new(),
                },
                HashMap::new(),
                HashMap::new(),
                vec![],
            )
        });
        if t > entry.0.last {
            entry.0.last = t;
        }
        if s(e, "kind") == "app_hang" {
            entry.0.hangs += 1;
            continue;
        }
        entry.0.crashes += 1;
        let module = s(e, "module");
        *entry.1.entry(module.to_string()).or_default() += 1;
        if let Some(c) = parse_code(&format!("0x{}", s(e, "code").trim_start_matches("0x"))) {
            *entry.2.entry(c).or_default() += 1;
        }
        entry.3.push(module_area(app, s(e, "app_path"), module, s(e, "module_path")));
    }
    let mut apps: Vec<AppGroup> = groups
        .into_values()
        .map(|(mut g, modules, codes, areas)| {
            let mut m: Vec<Counted> = modules.into_iter().map(|(name, count)| Counted { name: if name.is_empty() { "unknown".into() } else { name }, count, detail: None }).collect();
            m.sort_by(|a, b| b.count.cmp(&a.count));
            let mut c: Vec<Counted> = codes
                .into_iter()
                .map(|(code, count)| {
                    let n = exception_name(code);
                    Counted { name: format!("{} {}", hex(code), n.map(|x| x.0).unwrap_or("")).trim().to_string(), count, detail: n.map(|x| x.1.to_string()) }
                })
                .collect();
            c.sort_by(|a, b| b.count.cmp(&a.count));
            // The most common attribution wins.
            let mut tally: BTreeMap<Area, (usize, String)> = BTreeMap::new();
            for (a, why) in areas {
                let t = tally.entry(a).or_insert((0, why));
                t.0 += 1;
            }
            if let Some((a, (_, why))) = tally.into_iter().max_by_key(|(_, (n, _))| *n) {
                g.area = a;
                g.explanation = why;
            } else if g.hangs > 0 {
                g.area = Area::App;
                g.explanation = "It stopped responding and was closed. Hangs come from the app waiting on something — a slow disk, network, or a bug.".into();
            }
            g.area_label = g.area.label().into();
            g.modules = m;
            g.codes = c;
            g
        })
        .collect();
    apps.sort_by(|a, b| (b.crashes + b.hangs).cmp(&(a.crashes + a.hangs)).then(b.last.cmp(&a.last)));

    // ---- daily
    let start = now - Duration::days(WINDOW_DAYS - 1);
    let mut daily: BTreeMap<String, Day> = BTreeMap::new();
    for d in 0..WINDOW_DAYS {
        let day = (start + Duration::days(d)).format("%Y-%m-%d").to_string();
        daily.insert(day.clone(), Day { day, ..Default::default() });
    }
    let day_of = |t: DateTime<Utc>| t.format("%Y-%m-%d").to_string();
    for i in &incidents {
        if let Some(d) = daily.get_mut(&day_of(i.time)) {
            match i.kind {
                SystemKind::BlueScreen | SystemKind::HardwareError => d.blue_screens += 1,
                SystemKind::UnexpectedShutdown => d.shutdowns += 1,
            }
        }
    }
    for e in events.iter().filter(|e| matches!(s(e, "kind"), "app_crash" | "app_hang") && !s(e, "app").is_empty()) {
        if let (Some(t), hang) = (time(e), s(e, "kind") == "app_hang") {
            if let Some(d) = daily.get_mut(&day_of(t)) {
                if hang { d.hangs += 1 } else { d.app_crashes += 1 }
            }
        }
    }

    // ---- stability: last reading per day
    let mut stab: BTreeMap<String, (DateTime<Utc>, f64)> = BTreeMap::new();
    for v in stability_raw {
        let (Some(t), Some(ix)) = (time(v), v.get("index").and_then(|x| x.as_f64())) else { continue };
        let e = stab.entry(day_of(t)).or_insert((t, ix));
        if t >= e.0 {
            *e = (t, ix);
        }
    }

    let blue_screens = incidents.iter().filter(|i| i.kind == SystemKind::BlueScreen).count();
    let unexpected_shutdowns = incidents.iter().filter(|i| i.kind == SystemKind::UnexpectedShutdown).count();
    let hardware_errors = incidents.iter().filter(|i| i.kind == SystemKind::HardwareError).count();
    let app_crashes = apps.iter().map(|a| a.crashes).sum();
    let app_hangs = apps.iter().map(|a| a.hangs).sum();
    let clean_boots = events.iter().filter(|e| s(e, "kind") == "eventlog_6005").count();
    let findings = findings(&incidents, &apps, whea_corrected);

    CrashReport {
        window_days: WINDOW_DAYS,
        generated_at: now,
        blue_screens,
        unexpected_shutdowns,
        hardware_errors,
        app_crashes,
        app_hangs,
        clean_boots,
        last_system_crash: incidents.first().map(|i| i.time),
        findings,
        incidents,
        apps,
        daily: daily.into_values().collect(),
        stability: stab.into_iter().map(|(d, (_, ix))| (d, (ix * 100.0).round() / 100.0)).collect(),
        dumps,
    }
}

fn findings(incidents: &[SystemIncident], apps: &[AppGroup], whea_corrected: usize) -> Vec<Finding> {
    let mut out = Vec::new();
    let bsod: Vec<&SystemIncident> = incidents.iter().filter(|i| i.kind == SystemKind::BlueScreen).collect();
    let shutdowns = incidents.iter().filter(|i| i.kind == SystemKind::UnexpectedShutdown && i.area_label == Area::Power.label()).count();
    let forced = incidents.iter().filter(|i| i.kind == SystemKind::UnexpectedShutdown && i.area_label != Area::Power.label()).count();
    let hw = incidents.iter().filter(|i| i.kind == SystemKind::HardwareError).count();

    if bsod.is_empty() && shutdowns == 0 && forced == 0 && hw == 0 {
        out.push(Finding {
            severity: "good",
            title: format!("No blue screens or unexpected shutdowns in {WINDOW_DAYS} days"),
            detail: "Windows itself has been stable. Any problems below are limited to individual apps.".into(),
            steps: vec![],
        });
    }

    // Repeated stop codes.
    let mut by_code: BTreeMap<String, Vec<&SystemIncident>> = BTreeMap::new();
    for i in &bsod {
        by_code.entry(i.code.clone().unwrap_or_else(|| "unknown".into())).or_default().push(i);
    }
    for (code, list) in &by_code {
        let first = list[0];
        let name = first.code_name.clone().unwrap_or_else(|| code.clone());
        let steps = match first.area {
            Area::Graphics => vec![
                "Install the latest graphics driver from NVIDIA, AMD or Intel; if it started after an update, try the previous version.".into(),
                "Remove any GPU overclock or undervolt (MSI Afterburner, Adrenalin tuning) and test again.".into(),
                "Check GPU temperatures under load on the Hardware page.".into(),
            ],
            Area::Memory => vec![
                "If XMP/EXPO is enabled in the BIOS, test with it off — it's an overclock even when it's the advertised speed.".into(),
                "Run Windows Memory Diagnostic (search for it in Start) or MemTest86.".into(),
                "Update chipset, storage and graphics drivers; a driver can corrupt memory too.".into(),
            ],
            Area::Storage => vec![
                "Check the drive's health on the Hardware page and update the SSD's firmware from the maker's tool.".into(),
                "Reseat or replace SATA cables; update chipset/storage drivers.".into(),
                "Run 'chkdsk C: /scan' from an administrator terminal.".into(),
            ],
            Area::Hardware => vec![
                "Remove CPU/RAM overclocks and undervolts (including XMP/EXPO and PBO curve settings) and test at stock.".into(),
                "Update the BIOS — CPU stability fixes often ship there.".into(),
                "Check temperatures under load and make sure the power supply is adequate.".into(),
            ],
            Area::Power => vec!["Update chipset and device drivers; disable fast startup if crashes happen around sleep or shutdown.".into()],
            _ => vec![
                "Update the drivers for your graphics card, network adapter, audio and chipset from the makers' sites.".into(),
                "If it started after installing new software (antivirus, VPN, RGB/fan tools), try uninstalling it.".into(),
            ],
        };
        out.push(Finding {
            severity: if list.len() >= 2 { "critical" } else { "warning" },
            title: if list.len() >= 2 { format!("{name} happened {} times", list.len()) } else { format!("A blue screen: {name}") },
            detail: format!("{} Likely area: {}.", first.explanation, first.area_label.to_lowercase()),
            steps,
        });
    }

    if hw > 0 || whea_corrected >= 10 {
        out.push(Finding {
            severity: if hw > 0 { "critical" } else { "warning" },
            title: if hw > 0 { format!("{hw} fatal hardware error{} reported", if hw == 1 { "" } else { "s" }) } else { format!("{whea_corrected} corrected hardware errors") },
            detail: "Windows Hardware Error Architecture (WHEA) records errors reported by the CPU, memory controller or PCIe devices. Corrected errors were handled, but many of them point at an unstable component.".into(),
            steps: vec![
                "Return CPU, RAM and GPU to stock settings (no overclock, undervolt, XMP/EXPO) and see if the errors stop.".into(),
                "Update the BIOS and chipset drivers.".into(),
                "Check temperatures under load on the Hardware page.".into(),
            ],
        });
    }

    if shutdowns >= 2 {
        out.push(Finding {
            severity: "warning",
            title: format!("{shutdowns} unexpected power-offs with no error recorded"),
            detail: "When a PC turns off without a blue screen, the power was cut or the hardware stopped before Windows could log anything.".into(),
            steps: vec![
                "Watch CPU/GPU temperatures while gaming (Hardware page). Overheating PCs switch off to protect themselves.".into(),
                "Rule out the wall socket, power strip or UPS; a weak or failing power supply often trips under gaming load.".into(),
                "Remove overclocks and test at stock settings.".into(),
            ],
        });
    } else if shutdowns == 1 {
        out.push(Finding {
            severity: "info",
            title: "One unexpected power-off".into(),
            detail: "A single one is usually a power cut or the PC being switched off at the wall. Keep an eye out if it repeats.".into(),
            steps: vec![],
        });
    }
    if forced > 0 {
        out.push(Finding {
            severity: if forced >= 2 { "warning" } else { "info" },
            title: format!("Forced off {forced} time{} after freezing", if forced == 1 { "" } else { "s" }),
            detail: "The power button was held down. Freezes without a blue screen often come from graphics drivers, overheating, or unstable overclocks.".into(),
            steps: vec!["Update the graphics driver and check temperatures; note what was running when it froze.".into()],
        });
    }

    // Apps.
    let gpu_apps: Vec<&AppGroup> = apps.iter().filter(|a| a.area == Area::Graphics && a.crashes > 0).collect();
    if gpu_apps.iter().map(|a| a.crashes).sum::<usize>() >= 2 {
        out.push(Finding {
            severity: "warning",
            title: format!("Crashes inside the graphics driver ({})", gpu_apps.iter().map(|a| a.app.as_str()).collect::<Vec<_>>().join(", ")),
            detail: "Several app crashes happened in graphics-driver code. When more than one app is affected, the driver (or a GPU overclock) is the common factor.".into(),
            steps: vec![
                "Do a clean install of the latest graphics driver.".into(),
                "Remove any GPU overclock or undervolt and test again.".into(),
            ],
        });
    }
    for a in apps.iter().filter(|a| a.area == Area::Overlay && a.crashes > 0) {
        out.push(Finding {
            severity: "warning",
            title: format!("{} crashed inside an overlay", a.app),
            detail: a.explanation.clone(),
            steps: vec!["Turn off that overlay for this app (Steam, Discord, RivaTuner, NVIDIA or OBS game capture) and test again.".into()],
        });
    }
    for a in apps.iter().filter(|a| a.crashes + a.hangs >= 3 && !matches!(a.area, Area::Graphics | Area::Overlay)) {
        let code = a.codes.first();
        let mut steps = vec!["Update the app (or verify the game's files in Steam/Epic/GOG).".to_string()];
        if code.is_some_and(|c| c.name.contains("Access violation") || c.name.contains("Heap")) {
            steps.push("Remove mods, plug-ins or add-ons, then test again.".into());
        }
        if code.is_some_and(|c| c.name.contains("Missing DLL") || c.name.contains("DLL failed")) {
            steps.push("Reinstall the app and the Microsoft Visual C++ / .NET runtimes it needs.".into());
        }
        out.push(Finding {
            severity: "info",
            title: match (a.crashes, a.hangs) {
                (c, 0) => format!("{} crashed {}", a.app, plural(c, "time")),
                (0, h) => format!("{} stopped responding {}", a.app, plural(h, "time")),
                (c, h) => format!("{} crashed {} and stopped responding {}", a.app, plural(c, "time"), plural(h, "time")),
            },
            detail: format!("{}{}", a.explanation, code.and_then(|c| c.detail.as_ref()).map(|d| format!(" Most common error: {d}")).unwrap_or_default()),
            steps,
        });
    }
    out
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

// --------------------------------------------------------------- collection

fn dump_file(p: &std::path::Path) -> Option<DumpFile> {
    let m = std::fs::metadata(p).ok()?;
    Some(DumpFile { path: p.display().to_string(), size: m.len(), modified: m.modified().ok().map(DateTime::<Utc>::from) })
}

fn read_dumps() -> Dumps {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let dir = std::path::Path::new(&windir).join("Minidump");
    let minidumps = match std::fs::read_dir(&dir) {
        Ok(rd) => {
            let mut v: Vec<DumpFile> = rd.flatten().filter_map(|e| dump_file(&e.path())).collect();
            v.sort_by(|a, b| b.modified.cmp(&a.modified));
            Some(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(vec![]),
        Err(_) => None,
    };
    Dumps { minidumps, memory_dmp: dump_file(&std::path::Path::new(&windir).join("MEMORY.DMP")) }
}

pub fn report() -> AppResult<CrashReport> {
    let v = exec::powershell_json(PsScript::CrashEvents)?;
    let events = v.get("events").cloned().map(exec::as_array).unwrap_or_default();
    let stability = v.get("stability").cloned().map(exec::as_array).unwrap_or_default();
    Ok(analyse(&events, &stability, read_dumps(), Utc::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn no_dumps() -> Dumps {
        Dumps { minidumps: Some(vec![]), memory_dmp: None }
    }
    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-06T12:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn codes_parse() {
        assert_eq!(parse_code("0x00000116 (0xffff, 0x1)"), Some(0x116));
        assert_eq!(parse_code("26"), Some(0x1A));
        assert_eq!(parse_code("0xc000021a"), Some(0xC000_021A));
        assert_eq!(parse_code(""), None);
        assert_eq!(stop_code(0x124).unwrap().area, Area::Hardware);
        assert!(exception_name(0xC000_0005).is_some());
    }

    #[test]
    fn one_crash_three_records_counts_once() {
        let ev = vec![
            json!({"kind":"bugcheck","time":"2026-11-01T10:00:30Z","text":"0x00000116 (0x1, 0x2)","dump":"C:\\Windows\\Minidump\\110126-1.dmp"}),
            json!({"kind":"unexpected_shutdown","time":"2026-11-01T10:01:00Z","code":"278","sleep":"0","power_button":"0"}),
            json!({"kind":"eventlog_6008","time":"2026-11-01T10:01:05Z"}),
            json!({"kind":"unexpected_shutdown","time":"2026-11-03T22:00:00Z","code":"0","sleep":"0","power_button":"0"}),
        ];
        let r = analyse(&ev, &[], no_dumps(), now());
        assert_eq!(r.blue_screens, 1);
        assert_eq!(r.unexpected_shutdowns, 1);
        let b = r.incidents.iter().find(|i| i.kind == SystemKind::BlueScreen).unwrap();
        assert_eq!(b.code_name.as_deref(), Some("VIDEO_TDR_FAILURE"));
        assert_eq!(b.area, Area::Graphics);
        assert_eq!(b.evidence.len(), 3);
        assert!(r.findings.iter().any(|f| f.title.contains("VIDEO_TDR_FAILURE")));
        assert_eq!(r.daily.len(), WINDOW_DAYS as usize);
        assert_eq!(r.daily.iter().map(|d| d.blue_screens + d.shutdowns).sum::<usize>(), 2);
    }

    #[test]
    fn forced_power_off_is_recognised() {
        let ev = vec![json!({"kind":"unexpected_shutdown","time":"2026-11-03T22:00:00Z","code":"0","sleep":"0","power_button":"133","long_press":"true"})];
        let r = analyse(&ev, &[], no_dumps(), now());
        assert!(r.incidents[0].explanation.contains("power button"));
        assert!(r.findings.iter().any(|f| f.title.starts_with("Forced off")));
    }

    #[test]
    fn app_crashes_group_and_attribute() {
        let crash = |t: &str, module: &str, code: &str| {
            json!({"kind":"app_crash","time":t,"app":"Game.exe","app_version":"1.0","module":module,"code":code,
                   "app_path":"D:\\Games\\G\\Game.exe","module_path":format!("C:\\Windows\\System32\\DriverStore\\{module}")})
        };
        let ev = vec![
            crash("2026-11-01T10:00:00Z", "nvwgf2umx.dll", "c0000005"),
            crash("2026-11-02T10:00:00Z", "nvwgf2umx.dll", "c0000005"),
            crash("2026-11-03T10:00:00Z", "ntdll.dll", "c0000374"),
            json!({"kind":"app_hang","time":"2026-11-04T10:00:00Z","app":"Game.exe","app_version":"1.0"}),
        ];
        let r = analyse(&ev, &[], no_dumps(), now());
        assert_eq!(r.apps.len(), 1);
        let g = &r.apps[0];
        assert_eq!((g.crashes, g.hangs), (3, 1));
        assert_eq!(g.area, Area::Graphics);
        assert_eq!(g.modules[0].name, "nvwgf2umx.dll");
        assert!(g.codes[0].name.contains("Access violation"));
        assert!(r.findings.iter().any(|f| f.title.contains("graphics driver")));
        assert!(r.findings.iter().any(|f| f.severity == "good"), "no system crashes");
    }

    #[test]
    fn module_attribution() {
        assert_eq!(module_area("a.exe", r"D:\G\a.exe", "GameOverlayRenderer64.dll", r"C:\Steam\GameOverlayRenderer64.dll").0, Area::Overlay);
        assert_eq!(module_area("a.exe", r"D:\G\a.exe", "UnityPlayer.dll", r"D:\G\UnityPlayer.dll").0, Area::App);
        assert_eq!(module_area("a.exe", r"D:\G\a.exe", "KERNELBASE.dll", r"C:\Windows\System32\KERNELBASE.dll").0, Area::App);
        assert_eq!(module_area("a.exe", r"D:\G\a.exe", "weird.dll", r"C:\Tools\weird.dll").0, Area::Unknown);
    }

    #[test]
    fn stability_is_daily_last_value() {
        let st = vec![json!({"time":"2026-11-05T10:00:00Z","index":7.5}), json!({"time":"2026-11-05T20:00:00Z","index":7.9}), json!({"time":"2026-11-06T01:00:00Z","index":8.0})];
        let r = analyse(&[], &st, no_dumps(), now());
        assert_eq!(r.stability, vec![("2026-11-05".into(), 7.9), ("2026-11-06".into(), 8.0)]);
    }

    #[test]
    fn real_report_runs() {
        let r = report().unwrap();
        assert_eq!(r.daily.len(), WINDOW_DAYS as usize);
    }
}
