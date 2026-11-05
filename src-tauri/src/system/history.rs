//! Per-minute hardware history (7 days) and threshold alerts.
//!
//! The telemetry thread feeds every sample into a `MinuteAcc`; when the
//! minute rolls over, one averaged row is written. That is ~10 000 small rows
//! for a full week — negligible disk activity compared with per-second logs.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rusqlite::params;
use serde::Serialize;

use super::monitor::Sample;
use crate::db::Db;
use crate::error::AppResult;
use crate::settings::MonitorSettings;

const KEEP_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Default)]
pub struct MinuteAcc {
    minute: i64,
    n: u32,
    cpu: f64,
    cpu_max_core: f64,
    mem: f64,
    gpu: f64,
    gpu_n: u32,
    gpu_temp: Option<f32>,
    gpu_power: f64,
    gpu_power_n: u32,
    disk_r: u64,
    disk_w: u64,
    rx: u64,
    tx: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HistoryPoint {
    pub ts: i64,
    pub cpu: f32,
    pub cpu_max_core: f32,
    pub mem_pct: f32,
    pub gpu: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub gpu_power: Option<f32>,
    pub disk_read: u64,
    pub disk_write: u64,
    pub net_rx: u64,
    pub net_tx: u64,
}

impl MinuteAcc {
    /// Add a sample; returns a finished minute when the minute changes.
    pub fn add(&mut self, s: &Sample) -> Option<HistoryPoint> {
        let minute = s.ts / 60_000 * 60_000;
        let done = if self.n > 0 && minute != self.minute { self.finish() } else { None };
        if self.n == 0 || minute != self.minute {
            *self = MinuteAcc { minute, ..Default::default() };
        }
        self.n += 1;
        self.cpu += s.cpu_usage as f64;
        self.cpu_max_core += s.cpu_per_core.iter().cloned().fold(0.0f32, f32::max) as f64;
        self.mem += if s.mem_total > 0 { s.mem_used as f64 / s.mem_total as f64 * 100.0 } else { 0.0 };
        if let Some(g) = s.gpus.iter().filter_map(|g| g.usage).reduce(f32::max) {
            self.gpu += g as f64;
            self.gpu_n += 1;
        }
        if let Some(t) = s.gpus.iter().filter_map(|g| g.temperature_c).reduce(f32::max) {
            self.gpu_temp = Some(self.gpu_temp.map_or(t, |m| m.max(t)));
        }
        if let Some(p) = s.gpus.iter().filter_map(|g| g.power_w).reduce(|a, b| a + b) {
            self.gpu_power += p as f64;
            self.gpu_power_n += 1;
        }
        self.disk_r += s.disk_io.iter().map(|d| d.read_bps).sum::<u64>();
        self.disk_w += s.disk_io.iter().map(|d| d.write_bps).sum::<u64>();
        self.rx += s.net_rx_bps;
        self.tx += s.net_tx_bps;
        done
    }

    fn finish(&self) -> Option<HistoryPoint> {
        if self.n == 0 {
            return None;
        }
        let n = self.n as f64;
        Some(HistoryPoint {
            ts: self.minute,
            cpu: (self.cpu / n) as f32,
            cpu_max_core: (self.cpu_max_core / n) as f32,
            mem_pct: (self.mem / n) as f32,
            gpu: (self.gpu_n > 0).then(|| (self.gpu / self.gpu_n as f64) as f32),
            gpu_temp: self.gpu_temp,
            gpu_power: (self.gpu_power_n > 0).then(|| (self.gpu_power / self.gpu_power_n as f64) as f32),
            disk_read: self.disk_r / self.n as u64,
            disk_write: self.disk_w / self.n as u64,
            net_rx: self.rx / self.n as u64,
            net_tx: self.tx / self.n as u64,
        })
    }
}

pub fn store(db: &Db, p: &HistoryPoint) -> AppResult<()> {
    db.conn().execute(
        "INSERT OR REPLACE INTO telemetry_minute(ts, cpu, cpu_max_core, mem_pct, gpu, gpu_temp, gpu_power, disk_read, disk_write, net_rx, net_tx)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![p.ts, p.cpu, p.cpu_max_core, p.mem_pct, p.gpu, p.gpu_temp, p.gpu_power, p.disk_read as i64, p.disk_write as i64, p.net_rx as i64, p.net_tx as i64],
    )?;
    Ok(())
}

pub fn prune(db: &Db, now_ms: i64) -> AppResult<usize> {
    Ok(db.conn().execute("DELETE FROM telemetry_minute WHERE ts < ?1", [now_ms - KEEP_MS])?)
}

/// History for the last `minutes`, averaged into at most `max_points` buckets.
pub fn range(db: &Db, now_ms: i64, minutes: i64, max_points: usize) -> AppResult<Vec<HistoryPoint>> {
    let since = now_ms - minutes.clamp(1, 7 * 24 * 60) * 60_000;
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT ts, cpu, cpu_max_core, mem_pct, gpu, gpu_temp, gpu_power, disk_read, disk_write, net_rx, net_tx
         FROM telemetry_minute WHERE ts >= ?1 ORDER BY ts",
    )?;
    let rows: Vec<HistoryPoint> = st
        .query_map([since], |r| {
            Ok(HistoryPoint {
                ts: r.get(0)?,
                cpu: r.get(1)?,
                cpu_max_core: r.get(2)?,
                mem_pct: r.get(3)?,
                gpu: r.get(4)?,
                gpu_temp: r.get(5)?,
                gpu_power: r.get(6)?,
                disk_read: r.get::<_, i64>(7)? as u64,
                disk_write: r.get::<_, i64>(8)? as u64,
                net_rx: r.get::<_, i64>(9)? as u64,
                net_tx: r.get::<_, i64>(10)? as u64,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(downsample(rows, max_points.max(10)))
}

/// Average consecutive points into buckets; temperatures keep their maximum.
pub fn downsample(rows: Vec<HistoryPoint>, max: usize) -> Vec<HistoryPoint> {
    if rows.len() <= max {
        return rows;
    }
    let per = rows.len().div_ceil(max);
    rows.chunks(per)
        .map(|c| {
            let n = c.len() as f32;
            let avg = |f: fn(&HistoryPoint) -> f32| c.iter().map(f).sum::<f32>() / n;
            let avg_opt = |f: fn(&HistoryPoint) -> Option<f32>| {
                let v: Vec<f32> = c.iter().filter_map(f).collect();
                (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
            };
            let avg_u = |f: fn(&HistoryPoint) -> u64| c.iter().map(f).sum::<u64>() / c.len() as u64;
            HistoryPoint {
                ts: c[0].ts,
                cpu: avg(|p| p.cpu),
                cpu_max_core: avg(|p| p.cpu_max_core),
                mem_pct: avg(|p| p.mem_pct),
                gpu: avg_opt(|p| p.gpu),
                gpu_temp: c.iter().filter_map(|p| p.gpu_temp).reduce(f32::max),
                gpu_power: avg_opt(|p| p.gpu_power),
                disk_read: avg_u(|p| p.disk_read),
                disk_write: avg_u(|p| p.disk_write),
                net_rx: avg_u(|p| p.net_rx),
                net_tx: avg_u(|p| p.net_tx),
            }
        })
        .collect()
}

// ------------------------------------------------------------------ alerts

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Alert {
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
}

const SUSTAIN: Duration = Duration::from_secs(15);
const COOLDOWN: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub struct Alerts {
    since: HashMap<&'static str, Instant>,
    fired: HashMap<&'static str, Instant>,
}

impl Alerts {
    /// Fire an alert once a reading has stayed over its threshold for 15 s,
    /// then stay quiet for 15 minutes for that kind.
    pub fn check(&mut self, s: &Sample, cfg: &MonitorSettings, now: Instant) -> Vec<Alert> {
        if !cfg.alerts {
            self.since.clear();
            return vec![];
        }
        let mem_pct = if s.mem_total > 0 { s.mem_used as f32 / s.mem_total as f32 * 100.0 } else { 0.0 };
        let gpu_temp = s.gpus.iter().filter_map(|g| g.temperature_c).reduce(f32::max);
        let checks: [(&'static str, bool, Alert); 3] = [
            ("cpu", s.cpu_usage >= cfg.cpu_alert_pct, Alert {
                kind: "cpu",
                title: format!("CPU at {:.0}% for over 15 seconds", s.cpu_usage),
                detail: "Check Processes to see what's using it.".into(),
            }),
            ("ram", mem_pct >= cfg.ram_alert_pct, Alert {
                kind: "ram",
                title: format!("Memory {:.0}% full", mem_pct),
                detail: "Windows may start paging to disk, which causes stutter.".into(),
            }),
            ("gpu_temp", gpu_temp.is_some_and(|t| t >= cfg.gpu_temp_alert_c), Alert {
                kind: "gpu_temp",
                title: format!("GPU at {:.0} °C", gpu_temp.unwrap_or(0.0)),
                detail: format!("Above your {:.0} °C warning level. Check case airflow and fan curves.", cfg.gpu_temp_alert_c),
            }),
        ];
        let mut out = Vec::new();
        for (kind, over, alert) in checks {
            if !over {
                self.since.remove(kind);
                continue;
            }
            let start = *self.since.entry(kind).or_insert(now);
            let cooled = self.fired.get(kind).is_none_or(|t| now.duration_since(*t) >= COOLDOWN);
            if now.duration_since(start) >= SUSTAIN && cooled {
                self.fired.insert(kind, now);
                out.push(alert);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::monitor::GpuSample;

    fn sample(ts: i64, cpu: f32, mem_pct: f32, gpu_temp: Option<f32>) -> Sample {
        Sample {
            ts,
            cpu_usage: cpu,
            cpu_per_core: vec![cpu, cpu * 2.0],
            mem_total: 1000,
            mem_used: (mem_pct * 10.0) as u64,
            gpus: vec![GpuSample { name: "g".into(), usage: Some(50.0), temperature_c: gpu_temp, ..Default::default() }],
            ..Default::default()
        }
    }

    #[test]
    fn minute_rollover_averages() {
        let mut acc = MinuteAcc::default();
        assert!(acc.add(&sample(60_000, 10.0, 50.0, Some(60.0))).is_none());
        assert!(acc.add(&sample(61_000, 30.0, 50.0, Some(70.0))).is_none());
        let p = acc.add(&sample(120_000, 90.0, 50.0, None)).expect("minute finished");
        assert_eq!(p.ts, 60_000);
        assert_eq!(p.cpu, 20.0);
        assert_eq!(p.cpu_max_core, 40.0);
        assert_eq!(p.gpu_temp, Some(70.0), "temperature keeps the peak");
        assert_eq!(p.gpu, Some(50.0));
    }

    #[test]
    fn store_range_prune_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let now = 10 * 24 * 60 * 60 * 1000i64;
        let mut acc = MinuteAcc::default();
        for m in 0..30i64 {
            if let Some(p) = acc.add(&sample(now - (30 - m) * 60_000, m as f32, 40.0, None)) {
                store(&db, &p).unwrap();
            }
        }
        store(&db, &HistoryPoint { ts: now - 9 * 24 * 60 * 60 * 1000, cpu: 1.0, cpu_max_core: 1.0, mem_pct: 1.0, gpu: None, gpu_temp: None, gpu_power: None, disk_read: 0, disk_write: 0, net_rx: 0, net_tx: 0 }).unwrap();
        assert_eq!(range(&db, now, 60, 600).unwrap().len(), 29);
        assert_eq!(range(&db, now, 60, 10).unwrap().len(), 10);
        assert_eq!(prune(&db, now).unwrap(), 1, "only the 9-day-old row is pruned");
    }

    #[test]
    fn alerts_need_sustained_readings_and_cool_down() {
        let cfg = MonitorSettings::default();
        let mut a = Alerts::default();
        let t0 = Instant::now();
        let hot = sample(0, 99.0, 50.0, Some(90.0));
        assert!(a.check(&hot, &cfg, t0).is_empty(), "not yet sustained");
        let fired = a.check(&hot, &cfg, t0 + Duration::from_secs(16));
        let kinds: Vec<_> = fired.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec!["cpu", "gpu_temp"]);
        assert!(a.check(&hot, &cfg, t0 + Duration::from_secs(40)).is_empty(), "cooldown");
        // A dip resets the streak.
        a.check(&sample(0, 10.0, 50.0, Some(40.0)), &cfg, t0 + Duration::from_secs(41));
        let off = MonitorSettings { alerts: false, ..cfg };
        assert!(a.check(&hot, &off, t0 + Duration::from_secs(2000)).is_empty());
    }
}
