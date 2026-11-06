//! Driver centre: devices with problems (Device Manager error codes in plain
//! language), the drivers people actually update (graphics, network, audio,
//! Bluetooth, chipset, storage), recent driver installs, and links to the
//! makers' official support pages. Read-only: NOCTURN never downloads or
//! installs drivers.

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::exec::{self, PsScript};

// ---------------------------------------------------------------- knowledge

/// Official pages NOCTURN may open. Nothing else can be opened.
pub const LINKS: &[(&str, &str, &str)] = &[
    ("nvidia", "NVIDIA drivers", "https://www.nvidia.com/en-us/drivers/"),
    ("amd", "AMD drivers and support", "https://www.amd.com/en/support/download/drivers.html"),
    ("intel", "Intel Driver & Support Assistant", "https://www.intel.com/content/www/us/en/support/detect.html"),
    ("logitech", "Logitech support", "https://support.logi.com/"),
    ("asus", "ASUS download center", "https://www.asus.com/support/download-center/"),
    ("msi", "MSI support downloads", "https://www.msi.com/support/download"),
    ("gigabyte", "GIGABYTE support", "https://www.gigabyte.com/Support"),
    ("asrock", "ASRock support", "https://www.asrock.com/support/index.asp"),
    ("dell", "Dell drivers and downloads", "https://www.dell.com/support/home/"),
    ("hp", "HP software and drivers", "https://support.hp.com/drivers"),
    ("lenovo", "Lenovo support", "https://support.lenovo.com/"),
    ("windows_update", "Windows Update (optional driver updates)", "ms-settings:windowsupdate-optionalupdates"),
];

pub fn link(key: &str) -> Option<(&'static str, &'static str)> {
    LINKS.iter().find(|(k, _, _)| *k == key).map(|(_, l, u)| (*l, *u))
}

/// Maker by PCI/USB vendor ID, with the support link key when there is one.
fn vendor(vid: u16) -> Option<(&'static str, Option<&'static str>)> {
    Some(match vid {
        0x10DE => ("NVIDIA", Some("nvidia")),
        0x1002 | 0x1022 => ("AMD", Some("amd")),
        0x8086 | 0x8087 => ("Intel", Some("intel")),
        0x10EC | 0x0BDA => ("Realtek", None),
        0x14E4 | 0x0A5C => ("Broadcom", None),
        0x168C | 0x0CF3 | 0x17CB => ("Qualcomm", None),
        0x14C3 | 0x0E8D => ("MediaTek", None),
        0x1B21 => ("ASMedia", None),
        0x144D | 0x04E8 => ("Samsung", None),
        0x15B7 => ("Western Digital / SanDisk", None),
        0x1987 => ("Phison", None),
        0x046D => ("Logitech", Some("logitech")),
        0x1532 => ("Razer", None),
        0x1038 => ("SteelSeries", None),
        0x1B1C => ("Corsair", None),
        0x054C => ("Sony", None),
        0x045E => ("Microsoft", Some("windows_update")),
        0x0B05 => ("ASUS", Some("asus")),
        0x1462 => ("MSI", Some("msi")),
        _ => return None,
    })
}

/// What kind of device it is, from PCI class codes or USB class triples.
fn device_kind(ids: &[String]) -> Option<&'static str> {
    for id in ids {
        let u = id.to_ascii_uppercase();
        if let Some(i) = u.find("CC_") {
            let cc = &u[i + 3..u.len().min(i + 7)];
            let k = match cc {
                "0300" | "0302" | "0380" => "graphics card",
                "0200" => "Ethernet adapter",
                "0280" => "Wi-Fi adapter",
                "0403" | "0401" => "audio controller",
                "0C03" => "USB controller",
                "0108" => "NVMe storage controller",
                "0106" | "0104" => "SATA/RAID storage controller",
                "0C05" => "SMBus controller (chipset)",
                "0780" => "management engine interface (chipset)",
                "1180" | "0880" => "system device (chipset)",
                "0C80" | "0C09" => "serial bus controller (chipset)",
                _ => continue,
            };
            return Some(k);
        }
        if u.contains("CLASS_E0&SUBCLASS_01&PROT_01") {
            return Some("Bluetooth adapter");
        }
        for (pat, k) in [("CLASS_0E", "camera"), ("CLASS_01", "USB audio device"), ("CLASS_03", "input device"), ("CLASS_08", "USB storage device"), ("CLASS_0B", "smart card reader"), ("CLASS_02", "network/modem device")] {
            if u.contains(pat) {
                return Some(k);
            }
        }
    }
    None
}

fn vid_of(ids: &[String]) -> Option<u16> {
    for id in ids {
        let u = id.to_ascii_uppercase();
        for tag in ["VEN_", "VID_"] {
            if let Some(i) = u.find(tag) {
                let hex: String = u[i + 4..].chars().take(4).collect();
                if let Ok(v) = u16::from_str_radix(&hex, 16) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Device Manager problem codes (CM_PROB_*), with what to do.
pub fn problem(code: u32) -> (&'static str, &'static str, &'static str) {
    // (severity, meaning, what to do)
    match code {
        1 => ("warning", "Not configured correctly.", "Update or reinstall the driver from the device maker."),
        3 => ("warning", "The driver may be corrupted, or Windows is low on memory.", "Reinstall the driver; restart the PC."),
        10 => ("critical", "The device can't start.", "Install the maker's latest driver. If it began after an update, roll the driver back in Device Manager."),
        12 => ("warning", "Not enough free resources for this device.", "Disable a device you don't use, or update the BIOS."),
        14 => ("info", "Needs a restart to work.", "Restart the PC."),
        18 => ("warning", "The driver needs reinstalling.", "Reinstall the driver."),
        19 => ("warning", "Its registry configuration is damaged.", "Uninstall the device in Device Manager and restart so Windows reinstalls it."),
        21 => ("info", "Windows is removing this device.", "Restart the PC."),
        22 => ("info", "The device is disabled.", "Enable it in Device Manager if you need it."),
        24 => ("warning", "The device isn't present, isn't working, or doesn't have all its drivers.", "Reconnect it, or install the maker's driver."),
        28 => ("warning", "No driver is installed.", "Install the driver from the device or PC maker, or check Windows Update's optional updates."),
        29 => ("warning", "Disabled by the firmware (BIOS/UEFI).", "Enable the device in the BIOS if you need it."),
        31 => ("critical", "Windows can't load a driver for it.", "Install the maker's latest driver."),
        32 => ("warning", "A driver or service it needs is disabled.", "Reinstall the driver."),
        33..=36 => ("warning", "Windows can't determine the resources it needs.", "Update the BIOS or the device's driver."),
        37 => ("critical", "The driver failed to initialise.", "Reinstall or update the driver."),
        38 => ("info", "A previous copy of the driver is still loaded.", "Restart the PC."),
        39 => ("critical", "The driver is corrupted or missing.", "Reinstall the driver."),
        40 => ("warning", "Its service registry entry is missing or wrong.", "Reinstall the driver."),
        41 => ("warning", "The driver loaded but the hardware wasn't found.", "Reconnect the device, or uninstall and rescan in Device Manager."),
        42 => ("warning", "A duplicate device is already running.", "Restart the PC."),
        43 => ("critical", "The device reported a problem and Windows stopped it.", "Reconnect it, update the driver, and check the device itself (for GPUs: power cables and seating)."),
        47 => ("info", "Prepared for safe removal.", "Unplug and reconnect it."),
        48 => ("critical", "Blocked because it's known to cause problems with Windows.", "Get an updated driver from the maker."),
        49 => ("warning", "The system registry is too large.", "Contact support; this is rare."),
        50 | 51 => ("info", "Waiting on another device or for properties to be set.", "Restart the PC."),
        52 => ("critical", "Windows can't verify the driver's signature.", "Install a signed driver from the maker. Don't disable signature checks."),
        53 => ("info", "Reserved for the Windows kernel debugger.", "Nothing to do unless you disabled debugging."),
        54 => ("warning", "The device failed and is resetting.", "Restart; update the driver if it keeps happening."),
        _ => ("warning", "Windows reports a problem with this device.", "Open Device Manager for details."),
    }
}

// --------------------------------------------------------------------- model

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub key: &'static str,
    pub label: &'static str,
}

fn mk_link(key: &'static str) -> Option<Link> {
    link(key).map(|(label, _)| Link { key, label })
}

#[derive(Debug, Clone, Serialize)]
pub struct ProblemDevice {
    pub id: String,
    pub name: String,
    /// True when Windows has no name and NOCTURN identified it from its IDs.
    pub identified: bool,
    pub maker: Option<String>,
    pub code: u32,
    pub severity: &'static str,
    pub meaning: String,
    pub action: String,
    pub link: Option<Link>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyDriver {
    pub group: &'static str,
    pub device: String,
    pub provider: Option<String>,
    pub version: Option<String>,
    pub date: Option<String>,
    pub age_days: Option<i64>,
    pub built_in: bool,
    pub signed: Option<bool>,
    pub note: Option<String>,
    pub link: Option<Link>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Install {
    pub time: String,
    pub device: String,
    pub provider: Option<String>,
    pub version: Option<String>,
    pub inf: String,
    /// The device already had a driver (an update, not a first install).
    pub updated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DriverReport {
    pub problems: Vec<ProblemDevice>,
    pub key: Vec<KeyDriver>,
    pub installs: Vec<Install>,
    pub unsigned: usize,
    pub board_link: Option<Link>,
}

// ----------------------------------------------------------------- analysis

fn sv(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "null")
}

fn strings(v: &Value, k: &str) -> Vec<String> {
    match v.get(k) {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

fn provider_link(provider: &str) -> Option<&'static str> {
    let p = provider.to_ascii_lowercase();
    if p.contains("nvidia") {
        Some("nvidia")
    } else if p.contains("advanced micro devices") || p.starts_with("amd") || p.contains("ati technologies") {
        Some("amd")
    } else if p.contains("intel") {
        Some("intel")
    } else if p.contains("logitech") {
        Some("logitech")
    } else {
        None
    }
}

/// The PC/motherboard maker's support page, from the board or system maker.
pub fn board_link(board_maker: &str, system_maker: &str) -> Option<&'static str> {
    let m = format!("{board_maker} {system_maker}").to_ascii_lowercase();
    [("asus", "asus"), ("micro-star", "msi"), ("msi", "msi"), ("gigabyte", "gigabyte"), ("asrock", "asrock"), ("dell", "dell"), ("hewlett", "hp"), ("hp ", "hp"), ("lenovo", "lenovo")]
        .iter()
        .find(|(k, _)| m.contains(k))
        .map(|(_, v)| *v)
}

fn group_of(class: &str, device: &str) -> Option<&'static str> {
    let d = device.to_ascii_lowercase();
    // Windows reports classes in varying case (DISPLAY, Display).
    Some(match class.to_ascii_uppercase().as_str() {
        "DISPLAY" => "Graphics",
        "NET" => "Network",
        "MEDIA" => "Audio",
        "BLUETOOTH" => "Bluetooth",
        "SCSIADAPTER" | "HDC" => "Storage",
        "SYSTEM" if ["chipset", "management engine", "smbus", "serial io", "gpio", "pci express root", "psp", "platform"].iter().any(|k| d.contains(k)) => "Chipset",
        _ => return None,
    })
}

fn is_virtual(device: &str) -> bool {
    let d = device.to_ascii_lowercase();
    ["wan miniport", "virtual", "vpn", "tap-", "wintun", "wireguard", "hyper-v", "loopback", "bluetooth device (personal area", "bluetooth device (rfcomm", "kernel debug", "teredo", "6to4", "ip-https"]
        .iter()
        .any(|k| d.contains(k))
}

/// Board maker from the PCI subsystem vendor of onboard (chipset, audio,
/// network) devices — useful when the firmware reports a garbled board name.
/// Graphics cards are skipped: their subsystem vendor is the card's maker.
pub fn board_from_subsystems(drivers: &[Value]) -> Option<&'static str> {
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for d in drivers {
        if sv(d, "class").is_some_and(|c| c.eq_ignore_ascii_case("display")) {
            continue;
        }
        let Some(id) = sv(d, "id").map(|s| s.to_ascii_uppercase()) else { continue };
        if !id.starts_with("PCI\\") {
            continue;
        }
        let Some(i) = id.find("SUBSYS_") else { continue };
        let Some(vendor) = id.get(i + 11..i + 15).and_then(|h| u16::from_str_radix(h, 16).ok()) else { continue };
        let key = match vendor {
            0x1462 => "msi",
            0x1043 => "asus",
            0x1458 => "gigabyte",
            0x1849 => "asrock",
            0x1028 => "dell",
            0x103C => "hp",
            0x17AA => "lenovo",
            _ => continue,
        };
        *counts.entry(key).or_default() += 1;
    }
    // Require a few devices so one add-in card can't decide it.
    counts.into_iter().filter(|(_, n)| *n >= 3).max_by_key(|(_, n)| *n).map(|(k, _)| k)
}

pub fn analyse(v: &Value, board: Option<&'static str>, today: NaiveDate) -> DriverReport {
    let arr = |k: &str| v.get(k).cloned().map(exec::as_array).unwrap_or_default();
    let board = board.or_else(|| board_from_subsystems(&arr("drivers")));

    let mut problems: Vec<ProblemDevice> = arr("problems")
        .iter()
        .map(|p| {
            let code = p.get("code").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
            let ids: Vec<String> = strings(p, "hw").into_iter().chain(strings(p, "compat")).chain(sv(p, "id")).collect();
            let maker_v = vid_of(&ids).and_then(vendor);
            let kind = device_kind(&ids);
            let given = sv(p, "name");
            let identified = given.is_none();
            let name = given.unwrap_or_else(|| match (maker_v.map(|m| m.0), kind) {
                (Some(m), Some(k)) => format!("{m} {k}"),
                (None, Some(k)) => format!("Unnamed {k}"),
                (Some(m), None) => format!("Unnamed {m} device"),
                (None, None) => "Unnamed device".into(),
            });
            let (severity, meaning, action) = problem(code);
            let link = maker_v.and_then(|m| m.1).or(if code == 28 { Some("windows_update") } else { None }).and_then(mk_link);
            ProblemDevice {
                id: sv(p, "id").unwrap_or_default(),
                name,
                identified,
                maker: maker_v.map(|m| m.0.to_string()).or_else(|| sv(p, "manufacturer")),
                code,
                severity,
                meaning: meaning.into(),
                action: action.into(),
                link,
            }
        })
        .collect();
    let rank = |s: &str| match s {
        "critical" => 0,
        "warning" => 1,
        _ => 2,
    };
    problems.sort_by_key(|p| rank(p.severity));

    let drivers = arr("drivers");
    let unsigned = drivers.iter().filter(|d| d.get("signed").and_then(|s| s.as_bool()) == Some(false)).count();
    let mut key: Vec<KeyDriver> = Vec::new();
    for d in &drivers {
        let (Some(class), Some(device)) = (sv(d, "class"), sv(d, "device")) else { continue };
        let Some(group) = group_of(&class, &device) else { continue };
        if (group == "Network" || group == "Bluetooth") && is_virtual(&device) {
            continue;
        }
        let provider = sv(d, "provider");
        let built_in = provider.as_deref().is_some_and(|p| p.eq_ignore_ascii_case("microsoft"));
        // Storage/Bluetooth/chipset entries from Windows itself aren't interesting.
        if built_in && matches!(group, "Storage" | "Chipset" | "Bluetooth") {
            continue;
        }
        let date = sv(d, "date");
        let age_days = date.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).map(|dt| (today - dt).num_days());
        let note = match (group, age_days, built_in) {
            (_, _, true) => Some("Windows' built-in driver, updated through Windows Update. The maker's own driver may add features or fixes.".to_string()),
            ("Graphics", Some(a), false) if a > 365 => Some(format!("About {} old. New games often need a recent graphics driver.", age_words(a))),
            // Chipset drivers keep their original dates inside current packages.
            ("Chipset", Some(a), false) if a > 4 * 365 => Some("Dated years ago, which is normal for chipset drivers — the maker's current chipset package often still contains this version. Install the latest package rather than judging by the date.".to_string()),
            (_, Some(a), false) if a > 4 * 365 => Some(format!("About {} old. Check the maker's site for a newer version.", age_words(a))),
            _ => None,
        };
        // Built-in drivers still name the hardware maker in the device name.
        let link = provider
            .as_deref()
            .and_then(provider_link)
            .or_else(|| if built_in { provider_link(&device) } else { None })
            .or(match group {
                "Audio" | "Chipset" | "Network" | "Storage" => board,
                _ => None,
            });
        key.push(KeyDriver {
            group,
            device,
            provider,
            version: sv(d, "version"),
            date,
            age_days,
            built_in,
            signed: d.get("signed").and_then(|s| s.as_bool()),
            note,
            link: link.and_then(mk_link),
        });
    }
    // Built-in drivers only matter in a group that has no maker driver.
    let has_maker: Vec<&'static str> = key.iter().filter(|k| !k.built_in).map(|k| k.group).collect();
    key.retain(|k| !k.built_in || !has_maker.contains(&k.group));
    let order = ["Graphics", "Network", "Audio", "Bluetooth", "Chipset", "Storage"];
    key.sort_by_key(|k| (order.iter().position(|g| *g == k.group).unwrap_or(9), k.built_in, k.device.clone()));
    key.dedup_by(|a, b| a.group == b.group && a.device == b.device && a.version == b.version);

    // Installs: newest first, one row per device + driver version.
    let mut installs: Vec<Install> = Vec::new();
    let mut seen: HashMap<(String, String, String), ()> = HashMap::new();
    let mut raw = arr("installs");
    raw.sort_by(|a, b| sv(b, "time").cmp(&sv(a, "time")));
    for i in &raw {
        let id = sv(i, "id").unwrap_or_default();
        let inf = sv(i, "inf").unwrap_or_default();
        let version = sv(i, "version");
        if seen.insert((id.clone(), inf.clone(), version.clone().unwrap_or_default()), ()).is_some() {
            continue;
        }
        let device = sv(i, "name").unwrap_or_else(|| {
            let ids = vec![id.clone()];
            match (vid_of(&ids).and_then(vendor), id.split('\\').next()) {
                (Some(v), _) => format!("{} device", v.0),
                (None, Some(bus)) if !bus.is_empty() => format!("{bus} device (no longer connected)"),
                _ => "Unknown device".into(),
            }
        });
        installs.push(Install {
            time: sv(i, "time").unwrap_or_default(),
            device,
            provider: sv(i, "provider"),
            version,
            inf,
            updated: i.get("updated").and_then(|u| u.as_bool()).unwrap_or(false),
        });
    }

    DriverReport { problems, key, installs, unsigned, board_link: board.and_then(mk_link) }
}

fn age_words(days: i64) -> String {
    if days >= 730 {
        format!("{} years", days / 365)
    } else if days >= 365 {
        "a year".into()
    } else {
        format!("{} months", (days / 30).max(1))
    }
}

/// `board_maker` / `system_maker` come from the cached system inventory.
pub fn report(board_maker: &str, system_maker: &str) -> AppResult<DriverReport> {
    let v = exec::powershell_json(PsScript::DriverCentre)?;
    Ok(analyse(&v, board_link(board_maker, system_maker), Utc::now().date_naive()))
}

/// Open one of the fixed official pages in the default browser (or Settings).
pub fn open(db: &Db, key: &str) -> AppResult<()> {
    let (label, url) = link(key).ok_or_else(|| AppError::Forbidden(format!("unknown link '{key}'")))?;
    #[cfg(windows)]
    unsafe {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let r = ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
        if r.0 as isize <= 32 {
            return Err(AppError::msg(format!("Windows couldn't open {label}")));
        }
    }
    db.audit("info", "user", "link.open", Some(serde_json::json!({ "key": key, "url": url })));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
    }

    #[test]
    fn identifies_unnamed_bluetooth_adapter() {
        let v = json!({ "problems": [{ "id": "USB\\VID_8087&PID_07DC\\6&12D9436C&0&8", "name": null, "code": 28,
            "hw": ["USB\\VID_8087&PID_07DC&REV_0001", "USB\\VID_8087&PID_07DC"],
            "compat": ["USB\\COMPAT_VID_8087&Class_E0&SubClass_01&Prot_01", "USB\\Class_E0"] }] });
        let r = analyse(&v, None, today());
        let p = &r.problems[0];
        assert_eq!(p.name, "Intel Bluetooth adapter");
        assert!(p.identified);
        assert_eq!(p.link.as_ref().unwrap().key, "intel");
        assert!(p.meaning.contains("No driver"));
    }

    #[test]
    fn pci_class_codes() {
        let ids = vec!["PCI\\VEN_10DE&DEV_2684&CC_030000".to_string()];
        assert_eq!(device_kind(&ids), Some("graphics card"));
        assert_eq!(vid_of(&ids), Some(0x10DE));
    }

    #[test]
    fn key_drivers_and_age_notes() {
        let v = json!({ "drivers": [
            { "device": "NVIDIA GeForce RTX 4070", "class": "Display", "provider": "NVIDIA", "version": "32.0.15.5612", "date": "2024-06-01", "signed": true },
            { "device": "Realtek PCIe GbE Family Controller", "class": "Net", "provider": "Realtek", "version": "10.1", "date": "2019-01-01", "signed": true },
            { "device": "WAN Miniport (IP)", "class": "Net", "provider": "Microsoft", "date": "2006-06-21", "signed": true },
            { "device": "High Definition Audio Device", "class": "MEDIA", "provider": "Microsoft", "date": "2006-06-21", "signed": true },
            { "device": "Standard SATA AHCI Controller", "class": "HDC", "provider": "Microsoft", "date": "2006-06-21", "signed": true },
            { "device": "Some Tool", "class": "Net", "provider": "X", "date": "2025-01-01", "signed": false }
        ]});
        let r = analyse(&v, Some("msi"), today());
        assert_eq!(r.unsigned, 1);
        let gpu = r.key.iter().find(|k| k.group == "Graphics").unwrap();
        assert!(gpu.note.as_deref().unwrap().contains("2 years"));
        assert_eq!(gpu.link.as_ref().unwrap().key, "nvidia");
        assert!(!r.key.iter().any(|k| k.device.starts_with("WAN Miniport")), "virtual adapters skipped");
        assert!(!r.key.iter().any(|k| k.device.contains("AHCI")), "inbox storage skipped");
        let nic = r.key.iter().find(|k| k.device.starts_with("Realtek")).unwrap();
        assert!(nic.note.as_deref().unwrap().contains("7 years"));
        assert_eq!(nic.link.as_ref().unwrap().key, "msi", "board maker for onboard network");
        assert!(r.key.iter().find(|k| k.group == "Audio").unwrap().built_in, "kept: no maker audio driver");
        assert_eq!(group_of("DISPLAY", "x"), Some("Graphics"));
    }

    #[test]
    fn installs_dedupe_and_name() {
        let v = json!({ "installs": [
            { "time": "2026-11-06T13:36:58Z", "id": "ROOT\\MEDIA\\0002", "name": "VoiceMeeter Input", "inf": "oem25.inf", "version": "3.4.1.9", "provider": "VB-Audio Software", "updated": true },
            { "time": "2026-11-06T13:36:57Z", "id": "ROOT\\MEDIA\\0002", "name": "VoiceMeeter Input", "inf": "oem25.inf", "version": "3.4.1.9", "provider": "VB-Audio Software", "updated": false },
            { "time": "2026-10-09T21:49:13Z", "id": "USB\\VID_046D&PID_C26E&MI_00\\7&2c", "name": null, "inf": "oem18.inf", "version": "2026.0.1.0", "provider": "Logitech", "updated": true }
        ]});
        let r = analyse(&v, None, today());
        assert_eq!(r.installs.len(), 2);
        assert_eq!(r.installs[1].device, "Logitech device");
    }

    #[test]
    fn only_listed_links_open() {
        let db = Db::open_in_memory().unwrap();
        assert!(open(&db, "https://evil.example").is_err());
        assert!(open(&db, "").is_err());
        for (k, _, u) in LINKS {
            assert!(u.starts_with("https://") || u.starts_with("ms-settings:"), "{k}");
        }
    }

    #[test]
    fn board_from_onboard_subsystem_ids() {
        let d = |class: &str, id: &str| json!({ "class": class, "id": id });
        let drivers = vec![
            d("SYSTEM", r"PCI\VEN_1022&DEV_790B&SUBSYS_7C961462&REV_61\3&1"),
            d("SYSTEM", r"PCI\VEN_1022&DEV_790E&SUBSYS_7C961462&REV_51\3&2"),
            d("NET", r"PCI\VEN_10EC&DEV_8168&SUBSYS_7C961462&REV_15\4&3"),
            // An ASUS graphics card must not make it an ASUS board.
            d("DISPLAY", r"PCI\VEN_10DE&DEV_1E87&SUBSYS_86AC1043&REV_A1\4&4"),
            d("DISPLAY", r"PCI\VEN_10DE&DEV_1E87&SUBSYS_86AC1043&REV_A1\4&5"),
            d("DISPLAY", r"PCI\VEN_10DE&DEV_1E87&SUBSYS_86AC1043&REV_A1\4&6"),
        ];
        assert_eq!(board_from_subsystems(&drivers), Some("msi"));
        assert_eq!(board_from_subsystems(&drivers[..2]), None, "needs at least three devices");
    }

    #[test]
    fn board_makers() {
        assert_eq!(board_link("Micro-Star International Co., Ltd.", ""), Some("msi"));
        assert_eq!(board_link("ASUSTeK COMPUTER INC.", ""), Some("asus"));
        assert_eq!(board_link("426490HP-TRGT8291FA", "HOPESA-RSPPOF"), None);
    }

    #[test]
    fn real_report_runs() {
        let r = report("", "").unwrap();
        assert!(!r.key.is_empty());
        println!("board link: {:?}", r.board_link.as_ref().map(|l| l.key));
        for k in r.key.iter().filter(|k| k.group == "Network" || k.group == "Chipset") {
            println!("{}: {} -> {:?}", k.group, k.device, k.link.as_ref().map(|l| l.key));
        }
    }
}
