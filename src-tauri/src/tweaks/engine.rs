//! Status, preview, journaled apply and rollback.
//!
//! Apply sequence (per transaction):
//! 1. Resolve tweak IDs against the catalogue (unknown IDs abort).
//! 2. Read every original value and write a JSON backup file.
//! 3. Journal each change as `pending` **with its original** before touching
//!    the system.
//! 4. Elevated part first (optional restore point, then HKLM changes) via the
//!    UAC worker, so a restore point precedes every modification.
//! 5. Local part (HKCU, power plan, maintenance).
//! 6. Verify each change by reading it back; record success or the real error.

use serde::Serialize;

use super::registry;
use super::{catalog, maintenance, ops_for, Action, ChangeOp, Takes, Tweak};
use crate::error::{AppError, AppResult};
use crate::security::elevation;
use crate::system::power;

pub use super::executor::{
    apply, rollback, run_plan, ApplyReport, ApplyRequest, PlannedOp, Progress, RestorePointResult,
    RollbackReport,
};
pub(crate) use super::executor::apply_local;

// ---------------------------------------------------------------- status

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Applied,
    NotApplied,
    Partial,
    /// One-shot action — there is no persistent "applied" state.
    Action,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TweakStatus {
    pub state: State,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TweakView {
    #[serde(flatten)]
    pub tweak: Tweak,
    pub requires_admin: bool,
    pub reversible: bool,
    pub one_shot: bool,
    pub status: TweakStatus,
}

pub fn status_of(t: &Tweak) -> TweakStatus {
    match &t.action {
        Action::Registry { settings } => {
            let mut matched = 0;
            for s in settings {
                match registry::read(s.hive, &s.path, &s.name) {
                    Ok(Some(v)) if v.matches(&s.data) => matched += 1,
                    Ok(_) => {}
                    Err(e) => {
                        return TweakStatus { state: State::Unknown, detail: Some(e.to_string()) }
                    }
                }
            }
            let state = if matched == settings.len() {
                State::Applied
            } else if matched == 0 {
                State::NotApplied
            } else {
                State::Partial
            };
            let detail = (state == State::Partial)
                .then(|| format!("{matched} of {} values already set", settings.len()));
            TweakStatus { state, detail }
        }
        Action::PowerPlan { target } => match power::list() {
            Ok(list) => {
                let Some(scheme) = list.iter().find(|s| s.guid == target.guid()) else {
                    return TweakStatus {
                        state: State::Unavailable,
                        detail: Some("This power plan isn't installed on this PC (common on Modern Standby laptops).".into()),
                    };
                };
                TweakStatus {
                    state: if scheme.active { State::Applied } else { State::NotApplied },
                    detail: list.iter().find(|s| s.active).map(|s| format!("Active plan: {}", s.name)),
                }
            }
            Err(e) => TweakStatus { state: State::Unknown, detail: Some(e.to_string()) },
        },
        Action::FlushDns => TweakStatus { state: State::Action, detail: None },
        Action::CleanTempFiles => match maintenance::scan_temp() {
            Ok(s) => TweakStatus {
                state: State::Action,
                detail: Some(format!(
                    "{} eligible ({} files)",
                    crate::util::fmt_bytes(s.eligible_bytes),
                    s.eligible_files
                )),
            },
            Err(e) => TweakStatus { state: State::Unavailable, detail: Some(e.to_string()) },
        },
    }
}

pub fn list_views() -> Vec<TweakView> {
    catalog::all()
        .iter()
        .map(|t| TweakView {
            tweak: t.clone(),
            requires_admin: t.requires_admin(),
            reversible: t.reversible(),
            one_shot: t.is_one_shot(),
            status: status_of(t),
        })
        .collect()
}

// --------------------------------------------------------------- preview

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub tweak_id: String,
    pub tweak_title: String,
    pub target: String,
    pub current: String,
    pub proposed: String,
    pub will_change: bool,
    pub requires_admin: bool,
    pub reversible: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub items: Vec<PreviewItem>,
    pub changes: usize,
    pub requires_admin: bool,
    pub irreversible: Vec<String>,
    pub unavailable: Vec<String>,
    pub takes: Takes,
    pub is_elevated: bool,
}

pub(crate) fn resolve(ids: &[String]) -> AppResult<Vec<&'static Tweak>> {
    let mut out: Vec<&'static Tweak> = Vec::new();
    for id in ids {
        let t = catalog::get(id)
            .ok_or_else(|| AppError::Invalid(format!("unknown optimisation '{id}'")))?;
        if !out.iter().any(|x| x.id == t.id) {
            out.push(t);
        }
    }
    if out.is_empty() {
        return Err(AppError::Invalid("no optimisations selected".into()));
    }
    Ok(out)
}

pub(crate) fn takes_rank(t: Takes) -> u8 {
    match t {
        Takes::Immediately => 0,
        Takes::AppRestart => 1,
        Takes::SignOut => 2,
        Takes::Restart => 3,
    }
}

pub fn preview(ids: &[String]) -> AppResult<Preview> {
    let tweaks = resolve(ids)?;
    let power_list = power::list().ok();
    let mut items = Vec::new();
    let mut irreversible = Vec::new();
    let mut unavailable = Vec::new();
    let mut takes = Takes::Immediately;

    for t in &tweaks {
        if !t.reversible() {
            irreversible.push(t.id.to_string());
        }
        for op in ops_for(t) {
            let (current, proposed, will_change, note) = match &op {
                ChangeOp::Registry(s) => match registry::read(s.hive, &s.path, &s.name) {
                    Ok(cur) => {
                        let same = cur.as_ref().map(|v| v.matches(&s.data)).unwrap_or(false);
                        (
                            cur.map(|v| v.display()).unwrap_or_else(|| "(not set — Windows default)".into()),
                            registry::display_data(&s.data),
                            !same,
                            None,
                        )
                    }
                    Err(e) => ("(unreadable)".into(), registry::display_data(&s.data), true, Some(e.to_string())),
                },
                ChangeOp::PowerPlan { scheme } => {
                    let list = power_list.clone().unwrap_or_default();
                    let cur = list.iter().find(|s| s.active);
                    let target = list.iter().find(|s| &s.guid == scheme);
                    match target {
                        None => {
                            unavailable.push(t.id.to_string());
                            (
                                cur.map(|s| s.name.clone()).unwrap_or_default(),
                                "High performance".into(),
                                false,
                                Some("Not available on this PC — will be skipped.".into()),
                            )
                        }
                        Some(tg) => (
                            cur.map(|s| s.name.clone()).unwrap_or_else(|| "(unknown)".into()),
                            tg.name.clone(),
                            !tg.active,
                            None,
                        ),
                    }
                }
                ChangeOp::StartupApproval { enabled, .. } => (
                    "—".into(),
                    if *enabled { "Enabled".into() } else { "Disabled".into() },
                    true,
                    None,
                ),
                ChangeOp::FlushDns => ("—".into(), "Cache cleared".into(), true, None),
                // Not produced by catalogue tweaks (Network page only).
                ChangeOp::DnsServers { servers, .. } => ("—".into(), crate::system::dns::display(servers), true, None),
                ChangeOp::CleanTempFiles => match maintenance::scan_temp() {
                    Ok(s) => (
                        format!("{} in {} old files", crate::util::fmt_bytes(s.eligible_bytes), s.eligible_files),
                        "Deleted".into(),
                        s.eligible_files > 0,
                        Some("Permanent — temporary files only.".into()),
                    ),
                    Err(e) => ("(unavailable)".into(), "—".into(), false, Some(e.to_string())),
                },
            };
            if will_change && takes_rank(t.takes) > takes_rank(takes) {
                takes = t.takes;
            }
            items.push(PreviewItem {
                tweak_id: t.id.into(),
                tweak_title: t.title.into(),
                target: op.target(),
                current,
                proposed,
                will_change,
                requires_admin: op.needs_elevation(),
                reversible: t.reversible(),
                note,
            });
        }
    }
    let changes = items.iter().filter(|i| i.will_change).count();
    let requires_admin = items.iter().any(|i| i.will_change && i.requires_admin);
    Ok(Preview {
        items,
        changes,
        requires_admin,
        irreversible,
        unavailable,
        takes,
        is_elevated: elevation::is_elevated(),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_rejects_unknown_ids() {
        assert!(preview(&["does.not_exist".into()]).is_err());
        assert!(preview(&[]).is_err());
    }

    #[test]
    fn preview_reports_hklm_as_admin() {
        let p = preview(&["gaming.hardware_gpu_scheduling".into()]).unwrap();
        assert!(p.items.iter().all(|i| i.requires_admin));
        let p = preview(&["privacy.advertising_id_off".into()]).unwrap();
        assert!(p.items.iter().all(|i| !i.requires_admin));
    }

    #[test]
    fn status_reads_for_every_tweak() {
        for v in list_views() {
            let _ = v.status.state; // must not panic; Unknown is acceptable
        }
    }
}
