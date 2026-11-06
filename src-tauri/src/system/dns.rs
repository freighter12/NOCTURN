//! Per-adapter DNS server settings through the native IP Helper API
//! (`GetInterfaceDnsSettings` / `SetInterfaceDnsSettings`, Windows 10 2004+).
//!
//! No PowerShell or netsh: the interface is addressed by GUID and the server
//! list is a typed value, so nothing can be injected. Writing needs
//! administrator rights; changes go through the journaled engine as
//! `ChangeOp::DnsServers`, and the elevated worker only accepts the fixed
//! provider lists below (or, for rollback, a list of plain IP addresses).

use std::net::IpAddr;
use std::path::Path;

use serde::Serialize;
use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceGuidToLuid, ConvertInterfaceIndexToLuid, ConvertInterfaceLuidToGuid, FreeInterfaceDnsSettings, GetInterfaceDnsSettings,
    SetInterfaceDnsSettings, DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1, DNS_SETTING_IPV6,
    DNS_SETTING_NAMESERVER,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::tweaks::engine::{self, ApplyReport, PlannedOp, Progress};
use crate::tweaks::{ChangeOp, Takes};

#[derive(Debug, Clone, Serialize)]
pub struct DnsProvider {
    pub id: &'static str,
    pub name: &'static str,
    pub v4: &'static [&'static str],
    pub v6: &'static [&'static str],
    pub note: &'static str,
}

pub const PROVIDERS: &[DnsProvider] = &[
    DnsProvider {
        id: "automatic",
        name: "Automatic (from your router)",
        v4: &[],
        v6: &[],
        note: "Windows' default: uses whatever DNS your router or ISP hands out.",
    },
    DnsProvider {
        id: "cloudflare",
        name: "Cloudflare",
        v4: &["1.1.1.1", "1.0.0.1"],
        v6: &["2606:4700:4700::1111", "2606:4700:4700::1001"],
        note: "Fast public resolver with a no-logging policy. No filtering.",
    },
    DnsProvider {
        id: "google",
        name: "Google Public DNS",
        v4: &["8.8.8.8", "8.8.4.4"],
        v6: &["2001:4860:4860::8888", "2001:4860:4860::8844"],
        note: "Widely used and reliable. Google keeps temporary logs.",
    },
    DnsProvider {
        id: "quad9",
        name: "Quad9",
        v4: &["9.9.9.9", "149.112.112.112"],
        v6: &["2620:fe::fe", "2620:fe::9"],
        note: "Blocks domains known to host malware. Non-profit, no personal data logged.",
    },
];

pub fn provider(id: &str) -> Option<&'static DnsProvider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

impl DnsProvider {
    pub fn servers(&self, ipv6: bool) -> Vec<String> {
        (if ipv6 { self.v6 } else { self.v4 }).iter().map(|s| s.to_string()).collect()
    }
}

/// Which provider (if any) a server list belongs to.
pub fn identify(servers: &[String], ipv6: bool) -> Option<&'static str> {
    PROVIDERS.iter().find(|p| p.servers(ipv6) == servers).map(|p| p.id)
}

pub fn interface_guid(index: u32) -> AppResult<String> {
    let mut luid = NET_LUID_LH::default();
    let mut guid = GUID::zeroed();
    unsafe {
        if ConvertInterfaceIndexToLuid(index, &mut luid) != NO_ERROR {
            return Err(AppError::NotFound(format!("network interface {index}")));
        }
        if ConvertInterfaceLuidToGuid(&luid, &mut guid) != NO_ERROR {
            return Err(AppError::NotFound(format!("network interface {index}")));
        }
    }
    Ok(format!("{guid:?}"))
}

fn interface_exists(guid: &GUID) -> bool {
    let mut luid = NET_LUID_LH::default();
    unsafe { ConvertInterfaceGuidToLuid(guid, &mut luid) == NO_ERROR }
}

fn parse_guid(s: &str) -> AppResult<GUID> {
    GUID::try_from(s).map_err(|_| AppError::Invalid(format!("invalid interface id '{s}'")))
}

/// Split Windows' NameServer string (comma- and/or space-separated).
pub fn split_servers(raw: &str) -> Vec<String> {
    raw.split([',', ' ', ';']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Statically configured DNS servers; empty = automatic (DHCP / router).
pub fn read(interface: &str, ipv6: bool) -> AppResult<Vec<String>> {
    let guid = parse_guid(interface)?;
    let mut s = DNS_INTERFACE_SETTINGS { Version: DNS_INTERFACE_SETTINGS_VERSION1, ..Default::default() };
    if ipv6 {
        s.Flags = DNS_SETTING_IPV6 as u64;
    }
    let rc = unsafe { GetInterfaceDnsSettings(guid, &mut s) };
    if rc == ERROR_FILE_NOT_FOUND && interface_exists(&guid) {
        // No configuration stored for this address family: nothing static.
        return Ok(Vec::new());
    }
    if rc != NO_ERROR {
        return Err(AppError::msg(format!("couldn't read DNS settings for interface {interface} (error {})", rc.0)));
    }
    let raw = if s.NameServer.is_null() { String::new() } else { unsafe { s.NameServer.to_string() }.unwrap_or_default() };
    unsafe { FreeInterfaceDnsSettings(&mut s) };
    Ok(split_servers(&raw))
}

/// Check every entry is an IP address of the right family (at most 8).
pub fn validate_servers(servers: &[String], ipv6: bool) -> AppResult<()> {
    if servers.len() > 8 {
        return Err(AppError::Invalid("too many DNS servers".into()));
    }
    for s in servers {
        match s.parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) if !ipv6 => {}
            Ok(IpAddr::V6(_)) if ipv6 => {}
            _ => return Err(AppError::Invalid(format!("'{s}' isn't an IPv{} address", if ipv6 { 6 } else { 4 }))),
        }
    }
    Ok(())
}

/// Set the static DNS servers (empty = back to automatic) and verify. Needs admin.
pub fn write(interface: &str, ipv6: bool, servers: &[String]) -> AppResult<()> {
    validate_servers(servers, ipv6)?;
    let guid = parse_guid(interface)?;
    let mut wide: Vec<u16> = servers.join(",").encode_utf16().chain(std::iter::once(0)).collect();
    let mut flags = DNS_SETTING_NAMESERVER as u64;
    if ipv6 {
        flags |= DNS_SETTING_IPV6 as u64;
    }
    let s = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: flags,
        NameServer: PWSTR(wide.as_mut_ptr()),
        ..Default::default()
    };
    let rc = unsafe { SetInterfaceDnsSettings(guid, &s) };
    if rc != NO_ERROR {
        return Err(AppError::msg(match rc.0 {
            5 => "Windows refused the change (administrator rights needed)".to_string(),
            n => format!("Windows couldn't change the DNS servers (error {n})"),
        }));
    }
    let now = read(interface, ipv6)?;
    if now != servers {
        return Err(AppError::msg(format!("verification failed: DNS reads back as {}", display(&now))));
    }
    Ok(())
}

pub fn display(servers: &[String]) -> String {
    if servers.is_empty() {
        "Automatic".into()
    } else {
        let ipv6 = servers.iter().any(|s| s.contains(':'));
        match identify(servers, ipv6).and_then(provider) {
            Some(p) => format!("{} ({})", p.name, servers.join(", ")),
            None => servers.join(", "),
        }
    }
}

// ------------------------------------------------------------ switching

#[derive(Debug, Clone, Serialize)]
pub struct DnsPreviewItem {
    pub target: String,
    pub current: String,
    pub proposed: String,
    pub will_change: bool,
}

fn clean_alias(alias: &str) -> String {
    let a: String = alias.chars().filter(|c| !c.is_control()).take(80).collect();
    if a.trim().is_empty() {
        "network adapter".into()
    } else {
        a
    }
}

/// The IPv4 and IPv6 operations that switch adapter `index` to `provider_id`.
pub fn plan(index: u32, alias: &str, provider_id: &str) -> AppResult<Vec<(PlannedOp, DnsPreviewItem)>> {
    let p = provider(provider_id).ok_or_else(|| AppError::Invalid(format!("unknown DNS provider '{provider_id}'")))?;
    let interface = interface_guid(index)?;
    let alias = clean_alias(alias);
    let mut out = Vec::new();
    for ipv6 in [false, true] {
        let servers = p.servers(ipv6);
        let current = read(&interface, ipv6)?;
        let op = ChangeOp::DnsServers { interface: interface.clone(), alias: alias.clone(), ipv6, servers: servers.clone() };
        let item = DnsPreviewItem { target: op.target(), current: display(&current), proposed: display(&servers), will_change: current != servers };
        out.push((PlannedOp { tweak_id: "dns".into(), title: format!("{} DNS on {alias}", p.name), takes: Takes::Immediately, op }, item));
    }
    Ok(out)
}

pub fn preview(index: u32, alias: &str, provider_id: &str) -> AppResult<Vec<DnsPreviewItem>> {
    Ok(plan(index, alias, provider_id)?.into_iter().map(|(_, i)| i).collect())
}

pub fn apply(db: &Db, data_dir: &Path, index: u32, alias: &str, provider_id: &str, progress: &dyn Fn(Progress)) -> AppResult<ApplyReport> {
    let ops: Vec<PlannedOp> = plan(index, alias, provider_id)?.into_iter().map(|(op, _)| op).collect();
    let label = format!("DNS: {} on {}", provider(provider_id).map(|p| p.name).unwrap_or(provider_id), clean_alias(alias));
    engine::run_plan(db, data_dir, ops, "user", &label, false, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_lists_are_valid_and_identifiable() {
        for p in PROVIDERS {
            validate_servers(&p.servers(false), false).unwrap();
            validate_servers(&p.servers(true), true).unwrap();
            assert_eq!(identify(&p.servers(false), false), Some(p.id));
        }
        assert_eq!(identify(&["9.9.9.9".into()], false), None);
    }

    #[test]
    fn server_validation() {
        assert!(validate_servers(&["1.1.1.1".into()], true).is_err(), "family mismatch");
        assert!(validate_servers(&["::1".into()], false).is_err());
        assert!(validate_servers(&["1.1.1.1; calc.exe".into()], false).is_err());
        assert!(validate_servers(&vec!["1.1.1.1".to_string(); 9], false).is_err());
        assert_eq!(split_servers("1.1.1.1,1.0.0.1"), vec!["1.1.1.1", "1.0.0.1"]);
        assert_eq!(split_servers(" 8.8.8.8 8.8.4.4 "), vec!["8.8.8.8", "8.8.4.4"]);
        assert_eq!(display(&[]), "Automatic");
        assert_eq!(display(&["1.1.1.1".into(), "1.0.0.1".into()]), "Cloudflare (1.1.1.1, 1.0.0.1)");
    }

    #[test]
    fn reads_real_interfaces() {
        // Loopback always exists; reading it must work without admin rights.
        let guid = interface_guid(1).expect("loopback interface");
        assert_eq!(guid.len(), 36);
        read(&guid, false).unwrap();
        read(&guid, true).unwrap();
        assert!(read("not-a-guid", false).is_err());
    }

    #[test]
    fn plan_reads_current_and_targets_both_families() {
        let items = preview(1, "Loopback\u{7}", "cloudflare").unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].target.contains("IPv4") && items[1].target.contains("IPv6"));
        assert!(items[0].target.ends_with("Loopback"), "control chars stripped: {}", items[0].target);
        assert!(items.iter().all(|i| i.will_change && i.current == "Automatic"));
        assert!(preview(1, "x", "evil").is_err());
        assert!(preview(9999, "x", "cloudflare").is_err());
    }
}

