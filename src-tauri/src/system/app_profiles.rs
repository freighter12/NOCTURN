//! App profiles: "while Blender.exe is running, use the High performance
//! power plan, run it at Above normal priority and hold scheduled
//! maintenance". NOCTURN switches when the app starts and switches back when
//! it closes.
//!
//! * Power-plan switches go through the journal (source `auto`), so each one
//!   appears in History and the switch back is a journal rollback.
//! * If the user changes the power plan by hand while the app runs, NOCTURN
//!   doesn't switch it back.
//! * Priority uses the existing launch-rule engine; a profile never replaces
//!   a rule the user (or a game profile) made.
//! * The active switch is persisted, so a plan switched before NOCTURN closed
//!   is restored at the next start if the app is no longer running.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::state::Shared;
use crate::system::history::Alert;
use crate::system::process_control::Priority;
use crate::system::rules::{self, ProcessRule};
use crate::tweaks::{engine, ChangeOp, Takes};

const KEY: &str = "app_profiles";
const ACTIVE_KEY: &str = "app_profiles.active";
pub const RULE_NOTE_PREFIX: &str = "App profile: ";
/// How long the app must be gone before switching back (relaunches, updaters).
const GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppProfile {
    /// Program file name, e.g. `Blender.exe`.
    pub exe: String,
    pub label: String,
    /// Power plan GUID to use while it runs; `None` = leave the plan alone.
    pub power_plan: Option<String>,
    pub priority: Option<Priority>,
    pub pause_maintenance: bool,
    pub enabled: bool,
    #[serde(default)]
    pub updated_at: String,
}

/// The switch NOCTURN made for the running profile app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Active {
    pub exe: String,
    pub label: String,
    /// Journal transaction of the power-plan switch, if one was made.
    pub tx_id: Option<String>,
    /// The plan NOCTURN switched to (to detect manual changes).
    pub our_plan: Option<String>,
    pub since: String,
    pub pause_maintenance: bool,
}

#[derive(Default)]
pub struct ProfileState {
    absent_since: Mutex<Option<Instant>>,
    busy: Mutex<()>,
}

pub fn list(db: &Db) -> Vec<AppProfile> {
    db.get_setting(KEY).ok().flatten().unwrap_or_default()
}

pub fn active(db: &Db) -> Option<Active> {
    db.get_setting(ACTIVE_KEY).ok().flatten()
}

fn set_active(db: &Db, a: Option<&Active>) {
    let _ = db.set_setting(ACTIVE_KEY, &a);
}

/// True while a running profile app asked to hold scheduled maintenance.
pub fn pausing_maintenance(db: &Db) -> Option<String> {
    active(db).filter(|a| a.pause_maintenance).map(|a| a.label)
}

fn validate(p: &AppProfile) -> AppResult<()> {
    let n = p.exe.trim();
    if n.is_empty() || n.len() > 260 || n.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) || !n.to_ascii_lowercase().ends_with(".exe") {
        return Err(AppError::Invalid("use the program's file name, e.g. Blender.exe".into()));
    }
    if crate::system::processes::is_protected(n) {
        return Err(AppError::Invalid(format!("{n} is a core Windows process")));
    }
    if p.label.trim().is_empty() || p.label.len() > 80 {
        return Err(AppError::Invalid("give the profile a name (up to 80 characters)".into()));
    }
    if let Some(g) = &p.power_plan {
        crate::security::exec::SchemeGuid::parse(g)?;
    }
    if p.power_plan.is_none() && p.priority.is_none() && !p.pause_maintenance {
        return Err(AppError::Invalid("choose at least one thing for the profile to do".into()));
    }
    Ok(())
}

fn profile_rule(r: &ProcessRule) -> bool {
    r.note.starts_with(RULE_NOTE_PREFIX)
}

pub fn save(db: &Db, cache: &rules::RuleCache, mut p: AppProfile) -> AppResult<AppProfile> {
    p.exe = p.exe.trim().to_string();
    p.label = p.label.trim().to_string();
    validate(&p)?;
    if let Some(g) = &p.power_plan {
        if !crate::system::power::list()?.iter().any(|s| s.guid.eq_ignore_ascii_case(g)) {
            return Err(AppError::NotFound("that power plan isn't installed".into()));
        }
    }
    // Priority lives in a launch rule owned by this profile.
    let existing = rules::get(db, &p.exe)?;
    if let Some(r) = existing.as_ref().filter(|r| !profile_rule(r)) {
        if p.priority.is_some() {
            return Err(AppError::Invalid(format!(
                "{} already has a launch rule ({}). Edit it on the Processes page, or leave priority unset here.",
                p.exe,
                if r.note.is_empty() { "made by you".to_string() } else { r.note.clone() }
            )));
        }
    }
    match (p.priority, existing.as_ref().is_some_and(profile_rule)) {
        (Some(pri), _) => {
            rules::save(
                db,
                cache,
                ProcessRule {
                    name: p.exe.clone(),
                    priority: Some(pri),
                    affinity: None,
                    enabled: p.enabled,
                    note: format!("{RULE_NOTE_PREFIX}{}", p.label),
                    created_at: String::new(),
                    updated_at: String::new(),
                    applied_count: 0,
                    last_applied_at: None,
                    last_error: None,
                },
            )?;
        }
        (None, true) => rules::delete(db, cache, &p.exe)?,
        (None, false) => {}
    }
    p.updated_at = now();
    let mut all = list(db);
    all.retain(|x| !x.exe.eq_ignore_ascii_case(&p.exe));
    all.push(p.clone());
    all.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    db.set_setting(KEY, &all)?;
    db.audit("info", "user", "app_profile.saved", Some(serde_json::json!({ "exe": p.exe, "plan": p.power_plan, "priority": p.priority, "pause": p.pause_maintenance, "enabled": p.enabled })));
    Ok(p)
}

pub fn delete(db: &Db, cache: &rules::RuleCache, exe: &str) -> AppResult<()> {
    let mut all = list(db);
    let before = all.len();
    all.retain(|x| !x.exe.eq_ignore_ascii_case(exe));
    if all.len() == before {
        return Err(AppError::NotFound(format!("no profile for {exe}")));
    }
    db.set_setting(KEY, &all)?;
    if rules::get(db, exe)?.as_ref().is_some_and(profile_rule) {
        rules::delete(db, cache, exe)?;
    }
    db.audit("info", "user", "app_profile.deleted", Some(serde_json::json!({ "exe": exe })));
    Ok(())
}

// ----------------------------------------------------------------- engine

/// What the watcher should do, given what's running. Pure, for tests.
#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    Activate(AppProfile),
    /// The active app has been gone for the grace period.
    Deactivate,
    /// The app is gone but the grace period hasn't passed.
    Wait,
}

pub fn decide(profiles: &[AppProfile], running: &[String], active: Option<&Active>, gone_for: Option<Duration>) -> Step {
    let is_running = |exe: &str| running.iter().any(|r| r.eq_ignore_ascii_case(exe));
    match active {
        Some(a) if is_running(&a.exe) => Step::Nothing,
        Some(_) => match gone_for {
            Some(d) if d >= GRACE => Step::Deactivate,
            _ => Step::Wait,
        },
        None => profiles.iter().find(|p| p.enabled && is_running(&p.exe)).cloned().map(Step::Activate).unwrap_or(Step::Nothing),
    }
}

fn notify(app: Option<&AppHandle>, kind: &'static str, title: String, detail: String) {
    if let Some(app) = app {
        let _ = app.emit("alert", &Alert { kind, title, detail });
    }
}

fn activate(app: Option<&AppHandle>, st: &Shared, p: &AppProfile) {
    let mut tx_id = None;
    let mut note = String::new();
    if let Some(plan) = &p.power_plan {
        let current = crate::system::power::active_guid().ok();
        if current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(plan)) {
            note = " The power plan was already set.".into();
        } else {
            let name = crate::system::power::list().ok().and_then(|l| l.into_iter().find(|s| s.guid.eq_ignore_ascii_case(plan)).map(|s| s.name)).unwrap_or_else(|| plan.clone());
            let op = engine::PlannedOp { tweak_id: "app_profile".into(), title: format!("Power plan for {}: {name}", p.label), takes: Takes::Immediately, op: ChangeOp::PowerPlan { scheme: plan.to_lowercase() } };
            match engine::run_plan(&st.db, &st.data_dir, vec![op], "auto", &format!("App profile: {} started", p.label), false, &|_| {}) {
                Ok(r) if r.status == "completed" => {
                    tx_id = Some(r.tx_id);
                    note = format!(" Power plan: {name}.");
                }
                Ok(r) => note = format!(" The power plan couldn't be switched ({}).", r.results.first().and_then(|x| x.error.clone()).unwrap_or(r.status)),
                Err(e) => note = format!(" The power plan couldn't be switched ({e})."),
            }
        }
    }
    let a = Active { exe: p.exe.clone(), label: p.label.clone(), tx_id, our_plan: p.power_plan.clone(), since: now(), pause_maintenance: p.pause_maintenance };
    set_active(&st.db, Some(&a));
    st.db.audit("info", "system", "app_profile.on", Some(serde_json::json!({ "exe": p.exe, "tx": a.tx_id })));
    notify(app, "profile.on", format!("{} profile on", p.label), format!("{} started.{note}", p.exe));
}

fn deactivate(app: Option<&AppHandle>, st: &Shared, a: &Active) {
    let mut note = String::new();
    if let (Some(tx), Some(ours)) = (&a.tx_id, &a.our_plan) {
        let current = crate::system::power::active_guid().ok();
        if current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(ours)) {
            match engine::rollback(&st.db, tx, &|_| {}) {
                Ok(_) => note = " The previous power plan is back.".into(),
                Err(e) => note = format!(" The previous power plan couldn't be restored ({e}); change it on the Power plans card."),
            }
        } else {
            note = " You changed the power plan meanwhile, so NOCTURN left it as it is.".into();
        }
    }
    set_active(&st.db, None);
    st.db.audit("info", "system", "app_profile.off", Some(serde_json::json!({ "exe": a.exe })));
    notify(app, "profile.off", format!("{} profile off", a.label), format!("{} closed.{note}", a.exe));
}

pub fn tick(app: Option<&AppHandle>, st: &Shared) {
    let Ok(_g) = st.app_profiles.busy.try_lock() else { return };
    let profiles = list(&st.db);
    let current = active(&st.db);
    if profiles.is_empty() && current.is_none() {
        return;
    }
    let running: Vec<String> = st.telemetry.read().map(|t| t.processes.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
    if running.is_empty() {
        return; // process list not ready yet
    }
    let gone_for = {
        let mut absent = st.app_profiles.absent_since.lock().unwrap_or_else(|e| e.into_inner());
        match &current {
            Some(a) if !running.iter().any(|r| r.eq_ignore_ascii_case(&a.exe)) => Some(absent.get_or_insert_with(Instant::now).elapsed()),
            _ => {
                *absent = None;
                None
            }
        }
    };
    match decide(&profiles, &running, current.as_ref(), gone_for) {
        Step::Activate(p) => activate(app, st, &p),
        Step::Deactivate => {
            if let Some(a) = current {
                deactivate(app, st, &a);
            }
            if let Ok(mut absent) = st.app_profiles.absent_since.lock() {
                *absent = None;
            }
        }
        Step::Wait | Step::Nothing => {}
    }
}

pub fn start(app: AppHandle, st: Shared) {
    std::thread::Builder::new()
        .name("app-profiles".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3));
            tick(Some(&app), &st);
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prof(exe: &str, enabled: bool) -> AppProfile {
        AppProfile { exe: exe.into(), label: exe.trim_end_matches(".exe").into(), power_plan: None, priority: None, pause_maintenance: true, enabled, updated_at: String::new() }
    }
    fn act(exe: &str) -> Active {
        Active { exe: exe.into(), label: "x".into(), tx_id: None, our_plan: None, since: now(), pause_maintenance: false }
    }

    #[test]
    fn decisions() {
        let ps = vec![prof("Blender.exe", true), prof("Off.exe", false)];
        let run = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(decide(&ps, &run(&["blender.exe"]), None, None), Step::Activate(ps[0].clone()));
        assert_eq!(decide(&ps, &run(&["Off.exe"]), None, None), Step::Nothing, "disabled profiles don't fire");
        assert_eq!(decide(&ps, &run(&["Blender.exe"]), Some(&act("Blender.exe")), None), Step::Nothing);
        assert_eq!(decide(&ps, &run(&["x.exe"]), Some(&act("Blender.exe")), Some(Duration::from_secs(3))), Step::Wait);
        assert_eq!(decide(&ps, &run(&["x.exe"]), Some(&act("Blender.exe")), Some(GRACE)), Step::Deactivate);
    }

    #[test]
    fn save_validates_and_owns_its_rule() {
        let db = Db::open_in_memory().unwrap();
        let cache = rules::RuleCache::default();
        assert!(save(&db, &cache, AppProfile { exe: r"C:\x\Blender.exe".into(), ..prof("a", true) }).is_err(), "file name only");
        assert!(save(&db, &cache, AppProfile { pause_maintenance: false, ..prof("Blender.exe", true) }).is_err(), "must do something");
        assert!(save(&db, &cache, prof("csrss.exe", true)).is_err(), "protected");
        let p = AppProfile { priority: Some(Priority::AboveNormal), ..prof("Blender.exe", true) };
        save(&db, &cache, p.clone()).unwrap();
        assert!(rules::get(&db, "Blender.exe").unwrap().is_some_and(|r| profile_rule(&r)));
        save(&db, &cache, AppProfile { priority: None, ..p.clone() }).unwrap();
        assert!(rules::get(&db, "Blender.exe").unwrap().is_none(), "profile's own rule removed");
        // A user's own rule is never replaced.
        rules::save(&db, &cache, ProcessRule { name: "Game.exe".into(), priority: Some(Priority::High), affinity: None, enabled: true, note: String::new(), created_at: String::new(), updated_at: String::new(), applied_count: 0, last_applied_at: None, last_error: None }).unwrap();
        assert!(save(&db, &cache, AppProfile { priority: Some(Priority::AboveNormal), ..prof("Game.exe", true) }).is_err());
        save(&db, &cache, prof("Game.exe", true)).unwrap();
        delete(&db, &cache, "Game.exe").unwrap();
        assert_eq!(rules::get(&db, "Game.exe").unwrap().unwrap().priority, Some(Priority::High), "user rule kept");
        assert_eq!(list(&db).len(), 1);
    }

    #[test]
    fn maintenance_pause_follows_active() {
        let db = Db::open_in_memory().unwrap();
        assert!(pausing_maintenance(&db).is_none());
        set_active(&db, Some(&Active { pause_maintenance: true, label: "Blender".into(), ..act("Blender.exe") }));
        assert_eq!(pausing_maintenance(&db).as_deref(), Some("Blender"));
        set_active(&db, None);
        assert!(active(&db).is_none());
    }
}
