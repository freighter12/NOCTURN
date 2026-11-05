//! Per-game profiles.
//!
//! A profile combines settings from different mechanisms:
//! - **GPU preference** — `HKCU\Software\Microsoft\DirectX\UserGpuPreferences`,
//!   the same value Windows' Settings › Display › Graphics page writes.
//! - **Fullscreen optimisations off** — the per-app compatibility flag
//!   (`AppCompatFlags\Layers`, `DISABLEDXMAXIMIZEDWINDOWEDMODE`) that the
//!   exe's Properties › Compatibility checkbox sets. Existing flags are kept.
//! - **Priority / CPU affinity** — saved as a process rule, applied each time
//!   the game starts.
//!
//! Registry changes go through the journaled engine (originals recorded,
//! verified, rollback from History). Only these two fixed keys under HKCU are
//! ever written, with the game's validated exe path as the value name.

use std::path::Path;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::system::process_control::Priority;
use crate::system::rules::{self, ProcessRule, RuleCache};
use crate::tweaks::engine::{self, ApplyReport, PlannedOp, Progress};
use crate::tweaks::registry::{self};
use crate::tweaks::{ChangeOp, Hive, RegData, RegSetting, Takes};

pub const GPU_PREF_KEY: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
pub const LAYERS_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";
const FSO_FLAG: &str = "DISABLEDXMAXIMIZEDWINDOWEDMODE";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuPreference {
    /// Let Windows decide.
    Default,
    PowerSaving,
    HighPerformance,
}

impl GpuPreference {
    fn value(self) -> String {
        let n = match self {
            GpuPreference::Default => 0,
            GpuPreference::PowerSaving => 1,
            GpuPreference::HighPerformance => 2,
        };
        format!("GpuPreference={n};")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameProfile {
    pub game_id: String,
    /// Full path of the game executable the profile targets.
    pub exe: String,
    pub priority: Option<Priority>,
    /// Hex CPU mask; `None` = all CPUs.
    pub affinity: Option<String>,
    pub gpu_preference: Option<GpuPreference>,
    pub disable_fso: bool,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfilePreviewItem {
    pub kind: &'static str,
    pub title: String,
    pub target: String,
    pub current: String,
    pub proposed: String,
    pub will_change: bool,
    pub note: Option<String>,
}

pub fn validate_exe(exe: &str) -> AppResult<()> {
    let p = Path::new(exe);
    if !p.is_absolute() || !exe.to_ascii_lowercase().ends_with(".exe") || exe.contains("..") || exe.len() > 400 {
        return Err(AppError::Invalid("choose the game's .exe file".into()));
    }
    if !p.is_file() {
        return Err(AppError::NotFound(format!("{exe} doesn't exist")));
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    if exe.to_ascii_lowercase().starts_with(&windir) {
        return Err(AppError::Forbidden("Windows' own programs can't have game profiles".into()));
    }
    Ok(())
}

fn exe_name(exe: &str) -> String {
    Path::new(exe).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
}

/// Merge the FSO flag into an existing compatibility layer string, keeping
/// any other flags (e.g. RUNASADMIN) the user already has.
pub fn merge_layers(existing: Option<&str>) -> String {
    let mut flags: Vec<String> = existing
        .unwrap_or("")
        .split_whitespace()
        .filter(|f| *f != "~")
        .map(String::from)
        .collect();
    if !flags.iter().any(|f| f.eq_ignore_ascii_case(FSO_FLAG)) {
        flags.push(FSO_FLAG.into());
    }
    format!("~ {}", flags.join(" "))
}

fn read_sz(path: &str, name: &str) -> Option<String> {
    registry::read(Hive::Hkcu, path, name).ok().flatten().and_then(|v| v.as_string())
}

fn registry_ops(p: &GameProfile) -> Vec<(PlannedOp, ProfilePreviewItem)> {
    let mut out = Vec::new();
    if let Some(pref) = p.gpu_preference {
        let current = read_sz(GPU_PREF_KEY, &p.exe);
        let proposed = pref.value();
        out.push((
            PlannedOp {
                tweak_id: format!("game:{}", p.game_id),
                title: format!("GPU preference for {}", exe_name(&p.exe)),
                takes: Takes::AppRestart,
                op: ChangeOp::Registry(RegSetting { hive: Hive::Hkcu, path: GPU_PREF_KEY.into(), name: p.exe.clone(), data: RegData::Sz(proposed.clone()) }),
            },
            ProfilePreviewItem {
                kind: "registry",
                title: "Graphics preference".into(),
                target: format!(r"HKCU\{GPU_PREF_KEY}\{}", p.exe),
                will_change: current.as_deref() != Some(proposed.as_str()),
                current: current.unwrap_or_else(|| "not set (Windows decides)".into()),
                proposed,
                note: Some("Matters on PCs with two GPUs (e.g. laptops); on a single-GPU desktop it has no effect.".into()),
            },
        ));
    }
    if p.disable_fso {
        let current = read_sz(LAYERS_KEY, &p.exe);
        let proposed = merge_layers(current.as_deref());
        out.push((
            PlannedOp {
                tweak_id: format!("game:{}", p.game_id),
                title: format!("Disable fullscreen optimisations for {}", exe_name(&p.exe)),
                takes: Takes::AppRestart,
                op: ChangeOp::Registry(RegSetting { hive: Hive::Hkcu, path: LAYERS_KEY.into(), name: p.exe.clone(), data: RegData::Sz(proposed.clone()) }),
            },
            ProfilePreviewItem {
                kind: "registry",
                title: "Fullscreen optimisations".into(),
                target: format!(r"HKCU\{LAYERS_KEY}\{}", p.exe),
                will_change: current.as_deref() != Some(proposed.as_str()),
                current: current.unwrap_or_else(|| "not set".into()),
                proposed,
                note: Some("Only helps if this game stutters or lags in exclusive fullscreen. Can disable Auto HDR for it.".into()),
            },
        ));
    }
    out
}

fn rule_item(p: &GameProfile, existing: Option<&ProcessRule>) -> Option<ProfilePreviewItem> {
    if p.priority.is_none() && p.affinity.is_none() {
        return None;
    }
    let describe = |pri: Option<Priority>, aff: Option<&str>| {
        let mut parts = vec![pri.map(|x| x.label().to_string()).unwrap_or_else(|| "priority unchanged".into())];
        parts.push(aff.and_then(rules::parse_mask).map(|m| format!("{} CPUs", m.count_ones())).unwrap_or_else(|| "all CPUs".into()));
        parts.join(", ")
    };
    let proposed = describe(p.priority, p.affinity.as_deref());
    let current = existing.map(|r| describe(r.priority, r.affinity.as_deref())).unwrap_or_else(|| "no rule".into());
    Some(ProfilePreviewItem {
        kind: "rule",
        title: "Priority and CPUs at launch".into(),
        target: format!("Process rule for {}", exe_name(&p.exe)),
        will_change: current != proposed || existing.is_some_and(|r| !r.enabled),
        current,
        proposed,
        note: Some("Applied by NOCTURN each time the game starts, while NOCTURN is running.".into()),
    })
}

pub fn preview(db: &Db, p: &GameProfile) -> AppResult<Vec<ProfilePreviewItem>> {
    validate_exe(&p.exe)?;
    let mut items: Vec<ProfilePreviewItem> = registry_ops(p).into_iter().map(|(_, i)| i).collect();
    let existing = rules::get(db, &exe_name(&p.exe))?;
    items.extend(rule_item(p, existing.as_ref()));
    Ok(items)
}

#[derive(Debug, Serialize)]
pub struct ProfileOutcome {
    pub report: Option<ApplyReport>,
    pub rule: Option<ProcessRule>,
    pub rule_error: Option<String>,
}

pub fn apply(db: &Db, data_dir: &Path, rule_cache: &RuleCache, p: GameProfile, game_name: &str, progress: &dyn Fn(Progress)) -> AppResult<ProfileOutcome> {
    validate_exe(&p.exe)?;
    let ops: Vec<PlannedOp> = registry_ops(&p).into_iter().filter(|(_, i)| i.will_change).map(|(op, _)| op).collect();
    let report = if ops.is_empty() { None } else { Some(engine::run_plan(db, data_dir, ops, "user", &format!("Game profile: {game_name}"), false, progress)?) };
    let (rule, rule_error) = if p.priority.is_some() || p.affinity.is_some() {
        let r = ProcessRule {
            name: exe_name(&p.exe),
            priority: p.priority,
            affinity: p.affinity.clone(),
            enabled: true,
            note: format!("Game profile: {game_name}"),
            created_at: String::new(),
            updated_at: String::new(),
            applied_count: 0,
            last_applied_at: None,
            last_error: None,
        };
        match rules::save(db, rule_cache, r) {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(e.to_string())),
        }
    } else {
        (None, None)
    };
    save(db, &p)?;
    db.audit("info", "user", "game.profile.applied", Some(serde_json::json!({ "game": p.game_id, "exe": p.exe, "tx": report.as_ref().map(|r| &r.tx_id) })));
    Ok(ProfileOutcome { report, rule, rule_error })
}

pub fn save(db: &Db, p: &GameProfile) -> AppResult<()> {
    db.conn().execute(
        "INSERT INTO game_profiles(game_id, json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(game_id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        params![p.game_id, serde_json::to_string(p)?, now()],
    )?;
    Ok(())
}

pub fn get(db: &Db, game_id: &str) -> AppResult<Option<GameProfile>> {
    let raw: Option<(String, String)> = db
        .conn()
        .query_row("SELECT json, updated_at FROM game_profiles WHERE game_id = ?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(raw.and_then(|(j, t)| serde_json::from_str::<GameProfile>(&j).ok().map(|mut p| {
        p.updated_at = t;
        p
    })))
}

pub fn all(db: &Db) -> AppResult<Vec<GameProfile>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT json, updated_at FROM game_profiles")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows
        .flatten()
        .filter_map(|(j, t)| serde_json::from_str::<GameProfile>(&j).ok().map(|mut p| {
            p.updated_at = t;
            p
        }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_merge_keeps_existing_flags() {
        assert_eq!(merge_layers(None), "~ DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(merge_layers(Some("~ RUNASADMIN")), "~ RUNASADMIN DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(merge_layers(Some("~ DISABLEDXMAXIMIZEDWINDOWEDMODE")), "~ DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(GpuPreference::HighPerformance.value(), "GpuPreference=2;");
    }

    #[test]
    fn exe_validation() {
        assert!(validate_exe("game.exe").is_err(), "relative");
        assert!(validate_exe(r"C:\Windows\System32\notepad.exe").is_err(), "windows binaries refused");
        assert!(validate_exe(r"C:\nope\missing.exe").is_err());
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("g.exe");
        std::fs::write(&f, b"x").unwrap();
        assert!(validate_exe(f.to_str().unwrap()).is_ok());
        assert!(validate_exe(d.path().join("g.txt").to_str().unwrap()).is_err());
    }

    #[test]
    fn apply_preview_and_rollback_roundtrip() {
        // Real HKCU writes for a throwaway exe path, then rollback.
        let db = Db::open_in_memory().unwrap();
        let cache = RuleCache::default();
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("NocturnTestGame.exe");
        std::fs::write(&exe, b"x").unwrap();
        let exe = exe.display().to_string();
        let p = GameProfile {
            game_id: "custom:test".into(),
            exe: exe.clone(),
            priority: Some(Priority::AboveNormal),
            affinity: None,
            gpu_preference: Some(GpuPreference::HighPerformance),
            disable_fso: true,
            updated_at: String::new(),
        };
        let items = preview(&db, &p).unwrap();
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|i| i.will_change));
        let out = apply(&db, dir.path(), &cache, p.clone(), "Test Game", &|_| {}).unwrap();
        let report = out.report.expect("registry changes ran");
        assert!(report.results.iter().all(|r| r.status == "applied"), "{:?}", report.results);
        assert_eq!(read_sz(GPU_PREF_KEY, &exe).as_deref(), Some("GpuPreference=2;"));
        assert_eq!(read_sz(LAYERS_KEY, &exe).as_deref(), Some("~ DISABLEDXMAXIMIZEDWINDOWEDMODE"));
        assert_eq!(out.rule.unwrap().priority, Some(Priority::AboveNormal));
        assert_eq!(get(&db, "custom:test").unwrap().unwrap().exe, exe);
        // Second preview: nothing left to change.
        assert!(preview(&db, &p).unwrap().iter().all(|i| !i.will_change));
        // Roll back: both values removed again.
        let rb = engine::rollback(&db, &report.tx_id, &|_| {}).unwrap();
        assert!(rb.results.iter().all(|r| r.status == "rolled_back"), "{:?}", rb.results);
        assert_eq!(read_sz(GPU_PREF_KEY, &exe), None);
        assert_eq!(read_sz(LAYERS_KEY, &exe), None);
    }
}
