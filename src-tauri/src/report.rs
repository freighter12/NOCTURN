//! "Report a problem": one plain-text file someone can send to whoever
//! supports them. Built from NOCTURN's own information only — version, PC
//! summary, settings that matter (never keys), recent warnings/errors and the
//! user's description — and scrubbed of personal details before anyone sees
//! it: the Windows user name, PC name, email addresses, IP addresses and
//! anything that looks like a key or token.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReportInput {
    /// What happened, in the user's words.
    #[serde(default)]
    pub note: String,
    /// Recent errors caught on the page.
    #[serde(default)]
    pub page_errors: Vec<String>,
    /// The page that was open.
    #[serde(default)]
    pub page: Option<String>,
    /// Include recent log lines.
    #[serde(default = "yes")]
    pub include_logs: bool,
}

fn yes() -> bool {
    true
}

// ------------------------------------------------------------------ redaction

fn is_key_like(w: &str) -> bool {
    let t = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
    let lower = t.to_ascii_lowercase();
    (lower.starts_with("sk-") && t.len() > 12)
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("xox")
        || (lower.starts_with("eyj") && t.len() > 40) // JWTs
        || (t.len() >= 32 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && t.chars().any(|c| c.is_ascii_digit()) && t.chars().any(|c| c.is_ascii_alphabetic()) && !t.contains('-'))
}

fn is_email(w: &str) -> bool {
    let t = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.' && c != '_' && c != '-' && c != '+');
    match t.split_once('@') {
        Some((a, b)) => !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.'),
        None => false,
    }
}

fn is_ipv4(w: &str) -> bool {
    let t = w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
    let parts: Vec<&str> = t.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255)) && t != "0.0.0.0" && t != "127.0.0.1"
}

fn is_ipv6(w: &str) -> bool {
    let t = w.trim_matches(|c: char| !c.is_ascii_hexdigit() && c != ':');
    // Real addresses use "::" or have 5+ colons; times like 12:30:45 have two.
    let shaped = t.contains("::") || t.matches(':').count() >= 5;
    shaped && t.len() >= 6 && t.split(':').all(|g| g.len() <= 4) && t.chars().all(|c| c.is_ascii_hexdigit() || c == ':') && t.chars().any(|c| c.is_ascii_hexdigit()) && t != "::1"
}

/// Replace a word's core (keeping surrounding punctuation like quotes or commas).
fn replace_core(w: &str, keep: impl Fn(char) -> bool, with: &str) -> String {
    let start = w.find(|c: char| keep(c)).unwrap_or(0);
    let end = w.rfind(|c: char| keep(c)).map(|i| i + w[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1)).unwrap_or(w.len());
    format!("{}{}{}", &w[..start], with, &w[end..])
}

/// Remove personal details. `names` are the Windows user name and PC name.
pub fn redact(text: &str, names: &[String]) -> String {
    let mut s = text.to_string();
    for n in names.iter().filter(|n| n.chars().count() >= 3) {
        // Case-insensitive replace.
        let lower = s.to_lowercase();
        let ln = n.to_lowercase();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        while let Some(pos) = lower[i..].find(&ln) {
            let at = i + pos;
            let end = at + ln.len();
            // Whole words only: a user called "Admin" mustn't turn "administrator" into "<user>istrator".
            let before = s[..at].chars().next_back();
            let after = s[end..].chars().next();
            let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            out.push_str(&s[i..at]);
            if boundary(before) && boundary(after) {
                out.push_str("<user>");
            } else {
                out.push_str(&s[at..end]);
            }
            i = end;
        }
        out.push_str(&s[i..]);
        s = out;
    }
    s.lines()
        .map(|line| {
            line.split(' ')
                .map(|w| {
                    if is_email(w) {
                        replace_core(w, |c| c.is_alphanumeric(), "<email>")
                    } else if is_ipv4(w) {
                        replace_core(w, |c| c.is_ascii_digit(), "<ip>")
                    } else if is_ipv6(w) {
                        replace_core(w, |c| c.is_ascii_hexdigit(), "<ip>")
                    } else if is_key_like(w) {
                        replace_core(w, |c| c.is_alphanumeric(), "<key>")
                    } else {
                        w.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn personal_names() -> Vec<String> {
    let mut v = Vec::new();
    for k in ["USERNAME", "COMPUTERNAME", "USERDOMAIN"] {
        if let Ok(x) = std::env::var(k) {
            if !x.is_empty() && !v.contains(&x) {
                v.push(x);
            }
        }
    }
    // The profile folder name can differ from USERNAME.
    if let Some(p) = std::env::var_os("USERPROFILE") {
        if let Some(n) = Path::new(&p).file_name().map(|f| f.to_string_lossy().to_string()) {
            if !v.contains(&n) {
                v.push(n);
            }
        }
    }
    // Longest first, so "Admin-PC" goes before "Admin".
    v.sort_by_key(|n| std::cmp::Reverse(n.len()));
    v
}

// ------------------------------------------------------------------ logs

/// Warnings/errors from the last few daily logs, and the tail of today's.
fn log_excerpt(dir: &Path) -> (Vec<String>, Vec<String>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with("nocturn.log"))).collect())
        .unwrap_or_default();
    files.sort();
    let recent: Vec<&PathBuf> = files.iter().rev().take(3).collect();
    let mut problems = Vec::new();
    for f in recent.iter().rev() {
        if let Ok(t) = std::fs::read_to_string(f) {
            problems.extend(t.lines().filter(|l| l.contains(" WARN ") || l.contains(" ERROR ")).map(|l| l.chars().take(400).collect::<String>()));
        }
    }
    let keep = problems.len().saturating_sub(120);
    let problems = problems.split_off(keep);
    let tail = recent
        .first()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| {
            let lines: Vec<String> = t.lines().filter(|l| !l.contains(" DEBUG ") && !l.contains(" TRACE ")).map(|l| l.chars().take(400).collect()).collect();
            let skip = lines.len().saturating_sub(80);
            lines.into_iter().skip(skip).collect()
        })
        .unwrap_or_default();
    (problems, tail)
}

// ------------------------------------------------------------------ report

fn mb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1_073_741_824.0)
}

pub fn build(st: &AppState, input: &ReportInput) -> String {
    let mut r = String::new();
    let push = |r: &mut String, s: &str| {
        r.push_str(s);
        r.push('\n');
    };
    push(&mut r, "NOCTURN problem report");
    push(&mut r, &format!("Version {} · created {}", env!("CARGO_PKG_VERSION"), chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")));
    push(&mut r, "Personal details (user name, PC name, email and IP addresses, keys) have been removed.");
    push(&mut r, "");
    push(&mut r, "== What happened ==");
    let note = input.note.trim();
    push(&mut r, if note.is_empty() { "(no description given)" } else { note });
    if let Some(p) = input.page.as_deref().filter(|p| !p.is_empty()) {
        push(&mut r, &format!("Page open: {}", p.chars().take(60).collect::<String>()));
    }
    push(&mut r, "");

    push(&mut r, "== This PC ==");
    let info = st.system_info(false);
    let os = &info.os;
    // "Windows 11 Pro" already names the edition; don't add "Professional" after it.
    let edition = os.edition.clone().filter(|e| { let n = os.name.to_lowercase(); let e3: String = e.to_lowercase().chars().take(3).collect(); !n.contains(&e3) }).map(|e| format!(" {e}")).unwrap_or_default();
    push(&mut r, &format!("{}{} ({}) build {}{}{}", os.name, edition, os.display_version.clone().unwrap_or_default(), os.build, os.ubr.map(|u| format!(".{u}")).unwrap_or_default(), os.architecture.as_deref().map(|a| format!(" · {a}")).unwrap_or_default()));
    push(&mut r, &format!("CPU: {} ({} cores / {} threads)", info.cpu.name, info.cpu.cores.map(|c| c.to_string()).unwrap_or("?".into()), info.cpu.threads.map(|c| c.to_string()).unwrap_or("?".into())));
    for g in &info.gpus {
        push(&mut r, &format!("GPU: {} · driver {}{}", g.name, g.driver_version.clone().unwrap_or("?".into()), g.vram_bytes.map(|v| format!(" · {}", mb(v))).unwrap_or_default()));
    }
    if let Some(m) = info.memory_total_bytes {
        push(&mut r, &format!("RAM: {}", mb(m)));
    }
    push(&mut r, &format!("Laptop: {} · running as {}", if info.is_laptop { "yes" } else { "no" }, if info.is_elevated { "administrator" } else { "standard user" }));
    push(&mut r, "");

    push(&mut r, "== NOCTURN settings ==");
    let s = crate::settings::load(&st.db);
    push(&mut r, &format!("AI: {} · model {} · system access {}", s.ai.provider, s.ai.model, if s.ai.allow_system_context { "on" } else { "off" }));
    push(&mut r, &format!("Keep running in tray: {} · notifications: {} · update checks: {} · music streaming: {}", s.close_to_tray, s.desktop_notifications, s.auto_update_check, s.music_streaming));
    let games = st.games.games.read().map(|g| g.clone()).unwrap_or_default();
    let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
    for g in &games {
        *by.entry(g.launcher).or_default() += 1;
    }
    push(&mut r, &format!("Games found: {}{}", games.len(), if by.is_empty() { String::new() } else { format!(" ({})", by.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")) }));
    push(&mut r, "");

    push(&mut r, "== Recent problems NOCTURN recorded (last 7 days) ==");
    let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let rows: Vec<String> = st
        .db
        .conn()
        .prepare("SELECT ts, level, event, COALESCE(detail, '') FROM audit_log WHERE level IN ('warn', 'error') AND ts >= ?1 ORDER BY id DESC LIMIT 40")
        .and_then(|mut q| q.query_map([since], |row| Ok(format!("{} {} {} {}", row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?.chars().take(300).collect::<String>())))?.collect())
        .unwrap_or_default();
    if rows.is_empty() {
        push(&mut r, "(none)");
    }
    for l in rows {
        push(&mut r, &l);
    }
    push(&mut r, "");

    if !input.page_errors.is_empty() {
        push(&mut r, "== Errors on the page ==");
        for e in input.page_errors.iter().take(30) {
            push(&mut r, &e.chars().take(500).collect::<String>());
        }
        push(&mut r, "");
    }

    if input.include_logs {
        let (problems, tail) = log_excerpt(&st.log_dir);
        push(&mut r, "== Warnings and errors from NOCTURN's log (last 3 days) ==");
        if problems.is_empty() {
            push(&mut r, "(none)");
        }
        for l in problems {
            push(&mut r, &l);
        }
        push(&mut r, "");
        push(&mut r, "== Latest log lines ==");
        for l in tail {
            push(&mut r, &l);
        }
    }
    redact(&r, &personal_names())
}

/// Save the report to the Desktop (falls back to the data folder) and return where.
pub fn save(st: &AppState, text: &str) -> AppResult<PathBuf> {
    if text.len() > 2_000_000 {
        return Err(AppError::Invalid("report too large".into()));
    }
    let desktop = desktop_dir().filter(|d| d.is_dir());
    let path = save_in(&desktop.unwrap_or_else(|| st.data_dir.join("reports")), text)?;
    st.db.audit("info", "user", "report.save", None);
    Ok(path)
}

fn save_in(dir: &Path, text: &str) -> AppResult<PathBuf> {
    let name = format!("NOCTURN report {}.txt", chrono::Local::now().format("%Y-%m-%d %H-%M"));
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    // Re-scrub in case the text was changed on the page.
    std::fs::write(&path, redact(text, &personal_names()))?;
    Ok(path)
}

#[cfg(windows)]
fn desktop_dir() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

#[cfg(not(windows))]
fn desktop_dir() -> Option<PathBuf> {
    None
}

/// Show the saved file in File Explorer.
pub fn reveal(path: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(AppError::NotFound("report".into()));
    }
    crate::library::shell_run("explorer.exe", Some(&format!("/select,\"{}\"", path.display())), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_details_are_removed() {
        let names = vec!["DESKTOP-ABC123".to_string(), "Freighter".to_string()];
        let text = "Path C:\\Users\\Freighter\\AppData x\nPC DESKTOP-ABC123 joined\nmail me: someone@example.com, thanks\nDNS 192.168.1.1 and 2606:4700:4700::1111 ok\nkey sk-ant-api03-abcdefghijklmnop and token ghp_abcdefghijklmnopqrstuvwxyz0123\nversion 0.3.0 at 127.0.0.1 is fine; 12:30:45 is a time";
        let r = redact(text, &names);
        assert!(r.contains(r"C:\Users\<user>\AppData"), "{r}");
        assert!(r.contains("PC <user> joined"));
        assert!(r.contains("mail me: <email>, thanks"));
        assert!(r.contains("DNS <ip> and <ip> ok"));
        assert!(r.contains("key <key> and token <key>"));
        assert!(r.contains("version 0.3.0 at 127.0.0.1 is fine; 12:30:45 is a time"), "{r}");
        assert!(!r.to_lowercase().contains("freighter"));
    }

    #[test]
    fn names_are_replaced_as_whole_words_only() {
        let r = redact(r"running as administrator; C:\Users\Admin\Desktop; Admin-PC; Administrators group; admin", &["Admin".to_string()]);
        assert_eq!(r, r"running as administrator; C:\Users\<user>\Desktop; <user>-PC; Administrators group; <user>");
    }

    #[test]
    fn saved_file_is_scrubbed_again() {
        let d = tempfile::tempdir().unwrap();
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "nobody-here".into());
        let p = save_in(d.path(), &format!("hello {user}, mail x@example.com from 10.0.0.7")).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("NOCTURN report "));
        assert!(t.contains("<email>") && t.contains("<ip>"), "{t}");
        if user.len() >= 3 {
            assert!(!t.contains(&format!(" {user},")), "{t}");
        }
    }

    #[test]
    fn ordinary_words_survive() {
        let r = redact("Steam game Counter-Strike 2 crashed with 0xC0000005 in nvwgf2umx.dll", &[]);
        assert_eq!(r, "Steam game Counter-Strike 2 crashed with 0xC0000005 in nvwgf2umx.dll");
        // A long hash-like word is treated as a possible key.
        assert!(redact("token a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8", &[]).contains("<key>"));
    }
}
