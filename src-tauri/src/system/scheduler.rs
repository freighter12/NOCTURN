//! Scheduled maintenance: a short, fixed list of tasks NOCTURN runs on a
//! schedule while it's open. Tasks are deferred while a game or a benchmark is
//! running or the CPU is busy, and every run is recorded.
//!
//! Only temp-file cleanup changes anything (files older than 24 h in %TEMP%),
//! and it is off until the user turns it on. The rest are read-only checks
//! that raise an alert when something new needs attention.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::state::Shared;
use crate::system::history::Alert;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskId {
    CrashCheck,
    DriverCheck,
    DiskSpace,
    TempCleanup,
    NetworkCheck,
}

impl TaskId {
    pub const ALL: [TaskId; 5] = [TaskId::CrashCheck, TaskId::DriverCheck, TaskId::DiskSpace, TaskId::TempCleanup, TaskId::NetworkCheck];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskId::CrashCheck => "crash_check",
            TaskId::DriverCheck => "driver_check",
            TaskId::DiskSpace => "disk_space",
            TaskId::TempCleanup => "temp_cleanup",
            TaskId::NetworkCheck => "network_check",
        }
    }

    fn parse(s: &str) -> Option<TaskId> {
        TaskId::ALL.into_iter().find(|t| t.as_str() == s)
    }

    pub fn info(self) -> TaskInfo {
        let (title, description, changes_system, default_on, default_days) = match self {
            TaskId::CrashCheck => ("Check for new crashes", "Looks for new blue screens, unexpected shutdowns and hardware errors, and alerts you if there are any.", false, true, 1),
            TaskId::DriverCheck => ("Check for device problems", "Alerts you when a device starts reporting a problem in Device Manager.", false, true, 7),
            TaskId::DiskSpace => ("Check free disk space", "Alerts you when a drive drops below 10% free (or 10 GB on large drives).", false, true, 1),
            TaskId::TempCleanup => ("Clean temporary files", "Deletes files older than 24 hours from your temporary folder. Files in use are skipped; personal folders are never touched. Deleted files can't be restored.", true, false, 7),
            TaskId::NetworkCheck => ("Record a network stability test", "Pings your router and public DNS servers (Cloudflare, Google) so the Network page can show how your connection changes over time.", false, false, 7),
        };
        TaskInfo { id: self, title, description, changes_system, default_on, default_days }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    pub id: TaskId,
    pub title: &'static str,
    pub description: &'static str,
    /// True only for tasks that modify the PC.
    pub changes_system: bool,
    pub default_on: bool,
    pub default_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskConfig {
    pub enabled: bool,
    pub interval_days: u32,
}

const CONFIG_KEY: &str = "maintenance.tasks";
const STATE_KEY: &str = "maintenance.state";
const INTERVALS: [u32; 4] = [1, 7, 14, 30];

pub fn config(db: &Db) -> HashMap<TaskId, TaskConfig> {
    let saved: HashMap<String, TaskConfig> = db.get_setting(CONFIG_KEY).ok().flatten().unwrap_or_default();
    TaskId::ALL
        .into_iter()
        .map(|t| {
            let i = t.info();
            (t, saved.get(t.as_str()).cloned().unwrap_or(TaskConfig { enabled: i.default_on, interval_days: i.default_days }))
        })
        .collect()
}

pub fn set_config(db: &Db, id: TaskId, cfg: TaskConfig) -> AppResult<()> {
    if !INTERVALS.contains(&cfg.interval_days) {
        return Err(AppError::Invalid("interval must be 1, 7, 14 or 30 days".into()));
    }
    let mut saved: HashMap<String, TaskConfig> = db.get_setting(CONFIG_KEY).ok().flatten().unwrap_or_default();
    saved.insert(id.as_str().into(), cfg.clone());
    db.set_setting(CONFIG_KEY, &saved)?;
    db.audit("info", "user", "maintenance.config", Some(serde_json::json!({ "task": id.as_str(), "enabled": cfg.enabled, "days": cfg.interval_days })));
    Ok(())
}

/// What each check saw last time, so only *new* problems raise alerts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Seen {
    last_system_crash: Option<String>,
    app_crashes: usize,
    /// `None` until the first check: existing problems are a baseline, not news.
    problem_devices: Option<Vec<String>>,
    low_drives: Vec<String>,
}

// ------------------------------------------------------------------ runs

#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub id: i64,
    pub task: TaskId,
    pub started_at: String,
    pub finished_at: String,
    /// ok, attention, failed, deferred
    pub status: String,
    /// scheduled or manual
    pub trigger: String,
    pub summary: String,
}

fn record(db: &Db, task: TaskId, started: &str, status: &str, trigger: &str, summary: &str) -> AppResult<i64> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO maintenance_runs(task, started_at, finished_at, status, trigger, summary) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![task.as_str(), started, now(), status, trigger, summary],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute("DELETE FROM maintenance_runs WHERE started_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-365 days')", [])?;
    Ok(id)
}

pub fn history(db: &Db, limit: i64) -> AppResult<Vec<RunRecord>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, task, started_at, finished_at, status, trigger, summary FROM maintenance_runs ORDER BY id DESC LIMIT ?1")?;
    let rows = st
        .query_map([limit], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, t, started_at, finished_at, status, trigger, summary)| {
            Some(RunRecord { id, task: TaskId::parse(&t)?, started_at, finished_at, status, trigger, summary })
        })
        .collect())
}

/// Last time the task actually ran (deferrals don't count).
fn last_run(db: &Db, task: TaskId) -> Option<DateTime<Utc>> {
    let conn = db.conn();
    conn.query_row(
        "SELECT started_at FROM maintenance_runs WHERE task = ?1 AND status != 'deferred' ORDER BY id DESC LIMIT 1",
        [task.as_str()],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
    .map(|d| d.with_timezone(&Utc))
}

/// When the task was last deferred, if its most recent entry is a deferral.
fn last_deferral(db: &Db, task: TaskId) -> Option<DateTime<Utc>> {
    let conn = db.conn();
    let (at, status): (String, String) = conn
        .query_row("SELECT started_at, status FROM maintenance_runs WHERE task = ?1 ORDER BY id DESC LIMIT 1", [task.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok()?;
    (status == "deferred").then_some(())?;
    DateTime::parse_from_rfc3339(&at).ok().map(|d| d.with_timezone(&Utc))
}

pub fn is_due(last: Option<DateTime<Utc>>, interval_days: u32, now: DateTime<Utc>) -> bool {
    match last {
        None => true,
        Some(t) => now - t >= chrono::Duration::days(interval_days as i64),
    }
}

/// Why a scheduled run should wait, if it should.
pub fn defer_reason(game: Option<&str>, benchmark: bool, cpu_avg: Option<f32>) -> Option<String> {
    if let Some(g) = game {
        return Some(format!("{g} was running"));
    }
    if benchmark {
        return Some("a benchmark was running".into());
    }
    if let Some(c) = cpu_avg.filter(|c| *c > 70.0) {
        return Some(format!("the CPU was busy ({c:.0}%)"));
    }
    None
}

// ----------------------------------------------------------------- tasks

struct Outcome {
    status: &'static str,
    summary: String,
    alert: Option<Alert>,
}

fn ok(summary: String) -> Outcome {
    Outcome { status: "ok", summary, alert: None }
}

fn run_task(st: &Shared, task: TaskId) -> AppResult<Outcome> {
    let mut seen: Seen = st.db.get_setting(STATE_KEY).ok().flatten().unwrap_or_default();
    let out = match task {
        TaskId::CrashCheck => {
            let r = crate::system::crash::report()?;
            let latest = r.last_system_crash.map(|t| t.to_rfc3339());
            let new_system = latest.is_some() && latest != seen.last_system_crash && seen.last_system_crash.is_some();
            let first_run = seen.last_system_crash.is_none() && seen.app_crashes == 0;
            let new_apps = r.app_crashes.saturating_sub(seen.app_crashes);
            seen.last_system_crash = latest.or(seen.last_system_crash.clone());
            seen.app_crashes = r.app_crashes;
            let summary = format!(
                "{} blue screen(s), {} unexpected shutdown(s), {} app crash(es) in {} days",
                r.blue_screens + r.hardware_errors,
                r.unexpected_shutdowns,
                r.app_crashes,
                r.window_days
            );
            if new_system && !first_run {
                Outcome {
                    status: "attention",
                    summary: format!("New system crash found. {summary}"),
                    alert: Some(Alert { kind: "maintenance.crash", title: "New crash recorded".into(), detail: "Windows recorded a new blue screen or unexpected shutdown. See System Diagnostics › Crashes for what it points at.".into() }),
                }
            } else if new_apps >= 3 && !first_run {
                Outcome { status: "attention", summary: format!("{new_apps} new app crashes. {summary}"), alert: None }
            } else {
                ok(summary)
            }
        }
        TaskId::DriverCheck => {
            let info = st.system_info(false);
            let r = crate::system::driver_centre::report(
                info.board.board_manufacturer.as_deref().unwrap_or(""),
                info.board.system_manufacturer.as_deref().unwrap_or(""),
            )?;
            let ids: Vec<String> = r.problems.iter().filter(|p| p.severity != "info").map(|p| p.id.clone()).collect();
            let first = seen.problem_devices.is_none();
            let known = seen.problem_devices.clone().unwrap_or_default();
            let fresh: Vec<&crate::system::driver_centre::ProblemDevice> = r.problems.iter().filter(|p| p.severity != "info" && !known.contains(&p.id)).collect();
            let summary = if r.problems.is_empty() { "Every connected device is working".into() } else { format!("{} device(s) with problems", r.problems.len()) };
            let o = if fresh.is_empty() {
                ok(summary)
            } else if first {
                Outcome { status: "attention", summary, alert: None }
            } else {
                Outcome {
                    status: "attention",
                    summary: format!("New problem: {}. {summary}", fresh.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")),
                    alert: Some(Alert { kind: "maintenance.driver", title: format!("Device problem: {}", fresh[0].name), detail: format!("{} See System Diagnostics › Drivers.", fresh[0].meaning) }),
                }
            };
            seen.problem_devices = Some(ids);
            o
        }
        TaskId::DiskSpace => {
            let disks = sysinfo::Disks::new_with_refreshed_list().map_err(|e| AppError::msg(e.to_string()))?;
            let mut low = Vec::new();
            let mut parts = Vec::new();
            for d in disks.iter().filter(|d| !d.is_removable() && d.total_space() > 0) {
                let free = d.available_space();
                let total = d.total_space();
                let mount = d.mount_point().display().to_string();
                parts.push(format!("{mount} {} free", crate::util::fmt_bytes(free)));
                let threshold = (total / 10).min(10 << 30);
                if free < threshold {
                    low.push(mount);
                }
            }
            let fresh: Vec<String> = low.iter().filter(|m| !seen.low_drives.contains(m)).cloned().collect();
            seen.low_drives = low.clone();
            if low.is_empty() {
                ok(parts.join(" · "))
            } else {
                Outcome {
                    status: "attention",
                    summary: format!("Low space on {}. {}", low.join(", "), parts.join(" · ")),
                    alert: (!fresh.is_empty()).then(|| Alert { kind: "maintenance.disk", title: format!("Low disk space on {}", fresh.join(", ")), detail: "The Storage page shows what's using the space.".into() }),
                }
            }
        }
        TaskId::TempCleanup => {
            let r = crate::tweaks::maintenance::clean_temp()?;
            ok(format!("Freed {} ({} files deleted, {} in use skipped)", crate::util::fmt_bytes(r.freed_bytes), r.deleted_files, r.skipped_in_use))
        }
        TaskId::NetworkCheck => {
            let r = crate::system::network::stability_test(10);
            crate::system::network::save_report(&st.db, &r)?;
            let parts: Vec<String> = r.pings.iter().map(|p| format!("{} {}", p.label, p.avg_ms.map(|a| format!("{a:.0} ms")).unwrap_or_else(|| "no reply".into()))).collect();
            ok(parts.join(" · "))
        }
    };
    st.db.set_setting(STATE_KEY, &seen)?;
    Ok(out)
}

/// Only one task runs at a time (manual or scheduled).
static RUNNING: Mutex<()> = Mutex::new(());

pub fn run_now(app: Option<&AppHandle>, st: &Shared, task: TaskId, trigger: &str) -> AppResult<RunRecord> {
    let _g = RUNNING.try_lock().map_err(|_| AppError::msg("another maintenance task is running"))?;
    let started = now();
    let (status, summary, alert) = match run_task(st, task) {
        Ok(o) => (o.status, o.summary, o.alert),
        Err(e) => ("failed", e.to_string(), None),
    };
    let id = record(&st.db, task, &started, status, trigger, &summary)?;
    if let (Some(app), Some(a)) = (app, alert) {
        let _ = app.emit("alert", &a);
    }
    st.db.audit(if status == "failed" { "warn" } else { "info" }, if trigger == "manual" { "user" } else { "system" }, "maintenance.run", Some(serde_json::json!({ "task": task.as_str(), "status": status })));
    Ok(RunRecord { id, task, started_at: started, finished_at: now(), status: status.into(), trigger: trigger.into(), summary })
}

// ---------------------------------------------------------------- overview

#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub info: TaskInfo,
    pub enabled: bool,
    pub interval_days: u32,
    pub last_run: Option<String>,
    pub next_due: Option<String>,
}

pub fn overview(db: &Db) -> Vec<TaskView> {
    let cfg = config(db);
    TaskId::ALL
        .into_iter()
        .map(|t| {
            let c = cfg.get(&t).cloned().unwrap_or(TaskConfig { enabled: false, interval_days: 7 });
            let last = last_run(db, t);
            let next = c.enabled.then(|| last.map(|l| l + chrono::Duration::days(c.interval_days as i64)).unwrap_or_else(Utc::now));
            TaskView { info: t.info(), enabled: c.enabled, interval_days: c.interval_days, last_run: last.map(|l| l.to_rfc3339()), next_due: next.map(|n| n.to_rfc3339()) }
        })
        .collect()
}

pub fn parse_task(s: &str) -> AppResult<TaskId> {
    TaskId::parse(s).ok_or_else(|| AppError::Invalid(format!("unknown maintenance task '{s}'")))
}

// --------------------------------------------------------------- scheduler

/// Check for due tasks every minute, starting two minutes after launch.
pub fn start(app: AppHandle, st: Shared) {
    std::thread::Builder::new()
        .name("maintenance".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(120));
            loop {
                tick(Some(&app), &st);
                std::thread::sleep(Duration::from_secs(60));
            }
        })
        .ok();
}

fn tick(app: Option<&AppHandle>, st: &Shared) {
    let cfg = config(&st.db);
    let nowt = Utc::now();
    for task in TaskId::ALL {
        let Some(c) = cfg.get(&task).filter(|c| c.enabled) else { continue };
        if !is_due(last_run(&st.db, task), c.interval_days, nowt) {
            continue;
        }
        let game = st.games.live.lock().ok().and_then(|l| l.as_ref().map(|l| l.summary().game_name));
        let cpu = st.telemetry.read().ok().and_then(|t| {
            let recent: Vec<f32> = t.history.iter().rev().take(30).map(|s| s.cpu_usage).collect();
            (!recent.is_empty()).then(|| recent.iter().sum::<f32>() / recent.len() as f32)
        });
        if let Some(why) = defer_reason(game.as_deref(), crate::system::bench::running(), cpu) {
            // Log a deferral at most once an hour per task.
            if last_deferral(&st.db, task).is_none_or(|t| nowt - t > chrono::Duration::hours(1)) {
                let _ = record(&st.db, task, &now(), "deferred", "scheduled", &format!("Waiting: {why}."));
            }
            return; // try again next minute
        }
        if let Err(e) = run_now(app, st, task, "scheduled") {
            tracing::warn!(task = task.as_str(), "maintenance task failed to run: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_and_deferral_rules() {
        let n = Utc::now();
        assert!(is_due(None, 7, n));
        assert!(!is_due(Some(n - chrono::Duration::days(3)), 7, n));
        assert!(is_due(Some(n - chrono::Duration::days(7)), 7, n));
        assert!(defer_reason(Some("Cyberpunk 2077"), false, Some(10.0)).unwrap().contains("Cyberpunk"));
        assert!(defer_reason(None, true, None).is_some());
        assert!(defer_reason(None, false, Some(85.0)).unwrap().contains("85%"));
        assert!(defer_reason(None, false, Some(20.0)).is_none());
    }

    #[test]
    fn defaults_and_config() {
        let db = Db::open_in_memory().unwrap();
        let c = config(&db);
        assert!(!c[&TaskId::TempCleanup].enabled, "deleting files is opt-in");
        assert!(!c[&TaskId::NetworkCheck].enabled);
        assert!(c[&TaskId::CrashCheck].enabled);
        assert!(set_config(&db, TaskId::TempCleanup, TaskConfig { enabled: true, interval_days: 3 }).is_err());
        set_config(&db, TaskId::TempCleanup, TaskConfig { enabled: true, interval_days: 14 }).unwrap();
        assert_eq!(config(&db)[&TaskId::TempCleanup], TaskConfig { enabled: true, interval_days: 14 });
        // Only the cleanup task changes anything.
        assert_eq!(TaskId::ALL.iter().filter(|t| t.info().changes_system).count(), 1);
    }

    #[test]
    fn tick_runs_due_enabled_tasks_only() {
        let dir = tempfile::tempdir().unwrap();
        let st: Shared = std::sync::Arc::new(crate::state::AppState::new(Db::open_in_memory().unwrap(), dir.path().into(), dir.path().into()));
        for t in TaskId::ALL {
            set_config(&st.db, t, TaskConfig { enabled: t == TaskId::DiskSpace, interval_days: 1 }).unwrap();
        }
        tick(None, &st);
        let h = history(&st.db, 10).unwrap();
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!((h[0].task, h[0].trigger.as_str()), (TaskId::DiskSpace, "scheduled"));
        assert_ne!(h[0].status, "failed", "{}", h[0].summary);
        tick(None, &st);
        assert_eq!(history(&st.db, 10).unwrap().len(), 1, "not due again for a day");
    }

    #[test]
    fn history_and_last_run_ignore_deferrals() {
        let db = Db::open_in_memory().unwrap();
        record(&db, TaskId::DiskSpace, &now(), "deferred", "scheduled", "Waiting").unwrap();
        assert!(last_run(&db, TaskId::DiskSpace).is_none());
        assert!(last_deferral(&db, TaskId::DiskSpace).is_some());
        record(&db, TaskId::DiskSpace, &now(), "ok", "manual", "C:\\ 20 GB free").unwrap();
        assert!(last_run(&db, TaskId::DiskSpace).is_some());
        assert!(last_deferral(&db, TaskId::DiskSpace).is_none());
        assert_eq!(history(&db, 10).unwrap().len(), 2);
        assert!(parse_task("rm_rf").is_err());
    }
}
