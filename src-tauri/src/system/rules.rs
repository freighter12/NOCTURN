//! Saved per-app process rules: "when Fortnite starts, set its priority to
//! Above normal and keep it off CPU 0". Rules are created only by an explicit
//! user action, validated on save, cached in memory, and applied once per new
//! process by the telemetry thread. Every application is audited.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::process_control::{self, Priority};
use super::processes::is_protected;
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessRule {
    /// Exact process name, e.g. `FortniteClient-Win64-Shipping.exe`.
    pub name: String,
    pub priority: Option<Priority>,
    /// Hex affinity mask (e.g. "0xfffe"); `None` = leave affinity alone.
    pub affinity: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub applied_count: i64,
    #[serde(default)]
    pub last_applied_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl ProcessRule {
    pub fn mask(&self) -> Option<u64> {
        self.affinity.as_deref().and_then(parse_mask)
    }
}

pub fn parse_mask(s: &str) -> Option<u64> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u64::from_str_radix(hex, 16).ok()
}

pub type RuleCache = Arc<RwLock<Vec<ProcessRule>>>;

fn validate(r: &ProcessRule) -> AppResult<()> {
    let n = r.name.trim();
    if n.is_empty() || n.len() > 260 || n.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(AppError::Invalid("use the program's file name, e.g. Game.exe".into()));
    }
    if !n.to_ascii_lowercase().ends_with(".exe") {
        return Err(AppError::Invalid("rules apply to programs (.exe files)".into()));
    }
    if is_protected(n) || n.eq_ignore_ascii_case("nocturn.exe") {
        return Err(AppError::Forbidden(format!("{n} is a core Windows process; NOCTURN won't change it")));
    }
    if r.priority.is_none() && r.affinity.is_none() {
        return Err(AppError::Invalid("a rule needs a priority, a CPU selection, or both".into()));
    }
    if let Some(a) = &r.affinity {
        let mask = parse_mask(a).ok_or_else(|| AppError::Invalid(format!("'{a}' is not a valid CPU mask")))?;
        process_control::validate_mask(mask, process_control::system_mask())?;
    }
    if r.note.len() > 500 {
        return Err(AppError::Invalid("note is too long".into()));
    }
    Ok(())
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<ProcessRule> {
    let pri: Option<String> = r.get(1)?;
    Ok(ProcessRule {
        name: r.get(0)?,
        priority: pri.and_then(|p| serde_json::from_value(serde_json::Value::String(p)).ok()),
        affinity: r.get(2)?,
        enabled: r.get::<_, i64>(3)? != 0,
        note: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        applied_count: r.get(7)?,
        last_applied_at: r.get(8)?,
        last_error: r.get(9)?,
    })
}

const COLS: &str = "name, priority, affinity, enabled, note, created_at, updated_at, applied_count, last_applied_at, last_error";

pub fn list(db: &Db) -> AppResult<Vec<ProcessRule>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM process_rules ORDER BY name COLLATE NOCASE"))?;
    let rows = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get(db: &Db, name: &str) -> AppResult<Option<ProcessRule>> {
    Ok(db
        .conn()
        .query_row(&format!("SELECT {COLS} FROM process_rules WHERE name = ?1"), [name], row)
        .optional()?)
}

pub fn save(db: &Db, cache: &RuleCache, mut r: ProcessRule) -> AppResult<ProcessRule> {
    r.name = r.name.trim().to_string();
    r.note = r.note.trim().to_string();
    if let Some(m) = r.affinity.as_deref().and_then(parse_mask) {
        r.affinity = Some(format!("0x{m:x}"));
    }
    validate(&r)?;
    let pri = r.priority.and_then(|p| serde_json::to_value(p).ok().and_then(|v| v.as_str().map(String::from)));
    let ts = now();
    db.conn().execute(
        "INSERT INTO process_rules(name, priority, affinity, enabled, note, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(name) DO UPDATE SET priority = excluded.priority, affinity = excluded.affinity,
           enabled = excluded.enabled, note = excluded.note, updated_at = excluded.updated_at, last_error = NULL",
        params![r.name, pri, r.affinity, r.enabled as i64, r.note, ts],
    )?;
    db.audit("info", "user", "rule.saved", Some(serde_json::json!({ "name": r.name, "priority": r.priority, "affinity": r.affinity, "enabled": r.enabled })));
    reload(db, cache);
    get(db, &r.name)?.ok_or_else(|| AppError::msg("rule vanished after saving"))
}

pub fn delete(db: &Db, cache: &RuleCache, name: &str) -> AppResult<()> {
    let n = db.conn().execute("DELETE FROM process_rules WHERE name = ?1", [name])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("no rule for {name}")));
    }
    db.audit("info", "user", "rule.deleted", Some(serde_json::json!({ "name": name })));
    reload(db, cache);
    Ok(())
}

pub fn reload(db: &Db, cache: &RuleCache) {
    match list(db) {
        Ok(v) => {
            if let Ok(mut c) = cache.write() {
                *c = v;
            }
        }
        Err(e) => tracing::error!("could not load process rules: {e}"),
    }
}

/// Apply enabled rules to processes that appeared since the last scan.
/// `fresh` holds (pid, name) of new processes. Returns how many PIDs were
/// touched. Called from the telemetry thread; cheap when no rule matches.
pub fn apply_to_new(db: &Db, cache: &RuleCache, fresh: &[(u32, String)]) -> usize {
    let rules: Vec<ProcessRule> = match cache.read() {
        Ok(c) => c.iter().filter(|r| r.enabled).cloned().collect(),
        Err(_) => return 0,
    };
    if rules.is_empty() || fresh.is_empty() {
        return 0;
    }
    let mut touched = 0;
    for rule in rules {
        let pids: Vec<u32> = fresh.iter().filter(|(_, n)| n.eq_ignore_ascii_case(&rule.name)).map(|(p, _)| *p).collect();
        if pids.is_empty() {
            continue;
        }
        let mut errors: HashSet<String> = HashSet::new();
        let mut ok = 0;
        for pid in &pids {
            let mut pid_ok = true;
            if let Some(p) = rule.priority {
                if let Err(e) = process_control::set_priority(*pid, &rule.name, p) {
                    pid_ok = false;
                    errors.insert(e.to_string());
                }
            }
            if let Some(m) = rule.mask() {
                if let Err(e) = process_control::set_affinity(*pid, &rule.name, m) {
                    pid_ok = false;
                    errors.insert(e.to_string());
                }
            }
            if pid_ok {
                ok += 1;
            }
        }
        touched += pids.len();
        let err = errors.into_iter().next();
        let _ = db.conn().execute(
            "UPDATE process_rules SET applied_count = applied_count + ?1, last_applied_at = ?2, last_error = ?3 WHERE name = ?4",
            params![ok as i64, now(), err, rule.name],
        );
        // One audit row per rule per scan, not per PID (browsers run dozens).
        db.audit(
            if err.is_none() { "info" } else { "warn" },
            "rule",
            "rule.applied",
            Some(serde_json::json!({ "name": rule.name, "processes": pids.len(), "ok": ok, "error": err })),
        );
    }
    reload(db, cache);
    touched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str, priority: Option<Priority>, affinity: Option<&str>) -> ProcessRule {
        ProcessRule {
            name: name.into(),
            priority,
            affinity: affinity.map(String::from),
            enabled: true,
            note: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            applied_count: 0,
            last_applied_at: None,
            last_error: None,
        }
    }

    #[test]
    fn validation_rejects_bad_rules() {
        let db = Db::open_in_memory().unwrap();
        let cache = RuleCache::default();
        assert!(save(&db, &cache, rule("lsass.exe", Some(Priority::High), None)).is_err());
        assert!(save(&db, &cache, rule("C:\\x\\game.exe", Some(Priority::High), None)).is_err());
        assert!(save(&db, &cache, rule("game", Some(Priority::High), None)).is_err());
        assert!(save(&db, &cache, rule("game.exe", None, None)).is_err());
        assert!(save(&db, &cache, rule("game.exe", None, Some("0x0"))).is_err());
        assert!(save(&db, &cache, rule("game.exe", None, Some("zz"))).is_err());
    }

    #[test]
    fn save_list_update_delete() {
        let db = Db::open_in_memory().unwrap();
        let cache = RuleCache::default();
        let saved = save(&db, &cache, rule(" Game.exe ", Some(Priority::AboveNormal), Some("3"))).unwrap();
        assert_eq!(saved.name, "Game.exe");
        assert_eq!(saved.affinity.as_deref(), Some("0x3"));
        assert_eq!(cache.read().unwrap().len(), 1);
        // Upsert is case-insensitive on name.
        save(&db, &cache, rule("game.exe", Some(Priority::High), None)).unwrap();
        let all = list(&db).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].priority, Some(Priority::High));
        delete(&db, &cache, "GAME.EXE").unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert!(cache.read().unwrap().is_empty());
    }

    #[test]
    fn applies_to_a_new_process_once() {
        let db = Db::open_in_memory().unwrap();
        let cache = RuleCache::default();
        save(&db, &cache, rule("PING.EXE", Some(Priority::BelowNormal), None)).unwrap();
        /// Kills and reaps the child however the test ends.
        struct Reap(std::process::Child);
        impl Drop for Reap {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let child = Reap(std::process::Command::new(r"C:\Windows\System32\ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap());
        std::thread::sleep(std::time::Duration::from_millis(300));
        let pid = child.0.id();
        let n = apply_to_new(&db, &cache, &[(pid, "PING.EXE".into()), (999_999, "other.exe".into())]);
        assert_eq!(n, 1);
        assert_eq!(process_control::priority_label(pid).as_deref(), Some("Below normal"));
        let r = get(&db, "ping.exe").unwrap().unwrap();
        assert_eq!(r.applied_count, 1);
        assert!(r.last_error.is_none());
        drop(child);
    }
}
