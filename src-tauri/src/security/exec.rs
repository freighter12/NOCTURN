//! Allow-listed process execution.
//!
//! Every external command NOCTURN can run is a variant of [`SafeCommand`].
//! Programs are resolved to absolute paths inside `%SystemRoot%\System32`
//! (never via `PATH`, which could be hijacked), arguments are fixed, and the
//! few dynamic parameters are validated newtypes. PowerShell is only ever
//! invoked with one of the compile-time scripts in [`PsScript`].
//!
//! The one exception to "System32 only" is Nocturn Music streaming: yt-dlp
//! and Deno, which NOCTURN downloads itself (only when the user turns
//! streaming on), verifies against the publishers' SHA-256 checksums and
//! re-checks before running. They get fixed arguments and a validated video ID.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A validated power scheme GUID (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemeGuid(String);

impl SchemeGuid {
    pub fn parse(s: &str) -> AppResult<Self> {
        let s = s.trim().to_ascii_lowercase();
        let groups: Vec<&str> = s.split('-').collect();
        let lens = [8, 4, 4, 4, 12];
        let ok = groups.len() == 5
            && groups
                .iter()
                .zip(lens.iter())
                .all(|(g, &n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()));
        if ok {
            Ok(SchemeGuid(s))
        } else {
            Err(AppError::Invalid(format!("'{s}' is not a valid power scheme GUID")))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Fixed PowerShell scripts. Each one is read-only unless documented otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsScript {
    /// Physical/virtual network adapters with link speed and status.
    NetAdapters,
    /// IPv4/IPv6 DNS servers per interface.
    DnsServers,
    /// Default gateways.
    DefaultGateways,
    /// Critical/error events from the System and Application logs (last 7 days).
    RecentErrorEvents,
    /// Whether System Restore is available and the most recent restore points.
    RestorePointStatus,
    /// **Writes**: creates a restore point (requires elevation). Description is fixed.
    CreateRestorePoint,
    /// Signed drivers (device name, provider, version, date).
    Drivers,
    /// Services with start mode and state.
    Services,
    /// Whether Windows is waiting for a restart (servicing, Windows Update, file renames).
    PendingReboot,
    /// Crash-related events (last 90 days): bugchecks, unexpected shutdowns,
    /// app crashes/hangs, WHEA hardware errors, clean start/stop markers and
    /// the reliability (stability) index. Read-only.
    CrashEvents,
    /// Problem devices, drivers for the device classes people update, and
    /// non-inbox driver installs from the last 90 days. Read-only.
    DriverCentre,
    /// Kernel start, sign-in and boot type for each boot in the last 60 days. Read-only.
    BootTimes,
    /// Every app in the Start menu (desktop and Store) with its AppUserModelID. Read-only.
    StartApps,
    /// Store / Xbox packages that are games (they ship a MicrosoftGame.config). Read-only.
    GamePackages,
}

impl PsScript {
    fn source(self) -> &'static str {
        match self {
            PsScript::NetAdapters => r#"
Get-NetAdapter | Select-Object Name, InterfaceDescription, ifIndex, Status, LinkSpeed, MediaType, PhysicalMediaType, MacAddress, Virtual, DriverVersion, DriverDate | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::DnsServers => r#"
Get-DnsClientServerAddress | Where-Object { $_.ServerAddresses.Count -gt 0 } | Select-Object InterfaceAlias, InterfaceIndex, AddressFamily, ServerAddresses | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::DefaultGateways => r#"
Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Select-Object InterfaceAlias, InterfaceIndex, NextHop, RouteMetric | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::RecentErrorEvents => r#"
$since = (Get-Date).AddDays(-7)
$ev = @()
foreach ($log in 'System','Application') {
  try { $ev += Get-WinEvent -FilterHashtable @{ LogName = $log; Level = 1,2; StartTime = $since } -MaxEvents 60 -ErrorAction Stop } catch {}
}
$ev | Sort-Object TimeCreated -Descending | Select-Object -First 80 @{n='Log';e={$_.LogName}}, Id, ProviderName, @{n='Level';e={$_.Level}}, @{n='Time';e={$_.TimeCreated.ToUniversalTime().ToString('o')}}, @{n='Message';e={ if ($_.Message) { $_.Message.Substring(0, [Math]::Min(400, $_.Message.Length)) } else { '' } }} | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::RestorePointStatus => r#"
$enabled = $false
try { $null = Get-ComputerRestorePoint -ErrorAction Stop; $enabled = $true } catch {}
$rp = @()
try { $rp = Get-ComputerRestorePoint -ErrorAction Stop | Sort-Object SequenceNumber -Descending | Select-Object -First 5 Description, SequenceNumber, @{n='Created';e={ $_.ConvertToDateTime($_.CreationTime).ToUniversalTime().ToString('o') }} } catch {}
@{ queryable = $enabled; points = @($rp) } | ConvertTo-Json -Depth 4 -Compress"#,
            PsScript::CreateRestorePoint => r#"
$ErrorActionPreference = 'Stop'
try {
  Checkpoint-Computer -Description 'NOCTURN before optimisation' -RestorePointType MODIFY_SETTINGS -WarningVariable w -WarningAction SilentlyContinue
  if ($w) { @{ ok = $false; message = ($w | Out-String).Trim() } | ConvertTo-Json -Compress }
  else { @{ ok = $true; message = 'Restore point created' } | ConvertTo-Json -Compress }
} catch { @{ ok = $false; message = $_.Exception.Message } | ConvertTo-Json -Compress }"#,
            PsScript::Drivers => r#"
Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceName -and $_.DriverProviderName -and $_.DriverProviderName -ne 'Microsoft' } | Select-Object DeviceName, DeviceClass, DriverProviderName, DriverVersion, @{n='DriverDate';e={ if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') } else { $null } }}, IsSigned | Sort-Object DeviceClass, DeviceName | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::Services => r#"
Get-CimInstance Win32_Service | Select-Object Name, DisplayName, StartMode, State, PathName | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::CrashEvents => r#"
$since = (Get-Date).AddDays(-90)
function T($e) { $e.TimeCreated.ToUniversalTime().ToString('o') }
function P($e, $i) { if ($e.Properties.Count -gt $i -and $null -ne $e.Properties[$i].Value) { [string]$e.Properties[$i].Value } else { '' } }
function Q($h, $max) { try { @(Get-WinEvent -FilterHashtable $h -MaxEvents $max -ErrorAction Stop) } catch { @() } }
$out = @()
foreach ($e in (Q @{ LogName='System'; ProviderName='Microsoft-Windows-Kernel-Power'; Id=41; StartTime=$since } 200)) {
  $d = @{}; foreach ($x in ([xml]$e.ToXml()).Event.EventData.Data) { $d[$x.Name] = [string]$x.'#text' }
  $out += @{ kind='unexpected_shutdown'; time=(T $e); code=$d['BugcheckCode']; p1=$d['BugcheckParameter1']; sleep=$d['SleepInProgress']; power_button=$d['PowerButtonTimestamp']; long_press=$d['LongPowerButtonPressDetected'] }
}
foreach ($e in (Q @{ LogName='System'; ProviderName='Microsoft-Windows-WER-SystemErrorReporting'; Id=1001; StartTime=$since } 200)) {
  $out += @{ kind='bugcheck'; time=(T $e); text=(P $e 0); dump=(P $e 1) }
}
foreach ($e in (Q @{ LogName='System'; ProviderName='EventLog'; Id=6005,6006,6008; StartTime=$since } 600)) {
  $out += @{ kind=('eventlog_' + $e.Id); time=(T $e) }
}
foreach ($e in (Q @{ LogName='System'; ProviderName='Microsoft-Windows-WHEA-Logger'; StartTime=$since } 200)) {
  $out += @{ kind='whea'; time=(T $e); id=$e.Id; level=$e.Level; text=$(if ($e.Message) { $e.Message.Substring(0, [Math]::Min(300, $e.Message.Length)) } else { '' }) }
}
foreach ($e in (Q @{ LogName='Application'; ProviderName='Application Error'; Id=1000; StartTime=$since } 400)) {
  $out += @{ kind='app_crash'; time=(T $e); app=(P $e 0); app_version=(P $e 1); module=(P $e 3); module_version=(P $e 4); code=(P $e 6); app_path=(P $e 10); module_path=(P $e 11) }
}
foreach ($e in (Q @{ LogName='Application'; ProviderName='Application Hang'; Id=1002; StartTime=$since } 200)) {
  $out += @{ kind='app_hang'; time=(T $e); app=(P $e 0); app_version=(P $e 1); app_path=(P $e 5) }
}
$stab = @()
try { $stab = @(Get-CimInstance Win32_ReliabilityStabilityMetrics -ErrorAction Stop | Where-Object { $_.TimeGenerated -gt $since } | Select-Object -First 2200 | ForEach-Object { @{ time=$_.TimeGenerated.ToUniversalTime().ToString('o'); index=[double]$_.SystemStabilityIndex } }) } catch {}
@{ events=@($out); stability=@($stab) } | ConvertTo-Json -Depth 4 -Compress"#,
            PsScript::DriverCentre => r#"
$pnp = @(Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue)
$names = @{}; foreach ($d in $pnp) { if ($d.DeviceID -and $d.Name) { $names[$d.DeviceID.ToUpper()] = $d.Name } }
$problems = @($pnp | Where-Object { $_.ConfigManagerErrorCode -ne 0 -and $_.ConfigManagerErrorCode -ne 45 } | ForEach-Object {
  @{ id=$_.DeviceID; name=$_.Name; class=$_.PNPClass; manufacturer=$_.Manufacturer; code=[int]$_.ConfigManagerErrorCode; hw=@($_.HardwareID); compat=@($_.CompatibleID) } })
$classes = 'Display','Net','MEDIA','Bluetooth','System','SCSIAdapter','HDC','USB','AudioEndpoint','Camera','Image','Monitor','HIDClass','Biometric'
$drivers = @(Get-CimInstance Win32_PnPSignedDriver -ErrorAction SilentlyContinue | Where-Object { $_.DeviceName -and $classes -contains $_.DeviceClass } | ForEach-Object {
  @{ device=$_.DeviceName; class=$_.DeviceClass; provider=$_.DriverProviderName; manufacturer=$_.Manufacturer; version=$_.DriverVersion; date=$(if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') } else { $null }); signed=$_.IsSigned; inf=$_.InfName; id=$_.DeviceID } })
$installs = @()
try {
  foreach ($e in @(Get-WinEvent -FilterHashtable @{ LogName='Microsoft-Windows-Kernel-PnP/Configuration'; Id=400; StartTime=(Get-Date).AddDays(-90) } -MaxEvents 4000 -ErrorAction Stop)) {
    $d = @{}; foreach ($x in ([xml]$e.ToXml()).Event.EventData.Data) { $d[$x.Name] = [string]$x.'#text' }
    if ($d['DriverInbox'] -eq 'false' -and $d['DriverName'] -and $d['DriverName'] -ne 'null') {
      $dev = [string]$d['DeviceInstanceId']
      $installs += @{ time=$e.TimeCreated.ToUniversalTime().ToString('o'); id=$dev; name=$names[$dev.ToUpper()]; inf=$d['DriverName']; version=$d['DriverVersion']; provider=$d['DriverProvider']; date=$d['DriverDate']; updated=($d['DeviceUpdated'] -eq 'true') }
    }
  }
} catch {}
@{ problems=$problems; drivers=$drivers; installs=@($installs) } | ConvertTo-Json -Depth 4 -Compress"#,
            PsScript::BootTimes => r#"
$since = (Get-Date).AddDays(-60)
function Q($h) { try { @(Get-WinEvent -FilterHashtable $h -ErrorAction Stop) } catch { @() } }
$starts = Q @{ LogName='System'; ProviderName='Microsoft-Windows-Kernel-General'; Id=12; StartTime=$since }
$logons = Q @{ LogName='System'; ProviderName='Microsoft-Windows-Winlogon'; Id=7001; StartTime=$since }
$types = Q @{ LogName='System'; ProviderName='Microsoft-Windows-Kernel-Boot'; Id=27; StartTime=$since }
$out = foreach ($s in $starts) {
  $l = $logons | Where-Object { $_.TimeCreated -gt $s.TimeCreated } | Sort-Object TimeCreated | Select-Object -First 1
  $t = $types | Where-Object { [math]::Abs(($_.TimeCreated - $s.TimeCreated).TotalSeconds) -lt 120 } | Select-Object -First 1
  $bt = $null; if ($t) { $bt = (([xml]$t.ToXml()).Event.EventData.Data | Where-Object Name -eq 'BootType').'#text' }
  @{ start=$s.TimeCreated.ToUniversalTime().ToString('o'); logon=$(if ($l) { $l.TimeCreated.ToUniversalTime().ToString('o') } else { $null }); boot_type=$bt }
}
@($out) | ConvertTo-Json -Depth 3 -Compress"#,
            PsScript::StartApps => r#"
Get-StartApps | Select-Object Name, AppID | ConvertTo-Json -Depth 2 -Compress"#,
            PsScript::GamePackages => r#"
@(Get-AppxPackage | Where-Object { -not $_.IsFramework -and $_.InstallLocation -and (Test-Path -LiteralPath (Join-Path $_.InstallLocation 'MicrosoftGame.config')) } |
  ForEach-Object { [pscustomobject]@{ Family = $_.PackageFamilyName; Location = $_.InstallLocation } }) | ConvertTo-Json -Depth 2 -Compress"#,
            PsScript::PendingReboot => r#"
$r = @{
  cbs = Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending'
  wu  = Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired'
  pfro = $null -ne (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' -Name PendingFileRenameOperations -ErrorAction SilentlyContinue)
}
$r | ConvertTo-Json -Compress"#,
        }
    }

    /// Scripts that modify the system. Used by the audit layer.
    pub fn is_mutating(self) -> bool {
        matches!(self, PsScript::CreateRestorePoint)
    }
}

/// Every external command NOCTURN is allowed to run.
#[derive(Debug, Clone)]
pub enum SafeCommand {
    PowercfgList,
    PowercfgGetActive,
    PowercfgSetActive(SchemeGuid),
    IpconfigFlushDns,
    NetshWlanInterfaces,
    PowerShell(PsScript),
    /// `yt-dlp --version`
    YtDlpVersion { exe: PathBuf },
    /// Stream info (JSON) for one YouTube video, audio only.
    YtDlpExtract { exe: PathBuf, deno: PathBuf, video: VideoId },
    /// Unpack a downloaded archive with Windows' own tar.exe.
    TarExtract { archive: PathBuf, dest: PathBuf },
}

/// A YouTube video ID (11 characters of [A-Za-z0-9_-]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoId(String);

impl VideoId {
    pub fn parse(s: &str) -> AppResult<Self> {
        if s.len() == 11 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            Ok(VideoId(s.to_string()))
        } else {
            Err(AppError::Invalid("not a YouTube video ID".into()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SafeCommand {
    fn program_and_args(&self) -> (PathBuf, Vec<String>) {
        let sys32 = system32();
        match self {
            SafeCommand::PowercfgList => (sys32.join("powercfg.exe"), vec!["/list".into()]),
            SafeCommand::PowercfgGetActive => {
                (sys32.join("powercfg.exe"), vec!["/getactivescheme".into()])
            }
            SafeCommand::PowercfgSetActive(g) => {
                (sys32.join("powercfg.exe"), vec!["/setactive".into(), g.as_str().into()])
            }
            SafeCommand::IpconfigFlushDns => (sys32.join("ipconfig.exe"), vec!["/flushdns".into()]),
            SafeCommand::YtDlpVersion { exe } => (exe.clone(), vec!["--no-config".into(), "--version".into()]),
            SafeCommand::YtDlpExtract { exe, deno, video } => (
                exe.clone(),
                vec![
                    "--no-config".into(),
                    "--no-playlist".into(),
                    "--no-cache-dir".into(),
                    "--js-runtimes".into(),
                    format!("deno:{}", deno.display()),
                    "-f".into(),
                    "bestaudio[ext=m4a]/bestaudio[acodec^=mp4a]/bestaudio[ext=webm]/bestaudio".into(),
                    "-J".into(),
                    "--".into(),
                    video.as_str().into(),
                ],
            ),
            SafeCommand::TarExtract { archive, dest } => (
                sys32.join("tar.exe"),
                vec!["-xf".into(), archive.display().to_string(), "-C".into(), dest.display().to_string()],
            ),
            SafeCommand::NetshWlanInterfaces => (
                sys32.join("netsh.exe"),
                vec!["wlan".into(), "show".into(), "interfaces".into()],
            ),
            SafeCommand::PowerShell(script) => {
                let src = format!(
                    "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; $ProgressPreference = 'SilentlyContinue'; {}",
                    script.source()
                );
                (
                    sys32.join(r"WindowsPowerShell\v1.0\powershell.exe"),
                    vec![
                        "-NoLogo".into(),
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-ExecutionPolicy".into(),
                        "Bypass".into(),
                        "-Command".into(),
                        src,
                    ],
                )
            }
        }
    }

    fn default_timeout(&self) -> Duration {
        match self {
            SafeCommand::PowerShell(PsScript::CreateRestorePoint) => Duration::from_secs(180),
            SafeCommand::PowerShell(PsScript::Drivers) => Duration::from_secs(60),
            SafeCommand::PowerShell(_) => Duration::from_secs(30),
            SafeCommand::YtDlpExtract { .. } => Duration::from_secs(60),
            SafeCommand::TarExtract { .. } => Duration::from_secs(120),
            _ => Duration::from_secs(15),
        }
    }

    pub fn describe(&self) -> String {
        let (p, a) = self.program_and_args();
        match self {
            SafeCommand::PowerShell(s) => format!("powershell <{s:?}>"),
            _ => format!(
                "{} {}",
                p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
                a.join(" ")
            ),
        }
    }
}

#[derive(Debug)]
pub struct CmdOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
}

pub fn system32() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32")
}

/// Run an allow-listed command with no console window and a hard timeout.
pub fn run(cmd: &SafeCommand) -> AppResult<CmdOutput> {
    run_with_timeout(cmd, cmd.default_timeout())
}

pub fn run_with_timeout(cmd: &SafeCommand, timeout: Duration) -> AppResult<CmdOutput> {
    let (program, args) = cmd.program_and_args();
    match cmd {
        SafeCommand::PowerShell(s) if s.is_mutating() => tracing::info!("exec (modifies system): {}", cmd.describe()),
        SafeCommand::PowercfgSetActive(_) | SafeCommand::IpconfigFlushDns => tracing::info!("exec (modifies system): {}", cmd.describe()),
        _ => tracing::debug!("exec: {}", cmd.describe()),
    }

    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| AppError::msg(format!("could not start {}: {e}", program.display())))?;

    // Drain pipes on background threads so large outputs can't deadlock.
    let mut out_pipe = child.stdout.take().expect("stdout piped");
    let mut err_pipe = child.stderr.take().expect("stderr piped");
    let out_t = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = out_pipe.read_to_end(&mut v);
        v
    });
    let err_t = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = err_pipe.read_to_end(&mut v);
        v
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::msg(format!(
                    "{} timed out after {}s",
                    cmd.describe(),
                    timeout.as_secs()
                )));
            }
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    };

    let stdout = decode(&out_t.join().unwrap_or_default());
    let stderr = decode(&err_t.join().unwrap_or_default());
    Ok(CmdOutput { status: status.code(), stdout, stderr })
}

/// Run a PowerShell script that emits JSON and parse it. An empty output (no
/// rows) becomes `Value::Null`; a single object is *not* wrapped in an array —
/// use [`as_array`] when a list is expected.
pub fn powershell_json(script: PsScript) -> AppResult<serde_json::Value> {
    let out = run(&SafeCommand::PowerShell(script))?;
    let text = out.stdout.trim().trim_start_matches('\u{feff}');
    if text.is_empty() {
        if !out.success() && !out.stderr.trim().is_empty() {
            return Err(AppError::msg(format!("{script:?} failed: {}", out.stderr.trim())));
        }
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(text)
        .map_err(|e| AppError::msg(format!("{script:?} returned unreadable output: {e}")))
}

/// PowerShell's ConvertTo-Json emits a bare object for one row; normalise.
pub fn as_array(v: serde_json::Value) -> Vec<serde_json::Value> {
    match v {
        serde_json::Value::Null => vec![],
        serde_json::Value::Array(a) => a,
        other => vec![other],
    }
}

fn decode(bytes: &[u8]) -> String {
    // PowerShell output is UTF-8 (we set OutputEncoding). Native tools write in
    // the OEM code page; lossy decoding keeps ASCII (GUIDs, numbers) intact.
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_validation() {
        assert!(SchemeGuid::parse("381b4222-f694-41f0-9685-ff5bb260df2e").is_ok());
        assert!(SchemeGuid::parse(" 381B4222-F694-41F0-9685-FF5BB260DF2E ").is_ok());
        assert!(SchemeGuid::parse("381b4222-f694-41f0-9685-ff5bb260df2").is_err());
        assert!(SchemeGuid::parse("381b4222-f694-41f0-9685-ff5bb260df2e & calc").is_err());
        assert!(SchemeGuid::parse("; rm -rf").is_err());
        assert!(SchemeGuid::parse("").is_err());
    }

    #[test]
    fn programs_are_absolute_system32_paths() {
        let cmds = [
            SafeCommand::PowercfgList,
            SafeCommand::IpconfigFlushDns,
            SafeCommand::NetshWlanInterfaces,
            SafeCommand::PowerShell(PsScript::NetAdapters),
        ];
        for c in cmds {
            let (p, _) = c.program_and_args();
            assert!(p.is_absolute(), "{p:?}");
            assert!(p.starts_with(system32()), "{p:?}");
        }
    }

    #[test]
    fn only_restore_point_script_mutates() {
        assert!(PsScript::CreateRestorePoint.is_mutating());
        assert!(!PsScript::NetAdapters.is_mutating());
        assert!(!PsScript::RecentErrorEvents.is_mutating());
    }

    #[test]
    fn powercfg_list_runs() {
        let out = run(&SafeCommand::PowercfgList).expect("powercfg should run");
        assert!(out.success());
        assert!(out.stdout.contains('-'));
    }
}
