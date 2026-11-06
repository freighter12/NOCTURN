//! NOCTURN TWEAK — the optimisation engine.
//!
//! * [`catalog`] — the compiled-in list of supported optimisations. Nothing
//!   outside this list can ever be applied, by the UI *or* the AI.
//! * [`registry`] — faithful raw read/write/delete of single registry values.
//! * [`engine`] — status, preview, journaled apply with verification, rollback.
//! * [`elevated`] — the request handler that runs inside the UAC worker.
//! * [`maintenance`] — one-shot actions (temp cleanup, DNS flush).

pub mod catalog;
pub mod elevated;
pub mod engine;
mod executor;
pub mod journal;
pub mod maintenance;
pub mod registry;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Gaming,
    Performance,
    Network,
    Privacy,
}

/// How cautious the user should be.
/// * `Safe` — reversible, documented, no functional downside for most users.
/// * `Moderate` — reversible but trades something (battery, a Windows feature).
/// * `Advanced` — reversible, effect is hardware/driver dependent or contested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Safe,
    Moderate,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum Hive {
    Hkcu,
    Hklm,
}

impl Hive {
    pub fn label(self) -> &'static str {
        match self {
            Hive::Hkcu => "HKCU",
            Hive::Hklm => "HKLM",
        }
    }
}

/// When the change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Takes {
    Immediately,
    AppRestart,
    SignOut,
    Restart,
}

/// Desired registry data as declared in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum RegData {
    Dword(u32),
    Sz(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegSetting {
    pub hive: Hive,
    pub path: String,
    pub name: String,
    pub data: RegData,
}

impl RegSetting {
    pub fn target(&self) -> String {
        format!("{}\\{}\\{}", self.hive.label(), self.path, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerTarget {
    HighPerformance,
    Balanced,
}

impl PowerTarget {
    pub fn guid(&self) -> &'static str {
        match self {
            PowerTarget::HighPerformance => "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
            PowerTarget::Balanced => "381b4222-f694-41f0-9685-ff5bb260df2e",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Set one or more registry values. Reversible: originals are journaled.
    Registry { settings: Vec<RegSetting> },
    /// Switch the active power scheme. Reversible.
    PowerPlan { target: PowerTarget },
    /// Clear the DNS resolver cache. Harmless, nothing to roll back.
    FlushDns,
    /// Delete files older than 24h from the user's %TEMP%. Not reversible, but
    /// only touches temporary files — never personal folders.
    CleanTempFiles,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tweak {
    pub id: &'static str,
    pub category: Category,
    pub title: &'static str,
    /// One plain-language sentence.
    pub summary: &'static str,
    /// What exactly changes and why it can help.
    pub details: &'static str,
    /// Honest downside / caveat, if any.
    pub caveat: Option<&'static str>,
    pub risk: Risk,
    pub takes: Takes,
    pub action: Action,
}

impl Tweak {
    pub fn requires_admin(&self) -> bool {
        match &self.action {
            Action::Registry { settings } => settings.iter().any(|s| s.hive == Hive::Hklm),
            _ => false,
        }
    }

    pub fn reversible(&self) -> bool {
        matches!(self.action, Action::Registry { .. } | Action::PowerPlan { .. })
    }

    pub fn is_one_shot(&self) -> bool {
        matches!(self.action, Action::FlushDns | Action::CleanTempFiles)
    }
}

/// Where a startup entry is registered. Each location has a matching
/// `StartupApproved` key — the same mechanism Task Manager's Startup tab uses
/// to enable/disable entries without deleting them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StartupLocation {
    RunUser,
    RunMachine,
    RunMachine32,
    FolderUser,
    FolderCommon,
}

const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

impl StartupLocation {
    pub const ALL: [StartupLocation; 5] = [
        StartupLocation::RunUser,
        StartupLocation::RunMachine,
        StartupLocation::RunMachine32,
        StartupLocation::FolderUser,
        StartupLocation::FolderCommon,
    ];

    pub fn is_machine(self) -> bool {
        !matches!(self, StartupLocation::RunUser | StartupLocation::FolderUser)
    }

    /// (hive, path) of the StartupApproved key for this location.
    pub fn approved_key(self) -> (Hive, String) {
        match self {
            StartupLocation::RunUser => (Hive::Hkcu, format!(r"{APPROVED}\Run")),
            StartupLocation::RunMachine => (Hive::Hklm, format!(r"{APPROVED}\Run")),
            StartupLocation::RunMachine32 => (Hive::Hklm, format!(r"{APPROVED}\Run32")),
            StartupLocation::FolderUser => (Hive::Hkcu, format!(r"{APPROVED}\StartupFolder")),
            StartupLocation::FolderCommon => (Hive::Hklm, format!(r"{APPROVED}\StartupFolder")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StartupLocation::RunUser => "Registry (current user)",
            StartupLocation::RunMachine => "Registry (all users)",
            StartupLocation::RunMachine32 => "Registry (all users, 32-bit)",
            StartupLocation::FolderUser => "Startup folder (current user)",
            StartupLocation::FolderCommon => "Startup folder (all users)",
        }
    }
}

/// A single journaled operation. Serialised into `changes.op_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeOp {
    Registry(RegSetting),
    /// Remove a registry value so Windows falls back to its built-in default
    /// (Customise page "reset to Windows default"). Rollback writes it back.
    RegistryDelete { hive: Hive, path: String, name: String },
    PowerPlan { scheme: String },
    FlushDns,
    CleanTempFiles,
    /// Enable/disable a startup entry via its StartupApproved flag.
    StartupApproval { location: StartupLocation, name: String, enabled: bool },
    /// Static DNS servers for one adapter and address family; empty = automatic.
    /// `interface` is the adapter GUID, `alias` only labels it for people.
    DnsServers { interface: String, alias: String, ipv6: bool, servers: Vec<String> },
}

impl ChangeOp {
    pub fn target(&self) -> String {
        match self {
            ChangeOp::Registry(s) => s.target(),
            ChangeOp::RegistryDelete { hive, path, name } => format!("{}\\{path}\\{name}", hive.label()),
            ChangeOp::PowerPlan { .. } => "Active power plan".into(),
            ChangeOp::FlushDns => "DNS resolver cache".into(),
            ChangeOp::CleanTempFiles => "%TEMP% (files older than 24 hours)".into(),
            ChangeOp::StartupApproval { location, name, .. } => {
                format!("Startup entry \"{name}\" — {}", location.label())
            }
            ChangeOp::DnsServers { alias, ipv6, .. } => {
                format!("DNS servers (IPv{}) — {alias}", if *ipv6 { 6 } else { 4 })
            }
        }
    }

    pub fn needs_elevation(&self) -> bool {
        match self {
            ChangeOp::Registry(s) => s.hive == Hive::Hklm,
            ChangeOp::RegistryDelete { hive, .. } => *hive == Hive::Hklm,
            ChangeOp::StartupApproval { location, .. } => location.is_machine(),
            ChangeOp::DnsServers { .. } => true,
            _ => false,
        }
    }
}

/// The original state captured before an operation, serialised into
/// `changes.original_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Original {
    /// `None` = the value did not exist (rollback deletes it).
    Registry { value: Option<registry::RawValue> },
    PowerPlan { scheme: String },
    /// Static DNS servers before the change; empty = automatic.
    Dns { servers: Vec<String> },
    /// One-shot actions have nothing to restore.
    Nothing,
}

/// Expand a tweak into the concrete operations it performs.
pub fn ops_for(tweak: &Tweak) -> Vec<ChangeOp> {
    match &tweak.action {
        Action::Registry { settings } => settings.iter().cloned().map(ChangeOp::Registry).collect(),
        Action::PowerPlan { target } => vec![ChangeOp::PowerPlan { scheme: target.guid().into() }],
        Action::FlushDns => vec![ChangeOp::FlushDns],
        Action::CleanTempFiles => vec![ChangeOp::CleanTempFiles],
    }
}
