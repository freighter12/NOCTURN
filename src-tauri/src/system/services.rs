//! Service intelligence: every Win32 service with its description, image,
//! publisher, dependencies and a tier that decides what NOCTURN may change.
//!
//! Tiers:
//! * `Protected` — security, updates and core Windows plumbing. Never changed.
//! * `Windows` — other Microsoft services. Shown read-only: their impact
//!   isn't reviewed, so NOCTURN won't offer to change them.
//! * `Optional` — a curated list of Windows features many people don't use,
//!   each with an explanation of what stops working.
//! * `ThirdParty` — installed by apps and drivers.
//!
//! Only the start type is ever changed (never stopped, deleted or
//! reconfigured otherwise), through the journal and the elevated worker,
//! which re-reads the service and re-applies these rules itself.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::tweaks::engine::{self, ApplyReport, PlannedOp, Progress};
use crate::tweaks::{ChangeOp, Takes};

/// The `tweak_id` used for every journaled service change.
pub const JOURNAL_ID: &str = "service";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StartType {
    Automatic,
    AutomaticDelayed,
    Manual,
    Disabled,
}

impl StartType {
    pub const ALL: [StartType; 4] = [StartType::Automatic, StartType::AutomaticDelayed, StartType::Manual, StartType::Disabled];

    pub fn label(self) -> &'static str {
        match self {
            StartType::Automatic => "Automatic",
            StartType::AutomaticDelayed => "Automatic (delayed start)",
            StartType::Manual => "Manual",
            StartType::Disabled => "Disabled",
        }
    }

    fn starts_at_boot(self) -> bool {
        matches!(self, StartType::Automatic | StartType::AutomaticDelayed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Protected,
    Windows,
    Optional,
    ThirdParty,
}

/// Raw start configuration, captured faithfully for rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartConfig {
    /// SERVICE_START_TYPE (0 boot, 1 system, 2 auto, 3 demand, 4 disabled).
    pub start: u32,
    /// The delayed-auto-start flag. Windows keeps it even when the start
    /// type isn't Automatic, so it is captured separately.
    pub delayed: bool,
}

impl StartConfig {
    pub fn kind(self) -> Option<StartType> {
        match self.start {
            2 if self.delayed => Some(StartType::AutomaticDelayed),
            2 => Some(StartType::Automatic),
            3 => Some(StartType::Manual),
            4 => Some(StartType::Disabled),
            _ => None,
        }
    }

    pub fn label(self) -> String {
        match self.kind() {
            Some(k) => k.label().into(),
            None if self.start == 0 => "Boot".into(),
            None if self.start == 1 => "System".into(),
            None => format!("Start type {}", self.start),
        }
    }
}

// ------------------------------------------------------------- knowledge

/// Never changed, whoever asks.
const PROTECTED: &[&str] = &[
    // security
    "windefend", "wdnissvc", "sense", "mdcoresvc", "securityhealthservice", "wscsvc", "sgrmbroker", "mpssvc", "bfe",
    "webthreatdefsvc", "appidsvc", "keyiso", "vaultsvc", "samss", "ngcsvc", "ngcctnrsvc", "wbiosrvc", "bdesvc", "efs",
    "netlogon", "idsvc", "cryptsvc", "appinfo",
    // updates and installation
    "wuauserv", "usosvc", "waasmedicsvc", "bits", "dosvc", "trustedinstaller", "msiserver", "appxsvc", "clipsvc",
    "licensemanager", "sppsvc", "installservice", "edgeupdate", "edgeupdatem", "microsoftedgeelevationservice",
    // core plumbing
    "rpcss", "rpceptmapper", "dcomlaunch", "lsm", "eventlog", "eventsystem", "schedule", "profsvc", "winmgmt",
    "plugplay", "power", "gpsvc", "brokerinfrastructure", "systemeventsbroker", "coremessagingregistrar",
    "staterepository", "camsvc", "tokenbroker", "timebrokersvc", "usermanager", "themes", "shellhwdetection",
    "deviceinstall", "dsmsvc", "textinputmanagementservice", "tabletinputservice", "fontcache", "w32time",
    "audiosrv", "audioendpointbuilder", "dhcp", "dnscache", "nsi", "nlasvc", "netprofm", "wcmsvc", "wlansvc",
    "lanmanworkstation", "winhttpautoproxysvc", "vss", "swprv", "seclogon", "sens", "cdpsvc", "dispbrokerdesktopsvc",
    "displayenhancementservice", "wudfsvc", "storsvc", "wpnservice", "lxpsvc",
];

pub struct Known {
    pub name: &'static str,
    /// What it does and what stops working if it can't start.
    pub note: &'static str,
}

/// Windows features many people don't use. Every entry says what you lose.
pub const OPTIONAL: &[Known] = &[
    Known { name: "fax", note: "Sends and receives faxes. Nothing else depends on it." },
    Known { name: "spooler", note: "Print Spooler. Needed for any printing, including Microsoft Print to PDF. Safe to disable on a PC that never prints." },
    Known { name: "xblauthmanager", note: "Xbox Live sign-in for games and the Xbox app. Disabling stops Xbox/Game Pass games signing in." },
    Known { name: "xblgamesave", note: "Syncs Xbox game saves to the cloud. Disabling stops cloud saves for Xbox/Game Pass games." },
    Known { name: "xboxnetapisvc", note: "Xbox Live networking for multiplayer. Disabling can break online play in Xbox/Game Pass games." },
    Known { name: "xboxgipsvc", note: "Xbox accessory management: controller firmware updates and button remapping in the Xbox Accessories app." },
    Known { name: "mapsbroker", note: "Downloads offline maps for the Maps app. Nothing else needs it." },
    Known { name: "lfsvc", note: "Geolocation. Apps and Windows can't find your location (Weather, Maps, automatic time zone) when it can't start." },
    Known { name: "retaildemo", note: "Shop demo mode. Never needed on a personal PC." },
    Known { name: "remoteregistry", note: "Lets other computers edit this PC's registry over the network. Disabled by default; keep it that way unless IT needs it." },
    Known { name: "sharedaccess", note: "Internet Connection Sharing. Needed for Mobile hotspot and for sharing a connection with other devices." },
    Known { name: "icssvc", note: "Windows Mobile Hotspot. Only needed to share this PC's connection over Wi-Fi." },
    Known { name: "phonesvc", note: "Telephony for Phone Link calls. Disabling stops making calls from the PC." },
    Known { name: "wsearch", note: "Windows Search indexing. Without it, searches in Start, File Explorer and Outlook are slow and less complete." },
    Known { name: "sysmain", note: "SysMain (Superfetch) preloads apps you use often. Usually best left on; disabling rarely speeds anything up." },
    Known { name: "diagtrack", note: "Connected User Experiences and Telemetry: sends diagnostic data to Microsoft. Windows keeps working without it." },
    Known { name: "dmwappushservice", note: "Routes device-management push messages, used with diagnostics and work/school management." },
    Known { name: "wersvc", note: "Windows Error Reporting: sends crash reports so problems can be diagnosed. Crashes still happen, they're just not reported." },
    Known { name: "wisvc", note: "Windows Insider Service. Needed to receive Insider Preview builds — don't disable it if you're in the Insider Program." },
    Known { name: "stisvc", note: "Windows Image Acquisition: scanners and some cameras. Disabling stops scanning." },
    Known { name: "termservice", note: "Remote Desktop: lets other PCs connect to this one. Not needed to connect *from* this PC." },
    Known { name: "sessionenv", note: "Remote Desktop configuration. Only needed when others connect to this PC with Remote Desktop." },
    Known { name: "umrdpservice", note: "Remote Desktop device redirection (printers, drives). Only used during incoming Remote Desktop sessions." },
    Known { name: "scardsvr", note: "Smart card reader support. Needed if you sign in or sign documents with a smart card (common on work PCs)." },
    Known { name: "bthserv", note: "Bluetooth Support. Disabling stops every Bluetooth device: headphones, controllers, mice and keyboards." },
    Known { name: "wpcmonsvc", note: "Parental controls (Microsoft Family Safety). Only needed for child accounts." },
    Known { name: "semgrsvc", note: "Payments and NFC/secure element. Only used for tap-to-pay hardware." },
    Known { name: "walletservice", note: "Wallet for payment apps. Rarely used on PCs." },
    Known { name: "cscservice", note: "Offline Files for network folders. Only used on managed work networks." },
    Known { name: "pcasvc", note: "Program Compatibility Assistant: offers fixes when older programs fail. Without it you won't see compatibility prompts." },
    Known { name: "trkwks", note: "Distributed Link Tracking: keeps shortcuts working when linked files move between NTFS drives on a network." },
    Known { name: "wmpnetworksvc", note: "Windows Media Player library sharing over the network." },
];

const ANTI_CHEAT: &[&str] = &["beservice", "easyanticheat", "easyanticheat_eos", "vgc", "faceit", "faceitservice", "atvi-randgrid_sr", "eaanticheatservice", "nprotect", "xigncode", "ace-base", "ace-game"];

pub fn known(name: &str) -> Option<&'static Known> {
    let n = name.to_ascii_lowercase();
    OPTIONAL.iter().find(|k| k.name == n)
}

fn is_protected(name: &str) -> bool {
    PROTECTED.contains(&name.to_ascii_lowercase().as_str())
}

// ------------------------------------------------------------------ view

#[derive(Debug, Clone, Serialize)]
pub struct ServiceView {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    /// running, stopped, starting, stopping, paused, …
    pub state: String,
    pub pid: Option<u32>,
    pub start: StartConfig,
    pub start_kind: Option<StartType>,
    pub start_label: String,
    pub image: Option<String>,
    pub account: Option<String>,
    pub publisher: Option<String>,
    pub tier: Tier,
    pub tier_reason: String,
    /// Curated explanation (Optional tier) or a hint (anti-cheat, updater, driver).
    pub note: Option<String>,
    pub anti_cheat: bool,
    pub depends_on: Vec<String>,
    /// Services that depend on this one (names).
    pub dependents: Vec<String>,
    /// Start types NOCTURN may set.
    pub allowed: Vec<StartType>,
}

/// What the SCM reports, before NOCTURN's rules are applied.
#[derive(Debug, Clone)]
pub struct RawService {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub service_type: u32,
    pub state: u32,
    pub pid: u32,
    pub start: StartConfig,
    pub binary: String,
    pub account: Option<String>,
    pub depends_on: Vec<String>,
}

fn state_name(s: u32) -> &'static str {
    match s {
        1 => "stopped",
        2 => "starting",
        3 => "stopping",
        4 => "running",
        5 => "resuming",
        6 => "pausing",
        7 => "paused",
        _ => "unknown",
    }
}

/// The executable from an ImagePath / lpBinaryPathName.
pub fn image_of(binary: &str) -> Option<String> {
    let b = binary.trim();
    let raw = if let Some(rest) = b.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        let lower = b.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(i) => b[..i + 4].to_string(),
            None => b.split_whitespace().next()?.to_string(),
        }
    };
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let mut p = raw.replace("%SystemRoot%", &windir).replace("%systemroot%", &windir).replace("%windir%", &windir);
    if let Some(rest) = p.strip_prefix(r"\??\") {
        p = rest.to_string();
    }
    if p.to_ascii_lowercase().starts_with(r"\systemroot\") {
        p = format!("{windir}{}", &p[11..]);
    }
    if p.to_ascii_lowercase().starts_with(r"system32\") {
        p = format!(r"{windir}\{p}");
    }
    (!p.is_empty()).then_some(p)
}

/// Apply NOCTURN's rules to raw SCM data. `publisher_of` returns the
/// self-reported company of an executable.
pub fn classify(raw: &[RawService], publisher_of: &mut dyn FnMut(&str) -> Option<String>) -> Vec<ServiceView> {
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for s in raw {
        for d in &s.depends_on {
            // Group dependencies start with '+' (SC_GROUP_IDENTIFIER).
            if !d.starts_with('+') {
                dependents.entry(d.to_ascii_lowercase()).or_default().push(s.name.clone());
            }
        }
    }
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    raw.iter()
        .map(|s| {
            let lname = s.name.to_ascii_lowercase();
            let image = image_of(&s.binary);
            let limage = image.as_deref().unwrap_or("").to_ascii_lowercase();
            let publisher = image.as_deref().and_then(|i| publisher_of(i));
            let microsoft = publisher.as_deref().is_some_and(|p| p.to_ascii_lowercase().contains("microsoft"))
                || (publisher.is_none() && limage.starts_with(&windir));
            let user_service = s.service_type & 0xC0 != 0;
            let driver_store = limage.contains(r"\system32\driverstore\");
            let anti_cheat = ANTI_CHEAT.iter().any(|a| lname == *a || lname.starts_with(a))
                || ["anticheat", "battleye", "easyanticheat", "riot vanguard"].iter().any(|k| limage.contains(k));
            let curated = known(&s.name);

            let (tier, reason) = if user_service {
                (Tier::Protected, "Per-user service — Windows creates and manages these for each signed-in user.".to_string())
            } else if is_protected(&s.name) {
                (Tier::Protected, "Security, updates or core Windows — NOCTURN never changes this.".to_string())
            } else if s.start.kind().is_none() {
                (Tier::Protected, "Starts with the kernel (boot/system start).".to_string())
            } else if curated.is_some() && microsoft {
                (Tier::Optional, "Windows feature you may not use.".to_string())
            } else if microsoft {
                (Tier::Windows, "Part of Windows. Not on NOCTURN's reviewed list, so it's shown read-only.".to_string())
            } else {
                (Tier::ThirdParty, "Installed by an app or driver.".to_string())
            };

            let note = match tier {
                Tier::Optional => curated.map(|k| k.note.to_string()),
                Tier::ThirdParty if anti_cheat => Some("Anti-cheat. Games that use it won't start if it's disabled; Manual lets the game start it when needed.".into()),
                Tier::ThirdParty if driver_store => Some("Installed with a hardware driver (graphics, audio, input). Changing it can break driver features such as control panels or overlays.".into()),
                Tier::ThirdParty if lname.contains("update") || limage.contains("update") => {
                    Some("Updater. Manual is usually fine — the app can still update when you open it. Disabling can leave it unpatched.".into())
                }
                _ => None,
            };
            let allowed = match tier {
                Tier::Optional | Tier::ThirdParty => StartType::ALL.to_vec(),
                _ => vec![],
            };
            ServiceView {
                name: s.name.clone(),
                display_name: if s.display_name.is_empty() { s.name.clone() } else { s.display_name.clone() },
                description: s.description.clone(),
                state: state_name(s.state).into(),
                pid: (s.pid != 0).then_some(s.pid),
                start: s.start,
                start_kind: s.start.kind(),
                start_label: s.start.label(),
                image,
                account: s.account.clone(),
                publisher,
                tier,
                tier_reason: reason,
                note,
                anti_cheat,
                depends_on: s.depends_on.iter().map(|d| d.trim_start_matches('+').to_string()).collect(),
                dependents: dependents.remove(&lname).unwrap_or_default(),
                allowed,
            }
        })
        .collect()
}

/// Every Win32 service with NOCTURN's classification.
pub fn list() -> AppResult<Vec<ServiceView>> {
    let raw = scm::enumerate()?;
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut publisher = |p: &str| {
        cache
            .entry(p.to_ascii_lowercase())
            .or_insert_with(|| crate::system::fileinfo::version_facts(p).company)
            .clone()
    };
    let mut v = classify(&raw, &mut publisher);
    v.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
    Ok(v)
}

/// Why `start` can't be set on `s`, if it can't. Also used by the elevated
/// worker, which re-reads the service before acting.
pub fn refusal(s: &ServiceView, start: StartType, all: &[ServiceView]) -> Option<String> {
    if !s.allowed.contains(&start) {
        return Some(format!("{} can't be changed: {}", s.display_name, s.tier_reason));
    }
    if start == StartType::Disabled {
        let blocking: Vec<String> = all
            .iter()
            .filter(|d| s.dependents.iter().any(|n| n.eq_ignore_ascii_case(&d.name)))
            .filter(|d| d.start_kind.is_some_and(|k| k.starts_at_boot()))
            .map(|d| d.display_name.clone())
            .collect();
        if !blocking.is_empty() {
            return Some(format!(
                "{} can't be disabled: {} start{} automatically and need{} it.",
                s.display_name,
                blocking.join(", "),
                if blocking.len() == 1 { "s" } else { "" },
                if blocking.len() == 1 { "s" } else { "" },
            ));
        }
    }
    None
}

/// Validation the elevated worker runs for a service op.
pub fn validate_elevated(name: &str, start: StartType, applying: bool) -> AppResult<()> {
    if name.is_empty() || name.len() > 256 || name.chars().any(|c| c.is_control() || c == '\\' || c == '/') {
        return Err(AppError::Forbidden("invalid service name".into()));
    }
    let all = list()?;
    let s = all
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| AppError::Forbidden(format!("service '{name}' does not exist")))?;
    if s.allowed.is_empty() {
        return Err(AppError::Forbidden(format!("{} is never changed: {}", s.display_name, s.tier_reason)));
    }
    if applying {
        if let Some(why) = refusal(s, start, &all) {
            return Err(AppError::Forbidden(why));
        }
    }
    Ok(())
}

// ---------------------------------------------------------- preview/apply

#[derive(Debug, Clone, Deserialize)]
pub struct Selection {
    pub name: String,
    pub start: StartType,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub name: String,
    pub display_name: String,
    pub current: String,
    pub proposed: String,
    pub will_change: bool,
    pub running: bool,
    pub warnings: Vec<String>,
    /// Set when the change is refused; it won't be applied.
    pub refused: Option<String>,
}

fn plan(sel: &[Selection]) -> AppResult<(Vec<PlannedOp>, Vec<PreviewItem>)> {
    if sel.is_empty() {
        return Err(AppError::Invalid("no services selected".into()));
    }
    if sel.len() > 100 {
        return Err(AppError::Invalid("too many services at once".into()));
    }
    let all = list()?;
    let mut ops = Vec::new();
    let mut items: Vec<PreviewItem> = Vec::new();
    for x in sel {
        let s = all
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&x.name))
            .ok_or_else(|| AppError::NotFound(format!("service '{}'", x.name)))?;
        if items.iter().any(|i| i.name.eq_ignore_ascii_case(&s.name)) {
            return Err(AppError::Invalid(format!("{} was selected twice", s.display_name)));
        }
        let refused = refusal(s, x.start, &all);
        let will_change = s.start_kind != Some(x.start);
        let mut warnings = Vec::new();
        if x.start == StartType::Disabled {
            if !s.dependents.is_empty() {
                warnings.push(format!("Other services use it: {}.", s.dependents.join(", ")));
            }
            if s.anti_cheat {
                warnings.push("Games that use this anti-cheat won't start while it's disabled.".into());
            }
        }
        if s.state == "running" && matches!(x.start, StartType::Manual | StartType::Disabled) {
            warnings.push("It keeps running until you restart; NOCTURN only changes how it starts.".into());
        }
        if refused.is_none() && will_change {
            ops.push(PlannedOp {
                tweak_id: JOURNAL_ID.into(),
                title: format!("{}: {}", s.display_name, x.start.label()),
                takes: Takes::Restart,
                op: ChangeOp::ServiceStartType { name: s.name.clone(), display: s.display_name.clone(), start: x.start },
            });
        }
        items.push(PreviewItem {
            name: s.name.clone(),
            display_name: s.display_name.clone(),
            current: s.start_label.clone(),
            proposed: x.start.label().into(),
            will_change,
            running: s.state == "running",
            warnings,
            refused,
        });
    }
    Ok((ops, items))
}

pub fn preview(sel: &[Selection]) -> AppResult<Vec<PreviewItem>> {
    Ok(plan(sel)?.1)
}

pub fn apply(db: &Db, data_dir: &Path, sel: &[Selection], progress: &dyn Fn(Progress)) -> AppResult<ApplyReport> {
    let (ops, items) = plan(sel)?;
    if let Some(r) = items.iter().find_map(|i| i.refused.clone()) {
        return Err(AppError::Forbidden(r));
    }
    if ops.is_empty() {
        return Err(AppError::Invalid("every selected service already starts that way".into()));
    }
    let label = if items.len() == 1 {
        format!("Service: {} → {}", items[0].display_name, items[0].proposed)
    } else {
        format!("Services: {} start-type changes", ops.len())
    };
    let mut r = engine::run_plan(db, data_dir, ops, "user", &label, false, progress)?;
    for n in &mut r.notes {
        if n.starts_with("Restart your PC") {
            *n = "Takes effect the next time Windows starts. Running services keep running until then.".into();
        }
    }
    Ok(r)
}

// ------------------------------------------------------------- SCM access

pub fn read_config(name: &str) -> AppResult<StartConfig> {
    scm::read_config(name)
}

/// Set the start type and verify it reads back.
pub fn set_start(name: &str, target: StartConfig) -> AppResult<StartConfig> {
    scm::write_config(name, target)?;
    let now = scm::read_config(name)?;
    let ok = now.start == target.start && (target.start != 2 || now.delayed == target.delayed);
    if !ok {
        return Err(AppError::msg(format!("verification failed: service now reads {}", now.label())));
    }
    Ok(now)
}

pub fn config_for(start: StartType, current: StartConfig) -> StartConfig {
    match start {
        StartType::Automatic => StartConfig { start: 2, delayed: false },
        StartType::AutomaticDelayed => StartConfig { start: 2, delayed: true },
        // Leave the (ignored) delayed flag as it was for non-automatic types.
        StartType::Manual => StartConfig { start: 3, delayed: current.delayed },
        StartType::Disabled => StartConfig { start: 4, delayed: current.delayed },
    }
}

mod scm {
    use super::{RawService, StartConfig};
    use crate::error::{AppError, AppResult};
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::System::Services::*;

    struct Handle(SC_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }

    fn manager(access: u32) -> AppResult<Handle> {
        unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access) }
            .map(Handle)
            .map_err(|e| AppError::msg(format!("couldn't open the service manager: {e}")))
    }

    fn open(m: &Handle, name: &str, access: u32) -> AppResult<Handle> {
        unsafe { OpenServiceW(m.0, &HSTRING::from(name), access) }.map(Handle).map_err(|e| {
            if e.code().0 as u32 == 0x8007_0005 {
                AppError::Forbidden(format!("{name}: administrator rights required"))
            } else {
                AppError::msg(format!("couldn't open service {name}: {e}"))
            }
        })
    }

    unsafe fn pwstr(p: PWSTR) -> Option<String> {
        if p.is_null() {
            return None;
        }
        let s = unsafe { p.to_string() }.ok()?;
        let t = s.trim().to_string();
        (!t.is_empty()).then_some(t)
    }

    /// Double-NUL-terminated string list.
    unsafe fn multi_sz(p: PWSTR) -> Vec<String> {
        let mut out = Vec::new();
        if p.is_null() {
            return out;
        }
        let mut cur = p.0 as *const u16;
        loop {
            let mut len = 0usize;
            while unsafe { *cur.add(len) } != 0 {
                len += 1;
            }
            if len == 0 {
                break;
            }
            out.push(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(cur, len) }));
            cur = unsafe { cur.add(len + 1) };
        }
        out
    }

    fn query_config(h: &Handle) -> AppResult<(Vec<u64>, u32)> {
        let mut need = 0u32;
        let _ = unsafe { QueryServiceConfigW(h.0, None, 0, &mut need) };
        // u64 storage keeps the struct aligned.
        let mut buf = vec![0u64; (need as usize).div_ceil(8).max(8)];
        unsafe { QueryServiceConfigW(h.0, Some(buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW), (buf.len() * 8) as u32, &mut need) }
            .map_err(|e| AppError::msg(format!("couldn't read service configuration: {e}")))?;
        Ok((buf, need))
    }

    fn query2(h: &Handle, level: SERVICE_CONFIG) -> Option<Vec<u64>> {
        let mut need = 0u32;
        let _ = unsafe { QueryServiceConfig2W(h.0, level, None, &mut need) };
        if need == 0 {
            return None;
        }
        let mut buf = vec![0u64; (need as usize).div_ceil(8)];
        let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 8) };
        unsafe { QueryServiceConfig2W(h.0, level, Some(bytes), &mut need) }.ok()?;
        Some(buf)
    }

    fn delayed(h: &Handle) -> bool {
        query2(h, SERVICE_CONFIG_DELAYED_AUTO_START_INFO)
            .map(|b| unsafe { (*(b.as_ptr() as *const SERVICE_DELAYED_AUTO_START_INFO)).fDelayedAutostart.as_bool() })
            .unwrap_or(false)
    }

    fn description(h: &Handle) -> Option<String> {
        let b = query2(h, SERVICE_CONFIG_DESCRIPTION)?;
        let d = unsafe { pwstr((*(b.as_ptr() as *const SERVICE_DESCRIPTIONW)).lpDescription) }?;
        if d.starts_with('@') {
            return indirect(&d);
        }
        Some(d)
    }

    fn indirect(s: &str) -> Option<String> {
        let mut out = vec![0u16; 1024];
        unsafe { windows::Win32::UI::Shell::SHLoadIndirectString(&HSTRING::from(s), &mut out, None) }.ok()?;
        let len = out.iter().position(|&c| c == 0).unwrap_or(out.len());
        let t = String::from_utf16_lossy(&out[..len]);
        (!t.trim().is_empty()).then_some(t)
    }

    pub fn enumerate() -> AppResult<Vec<RawService>> {
        let m = manager(SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE)?;
        let mut need = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        let _ = unsafe { EnumServicesStatusExW(m.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL, None, &mut need, &mut count, Some(&mut resume), PCWSTR::null()) };
        let mut buf = vec![0u64; (need as usize).div_ceil(8) + 512];
        let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 8) };
        resume = 0;
        unsafe { EnumServicesStatusExW(m.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL, Some(bytes), &mut need, &mut count, Some(&mut resume), PCWSTR::null()) }
            .map_err(|e| AppError::msg(format!("couldn't list services: {e}")))?;
        let entries = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize) };
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            let name = unsafe { pwstr(e.lpServiceName) }.unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let display_name = unsafe { pwstr(e.lpDisplayName) }.unwrap_or_default();
            let st = e.ServiceStatusProcess;
            let mut raw = RawService {
                name: name.clone(),
                display_name,
                description: None,
                service_type: st.dwServiceType.0,
                state: st.dwCurrentState.0,
                pid: st.dwProcessId,
                start: StartConfig { start: 3, delayed: false },
                binary: String::new(),
                account: None,
                depends_on: vec![],
            };
            if let Ok(h) = open(&m, &name, SERVICE_QUERY_CONFIG) {
                if let Ok((cfg, _)) = query_config(&h) {
                    let c = unsafe { &*(cfg.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
                    raw.start = StartConfig { start: c.dwStartType.0, delayed: delayed(&h) };
                    raw.binary = unsafe { pwstr(c.lpBinaryPathName) }.unwrap_or_default();
                    raw.account = unsafe { pwstr(c.lpServiceStartName) };
                    raw.depends_on = unsafe { multi_sz(c.lpDependencies) };
                }
                raw.description = description(&h);
            }
            out.push(raw);
        }
        Ok(out)
    }

    pub fn read_config(name: &str) -> AppResult<StartConfig> {
        let m = manager(SC_MANAGER_CONNECT)?;
        let h = open(&m, name, SERVICE_QUERY_CONFIG)?;
        let (cfg, _) = query_config(&h)?;
        let c = unsafe { &*(cfg.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
        Ok(StartConfig { start: c.dwStartType.0, delayed: delayed(&h) })
    }

    pub fn write_config(name: &str, target: StartConfig) -> AppResult<()> {
        if !(2..=4).contains(&target.start) {
            return Err(AppError::Forbidden("only Automatic, Manual or Disabled can be set".into()));
        }
        let m = manager(SC_MANAGER_CONNECT)?;
        let h = open(&m, name, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)?;
        unsafe {
            ChangeServiceConfigW(
                h.0,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                SERVICE_START_TYPE(target.start),
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
            )
        }
        .map_err(|e| AppError::msg(format!("couldn't change {name}: {e}")))?;
        if delayed(&h) != target.delayed {
            let info = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: target.delayed.into() };
            unsafe { ChangeServiceConfig2W(h.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(&info as *const _ as *const core::ffi::c_void)) }
                .map_err(|e| AppError::msg(format!("couldn't set delayed start for {name}: {e}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, binary: &str, start: u32, deps: &[&str]) -> RawService {
        RawService {
            name: name.into(),
            display_name: name.into(),
            description: None,
            service_type: 0x10,
            state: 1,
            pid: 0,
            start: StartConfig { start, delayed: false },
            binary: binary.into(),
            account: None,
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn views(list: &[RawService]) -> Vec<ServiceView> {
        classify(list, &mut |p: &str| {
            let l = p.to_ascii_lowercase();
            if l.contains(r"\nv\") { Some("NVIDIA Corporation".into()) } else if l.contains(r"\windows\") { Some("Microsoft Corporation".into()) } else { None }
        })
    }

    #[test]
    fn image_paths() {
        assert_eq!(image_of(r#""C:\Program Files\X\a b.exe" /svc"#).as_deref(), Some(r"C:\Program Files\X\a b.exe"));
        assert_eq!(image_of(r"C:\Windows\system32\svchost.exe -k netsvcs -p").as_deref(), Some(r"C:\Windows\system32\svchost.exe"));
        assert!(image_of(r"%SystemRoot%\System32\x.exe").unwrap().ends_with(r"\System32\x.exe"));
        assert!(!image_of(r"%SystemRoot%\System32\x.exe").unwrap().contains('%'));
    }

    #[test]
    fn tiers() {
        let sys = r"C:\Windows\system32\svchost.exe -k netsvcs";
        let v = views(&[
            raw("WinDefend", sys, 2, &[]),
            raw("Spooler", r"C:\Windows\System32\spoolsv.exe", 2, &[]),
            raw("DPS", sys, 2, &[]),
            raw("SteamService", r#""C:\Program Files (x86)\Steam\steamservice.exe" /RunAsService"#, 3, &[]),
            raw("BEService", r#""C:\Program Files (x86)\Common Files\BattlEye\BEService.exe""#, 3, &[]),
            raw("NVDisplay.ContainerLocalSystem", r"C:\Windows\System32\DriverStore\FileRepository\nv\nvcontainer.exe", 2, &[]),
            raw("FakeSpooler", r"C:\Users\x\AppData\evil.exe", 2, &[]),
        ]);
        let t = |n: &str| v.iter().find(|s| s.name == n).unwrap();
        assert_eq!(t("WinDefend").tier, Tier::Protected);
        assert!(t("WinDefend").allowed.is_empty());
        assert_eq!(t("Spooler").tier, Tier::Optional);
        assert!(t("Spooler").note.is_some());
        assert_eq!(t("DPS").tier, Tier::Windows);
        assert!(t("DPS").allowed.is_empty());
        assert_eq!(t("SteamService").tier, Tier::ThirdParty);
        assert!(t("BEService").anti_cheat);
        assert_eq!(t("NVDisplay.ContainerLocalSystem").tier, Tier::ThirdParty, "driver-store helpers aren't Microsoft");
        assert!(t("NVDisplay.ContainerLocalSystem").note.as_deref().unwrap().contains("hardware driver"));
        // A curated name alone isn't enough: the image must be Microsoft's.
        assert_eq!(t("FakeSpooler").tier, Tier::ThirdParty);
    }

    #[test]
    fn user_services_and_boot_start_are_protected() {
        let mut u = raw("CDPUserSvc_1a2b3", r"C:\Windows\system32\svchost.exe", 2, &[]);
        u.service_type = 0xE0;
        let b = raw("SomeThing", r"C:\Program Files\x\y.exe", 1, &[]);
        let v = views(&[u, b]);
        assert!(v.iter().all(|s| s.tier == Tier::Protected && s.allowed.is_empty()));
    }

    #[test]
    fn disabling_is_refused_while_an_automatic_service_depends_on_it() {
        let v = views(&[
            raw("VendorCore", r"C:\Program Files\V\core.exe", 3, &[]),
            raw("VendorAgent", r"C:\Program Files\V\agent.exe", 2, &["VendorCore"]),
            raw("VendorTray", r"C:\Program Files\V\tray.exe", 3, &["VendorCore", "+SomeGroup"]),
        ]);
        let core = v.iter().find(|s| s.name == "VendorCore").unwrap();
        assert_eq!(core.dependents.len(), 2);
        assert!(refusal(core, StartType::Disabled, &v).unwrap().contains("VendorAgent"));
        assert!(refusal(core, StartType::Manual, &v).is_none());
        let agent = v.iter().find(|s| s.name == "VendorAgent").unwrap();
        assert!(refusal(agent, StartType::Disabled, &v).is_none());
    }

    #[test]
    fn start_config_mapping() {
        let cur = StartConfig { start: 3, delayed: true };
        assert_eq!(config_for(StartType::AutomaticDelayed, cur).kind(), Some(StartType::AutomaticDelayed));
        assert_eq!(config_for(StartType::Manual, cur), StartConfig { start: 3, delayed: true }, "stale flag preserved");
        assert_eq!(StartConfig { start: 3, delayed: true }.kind(), Some(StartType::Manual));
        assert_eq!(StartConfig { start: 0, delayed: false }.kind(), None);
    }

    #[test]
    fn curated_and_protected_lists_are_disjoint_and_lowercase() {
        for k in OPTIONAL {
            assert_eq!(k.name, k.name.to_ascii_lowercase());
            assert!(!is_protected(k.name), "{} is both optional and protected", k.name);
        }
        for p in PROTECTED {
            assert_eq!(*p, p.to_ascii_lowercase());
        }
    }

    #[test]
    fn lists_real_services() {
        let v = list().unwrap();
        assert!(v.len() > 50, "only {} services", v.len());
        let def = v.iter().find(|s| s.name.eq_ignore_ascii_case("WinDefend"));
        if let Some(d) = def {
            assert_eq!(d.tier, Tier::Protected);
        }
        assert!(v.iter().any(|s| s.description.is_some()), "descriptions resolved");
        assert!(v.iter().all(|s| s.description.as_deref().is_none_or(|d| !d.starts_with('@'))), "indirect strings resolved");
    }

    #[test]
    fn apply_refuses_protected_services_before_journaling() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        for name in ["wuauserv", "WinDefend", "mpssvc", "Appinfo"] {
            if read_config(name).is_err() {
                continue; // not installed on this machine
            }
            let sel = vec![Selection { name: name.into(), start: StartType::Disabled }];
            let e = apply(&db, dir.path(), &sel, &|_| {}).unwrap_err();
            assert!(matches!(e, AppError::Forbidden(_)), "{name}: {e}");
        }
        assert!(crate::tweaks::journal::list_tx(&db, 10).unwrap().is_empty(), "nothing journaled");
    }

    #[test]
    fn refuses_bad_names_without_touching_anything() {
        assert!(validate_elevated("", StartType::Manual, true).is_err());
        assert!(validate_elevated(r"..\x", StartType::Manual, true).is_err());
        assert!(validate_elevated("WinDefend", StartType::Disabled, true).is_err());
        assert!(validate_elevated("wuauserv", StartType::Disabled, false).is_err(), "not even for rollback");
        assert!(validate_elevated("NoSuchServiceNocturn", StartType::Manual, true).is_err());
    }
}
