//! Static hardware & OS inventory (WMI + registry). Collected once, cached,
//! refreshable on demand. Every field comes from the machine; anything that
//! can't be read is `None` and shown as unavailable — never invented.

use std::collections::HashMap;

use serde::Serialize;
use winreg::enums::*;
use winreg::RegKey;
use wmi::{Variant, WMIConnection};

use crate::error::AppResult;
use crate::security::elevation;
use crate::util::cim_datetime;

type Row = HashMap<String, Variant>;

#[derive(Debug, Clone, Serialize, Default)]
pub struct OsInfo {
    pub name: String,
    pub edition: Option<String>,
    pub display_version: Option<String>,
    pub version: String,
    pub build: String,
    pub ubr: Option<u32>,
    pub architecture: Option<String>,
    pub install_date: Option<String>,
    pub last_boot: Option<String>,
    pub hostname: Option<String>,
    pub is_windows_11: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CpuInfo {
    pub name: String,
    pub manufacturer: Option<String>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub max_clock_mhz: Option<u32>,
    pub l2_kb: Option<u32>,
    pub l3_kb: Option<u32>,
    pub virtualization_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Option<String>,
    pub driver_version: Option<String>,
    pub driver_date: Option<String>,
    pub vram_bytes: Option<u64>,
    pub resolution: Option<String>,
    pub refresh_hz: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MemoryModule {
    pub slot: Option<String>,
    pub capacity_bytes: Option<u64>,
    pub speed_mts: Option<u32>,
    pub configured_mts: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct BoardInfo {
    pub system_manufacturer: Option<String>,
    pub system_model: Option<String>,
    pub board_manufacturer: Option<String>,
    pub board_product: Option<String>,
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PhysicalDisk {
    pub model: String,
    pub media_type: Option<String>,
    pub bus_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub health: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemInfo {
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub gpus: Vec<GpuInfo>,
    pub memory_total_bytes: Option<u64>,
    pub memory_modules: Vec<MemoryModule>,
    pub board: BoardInfo,
    pub disks: Vec<PhysicalDisk>,
    pub is_laptop: bool,
    pub has_battery: bool,
    pub is_elevated: bool,
    pub collected_at: String,
    /// Sections that could not be read, with the reason.
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------ variant utils

fn s(r: &Row, k: &str) -> Option<String> {
    match r.get(k)? {
        Variant::String(v) => {
            let t = v.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        Variant::Null | Variant::Empty => None,
        other => Some(format!("{other:?}")),
    }
}

fn u64v(r: &Row, k: &str) -> Option<u64> {
    match r.get(k)? {
        Variant::UI1(v) => Some(*v as u64),
        Variant::UI2(v) => Some(*v as u64),
        Variant::UI4(v) => Some(*v as u64),
        Variant::UI8(v) => Some(*v),
        Variant::I1(v) => u64::try_from(*v).ok(),
        Variant::I2(v) => u64::try_from(*v).ok(),
        Variant::I4(v) => u64::try_from(*v).ok(),
        Variant::I8(v) => u64::try_from(*v).ok(),
        // WMI returns uint64 as strings.
        Variant::String(v) => v.trim().parse().ok(),
        _ => None,
    }
}

fn u32v(r: &Row, k: &str) -> Option<u32> {
    u64v(r, k).and_then(|v| u32::try_from(v).ok()).filter(|&v| v > 0)
}

fn boolv(r: &Row, k: &str) -> Option<bool> {
    match r.get(k)? {
        Variant::Bool(b) => Some(*b),
        _ => None,
    }
}

fn query(conn: &WMIConnection, q: &str) -> AppResult<Vec<Row>> {
    Ok(conn.raw_query::<Row>(q)?)
}

fn media_type(v: Option<u64>) -> Option<String> {
    Some(match v? {
        3 => "HDD",
        4 => "SSD",
        5 => "SCM",
        _ => return None,
    }
    .to_string())
}

fn bus_type(v: Option<u64>) -> Option<String> {
    Some(match v? {
        1 => "SCSI",
        3 => "ATA",
        7 => "USB",
        8 => "RAID",
        10 => "SAS",
        11 => "SATA",
        12 => "SD",
        13 => "MMC",
        15 => "File-backed virtual",
        16 => "Storage Spaces",
        17 => "NVMe",
        _ => return None,
    }
    .to_string())
}

fn health(v: Option<u64>) -> Option<String> {
    Some(match v? {
        0 => "Healthy",
        1 => "Warning",
        2 => "Unhealthy",
        _ => "Unknown",
    }
    .to_string())
}

// ------------------------------------------------------------- collection

fn windows_version_from_registry(os: &mut OsInfo) {
    let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ)
    else {
        return;
    };
    os.display_version = k.get_value::<String, _>("DisplayVersion").ok();
    os.edition = k.get_value::<String, _>("EditionID").ok();
    os.ubr = k.get_value::<u32, _>("UBR").ok();
    if let Ok(b) = k.get_value::<String, _>("CurrentBuild") {
        if os.build.is_empty() {
            os.build = b;
        }
    }
}

/// VRAM from the display adapter class key — WMI's AdapterRAM is a 32-bit
/// field and wraps at 4 GB, so it can't be trusted for modern GPUs.
fn vram_by_name() -> HashMap<String, u64> {
    let mut map = HashMap::new();
    let Ok(class) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}",
        KEY_READ,
    ) else {
        return map;
    };
    for sub in class.enum_keys().flatten() {
        let Ok(k) = class.open_subkey_with_flags(&sub, KEY_READ) else { continue };
        let name: Option<String> = k.get_value("DriverDesc").ok();
        let bytes = k
            .get_raw_value("HardwareInformation.qwMemorySize")
            .ok()
            .filter(|v| v.bytes.len() >= 8)
            .map(|v| u64::from_le_bytes(v.bytes[..8].try_into().unwrap_or_default()))
            .or_else(|| {
                k.get_raw_value("HardwareInformation.MemorySize")
                    .ok()
                    .filter(|v| v.bytes.len() >= 4)
                    .map(|v| u32::from_le_bytes(v.bytes[..4].try_into().unwrap_or_default()) as u64)
            });
        if let (Some(n), Some(b)) = (name, bytes) {
            if b > 0 {
                map.insert(n, b);
            }
        }
    }
    map
}

fn collect_inner() -> SystemInfo {
    let mut info = SystemInfo {
        collected_at: crate::db::now(),
        is_elevated: elevation::is_elevated(),
        ..Default::default()
    };
    info.os.hostname = sysinfo::System::host_name().ok();

    let conn = match WMIConnection::new() {
        Ok(c) => c,
        Err(e) => {
            info.warnings.push(format!("WMI unavailable: {e}"));
            windows_version_from_registry(&mut info.os);
            return info;
        }
    };

    macro_rules! section {
        ($name:literal, $q:expr, |$rows:ident| $body:block) => {
            match query(&conn, $q) {
                Ok($rows) => $body,
                Err(e) => info.warnings.push(format!("{}: {}", $name, e)),
            }
        };
    }

    section!("Operating system", "SELECT Caption, Version, BuildNumber, OSArchitecture, LastBootUpTime, InstallDate FROM Win32_OperatingSystem", |rows| {
        if let Some(r) = rows.first() {
            info.os.name = s(r, "Caption").unwrap_or_else(|| "Windows".into()).replace("Microsoft ", "");
            info.os.version = s(r, "Version").unwrap_or_default();
            info.os.build = s(r, "BuildNumber").unwrap_or_default();
            info.os.architecture = s(r, "OSArchitecture");
            info.os.last_boot = s(r, "LastBootUpTime").and_then(|d| cim_datetime(&d));
            info.os.install_date = s(r, "InstallDate").and_then(|d| cim_datetime(&d));
        }
    });
    windows_version_from_registry(&mut info.os);
    info.os.is_windows_11 = info.os.build.parse::<u32>().map(|b| b >= 22000).unwrap_or(false);

    section!("Processor", "SELECT Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, L2CacheSize, L3CacheSize, VirtualizationFirmwareEnabled FROM Win32_Processor", |rows| {
        if let Some(r) = rows.first() {
            info.cpu = CpuInfo {
                name: s(r, "Name").unwrap_or_else(|| "Unknown processor".into()),
                manufacturer: s(r, "Manufacturer"),
                cores: rows.iter().filter_map(|r| u32v(r, "NumberOfCores")).reduce(|a, b| a + b),
                threads: rows.iter().filter_map(|r| u32v(r, "NumberOfLogicalProcessors")).reduce(|a, b| a + b),
                max_clock_mhz: u32v(r, "MaxClockSpeed"),
                l2_kb: u32v(r, "L2CacheSize"),
                l3_kb: u32v(r, "L3CacheSize"),
                virtualization_enabled: boolv(r, "VirtualizationFirmwareEnabled"),
            };
        }
    });

    let vram = vram_by_name();
    section!("Graphics", "SELECT Name, AdapterCompatibility, DriverVersion, DriverDate, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate FROM Win32_VideoController", |rows| {
        info.gpus = rows
            .iter()
            .filter(|r| {
                // Skip remote/virtual display adapters that aren't real GPUs.
                let n = s(r, "Name").unwrap_or_default().to_ascii_lowercase();
                !n.contains("remote display") && !n.contains("basic render")
            })
            .map(|r| {
                let name = s(r, "Name").unwrap_or_else(|| "Unknown GPU".into());
                let res = match (u32v(r, "CurrentHorizontalResolution"), u32v(r, "CurrentVerticalResolution")) {
                    (Some(w), Some(h)) => Some(format!("{w} × {h}")),
                    _ => None,
                };
                GpuInfo {
                    vram_bytes: vram.get(&name).copied(),
                    vendor: s(r, "AdapterCompatibility"),
                    driver_version: s(r, "DriverVersion"),
                    driver_date: s(r, "DriverDate").and_then(|d| cim_datetime(&d)),
                    resolution: res,
                    refresh_hz: u32v(r, "CurrentRefreshRate"),
                    name,
                }
            })
            .collect();
    });

    section!("System", "SELECT Manufacturer, Model, TotalPhysicalMemory, PCSystemType FROM Win32_ComputerSystem", |rows| {
        if let Some(r) = rows.first() {
            info.board.system_manufacturer = s(r, "Manufacturer");
            info.board.system_model = s(r, "Model");
            info.memory_total_bytes = u64v(r, "TotalPhysicalMemory");
            // PCSystemType 2 = Mobile.
            info.is_laptop = u64v(r, "PCSystemType") == Some(2);
        }
    });
    section!("Motherboard", "SELECT Manufacturer, Product FROM Win32_BaseBoard", |rows| {
        if let Some(r) = rows.first() {
            info.board.board_manufacturer = s(r, "Manufacturer");
            info.board.board_product = s(r, "Product");
        }
    });
    section!("BIOS", "SELECT Manufacturer, SMBIOSBIOSVersion, ReleaseDate FROM Win32_BIOS", |rows| {
        if let Some(r) = rows.first() {
            info.board.bios_vendor = s(r, "Manufacturer");
            info.board.bios_version = s(r, "SMBIOSBIOSVersion");
            info.board.bios_date = s(r, "ReleaseDate").and_then(|d| cim_datetime(&d));
        }
    });
    section!("Memory modules", "SELECT DeviceLocator, Capacity, Speed, ConfiguredClockSpeed, Manufacturer, PartNumber FROM Win32_PhysicalMemory", |rows| {
        info.memory_modules = rows
            .iter()
            .map(|r| MemoryModule {
                slot: s(r, "DeviceLocator"),
                capacity_bytes: u64v(r, "Capacity"),
                speed_mts: u32v(r, "Speed"),
                configured_mts: u32v(r, "ConfiguredClockSpeed"),
                manufacturer: s(r, "Manufacturer"),
                part_number: s(r, "PartNumber"),
            })
            .collect();
    });
    section!("Battery", "SELECT Name FROM Win32_Battery", |rows| {
        info.has_battery = !rows.is_empty();
        if info.has_battery {
            info.is_laptop = true;
        }
    });

    match WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\Storage") {
        Ok(storage) => match query(&storage, "SELECT FriendlyName, MediaType, BusType, Size, HealthStatus FROM MSFT_PhysicalDisk") {
            Ok(rows) => {
                info.disks = rows
                    .iter()
                    .map(|r| PhysicalDisk {
                        model: s(r, "FriendlyName").unwrap_or_else(|| "Disk".into()),
                        media_type: media_type(u64v(r, "MediaType")),
                        bus_type: bus_type(u64v(r, "BusType")),
                        size_bytes: u64v(r, "Size"),
                        health: health(u64v(r, "HealthStatus")),
                    })
                    .collect()
            }
            Err(e) => info.warnings.push(format!("Physical disks: {e}")),
        },
        Err(e) => info.warnings.push(format!("Storage namespace: {e}")),
    }

    info
}

/// Collect on a dedicated thread: WMI requires COM, which must be initialised
/// per thread and must not clash with the async runtime's threads.
pub fn collect() -> SystemInfo {
    std::thread::spawn(collect_inner).join().unwrap_or_else(|_| SystemInfo {
        warnings: vec!["System information collector crashed".into()],
        collected_at: crate::db::now(),
        ..Default::default()
    })
}

// ---------------------------------------------------------- drive health

#[derive(Debug, Clone, Serialize, Default)]
pub struct DriveHealth {
    pub device_id: String,
    pub model: String,
    pub media_type: Option<String>,
    pub bus_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub health: Option<String>,
    pub temperature_c: Option<u32>,
    pub temperature_max_c: Option<u32>,
    /// Percentage of rated endurance used (SSDs).
    pub wear_pct: Option<u32>,
    pub power_on_hours: Option<u64>,
    pub read_errors: Option<u64>,
    pub write_errors: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StorageHealth {
    pub drives: Vec<DriveHealth>,
    /// Whether Windows returned reliability counters (temperature, wear…).
    pub reliability_available: bool,
    pub note: Option<String>,
}

fn storage_health_inner() -> StorageHealth {
    let mut out = StorageHealth::default();
    let conn = match WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\Storage") {
        Ok(c) => c,
        Err(e) => {
            out.note = Some(format!("Storage information unavailable: {e}"));
            return out;
        }
    };
    match query(&conn, "SELECT DeviceId, FriendlyName, MediaType, BusType, Size, HealthStatus FROM MSFT_PhysicalDisk") {
        Ok(rows) => {
            out.drives = rows
                .iter()
                .map(|r| DriveHealth {
                    device_id: s(r, "DeviceId").unwrap_or_default(),
                    model: s(r, "FriendlyName").unwrap_or_else(|| "Disk".into()),
                    media_type: media_type(u64v(r, "MediaType")),
                    bus_type: bus_type(u64v(r, "BusType")),
                    size_bytes: u64v(r, "Size"),
                    health: health(u64v(r, "HealthStatus")),
                    ..Default::default()
                })
                .collect()
        }
        Err(e) => out.note = Some(format!("Physical disks: {e}")),
    }
    match query(&conn, "SELECT DeviceId, Temperature, TemperatureMax, Wear, PowerOnHours, ReadErrorsTotal, WriteErrorsTotal FROM MSFT_StorageReliabilityCounter") {
        Ok(rows) if !rows.is_empty() => {
            out.reliability_available = true;
            for r in &rows {
                let id = s(r, "DeviceId").unwrap_or_default();
                if let Some(d) = out.drives.iter_mut().find(|d| d.device_id == id) {
                    // 0 means "not reported" for these counters.
                    d.temperature_c = u32v(r, "Temperature");
                    d.temperature_max_c = u32v(r, "TemperatureMax");
                    d.wear_pct = u64v(r, "Wear").and_then(|w| u32::try_from(w).ok());
                    d.power_on_hours = u64v(r, "PowerOnHours").filter(|&h| h > 0);
                    d.read_errors = u64v(r, "ReadErrorsTotal");
                    d.write_errors = u64v(r, "WriteErrorsTotal");
                }
            }
        }
        Ok(_) | Err(_) => {
            out.note = Some(if elevation::is_elevated() {
                "Your drives don't report temperature or wear to Windows.".into()
            } else {
                "Drive temperature and wear need administrator rights to read. Run NOCTURN as administrator to see them.".into()
            });
        }
    }
    out
}

/// Drive health, temperature and wear (temperature/wear usually need admin).
pub fn storage_health() -> StorageHealth {
    std::thread::spawn(storage_health_inner).join().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_real_core_fields() {
        let i = collect();
        assert!(!i.os.name.is_empty(), "{:?}", i.warnings);
        assert!(!i.os.build.is_empty());
        assert!(!i.cpu.name.is_empty());
        assert!(i.memory_total_bytes.unwrap_or(0) > 0);
    }

    #[test]
    fn storage_health_lists_drives_and_explains_gaps() {
        let h = storage_health();
        eprintln!("{h:?}");
        assert!(!h.drives.is_empty(), "{:?}", h.note);
        assert!(h.reliability_available || h.note.is_some(), "missing data must be explained");
    }
}
