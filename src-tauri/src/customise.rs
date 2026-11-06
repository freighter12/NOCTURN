//! Customise — taskbar, File Explorer and notification settings.
//!
//! Unlike the optimisation catalogue (on/off tweaks), each setting here is a
//! choice between documented values of one per-user registry value. Rules:
//! * Per-user (HKCU) only, so no administrator approval is ever needed and
//!   the elevated worker refuses these operations.
//! * Only the compiled-in settings and choices below can be written.
//! * "Windows default" removes the value (`ChangeOp::RegistryDelete`) so
//!   Windows falls back to its built-in behaviour, instead of NOCTURN guessing
//!   what that default is on a given build.
//! * Every change goes through the journal and can be undone from History.

use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::tweaks::engine::{self, ApplyReport, PlannedOp, Progress};
use crate::tweaks::registry;
use crate::tweaks::{ChangeOp, Hive, RegData, RegSetting, Takes};

/// The `tweak_id` used for every journaled Customise change.
pub const JOURNAL_ID: &str = "customise";
/// Choice ID meaning "remove the value; let Windows decide".
pub const DEFAULT: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Taskbar,
    Explorer,
    Notifications,
}

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub id: &'static str,
    pub label: &'static str,
    #[serde(skip)]
    pub data: RegData,
}

#[derive(Debug, Clone, Serialize)]
pub struct Setting {
    pub id: &'static str,
    pub section: Section,
    pub title: &'static str,
    pub description: &'static str,
    #[serde(skip)]
    pub path: &'static str,
    #[serde(skip)]
    pub name: &'static str,
    pub choices: Vec<Choice>,
    /// The choice Windows behaves like when the value is not set, if it is
    /// the same on every supported build. `None` = varies by build/edition.
    pub windows_default: Option<&'static str>,
    pub takes: Takes,
    /// First Windows build where the setting exists.
    pub min_build: Option<u32>,
}

impl Setting {
    pub fn target(&self) -> String {
        format!(r"HKCU\{}\{}", self.path, self.name)
    }

    fn choice(&self, id: &str) -> Option<&Choice> {
        self.choices.iter().find(|c| c.id == id)
    }

    fn label_of(&self, choice: &str) -> String {
        if choice == DEFAULT {
            return match self.windows_default.and_then(|d| self.choice(d)) {
                Some(c) => format!("Windows default ({})", c.label.to_lowercase()),
                None => "Windows default".into(),
            };
        }
        self.choice(choice).map(|c| c.label.to_string()).unwrap_or_else(|| choice.to_string())
    }

    /// The journaled operation that selects `choice`.
    fn op(&self, choice: &str) -> AppResult<ChangeOp> {
        if choice == DEFAULT {
            return Ok(ChangeOp::RegistryDelete { hive: Hive::Hkcu, path: self.path.into(), name: self.name.into() });
        }
        let c = self
            .choice(choice)
            .ok_or_else(|| AppError::Invalid(format!("'{choice}' is not an option for {}", self.title)))?;
        Ok(ChangeOp::Registry(RegSetting {
            hive: Hive::Hkcu,
            path: self.path.into(),
            name: self.name.into(),
            data: c.data.clone(),
        }))
    }
}

const ADV: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const EXPLORER: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer";
const NOTIF: &str = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings";

fn c(id: &'static str, label: &'static str, v: u32) -> Choice {
    Choice { id, label, data: RegData::Dword(v) }
}
fn on_off(on: u32, off: u32) -> Vec<Choice> {
    vec![c("on", "On", on), c("off", "Off", off)]
}
fn show_hide(show: u32, hide: u32) -> Vec<Choice> {
    vec![c("show", "Show", show), c("hide", "Hide", hide)]
}

fn build() -> Vec<Setting> {
    use Section::*;
    vec![
        // ------------------------------------------------------------ Taskbar
        Setting {
            id: "taskbar.alignment",
            section: Taskbar,
            title: "Taskbar alignment",
            description: "Where the Start button and app icons sit on the taskbar.",
            path: ADV,
            name: "TaskbarAl",
            choices: vec![c("left", "Left", 0), c("center", "Centre", 1)],
            windows_default: Some("center"),
            takes: Takes::Immediately,
            min_build: Some(22000),
        },
        Setting {
            id: "taskbar.search",
            section: Taskbar,
            title: "Search on the taskbar",
            description: "How the search entry appears on the taskbar. Search from Start keeps working whichever you pick.",
            path: r"Software\Microsoft\Windows\CurrentVersion\Search",
            name: "SearchboxTaskbarMode",
            choices: vec![c("hidden", "Hidden", 0), c("icon", "Icon only", 1), c("box", "Search box", 2), c("label", "Icon and label", 3)],
            windows_default: None,
            takes: Takes::Immediately,
            min_build: None,
        },
        Setting {
            id: "taskbar.task_view",
            section: Taskbar,
            title: "Task View button",
            description: "The button that shows open windows and virtual desktops. Win+Tab works either way.",
            path: ADV,
            name: "ShowTaskViewButton",
            choices: show_hide(1, 0),
            windows_default: Some("show"),
            takes: Takes::Immediately,
            min_build: None,
        },
        Setting {
            id: "taskbar.combine",
            section: Taskbar,
            title: "Combine taskbar buttons",
            description: "Whether several windows of one app share a single taskbar button, and whether labels are shown.",
            path: ADV,
            name: "TaskbarGlomLevel",
            choices: vec![c("always", "Always", 0), c("when_full", "When the taskbar is full", 1), c("never", "Never", 2)],
            windows_default: Some("always"),
            takes: Takes::Immediately,
            min_build: Some(22631),
        },
        Setting {
            id: "taskbar.seconds",
            section: Taskbar,
            title: "Seconds in the clock",
            description: "Shows seconds in the taskbar clock. Uses slightly more power on laptops.",
            path: ADV,
            name: "ShowSecondsInSystemClock",
            choices: on_off(1, 0),
            windows_default: Some("off"),
            takes: Takes::Immediately,
            min_build: Some(22621),
        },
        Setting {
            id: "taskbar.end_task",
            section: Taskbar,
            title: "\"End task\" on right-click",
            description: "Adds End task to a taskbar button's right-click menu, to close an app that has stopped responding without opening Task Manager.",
            path: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings",
            name: "TaskbarEndTask",
            choices: on_off(1, 0),
            windows_default: Some("off"),
            takes: Takes::Immediately,
            min_build: Some(22631),
        },
        // ----------------------------------------------------------- Explorer
        Setting {
            id: "explorer.extensions",
            section: Explorer,
            title: "File name extensions",
            description: "Shows the end of file names such as .exe or .pdf. Seeing them makes it easier to spot a program pretending to be a document.",
            path: ADV,
            name: "HideFileExt",
            choices: show_hide(0, 1),
            windows_default: Some("hide"),
            takes: Takes::Immediately,
            min_build: None,
        },
        Setting {
            id: "explorer.hidden",
            section: Explorer,
            title: "Hidden files and folders",
            description: "Shows items marked hidden, such as AppData. Protected operating-system files stay hidden either way.",
            path: ADV,
            name: "Hidden",
            choices: show_hide(1, 2),
            windows_default: Some("hide"),
            takes: Takes::Immediately,
            min_build: None,
        },
        Setting {
            id: "explorer.launch_to",
            section: Explorer,
            title: "Open File Explorer to",
            description: "The first page File Explorer shows when you open it.",
            path: ADV,
            name: "LaunchTo",
            choices: vec![c("this_pc", "This PC", 1), c("home", "Home", 2)],
            windows_default: Some("home"),
            takes: Takes::Immediately,
            min_build: Some(22000),
        },
        Setting {
            id: "explorer.compact",
            section: Explorer,
            title: "Compact view",
            description: "Reduces the space between items so more files fit on screen.",
            path: ADV,
            name: "UseCompactMode",
            choices: on_off(1, 0),
            windows_default: Some("off"),
            takes: Takes::AppRestart,
            min_build: Some(22000),
        },
        Setting {
            id: "explorer.full_path",
            section: Explorer,
            title: "Full path in the title bar",
            description: "Shows the whole folder path (for example C:\\Users\\you\\Documents) in the window title.",
            path: r"Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState",
            name: "FullPath",
            choices: on_off(1, 0),
            windows_default: Some("off"),
            takes: Takes::AppRestart,
            min_build: None,
        },
        Setting {
            id: "explorer.recent",
            section: Explorer,
            title: "Recently used files in Home",
            description: "Lists files you opened recently on File Explorer's Home page.",
            path: EXPLORER,
            name: "ShowRecent",
            choices: show_hide(1, 0),
            windows_default: Some("show"),
            takes: Takes::AppRestart,
            min_build: None,
        },
        Setting {
            id: "explorer.frequent",
            section: Explorer,
            title: "Frequently used folders in Home",
            description: "Lists folders you open often on File Explorer's Home page. Pinned folders stay either way.",
            path: EXPLORER,
            name: "ShowFrequent",
            choices: show_hide(1, 0),
            windows_default: Some("show"),
            takes: Takes::AppRestart,
            min_build: None,
        },
        Setting {
            id: "explorer.sync_notifications",
            section: Explorer,
            title: "Sync provider notifications",
            description: "Banners inside File Explorer from OneDrive and other sync apps, including Microsoft 365 promotions.",
            path: ADV,
            name: "ShowSyncProviderNotifications",
            choices: show_hide(1, 0),
            windows_default: Some("show"),
            takes: Takes::AppRestart,
            min_build: None,
        },
        // ------------------------------------------------------ Notifications
        Setting {
            id: "notifications.banners",
            section: Notifications,
            title: "Notifications",
            description: "Lets apps and Windows show notification banners. Turning this off silences every app, including reminders and messages.",
            path: r"Software\Microsoft\Windows\CurrentVersion\PushNotifications",
            name: "ToastEnabled",
            choices: on_off(1, 0),
            windows_default: Some("on"),
            takes: Takes::SignOut,
            min_build: None,
        },
        Setting {
            id: "notifications.lock_screen",
            section: Notifications,
            title: "Notifications on the lock screen",
            description: "Shows notification banners while the PC is locked, where anyone nearby can read them.",
            path: NOTIF,
            name: "NOC_GLOBAL_SETTING_ALLOW_TOASTS_ABOVE_LOCK",
            choices: on_off(1, 0),
            windows_default: Some("on"),
            takes: Takes::SignOut,
            min_build: None,
        },
        Setting {
            id: "notifications.sound",
            section: Notifications,
            title: "Notification sounds",
            description: "Plays a sound when a notification arrives.",
            path: NOTIF,
            name: "NOC_GLOBAL_SETTING_ALLOW_NOTIFICATION_SOUND",
            choices: on_off(1, 0),
            windows_default: Some("on"),
            takes: Takes::SignOut,
            min_build: None,
        },
        Setting {
            id: "notifications.welcome",
            section: Notifications,
            title: "Welcome experience after updates",
            description: "The full-screen \"what's new\" page Windows sometimes shows after an update or when you sign in.",
            path: r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
            name: "SubscribedContent-310093Enabled",
            choices: on_off(1, 0),
            windows_default: Some("on"),
            takes: Takes::SignOut,
            min_build: None,
        },
        Setting {
            id: "notifications.finish_setup",
            section: Notifications,
            title: "\"Finish setting up\" suggestions",
            description: "Prompts suggesting ways to finish setting up the device, such as Microsoft 365 or OneDrive backup offers.",
            path: r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement",
            name: "ScoobeSystemSettingEnabled",
            choices: on_off(1, 0),
            windows_default: Some("on"),
            takes: Takes::SignOut,
            min_build: None,
        },
    ]
}

pub fn all() -> &'static [Setting] {
    static ALL: OnceLock<Vec<Setting>> = OnceLock::new();
    ALL.get_or_init(build)
}

pub fn get(id: &str) -> Option<&'static Setting> {
    all().iter().find(|s| s.id == id)
}

fn os_build() -> Option<u32> {
    registry::read(Hive::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuild")
        .ok()
        .flatten()
        .and_then(|v| v.as_string())
        .and_then(|s| s.trim().parse().ok())
}

// ------------------------------------------------------------------ views

#[derive(Debug, Clone, Serialize)]
pub struct SettingView {
    #[serde(flatten)]
    pub setting: Setting,
    pub target: String,
    /// The matching choice ID, `"default"` when the value is not set, or
    /// `None` when the value is something NOCTURN doesn't offer.
    pub current: Option<String>,
    /// Human-readable current state.
    pub current_label: String,
    pub available: bool,
    pub error: Option<String>,
}

fn current_of(s: &Setting) -> (Option<String>, String, Option<String>) {
    match registry::read(Hive::Hkcu, s.path, s.name) {
        Ok(None) => (Some(DEFAULT.into()), s.label_of(DEFAULT), None),
        Ok(Some(v)) => match s.choices.iter().find(|c| v.matches(&c.data)) {
            Some(c) => (Some(c.id.into()), c.label.into(), None),
            None => (None, format!("Custom value {}", v.display()), None),
        },
        Err(e) => (None, "Unreadable".into(), Some(e.to_string())),
    }
}

pub fn list() -> Vec<SettingView> {
    let build = os_build();
    all()
        .iter()
        .map(|s| {
            let (current, current_label, error) = current_of(s);
            SettingView {
                setting: s.clone(),
                target: s.target(),
                current,
                current_label,
                available: match (s.min_build, build) {
                    (Some(min), Some(b)) => b >= min,
                    _ => true,
                },
                error,
            }
        })
        .collect()
}

// ---------------------------------------------------------- preview/apply

#[derive(Debug, Clone, Deserialize)]
pub struct Selection {
    pub id: String,
    /// A choice ID, or `"default"`.
    pub choice: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub id: String,
    pub title: String,
    pub target: String,
    pub current: String,
    pub proposed: String,
    pub will_change: bool,
    pub takes: Takes,
}

fn plan(sel: &[Selection]) -> AppResult<Vec<(PlannedOp, PreviewItem)>> {
    if sel.is_empty() {
        return Err(AppError::Invalid("nothing selected".into()));
    }
    if sel.len() > all().len() {
        return Err(AppError::Invalid("too many selections".into()));
    }
    let mut out: Vec<(PlannedOp, PreviewItem)> = Vec::new();
    for x in sel {
        let s = get(&x.id).ok_or_else(|| AppError::Invalid(format!("unknown setting '{}'", x.id)))?;
        if out.iter().any(|(_, i)| i.id == s.id) {
            return Err(AppError::Invalid(format!("{} was selected twice", s.title)));
        }
        let op = s.op(&x.choice)?;
        let (cur_id, cur_label, _) = current_of(s);
        let will_change = cur_id.as_deref() != Some(x.choice.as_str());
        let mut proposed = s.label_of(&x.choice);
        if will_change && x.choice == DEFAULT && cur_id.is_some() && cur_id.as_deref() == s.windows_default {
            proposed.push_str(" — same behaviour, stored value removed");
        }
        out.push((
            PlannedOp {
                tweak_id: JOURNAL_ID.into(),
                title: format!("{}: {proposed}", s.title),
                takes: s.takes,
                op,
            },
            PreviewItem {
                id: s.id.into(),
                title: s.title.into(),
                target: s.target(),
                current: cur_label,
                proposed,
                will_change,
                takes: s.takes,
            },
        ));
    }
    Ok(out)
}

pub fn preview(sel: &[Selection]) -> AppResult<Vec<PreviewItem>> {
    Ok(plan(sel)?.into_iter().map(|(_, i)| i).collect())
}

/// Selections that reset every setting in `section` (or everything) to the
/// Windows default.
pub fn reset_selection(section: Option<Section>) -> Vec<Selection> {
    all()
        .iter()
        .filter(|s| section.is_none_or(|x| s.section == x))
        .map(|s| Selection { id: s.id.into(), choice: DEFAULT.into() })
        .collect()
}

pub fn apply(db: &Db, data_dir: &Path, sel: &[Selection], label: &str, progress: &dyn Fn(Progress)) -> AppResult<ApplyReport> {
    let ops: Vec<PlannedOp> = plan(sel)?.into_iter().map(|(op, _)| op).collect();
    let label: String = label.chars().filter(|c| !c.is_control()).take(80).collect();
    let mut r = engine::run_plan(db, data_dir, ops, "user", &format!("Customise: {label}"), false, progress)?;
    notify_shell();
    // The engine's wording is written for games; say what applies here.
    for n in &mut r.notes {
        if r.takes == Takes::AppRestart && n.starts_with("Restart any running games") {
            *n = "Close and reopen File Explorer windows to see the change.".into();
        }
    }
    Ok(r)
}

// ------------------------------------------------------------------ shell

/// Tell Explorer and other apps that per-user settings changed, so most take
/// effect without signing out. Harmless if nothing is listening.
pub fn notify_shell() {
    #[cfg(windows)]
    unsafe {
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
        for area in [None, Some(w!("TraySettings")), Some(w!("Policy"))] {
            let lp = area.map(|p| LPARAM(p.as_ptr() as isize)).unwrap_or(LPARAM(0));
            let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), lp, SMTO_ABORTIFHUNG, 1000, None);
        }
    }
}

/// Restart the Windows shell (taskbar and File Explorer windows) so changes
/// marked "app restart" show up. Open File Explorer windows close.
pub fn restart_explorer(db: &Db) -> AppResult<()> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

        let shell = GetShellWindow();
        let mut pid = 0u32;
        if !shell.is_invalid() {
            GetWindowThreadProcessId(shell, Some(&mut pid));
        }
        if pid != 0 {
            let h = OpenProcess(PROCESS_TERMINATE, false, pid)
                .map_err(|e| AppError::msg(format!("couldn't open the Explorer process: {e}")))?;
            let r = TerminateProcess(h, 1);
            let _ = CloseHandle(h);
            r.map_err(|e| AppError::msg(format!("couldn't stop Explorer: {e}")))?;
        }
        // Windows normally restarts the shell itself; start it if it hasn't.
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if !GetShellWindow().is_invalid() {
                db.audit("info", "user", "explorer.restart", Some(serde_json::json!({ "pid": pid, "auto": true })));
                return Ok(());
            }
        }
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::process::Command::new(Path::new(&windir).join("explorer.exe"))
            .spawn()
            .map_err(|e| AppError::msg(format!("Explorer stopped but couldn't be started again: {e}. Press Ctrl+Shift+Esc and run explorer.exe.")))?;
        db.audit("info", "user", "explorer.restart", Some(serde_json::json!({ "pid": pid, "auto": false })));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_unique_and_choices_well_formed() {
        let mut ids = HashSet::new();
        let mut targets = HashSet::new();
        for s in all() {
            assert!(ids.insert(s.id), "duplicate id {}", s.id);
            assert!(targets.insert(s.target()), "{} shares a registry value with another setting", s.id);
            let prefix = match s.section {
                Section::Taskbar => "taskbar.",
                Section::Explorer => "explorer.",
                Section::Notifications => "notifications.",
            };
            assert!(s.id.starts_with(prefix), "{}", s.id);
            assert!(s.choices.len() >= 2, "{}", s.id);
            let mut cids = HashSet::new();
            for c in &s.choices {
                assert_ne!(c.id, DEFAULT, "{}: 'default' is reserved", s.id);
                assert!(cids.insert(c.id), "{}: duplicate choice {}", s.id, c.id);
            }
            if let Some(d) = s.windows_default {
                assert!(s.choice(d).is_some(), "{}: default {d} is not a choice", s.id);
            }
        }
    }

    #[test]
    fn only_per_user_values_outside_security_areas() {
        let forbidden = ["policies", "defender", "smartscreen", "windowsupdate", "\\services\\", "firewall"];
        for s in all() {
            for choice in s.choices.iter().map(|c| c.id).chain([DEFAULT]) {
                let op = s.op(choice).unwrap();
                assert!(!op.needs_elevation(), "{} must be HKCU", s.id);
            }
            let p = s.target().to_ascii_lowercase();
            for f in forbidden {
                assert!(!p.contains(f), "{} touches {p}", s.id);
            }
        }
    }

    #[test]
    fn no_overlap_with_the_optimisation_catalogue() {
        use crate::tweaks::{catalog, Action};
        for t in catalog::all() {
            if let Action::Registry { settings } = &t.action {
                for r in settings {
                    assert!(all().iter().all(|s| s.target() != r.target()), "{} overlaps {}", t.id, r.target());
                }
            }
        }
    }

    #[test]
    fn plan_validates_input() {
        let sel = |id: &str, choice: &str| vec![Selection { id: id.into(), choice: choice.into() }];
        assert!(preview(&[]).is_err());
        assert!(preview(&sel("nope", "on")).is_err());
        assert!(preview(&sel("taskbar.seconds", "sideways")).is_err());
        let twice = vec![
            Selection { id: "taskbar.seconds".into(), choice: "on".into() },
            Selection { id: "taskbar.seconds".into(), choice: "off".into() },
        ];
        assert!(preview(&twice).is_err());
        let p = preview(&sel("taskbar.seconds", DEFAULT)).unwrap();
        assert!(p[0].proposed.starts_with("Windows default"));
        assert_eq!(reset_selection(Some(Section::Taskbar)).len(), all().iter().filter(|s| s.section == Section::Taskbar).count());
    }

    #[test]
    fn elevated_worker_refuses_customise_ops() {
        let s = get("explorer.extensions").unwrap();
        let op = s.op(DEFAULT).unwrap();
        assert!(matches!(op, ChangeOp::RegistryDelete { .. }));
        assert!(crate::tweaks::elevated::validate_for_test(JOURNAL_ID, &op).is_err());
    }

    #[test]
    fn list_reads_every_setting() {
        assert_eq!(list().len(), all().len());
    }

    /// Apply a choice, reset to default, then undo both on the real HKCU
    /// value; the original must come back byte-for-byte. Run explicitly:
    /// `cargo test -- --ignored customise_roundtrip`.
    #[test]
    #[ignore]
    fn customise_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let s = get("explorer.full_path").unwrap();
        let before = registry::read(Hive::Hkcu, s.path, s.name).unwrap();
        let sel = |c: &str| vec![Selection { id: s.id.into(), choice: c.into() }];

        let flip = if current_of(s).0.as_deref() == Some("on") { "off" } else { "on" };
        let a = apply(&db, dir.path(), &sel(flip), "test", &|_| {}).unwrap();
        assert_eq!(a.status, "completed", "{a:?}");
        assert_eq!(current_of(s).0.as_deref(), Some(flip));

        let b = apply(&db, dir.path(), &sel(DEFAULT), "test", &|_| {}).unwrap();
        assert_eq!(b.status, "completed", "{b:?}");
        assert_eq!(registry::read(Hive::Hkcu, s.path, s.name).unwrap(), None);

        engine::rollback(&db, &b.tx_id, &|_| {}).unwrap();
        engine::rollback(&db, &a.tx_id, &|_| {}).unwrap();
        assert_eq!(registry::read(Hive::Hkcu, s.path, s.name).unwrap(), before);
        notify_shell();
    }
}
