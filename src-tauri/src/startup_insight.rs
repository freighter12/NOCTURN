//! Startup intelligence: who made each startup entry, whether its signature
//! verifies, and plain-language reasons an entry is worth a closer look.
//!
//! Review reasons describe something to check, never a verdict — plenty of
//! legitimate apps install per-user or ship unsigned.

use serde::Serialize;

use crate::db::Db;
use crate::error::AppResult;
use crate::startup::{list, StartupItem};
use crate::system::fileinfo::{self, Signature};

#[derive(Debug, Clone, Serialize, Default)]
pub struct StartupInsight {
    /// The program actually launched (shortcuts resolved).
    pub target: Option<String>,
    /// From the file's version resource — self-reported by the file.
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub signature: Option<Signature>,
    /// Lives under the Windows folder (catalog-signed system component).
    pub windows_component: bool,
    pub review: Vec<String>,
}

const SCRIPT_HOSTS: &[&str] = &["powershell.exe", "pwsh.exe", "wscript.exe", "cscript.exe", "mshta.exe", "cmd.exe"];
const DLL_LOADERS: &[&str] = &["rundll32.exe", "regsvr32.exe"];

fn file_name_lower(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn review_reasons(
    command: &str,
    target: Option<&str>,
    exists: Option<bool>,
    sig: Option<Signature>,
    publisher: Option<&str>,
    windows_component: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if exists == Some(false) {
        out.push("The program it launches no longer exists — usually left behind by an uninstalled app.".into());
        return out;
    }
    let cmd = command.to_ascii_lowercase();
    let exe = target.map(file_name_lower).unwrap_or_default();
    if SCRIPT_HOSTS.contains(&exe.as_str()) {
        out.push(format!("Starts through a script host ({exe}) rather than a normal program, so the command line is what actually runs."));
    }
    if DLL_LOADERS.contains(&exe.as_str()) && (!cmd.contains(r"\windows\") || cmd.contains(r"\appdata\") || cmd.contains(r"\temp\")) {
        out.push(format!("Loads a DLL through {exe} from outside the Windows folder."));
    }
    if sig == Some(Signature::Invalid) {
        out.push("Its digital signature doesn't verify — the file may have been altered after it was signed.".into());
    }
    let t = target.unwrap_or("").to_ascii_lowercase();
    let user_writable = [r"\appdata\", r"\programdata\", r"\users\public\", r"\downloads\"].iter().any(|d| t.contains(d));
    if t.contains(r"\temp\") || t.contains(r"\tmp\") {
        out.push("Runs from a temporary folder, which installed apps rarely do.".into());
    } else if sig == Some(Signature::Unsigned) && !windows_component && user_writable {
        out.push("Unsigned program in a folder any app can write to. Many legitimate per-user apps do this; check you recognise it.".into());
    }
    // A valid signature already identifies a publisher, so only unsigned
    // files without any maker information are worth mentioning.
    if publisher.is_none() && sig != Some(Signature::Valid) && !windows_component && target.is_some() && !SCRIPT_HOSTS.contains(&exe.as_str()) {
        out.push("The file doesn't say who made it.".into());
    }
    out
}

pub fn insight(item: &StartupItem) -> StartupInsight {
    let target = match &item.file_path {
        Some(p) if p.to_ascii_lowercase().ends_with(".lnk") => fileinfo::shortcut_target(p).or_else(|| Some(p.clone())),
        other => other.clone(),
    };
    let exists = target.as_ref().map(|t| std::path::Path::new(t).exists()).or(item.file_exists);
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    let windows_component = target.as_ref().is_some_and(|t| t.to_ascii_lowercase().starts_with(&windir));
    let (facts, sig) = match (&target, exists) {
        (Some(t), Some(true)) => (fileinfo::version_facts(t), Some(fileinfo::signature(t))),
        _ => (Default::default(), None),
    };
    let review = review_reasons(&item.command, target.as_deref(), exists, sig, facts.company.as_deref(), windows_component);
    StartupInsight { publisher: facts.company, description: facts.description.or(facts.product), signature: sig, windows_component, review, target }
}

/// `list()` plus insight for every entry. Slower (signature checks), so only
/// the Startup page and the AI use it.
pub fn with_insights() -> Vec<StartupItem> {
    let mut items = list();
    for it in &mut items {
        it.insight = Some(insight(it));
    }
    items
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupChange {
    pub tx_id: String,
    pub target: String,
    pub status: String,
    pub source: String,
    pub applied_at: Option<String>,
    pub rolled_back_at: Option<String>,
}

/// Recent startup changes NOCTURN made, from the change journal.
pub fn recent_changes(db: &Db, limit: i64) -> AppResult<Vec<StartupChange>> {
    recent_changes_for(db, "startup", limit)
}

/// Recent journaled changes made under one journal ID (startup, service, …).
pub fn recent_changes_for(db: &Db, tweak_id: &str, limit: i64) -> AppResult<Vec<StartupChange>> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT c.tx_id, c.target, c.status, t.source, c.applied_at, c.rolled_back_at
         FROM changes c JOIN transactions t ON t.id = c.tx_id
         WHERE c.tweak_id = ?1 ORDER BY c.id DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(rusqlite::params![tweak_id, limit], |r| {
            Ok(StartupChange { tx_id: r.get(0)?, target: r.get(1)?, status: r.get(2)?, source: r.get(3)?, applied_at: r.get(4)?, rolled_back_at: r.get(5)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_the_only_reason() {
        let r = review_reasons("x", Some(r"C:\gone\app.exe"), Some(false), None, None, false);
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("no longer exists"));
    }

    #[test]
    fn script_hosts_and_dll_loaders_are_flagged() {
        let ps = review_reasons("powershell -enc AAA", Some(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"), Some(true), Some(Signature::Unsigned), Some("Microsoft"), true);
        assert!(ps.iter().any(|m| m.contains("script host")));
        let dll = review_reasons(r"rundll32.exe C:\Users\a\AppData\Roaming\x.dll,Run", Some(r"C:\Windows\System32\rundll32.exe"), Some(true), Some(Signature::Unsigned), Some("Microsoft"), true);
        assert!(dll.iter().any(|m| m.contains("DLL")));
        let sys_dll = review_reasons(r"rundll32.exe C:\Windows\System32\shell32.dll,Control_RunDLL", Some(r"C:\Windows\System32\rundll32.exe"), Some(true), Some(Signature::Unsigned), Some("Microsoft"), true);
        assert!(sys_dll.is_empty(), "{sys_dll:?}");
    }

    #[test]
    fn signed_per_user_apps_are_not_flagged() {
        let discord = review_reasons("Discord.exe", Some(r"C:\Users\a\AppData\Local\Discord\Update.exe"), Some(true), Some(Signature::Valid), Some("Discord Inc."), false);
        assert!(discord.is_empty(), "{discord:?}");
        let unsigned = review_reasons("x.exe", Some(r"C:\Users\a\AppData\Roaming\x\x.exe"), Some(true), Some(Signature::Unsigned), None, false);
        assert_eq!(unsigned.len(), 2, "{unsigned:?}");
        let signed_anon = review_reasons("o.exe", Some(r"C:UsersaAppDataLocalProgramsOllamao.exe"), Some(true), Some(Signature::Valid), None, false);
        assert!(signed_anon.is_empty(), "{signed_anon:?}");
        let temp = review_reasons("x.exe", Some(r"C:\Users\a\AppData\Local\Temp\x.exe"), Some(true), Some(Signature::Valid), Some("X"), false);
        assert!(temp[0].contains("temporary"));
        let tampered = review_reasons("x.exe", Some(r"C:\Program Files\X\x.exe"), Some(true), Some(Signature::Invalid), Some("X"), false);
        assert!(tampered[0].contains("signature"));
    }

    #[test]
    fn real_entries_get_insights() {
        for item in with_insights() {
            let i = item.insight.expect("insight filled");
            if item.file_exists == Some(true) && !item.command.is_empty() {
                assert!(i.target.is_some(), "{}", item.name);
            }
            eprintln!("{} | {:?} | {:?} | {:?}", item.name, i.publisher, i.signature, i.review);
        }
    }
}
