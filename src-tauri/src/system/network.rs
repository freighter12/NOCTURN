//! Network diagnostics: adapters, DNS, gateways, Wi-Fi and latency tests.

use std::net::{Ipv4Addr, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use windows::Win32::NetworkManagement::IpHelper::{
    IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IP_SUCCESS, IP_TTL_EXPIRED_TRANSIT,
};

use rusqlite::params;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::exec::{self, as_array, PsScript, SafeCommand};

#[derive(Debug, Clone, Serialize)]
pub struct Adapter {
    pub name: String,
    pub description: String,
    pub index: i64,
    pub status: String,
    pub link_speed: Option<String>,
    pub media: Option<String>,
    pub mac: Option<String>,
    pub is_virtual: bool,
    pub driver_version: Option<String>,
    pub dns_servers: Vec<String>,
    pub gateway: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct WifiInfo {
    pub connected: bool,
    pub ssid: Option<String>,
    pub signal_percent: Option<u32>,
    pub radio: Option<String>,
    pub band: Option<String>,
    pub channel: Option<String>,
    pub receive_mbps: Option<f32>,
    pub transmit_mbps: Option<f32>,
    /// All key/value pairs as reported (localised on non-English Windows).
    pub raw: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkOverview {
    pub adapters: Vec<Adapter>,
    pub wifi: Option<WifiInfo>,
    pub warnings: Vec<String>,
}

fn st(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `Get-NetAdapter` status/media fields may be ints or strings depending on
/// the PowerShell version; normalise the common ones.
fn adapter_status(v: &Value) -> String {
    match v.get("Status") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(1) => "Up".into(),
            Some(2) => "Down".into(),
            Some(7) => "Not present".into(),
            Some(x) => format!("Status {x}"),
            None => "Unknown".into(),
        },
        _ => "Unknown".into(),
    }
}

pub fn overview() -> NetworkOverview {
    // The four queries are independent and each spawns a process (~2 s of
    // PowerShell start-up apiece), so run them concurrently.
    let (adapters_r, dns_r, gw_r, wifi_info) = std::thread::scope(|sc| {
        let a = sc.spawn(|| exec::powershell_json(PsScript::NetAdapters));
        let d = sc.spawn(|| exec::powershell_json(PsScript::DnsServers));
        let g = sc.spawn(|| exec::powershell_json(PsScript::DefaultGateways));
        let w = sc.spawn(|| wifi().ok().flatten());
        let join = |h: std::thread::ScopedJoinHandle<'_, AppResult<Value>>| h.join().unwrap_or_else(|_| Err(AppError::msg("query thread panicked")));
        (join(a), join(d), join(g), w.join().ok().flatten())
    });
    let mut warnings = Vec::new();
    let mut take = |r: AppResult<Value>, what: &str| match r {
        Ok(v) => as_array(v),
        Err(e) => {
            warnings.push(format!("{what}: {e}"));
            vec![]
        }
    };
    let adapters_raw = take(adapters_r, "adapters");
    let dns_raw = take(dns_r, "DNS");
    let gw_raw = take(gw_r, "gateways");

    let adapters = adapters_raw
        .iter()
        .map(|a| {
            let index = a.get("ifIndex").and_then(|v| v.as_i64()).unwrap_or(-1);
            let mut dns: Vec<String> = dns_raw
                .iter()
                .filter(|d| d.get("InterfaceIndex").and_then(|v| v.as_i64()) == Some(index))
                .flat_map(|d| match d.get("ServerAddresses") {
                    Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                    Some(Value::String(s)) => vec![s.clone()],
                    _ => vec![],
                })
                .collect();
            dns.dedup();
            let gateway = gw_raw
                .iter()
                .filter(|g| g.get("InterfaceIndex").and_then(|v| v.as_i64()) == Some(index))
                .min_by_key(|g| g.get("RouteMetric").and_then(|v| v.as_i64()).unwrap_or(i64::MAX))
                .and_then(|g| st(g, "NextHop"));
            Adapter {
                name: st(a, "Name").unwrap_or_default(),
                description: st(a, "InterfaceDescription").unwrap_or_default(),
                index,
                status: adapter_status(a),
                link_speed: st(a, "LinkSpeed"),
                media: st(a, "PhysicalMediaType").or_else(|| st(a, "MediaType")),
                mac: st(a, "MacAddress"),
                is_virtual: a.get("Virtual").and_then(|v| v.as_bool()).unwrap_or(false),
                driver_version: st(a, "DriverVersion"),
                dns_servers: dns,
                gateway,
            }
        })
        .collect();

    NetworkOverview { adapters, wifi: wifi_info, warnings }
}

pub fn parse_wlan(out: &str) -> Option<WifiInfo> {
    let mut raw = Vec::new();
    for line in out.lines() {
        if let Some((k, v)) = line.split_once(" : ").or_else(|| line.split_once(": ")) {
            let k = k.trim();
            if !k.is_empty() {
                raw.push((k.to_string(), v.trim().to_string()));
            }
        }
    }
    if raw.is_empty() {
        return None;
    }
    let get = |names: &[&str]| -> Option<String> {
        raw.iter()
            .find(|(k, _)| names.iter().any(|n| k.eq_ignore_ascii_case(n)))
            .map(|(_, v)| v.clone())
    };
    let state = get(&["State"]);
    let signal = get(&["Signal"]).and_then(|s| s.trim_end_matches('%').trim().parse().ok());
    Some(WifiInfo {
        connected: state.as_deref().map(|s| s.eq_ignore_ascii_case("connected")).unwrap_or(signal.is_some()),
        ssid: get(&["SSID"]),
        signal_percent: signal,
        radio: get(&["Radio type"]),
        band: get(&["Band"]),
        channel: get(&["Channel"]),
        receive_mbps: get(&["Receive rate (Mbps)"]).and_then(|s| s.parse().ok()),
        transmit_mbps: get(&["Transmit rate (Mbps)"]).and_then(|s| s.parse().ok()),
        raw,
    })
}

/// Wi-Fi status, or `None` when the PC has no wireless interface.
pub fn wifi() -> AppResult<Option<WifiInfo>> {
    let out = exec::run(&SafeCommand::NetshWlanInterfaces)?;
    if !out.success() {
        // "The Wireless AutoConfig Service (wlansvc) is not running" etc.
        return Ok(None);
    }
    Ok(parse_wlan(&out.stdout).filter(|w| w.ssid.is_some() || w.signal_percent.is_some()))
}

// ------------------------------------------------------------ latency test

#[derive(Debug, Clone, Serialize)]
pub struct PingResult {
    pub target: String,
    pub label: String,
    pub sent: u32,
    pub received: u32,
    pub loss_percent: f32,
    pub min_ms: Option<u32>,
    pub avg_ms: Option<f32>,
    pub max_ms: Option<u32>,
    /// Mean absolute difference between consecutive replies.
    pub jitter_ms: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsTiming {
    pub host: String,
    pub ms: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StabilityReport {
    pub pings: Vec<PingResult>,
    pub dns: Vec<DnsTiming>,
    pub tested_at: String,
}

pub fn ping(addr: Ipv4Addr, label: &str, count: u32, timeout: Duration) -> AppResult<PingResult> {
    let handle = unsafe { IcmpCreateFile() }.map_err(|e| AppError::msg(format!("ICMP unavailable: {e}")))?;
    let payload = [0x4eu8; 32];
    let mut buf = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8 + 64];
    let mut rtts: Vec<u32> = Vec::new();
    for i in 0..count {
        let n = unsafe {
            IcmpSendEcho(
                handle,
                u32::from_ne_bytes(addr.octets()),
                payload.as_ptr() as *const _,
                payload.len() as u16,
                None,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                timeout.as_millis() as u32,
            )
        };
        if n > 0 {
            let reply = unsafe { &*(buf.as_ptr() as *const ICMP_ECHO_REPLY) };
            if reply.Status == 0 {
                rtts.push(reply.RoundTripTime);
            }
        }
        if i + 1 < count {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    unsafe {
        let _ = IcmpCloseHandle(handle);
    }
    let received = rtts.len() as u32;
    let jitter = (rtts.len() > 1).then(|| {
        rtts.windows(2).map(|w| (w[1] as f32 - w[0] as f32).abs()).sum::<f32>() / (rtts.len() - 1) as f32
    });
    Ok(PingResult {
        target: addr.to_string(),
        label: label.into(),
        sent: count,
        received,
        loss_percent: if count == 0 { 0.0 } else { (count - received) as f32 * 100.0 / count as f32 },
        min_ms: rtts.iter().copied().min(),
        avg_ms: (!rtts.is_empty()).then(|| rtts.iter().sum::<u32>() as f32 / rtts.len() as f32),
        max_ms: rtts.iter().copied().max(),
        jitter_ms: jitter,
    })
}

fn time_dns(host: &str) -> DnsTiming {
    let start = Instant::now();
    match (host, 443).to_socket_addrs().map(|mut it| it.next().is_some()) {
        Ok(true) => DnsTiming {
            host: host.into(),
            ms: Some(start.elapsed().as_millis() as u32),
            error: None,
        },
        Ok(false) => DnsTiming { host: host.into(), ms: None, error: Some("no addresses returned".into()) },
        Err(e) => DnsTiming { host: host.into(), ms: None, error: Some(e.to_string()) },
    }
}

/// Ping the default gateway (local network quality) and two public resolvers
/// (internet path quality), and time DNS lookups.
pub fn stability_test(count: u32) -> StabilityReport {
    let count = count.clamp(4, 50);
    let mut targets: Vec<(Ipv4Addr, String)> = Vec::new();
    if let Ok(gws) = exec::powershell_json(PsScript::DefaultGateways).map(as_array) {
        if let Some(gw) = gws
            .iter()
            .filter_map(|g| g.get("NextHop").and_then(|v| v.as_str()))
            .filter_map(|s| s.parse::<Ipv4Addr>().ok())
            .find(|ip| !ip.is_unspecified())
        {
            targets.push((gw, "Router (default gateway)".into()));
        }
    }
    targets.push((Ipv4Addr::new(1, 1, 1, 1), "Cloudflare (1.1.1.1)".into()));
    targets.push((Ipv4Addr::new(8, 8, 8, 8), "Google (8.8.8.8)".into()));

    let handles: Vec<_> = targets
        .into_iter()
        .map(|(ip, label)| {
            std::thread::spawn(move || ping(ip, &label, count, Duration::from_millis(1500)))
        })
        .collect();
    let pings = handles.into_iter().filter_map(|h| h.join().ok()).filter_map(Result::ok).collect();
    let dns = ["www.microsoft.com", "www.cloudflare.com", "www.google.com"].iter().map(|h| time_dns(h)).collect();
    StabilityReport { pings, dns, tested_at: crate::db::now() }
}

// ------------------------------------------------------------- traceroute

#[derive(Debug, Clone, Serialize)]
pub struct Hop {
    pub ttl: u8,
    /// Router that answered; `None` if every probe timed out.
    pub address: Option<String>,
    /// Round-trip time of each probe (`None` = no reply).
    pub rtts: Vec<Option<u32>>,
    pub private: bool,
    pub reached: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceReport {
    pub target: String,
    pub address: String,
    pub hops: Vec<Hop>,
    pub reached: bool,
    pub note: Option<String>,
}

const MAX_HOPS: u8 = 30;
const PROBES: usize = 3;
/// Give up after this many silent hops in a row (firewalled path).
const SILENT_LIMIT: usize = 5;

/// Accept a hostname or IPv4 address and resolve it to IPv4.
pub fn resolve_target(host: &str) -> AppResult<Ipv4Addr> {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err(AppError::Invalid("enter a host name like example.com or an IPv4 address".into()));
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(ip);
    }
    (host, 0)
        .to_socket_addrs()
        .map_err(|e| AppError::NotFound(format!("couldn't resolve {host}: {e}")))?
        .find_map(|a| match a.ip() {
            std::net::IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
        .ok_or_else(|| AppError::NotFound(format!("{host} has no IPv4 address")))
}

fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
}

/// One probe with the given TTL: (answering router, rtt, reached destination).
fn probe(handle: windows::Win32::Foundation::HANDLE, dest: Ipv4Addr, ttl: u8, timeout_ms: u32) -> (Option<Ipv4Addr>, Option<u32>, bool) {
    let payload = [0x4eu8; 32];
    let mut buf = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8 + 64];
    let opts = IP_OPTION_INFORMATION { Ttl: ttl, ..Default::default() };
    let start = Instant::now();
    unsafe {
        IcmpSendEcho(
            handle,
            u32::from_ne_bytes(dest.octets()),
            payload.as_ptr() as *const _,
            payload.len() as u16,
            Some(&opts),
            buf.as_mut_ptr() as *mut _,
            buf.len() as u32,
            timeout_ms,
        );
    }
    // The reply buffer is filled for "TTL expired" too, even when the call
    // reports failure, so inspect it rather than the return value.
    let reply = unsafe { &*(buf.as_ptr() as *const ICMP_ECHO_REPLY) };
    if reply.Address == 0 {
        return (None, None, false);
    }
    let from = Ipv4Addr::from(reply.Address.to_ne_bytes());
    match reply.Status {
        IP_SUCCESS => (Some(from), Some(reply.RoundTripTime), true),
        // RoundTripTime isn't set for TTL-expired replies; time it ourselves.
        IP_TTL_EXPIRED_TRANSIT => (Some(from), Some(start.elapsed().as_millis() as u32), false),
        _ => (Some(from), None, false),
    }
}

pub fn traceroute(host: &str, on_hop: &dyn Fn(&Hop)) -> AppResult<TraceReport> {
    let dest = resolve_target(host)?;
    let handle = unsafe { IcmpCreateFile() }.map_err(|e| AppError::msg(format!("ICMP unavailable: {e}")))?;
    let mut hops = Vec::new();
    let mut reached = false;
    let mut silent = 0;
    let mut note = None;
    for ttl in 1..=MAX_HOPS {
        let mut address = None;
        let mut rtts = Vec::with_capacity(PROBES);
        let mut hit = false;
        for _ in 0..PROBES {
            let (from, rtt, done) = probe(handle, dest, ttl, 1000);
            address = address.or(from);
            rtts.push(rtt);
            hit |= done;
        }
        let hop = Hop { ttl, private: address.is_some_and(is_private), address: address.map(|a| a.to_string()), rtts, reached: hit };
        on_hop(&hop);
        silent = if hop.address.is_none() { silent + 1 } else { 0 };
        hops.push(hop);
        if hit {
            reached = true;
            break;
        }
        if silent >= SILENT_LIMIT {
            note = Some(format!("Stopped after {SILENT_LIMIT} hops in a row didn't answer — routers on this path probably don't reply to traceroute. That alone isn't a fault."));
            break;
        }
    }
    unsafe {
        let _ = IcmpCloseHandle(handle);
    }
    if !reached && note.is_none() {
        note = Some(format!("Didn't reach {dest} within {MAX_HOPS} hops."));
    }
    // Trailing silent hops add nothing once we've stopped.
    while hops.last().is_some_and(|h| h.address.is_none()) && hops.len() > 1 && !reached {
        hops.pop();
    }
    Ok(TraceReport { target: host.trim().to_string(), address: dest.to_string(), hops, reached, note })
}

// ---------------------------------------------------------- test history

#[derive(Debug, Clone, Serialize)]
pub struct StoredPing {
    pub tested_at: String,
    pub label: String,
    pub target: String,
    pub avg_ms: Option<f32>,
    pub jitter_ms: Option<f32>,
    pub loss_percent: f32,
}

pub fn save_report(db: &Db, r: &StabilityReport) -> AppResult<()> {
    let conn = db.conn();
    for p in &r.pings {
        conn.execute(
            "INSERT INTO network_tests(tested_at, label, target, avg_ms, jitter_ms, loss_pct) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![r.tested_at, p.label, p.target, p.avg_ms, p.jitter_ms, p.loss_percent],
        )?;
    }
    // Keep a year of results at most.
    conn.execute("DELETE FROM network_tests WHERE tested_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-365 days')", [])?;
    Ok(())
}

pub fn history(db: &Db, limit: i64) -> AppResult<Vec<StoredPing>> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT tested_at, label, target, avg_ms, jitter_ms, loss_pct FROM network_tests ORDER BY tested_at DESC, id LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit], |r| {
            Ok(StoredPing { tested_at: r.get(0)?, label: r.get(1)?, target: r.get(2)?, avg_ms: r.get(3)?, jitter_ms: r.get(4)?, loss_percent: r.get(5)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_netsh_wlan() {
        let out = "\n    Name                   : Wi-Fi\n    State                  : connected\n    SSID                   : HomeNet\n    Radio type             : 802.11ax\n    Band                   : 5 GHz\n    Channel                : 44\n    Receive rate (Mbps)    : 1201\n    Transmit rate (Mbps)   : 960\n    Signal                 : 87%\n";
        let w = parse_wlan(out).unwrap();
        assert!(w.connected);
        assert_eq!(w.ssid.as_deref(), Some("HomeNet"));
        assert_eq!(w.signal_percent, Some(87));
        assert_eq!(w.receive_mbps, Some(1201.0));
    }

    #[test]
    fn ping_loopback() {
        let r = ping(Ipv4Addr::LOCALHOST, "loopback", 2, Duration::from_millis(500)).unwrap();
        assert_eq!(r.sent, 2);
        assert_eq!(r.received, 2);
    }
}

#[cfg(test)]
mod trace_tests {
    use super::*;

    #[test]
    fn target_validation() {
        assert!(resolve_target("").is_err());
        assert!(resolve_target("example.com; calc").is_err());
        assert!(resolve_target("a b").is_err());
        assert_eq!(resolve_target(" 1.1.1.1 ").unwrap(), Ipv4Addr::new(1, 1, 1, 1));
        assert_eq!(resolve_target("localhost").unwrap(), Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn trace_to_loopback_reaches_in_one_hop() {
        let seen = std::sync::Mutex::new(0);
        let r = traceroute("127.0.0.1", &|_| *seen.lock().unwrap() += 1).unwrap();
        assert!(r.reached, "{r:?}");
        assert_eq!(r.hops.len(), 1);
        assert_eq!(r.hops[0].address.as_deref(), Some("127.0.0.1"));
        assert_eq!(*seen.lock().unwrap(), 1);
    }

    #[test]
    fn history_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let r = StabilityReport {
            pings: vec![PingResult { target: "1.1.1.1".into(), label: "Cloudflare".into(), sent: 4, received: 3, loss_percent: 25.0, min_ms: Some(9), avg_ms: Some(10.5), max_ms: Some(12), jitter_ms: Some(1.0) }],
            dns: vec![],
            tested_at: crate::db::now(),
        };
        save_report(&db, &r).unwrap();
        let h = history(&db, 10).unwrap();
        assert_eq!(h.len(), 1, "fresh rows survive the retention purge");
        assert_eq!(h[0].avg_ms, Some(10.5));
        assert_eq!(h[0].loss_percent, 25.0);
    }
}
