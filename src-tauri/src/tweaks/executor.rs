//! Journaled apply and rollback (re-exported through `tweaks::engine`).

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::engine::{resolve, takes_rank};
use super::journal::{self, status, tx_status};
use super::registry::{self, RawValue};
use super::{maintenance, ops_for, ChangeOp, Original, Takes};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::elevation;
use crate::system::power;

/// Only one apply/rollback may run at a time.
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Deserialize)]
pub struct ApplyRequest {
    pub tweak_ids: Vec<String>,
    /// `user`, `profile:<id>` or `ai`.
    pub source: String,
    pub label: String,
    #[serde(default)]
    pub create_restore_point: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeResult {
    pub change_id: i64,
    pub tweak_id: String,
    pub target: String,
    pub status: String,
    pub before: String,
    pub after: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestorePointResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub tx_id: String,
    pub status: String,
    pub results: Vec<ChangeResult>,
    pub restore_point: Option<RestorePointResult>,
    pub notes: Vec<String>,
    pub takes: Takes,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub tx_id: String,
    pub done: usize,
    pub total: usize,
    pub message: String,
}

/// One operation the engine is asked to perform. `tweak_id` is a catalogue ID
/// or the pseudo-ID `startup` for Startup Manager toggles.
#[derive(Debug, Clone)]
pub struct PlannedOp {
    pub tweak_id: String,
    pub title: String,
    pub takes: Takes,
    pub op: ChangeOp,
}

struct Pending {
    change_id: i64,
    tweak_id: String,
    title: String,
    op: ChangeOp,
    original: Original,
}

/// Human-readable form of a captured original, in the context of its op.
pub fn display_original(op: &ChangeOp, o: &Original) -> String {
    match (op, o) {
        (ChangeOp::StartupApproval { .. }, Original::Registry { value }) => {
            if crate::startup::is_enabled(value) {
                "Enabled".into()
            } else {
                "Disabled".into()
            }
        }
        (_, Original::Registry { value: Some(v) }) => v.display(),
        (_, Original::Registry { value: None }) => "(not set)".into(),
        (_, Original::PowerPlan { scheme }) => power::list()
            .ok()
            .and_then(|l| l.into_iter().find(|s| &s.guid == scheme).map(|s| s.name))
            .unwrap_or_else(|| scheme.clone()),
        (_, Original::Nothing) => "—".into(),
    }
}

fn capture_original(op: &ChangeOp) -> AppResult<Original> {
    Ok(match op {
        ChangeOp::Registry(s) => Original::Registry { value: registry::read(s.hive, &s.path, &s.name)? },
        ChangeOp::StartupApproval { location, name, .. } => {
            let (h, p) = location.approved_key();
            Original::Registry { value: registry::read(h, &p, name)? }
        }
        ChangeOp::PowerPlan { .. } => Original::PowerPlan { scheme: power::active_guid()? },
        ChangeOp::FlushDns | ChangeOp::CleanTempFiles => Original::Nothing,
    })
}

fn already_satisfied(op: &ChangeOp, original: &Original) -> bool {
    match (op, original) {
        (ChangeOp::Registry(s), Original::Registry { value: Some(v) }) => v.matches(&s.data),
        (ChangeOp::StartupApproval { enabled, .. }, Original::Registry { value }) => {
            crate::startup::is_enabled(value) == *enabled
        }
        (ChangeOp::PowerPlan { scheme }, Original::PowerPlan { scheme: cur }) => scheme == cur,
        _ => false,
    }
}

/// Apply one op in-process and verify. Returns a description of the new state.
pub(crate) fn apply_local(op: &ChangeOp) -> AppResult<String> {
    match op {
        ChangeOp::Registry(s) => {
            let v = RawValue::from_data(&s.data);
            registry::write(s.hive, &s.path, &s.name, &v)?;
            match registry::read(s.hive, &s.path, &s.name)? {
                Some(now) if now.matches(&s.data) => Ok(now.display()),
                Some(now) => Err(AppError::msg(format!(
                    "verification failed: value reads back as {} (another policy may be overriding it)",
                    now.display()
                ))),
                None => Err(AppError::msg("verification failed: value is missing after writing")),
            }
        }
        ChangeOp::StartupApproval { location, name, enabled } => {
            let (h, p) = location.approved_key();
            registry::write(h, &p, name, &crate::startup::approval_bytes(*enabled))?;
            let now = registry::read(h, &p, name)?;
            if crate::startup::is_enabled(&now) == *enabled {
                Ok(if *enabled { "Enabled".into() } else { "Disabled".into() })
            } else {
                Err(AppError::msg("verification failed: startup state did not change"))
            }
        }
        ChangeOp::PowerPlan { scheme } => {
            power::set_active(scheme)?;
            Ok(display_original(op, &Original::PowerPlan { scheme: scheme.clone() }))
        }
        ChangeOp::FlushDns => maintenance::flush_dns(),
        ChangeOp::CleanTempFiles => {
            let r = maintenance::clean_temp()?;
            Ok(format!(
                "Freed {} ({} files deleted, {} in use skipped)",
                crate::util::fmt_bytes(r.freed_bytes),
                r.deleted_files,
                r.skipped_in_use
            ))
        }
    }
}

/// Restore one op's original in-process and verify.
pub(crate) fn restore_local(op: &ChangeOp, original: &Original) -> AppResult<String> {
    match (op, original) {
        (ChangeOp::Registry(s), Original::Registry { value }) => {
            registry::restore(s.hive, &s.path, &s.name, value)?;
            if &registry::read(s.hive, &s.path, &s.name)? != value {
                return Err(AppError::msg("value did not read back as the original"));
            }
        }
        (ChangeOp::StartupApproval { location, name, .. }, Original::Registry { value }) => {
            let (h, p) = location.approved_key();
            registry::restore(h, &p, name, value)?;
            if &registry::read(h, &p, name)? != value {
                return Err(AppError::msg("startup flag did not read back as the original"));
            }
        }
        (ChangeOp::PowerPlan { .. }, Original::PowerPlan { scheme }) => power::set_active(scheme)?,
        _ => return Err(AppError::Invalid("one-time action; nothing to restore".into())),
    }
    Ok(display_original(op, original))
}

fn write_backup(data_dir: &Path, tx_id: &str, pending: &[Pending]) -> AppResult<String> {
    let dir = data_dir.join("backups");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{tx_id}.json"));
    let items: Vec<_> = pending
        .iter()
        .map(|p| {
            serde_json::json!({
                "change_id": p.change_id,
                "tweak_id": p.tweak_id,
                "target": p.op.target(),
                "op": p.op,
                "original": p.original,
                "original_display": display_original(&p.op, &p.original),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "format": "nocturn-backup/1",
        "tx_id": tx_id,
        "created_at": crate::db::now(),
        "items": items,
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&doc)?)?;
    Ok(path.display().to_string())
}

fn result(p: &Pending, st: &str, before: String, after: String, error: Option<String>) -> ChangeResult {
    ChangeResult {
        change_id: p.change_id,
        tweak_id: p.tweak_id.clone(),
        target: p.op.target(),
        status: st.into(),
        before,
        after,
        error,
    }
}

fn actor_of(source: &str) -> &'static str {
    if source == "ai" {
        "ai"
    } else {
        "user"
    }
}

fn worker_result<'a>(arr: &'a [serde_json::Value], change_id: i64) -> Option<&'a serde_json::Value> {
    arr.iter().find(|r| r.get("change_id").and_then(|x| x.as_i64()) == Some(change_id))
}

/// Apply catalogue tweaks.
pub fn apply(db: &Db, data_dir: &Path, req: ApplyRequest, progress: &dyn Fn(Progress)) -> AppResult<ApplyReport> {
    let tweaks = resolve(&req.tweak_ids)?;
    let plan = tweaks
        .iter()
        .flat_map(|t| {
            ops_for(t).into_iter().map(move |op| PlannedOp {
                tweak_id: t.id.into(),
                title: t.title.into(),
                takes: t.takes,
                op,
            })
        })
        .collect();
    run_plan(db, data_dir, plan, &req.source, &req.label, req.create_restore_point, progress)
}

/// The journaled executor shared by catalogue tweaks and Startup Manager.
pub fn run_plan(
    db: &Db,
    data_dir: &Path,
    plan: Vec<PlannedOp>,
    source: &str,
    label: &str,
    want_rp: bool,
    progress: &dyn Fn(Progress),
) -> AppResult<ApplyReport> {
    let _guard = ENGINE_LOCK
        .try_lock()
        .map_err(|_| AppError::msg("another optimisation is already running"))?;

    let tx_id = uuid::Uuid::new_v4().to_string();
    journal::create_tx(db, &tx_id, source, label)?;
    db.audit(
        "info",
        actor_of(source),
        "apply.start",
        Some(serde_json::json!({
            "tx": tx_id,
            "label": label,
            "ops": plan.iter().map(|p| p.op.target()).collect::<Vec<_>>(),
            "restore_point": want_rp,
        })),
    );

    let mut results: Vec<ChangeResult> = Vec::new();
    let mut pending: Vec<Pending> = Vec::new();
    let mut notes = Vec::new();
    let mut takes = Takes::Immediately;

    // Capture originals and journal them before any modification.
    for planned in plan {
        let PlannedOp { tweak_id, title, takes: t_takes, op } = planned;
        let original = match capture_original(&op) {
            Ok(o) => o,
            Err(e) => {
                let id = journal::insert_change(db, &tx_id, &tweak_id, &op, &Original::Nothing, status::FAILED)?;
                let msg = format!("could not read current value: {e}");
                journal::mark_applied(db, id, status::FAILED, Some(&msg))?;
                let p = Pending { change_id: id, tweak_id, title, op, original: Original::Nothing };
                results.push(result(&p, status::FAILED, "(unreadable)".into(), "—".into(), Some(msg)));
                continue;
            }
        };
        if already_satisfied(&op, &original) {
            let id = journal::insert_change(db, &tx_id, &tweak_id, &op, &original, status::UNCHANGED)?;
            let shown = display_original(&op, &original);
            let p = Pending { change_id: id, tweak_id, title, op, original };
            results.push(result(&p, status::UNCHANGED, shown.clone(), shown, None));
            continue;
        }
        if let ChangeOp::PowerPlan { scheme } = &op {
            let installed = power::list().map(|l| l.iter().any(|s| &s.guid == scheme)).unwrap_or(false);
            if !installed {
                let id = journal::insert_change(db, &tx_id, &tweak_id, &op, &original, status::SKIPPED)?;
                let msg = "this power plan isn't installed on this PC";
                journal::mark_applied(db, id, status::SKIPPED, Some(msg))?;
                let shown = display_original(&op, &original);
                let p = Pending { change_id: id, tweak_id, title, op, original };
                results.push(result(&p, status::SKIPPED, shown.clone(), shown, Some(msg.into())));
                continue;
            }
        }
        let id = journal::insert_change(db, &tx_id, &tweak_id, &op, &original, status::PENDING)?;
        if takes_rank(t_takes) > takes_rank(takes) {
            takes = t_takes;
        }
        pending.push(Pending { change_id: id, tweak_id, title, op, original });
    }

    let total = pending.len();
    if total == 0 {
        let st = if results.iter().any(|r| r.status == status::FAILED || r.status == status::SKIPPED) {
            tx_status::FAILED
        } else {
            notes.push("Everything selected was already configured — no changes were needed.".into());
            tx_status::NOTHING_TO_DO
        };
        journal::finish_tx(db, &tx_id, st)?;
        return Ok(ApplyReport { tx_id, status: st.into(), results, restore_point: None, notes, takes });
    }

    match write_backup(data_dir, &tx_id, &pending) {
        Ok(p) => journal::set_tx_backup(db, &tx_id, &p)?,
        Err(e) => notes.push(format!(
            "Backup file could not be written ({e}); originals are still recorded in the journal."
        )),
    }

    let elevated_now = elevation::is_elevated();
    let (needs_admin, local): (Vec<Pending>, Vec<Pending>) =
        pending.into_iter().partition(|p| p.op.needs_elevation() && !elevated_now);
    let mut done = 0usize;
    let mut restore_point = None;

    // ---- elevated part: restore point first, then machine-wide changes
    if !needs_admin.is_empty() || (want_rp && !elevated_now) {
        progress(Progress { tx_id: tx_id.clone(), done, total, message: "Waiting for administrator approval…".into() });
        let request = serde_json::json!({
            "kind": "apply",
            "create_restore_point": want_rp,
            "items": needs_admin.iter().map(|p| serde_json::json!({
                "change_id": p.change_id, "tweak_id": p.tweak_id, "op": p.op,
            })).collect::<Vec<_>>(),
        });
        match elevation::run_elevated(&request) {
            Ok(resp) => {
                if let Some(rp) = resp.get("restore_point").filter(|v| !v.is_null()) {
                    let r = RestorePointResult {
                        ok: rp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                        message: rp.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    };
                    journal::set_tx_restore_point(
                        db,
                        &tx_id,
                        &format!("{}: {}", if r.ok { "created" } else { "failed" }, r.message),
                    )?;
                    if !r.ok {
                        notes.push("Windows did not create a restore point (see details). NOCTURN's own rollback is still available for every reversible change.".into());
                    }
                    restore_point = Some(r);
                }
                let arr = resp.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                for p in &needs_admin {
                    done += 1;
                    let r = worker_result(&arr, p.change_id);
                    let ok = r.and_then(|r| r.get("ok")).and_then(|v| v.as_bool()).unwrap_or(false);
                    let err = r
                        .and_then(|r| r.get("error"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .or_else(|| (!ok).then(|| "no result from elevated worker".to_string()));
                    // The elevated worker's capture of the original is authoritative.
                    let mut original = p.original.clone();
                    if let Some(o) = r.and_then(|r| r.get("original")).filter(|v| !v.is_null()) {
                        if let Ok(o) = serde_json::from_value::<Original>(o.clone()) {
                            journal::update_original(db, p.change_id, &o)?;
                            original = o;
                        }
                    }
                    let st = if ok { status::APPLIED } else { status::FAILED };
                    journal::mark_applied(db, p.change_id, st, err.as_deref())?;
                    let after = r
                        .and_then(|r| r.get("after"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| display_original(&p.op, &original));
                    results.push(result(p, st, display_original(&p.op, &original), after, err));
                    progress(Progress { tx_id: tx_id.clone(), done, total, message: p.title.clone() });
                }
                if let Some(e) = resp.get("error").and_then(|v| v.as_str()) {
                    notes.push(format!("Elevated worker reported: {e}"));
                }
            }
            Err(e) => {
                let msg = e.to_string();
                for p in &needs_admin {
                    done += 1;
                    journal::mark_applied(db, p.change_id, status::SKIPPED, Some(&msg))?;
                    let shown = display_original(&p.op, &p.original);
                    results.push(result(p, status::SKIPPED, shown.clone(), shown, Some(msg.clone())));
                }
                if want_rp {
                    // A restore point was requested before any change. Without
                    // one, stop and make no further changes.
                    let why = "not applied: the restore point could not be created";
                    for p in &local {
                        journal::mark_applied(db, p.change_id, status::SKIPPED, Some(why))?;
                        let shown = display_original(&p.op, &p.original);
                        results.push(result(p, status::SKIPPED, shown.clone(), shown, Some(why.into())));
                    }
                    restore_point = Some(RestorePointResult { ok: false, message: msg.clone() });
                    notes.push("No changes were made because the restore point you asked for could not be created.".into());
                    journal::finish_tx(db, &tx_id, tx_status::FAILED)?;
                    db.audit(
                        "warn",
                        actor_of(source),
                        "apply.aborted",
                        Some(serde_json::json!({ "tx": tx_id, "reason": msg })),
                    );
                    return Ok(ApplyReport {
                        tx_id,
                        status: tx_status::FAILED.into(),
                        results,
                        restore_point,
                        notes,
                        takes,
                    });
                }
                notes.push(format!("Changes that need administrator rights were skipped: {msg}"));
            }
        }
    } else if want_rp && elevated_now {
        let r = super::elevated::create_restore_point();
        journal::set_tx_restore_point(
            db,
            &tx_id,
            &format!("{}: {}", if r.ok { "created" } else { "failed" }, r.message),
        )?;
        restore_point = Some(r);
    }

    // ---- local part
    for p in &local {
        progress(Progress { tx_id: tx_id.clone(), done, total, message: p.title.clone() });
        let res = apply_local(&p.op);
        done += 1;
        let before = display_original(&p.op, &p.original);
        let (st, after, err) = match res {
            Ok(after) => (status::APPLIED, after, None),
            Err(e) => (status::FAILED, before.clone(), Some(e.to_string())),
        };
        journal::mark_applied(db, p.change_id, st, err.as_deref())?;
        results.push(result(p, st, before, after, err));
    }
    progress(Progress { tx_id: tx_id.clone(), done, total, message: "Finished".into() });

    let applied = results.iter().filter(|r| r.status == status::APPLIED).count();
    let attempted = results.iter().filter(|r| r.status != status::UNCHANGED).count();
    let final_status = if applied == 0 {
        tx_status::FAILED
    } else if applied == attempted {
        tx_status::COMPLETED
    } else {
        tx_status::PARTIAL
    };
    journal::finish_tx(db, &tx_id, final_status)?;
    db.audit(
        if final_status == tx_status::COMPLETED { "info" } else { "warn" },
        actor_of(source),
        "apply.finish",
        Some(serde_json::json!({ "tx": tx_id, "status": final_status, "applied": applied, "attempted": attempted })),
    );
    if applied > 0 {
        match takes {
            Takes::Restart => notes.push("Restart your PC for every change to take effect.".into()),
            Takes::SignOut => notes.push("Sign out and back in for every change to take effect.".into()),
            Takes::AppRestart => notes.push("Restart any running games for the change to take effect.".into()),
            Takes::Immediately => {}
        }
    }
    Ok(ApplyReport { tx_id, status: final_status.into(), results, restore_point, notes, takes })
}

// -------------------------------------------------------------- rollback

#[derive(Debug, Clone, Serialize)]
pub struct RollbackReport {
    pub tx_id: String,
    pub status: String,
    pub results: Vec<ChangeResult>,
    pub notes: Vec<String>,
}

fn cr(c: &journal::ChangeRow, st: &str, after: &str, error: Option<String>) -> ChangeResult {
    ChangeResult {
        change_id: c.id,
        tweak_id: c.tweak_id.clone(),
        target: c.target.clone(),
        status: st.into(),
        before: "—".into(),
        after: after.into(),
        error,
    }
}

pub fn rollback(db: &Db, tx_id: &str, progress: &dyn Fn(Progress)) -> AppResult<RollbackReport> {
    let _guard = ENGINE_LOCK
        .try_lock()
        .map_err(|_| AppError::msg("another optimisation is already running"))?;
    if !journal::tx_exists(db, tx_id)? {
        return Err(AppError::NotFound(format!("transaction {tx_id}")));
    }

    let mut changes = journal::changes_for(db, tx_id)?;
    changes.reverse(); // undo in reverse order
    let candidates: Vec<_> = changes
        .into_iter()
        .filter(|c| matches!(c.status.as_str(), status::APPLIED | status::PENDING | status::UNKNOWN))
        .collect();
    if candidates.is_empty() {
        return Err(AppError::Invalid("this entry has no applied changes left to roll back".into()));
    }
    db.audit("info", "user", "rollback.start", Some(serde_json::json!({ "tx": tx_id })));

    let mut results = Vec::new();
    let mut notes = Vec::new();
    let total = candidates.len();
    let elevated_now = elevation::is_elevated();
    let mut done = 0;
    let mut admin_items = Vec::new();

    for c in &candidates {
        let Some(original) = c.original.clone() else {
            journal::mark_rolled_back(db, c.id, status::ROLLBACK_FAILED, Some("no original value recorded"))?;
            results.push(cr(c, status::ROLLBACK_FAILED, "—", Some("no original value recorded".into())));
            continue;
        };
        if matches!(original, Original::Nothing) {
            done += 1;
            journal::mark_rolled_back(db, c.id, status::NOT_REVERSIBLE, Some("one-time action; nothing to restore"))?;
            results.push(cr(c, status::NOT_REVERSIBLE, "—", None));
            continue;
        }
        if c.op.needs_elevation() && !elevated_now {
            admin_items.push((c, original));
            continue;
        }
        done += 1;
        progress(Progress { tx_id: tx_id.into(), done, total, message: c.target.clone() });
        match restore_local(&c.op, &original) {
            Ok(shown) => {
                journal::mark_rolled_back(db, c.id, status::ROLLED_BACK, None)?;
                results.push(cr(c, status::ROLLED_BACK, &shown, None));
            }
            Err(e) => {
                journal::mark_rolled_back(db, c.id, status::ROLLBACK_FAILED, Some(&e.to_string()))?;
                results.push(cr(c, status::ROLLBACK_FAILED, "—", Some(e.to_string())));
            }
        }
    }

    if !admin_items.is_empty() {
        progress(Progress { tx_id: tx_id.into(), done, total, message: "Waiting for administrator approval…".into() });
        let request = serde_json::json!({
            "kind": "rollback",
            "items": admin_items.iter().map(|(c, o)| {
                let value = match o { Original::Registry { value } => value.clone(), _ => None };
                serde_json::json!({ "change_id": c.id, "tweak_id": c.tweak_id, "op": c.op, "original": value })
            }).collect::<Vec<_>>(),
        });
        match elevation::run_elevated(&request) {
            Ok(resp) => {
                let arr = resp.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                for (c, o) in &admin_items {
                    let r = worker_result(&arr, c.id);
                    let ok = r.and_then(|r| r.get("ok")).and_then(|v| v.as_bool()).unwrap_or(false);
                    let err = r
                        .and_then(|r| r.get("error"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .or_else(|| (!ok).then(|| "no result from elevated worker".into()));
                    let st = if ok { status::ROLLED_BACK } else { status::ROLLBACK_FAILED };
                    journal::mark_rolled_back(db, c.id, st, err.as_deref())?;
                    results.push(cr(c, st, &display_original(&c.op, o), err));
                }
            }
            Err(e) => {
                for (c, _) in &admin_items {
                    results.push(cr(c, status::ROLLBACK_FAILED, "—", Some(e.to_string())));
                }
                notes.push(format!("Changes that need administrator rights were not rolled back: {e}"));
            }
        }
    }

    let reverted = results.iter().filter(|r| r.status == status::ROLLED_BACK).count();
    let reversible = results.iter().filter(|r| r.status != status::NOT_REVERSIBLE).count();
    let final_status = if reverted == reversible {
        tx_status::ROLLED_BACK
    } else {
        tx_status::PARTIALLY_ROLLED_BACK
    };
    journal::finish_tx(db, tx_id, final_status)?;
    db.audit(
        "info",
        "user",
        "rollback.finish",
        Some(serde_json::json!({ "tx": tx_id, "status": final_status, "reverted": reverted })),
    );
    progress(Progress { tx_id: tx_id.into(), done: total, total, message: "Finished".into() });
    Ok(RollbackReport { tx_id: tx_id.into(), status: final_status.into(), results, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::{catalog, Action};

    /// End-to-end on the real registry with a harmless HKCU setting:
    /// apply → verify → rollback → original restored exactly.
    /// Run explicitly: `cargo test -- --ignored apply_and_rollback`.
    #[test]
    #[ignore]
    fn apply_and_rollback_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let t = catalog::get("privacy.feedback_prompts_off").unwrap();
        let Action::Registry { settings } = &t.action else { panic!() };
        let s = &settings[0];
        let before = registry::read(s.hive, &s.path, &s.name).unwrap();

        let req = ApplyRequest {
            tweak_ids: vec![t.id.into()],
            source: "test".into(),
            label: "test".into(),
            create_restore_point: false,
        };
        let rep = apply(&db, dir.path(), req, &|_| {}).unwrap();
        let after = registry::read(s.hive, &s.path, &s.name).unwrap();
        assert!(after.as_ref().unwrap().matches(&s.data));
        if rep.status == tx_status::NOTHING_TO_DO {
            return; // already set on this machine; nothing to roll back
        }
        assert_eq!(rep.status, tx_status::COMPLETED, "{rep:?}");
        assert!(journal::get_tx(&db, &rep.tx_id).unwrap().backup_path.is_some());

        let rb = rollback(&db, &rep.tx_id, &|_| {}).unwrap();
        assert_eq!(rb.status, tx_status::ROLLED_BACK, "{rb:?}");
        assert_eq!(registry::read(s.hive, &s.path, &s.name).unwrap(), before);
    }

    #[test]
    fn rollback_of_unknown_tx_fails() {
        let db = Db::open_in_memory().unwrap();
        assert!(rollback(&db, "nope", &|_| {}).is_err());
    }
}
