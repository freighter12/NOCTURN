//! Shared application state managed by Tauri.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::db::Db;
use crate::system::diagnostics::{self, EventEntry, PendingReboot};
use crate::system::health::{self, HealthReport};
use crate::system::info::{self, SystemInfo};
use crate::system::monitor::{SharedTelemetry, TelemetryStore};
use crate::system::power;

struct Cached<T> {
    at: Instant,
    value: T,
}

#[derive(Default)]
struct Caches {
    events: Option<Cached<Vec<EventEntry>>>,
    pending: Option<Cached<PendingReboot>>,
    health: Option<Cached<HealthReport>>,
}

pub struct AppState {
    pub db: Db,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub telemetry: SharedTelemetry,
    sysinfo: RwLock<Option<SystemInfo>>,
    caches: Mutex<Caches>,
    pub ai: crate::ai::AiRuntime,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(db: Db, data_dir: PathBuf, log_dir: PathBuf) -> Self {
        AppState {
            db,
            data_dir,
            log_dir,
            telemetry: Arc::new(RwLock::new(TelemetryStore::default())),
            sysinfo: RwLock::new(None),
            caches: Mutex::new(Caches::default()),
            ai: crate::ai::AiRuntime::default(),
        }
    }

    /// Cached hardware inventory (collected on first use; ~1–2 s).
    pub fn system_info(&self, refresh: bool) -> SystemInfo {
        if !refresh {
            if let Some(i) = self.sysinfo.read().ok().and_then(|g| g.clone()) {
                return i;
            }
        }
        let fresh = info::collect();
        if let Ok(mut g) = self.sysinfo.write() {
            *g = Some(fresh.clone());
        }
        fresh
    }

    pub fn events(&self, max_age: Duration) -> Option<Vec<EventEntry>> {
        if let Some(c) = self.caches.lock().ok().and_then(|c| c.events.as_ref().map(|e| (e.at, e.value.clone()))) {
            if c.0.elapsed() < max_age {
                return Some(c.1);
            }
        }
        let v = diagnostics::recent_errors().ok()?;
        if let Ok(mut c) = self.caches.lock() {
            c.events = Some(Cached { at: Instant::now(), value: v.clone() });
        }
        Some(v)
    }

    pub fn pending_reboot(&self, max_age: Duration) -> Option<PendingReboot> {
        if let Some((at, v)) = self.caches.lock().ok().and_then(|c| c.pending.as_ref().map(|e| (e.at, e.value.clone()))) {
            if at.elapsed() < max_age {
                return Some(v);
            }
        }
        let v = diagnostics::pending_reboot().ok()?;
        if let Ok(mut c) = self.caches.lock() {
            c.pending = Some(Cached { at: Instant::now(), value: v.clone() });
        }
        Some(v)
    }

    pub fn invalidate_health(&self) {
        if let Ok(mut c) = self.caches.lock() {
            c.health = None;
        }
    }

    pub fn health(&self, force: bool) -> HealthReport {
        if !force {
            if let Some((at, v)) = self.caches.lock().ok().and_then(|c| c.health.as_ref().map(|e| (e.at, e.value.clone()))) {
                if at.elapsed() < Duration::from_secs(120) {
                    return v;
                }
            }
        }
        let info = self.system_info(false);
        let (sample, processes) = self
            .telemetry
            .read()
            .map(|t| (t.latest.clone(), t.processes.clone()))
            .unwrap_or_default();
        let startup = crate::startup::list();
        let tweaks = crate::tweaks::engine::list_views();
        let max_age = if force { Duration::ZERO } else { Duration::from_secs(600) };
        let events = self.events(max_age);
        let pending = self.pending_reboot(max_age);
        let power = power::list().ok();
        let report = health::analyse(&health::Inputs {
            info: &info,
            sample: sample.as_ref(),
            processes: &processes,
            startup: &startup,
            tweaks: &tweaks,
            events: events.as_deref(),
            pending: pending.as_ref(),
            power: power.as_deref(),
        });
        if let Ok(mut c) = self.caches.lock() {
            c.health = Some(Cached { at: Instant::now(), value: report.clone() });
        }
        report
    }
}
