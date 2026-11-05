//! The optimisation catalogue.
//!
//! Every entry is a documented Windows setting with a known default. Rules:
//! * Never disable Windows Defender, Windows Update, SmartScreen, UAC,
//!   firewall or other security features.
//! * Never disable services as a generic "speed-up".
//! * Every entry must state its downside honestly and must not promise FPS.
//! * Registry and power-plan entries are fully reversible via the journal.

use std::sync::OnceLock;

use serde::Serialize;

use super::{Action, Category, Hive, PowerTarget, RegData, RegSetting, Risk, Takes, Tweak};

fn dw(hive: Hive, path: &str, name: &str, v: u32) -> RegSetting {
    RegSetting { hive, path: path.into(), name: name.into(), data: RegData::Dword(v) }
}
fn sz(hive: Hive, path: &str, name: &str, v: &str) -> RegSetting {
    RegSetting { hive, path: path.into(), name: name.into(), data: RegData::Sz(v.into()) }
}
fn reg(settings: Vec<RegSetting>) -> Action {
    Action::Registry { settings }
}

use Hive::{Hkcu, Hklm};

const GAMECFG: &str = r"System\GameConfigStore";
const MMCSS: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile";
const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

fn build() -> Vec<Tweak> {
    vec![
        // ------------------------------------------------------------ Gaming
        Tweak {
            id: "gaming.game_mode",
            category: Category::Gaming,
            title: "Enable Game Mode",
            summary: "Lets Windows prioritise the game you're playing and pause Windows Update installs while you play.",
            details: "Turns on Windows Game Mode (Settings › Gaming › Game Mode). It is on by default on most PCs; this makes sure it hasn't been switched off.",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: reg(vec![
                dw(Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled", 1),
                dw(Hkcu, r"Software\Microsoft\GameBar", "AllowAutoGameMode", 1),
            ]),
        },
        Tweak {
            id: "gaming.disable_background_recording",
            category: Category::Gaming,
            title: "Turn off background game recording",
            summary: "Stops Xbox Game Bar from continuously recording gameplay in the background.",
            details: "Disables Game DVR / background capture (\"Record what happened\"). Background recording keeps a video encoder running while you play, which costs GPU encoder time and disk writes.",
            caveat: Some("You won't be able to save clips with Win+Alt+G. Other recorders (NVIDIA ShadowPlay, AMD ReLive, OBS) are unaffected."),
            risk: Risk::Safe,
            takes: Takes::AppRestart,
            action: reg(vec![
                dw(Hkcu, GAMECFG, "GameDVR_Enabled", 0),
                dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled", 0),
            ]),
        },
        Tweak {
            id: "gaming.mouse_precision_off",
            category: Category::Gaming,
            title: "Turn off mouse acceleration",
            summary: "Makes pointer movement match your hand movement 1:1, which keeps aim consistent.",
            details: "Disables \"Enhance pointer precision\" (Control Panel › Mouse › Pointer Options). Most games use raw input, but many menus, launchers and older titles don't.",
            caveat: Some("Slow movements on the desktop will feel faster/slower than before; adjust pointer speed to taste."),
            risk: Risk::Safe,
            takes: Takes::SignOut,
            action: reg(vec![
                sz(Hkcu, r"Control Panel\Mouse", "MouseSpeed", "0"),
                sz(Hkcu, r"Control Panel\Mouse", "MouseThreshold1", "0"),
                sz(Hkcu, r"Control Panel\Mouse", "MouseThreshold2", "0"),
            ]),
        },
        Tweak {
            id: "gaming.power_high_performance",
            category: Category::Gaming,
            title: "Use the High performance power plan",
            summary: "Keeps the CPU ready to boost instead of saving power between bursts of work.",
            details: "Switches the active power plan to Windows' built-in High performance plan. The previous plan is recorded and restored on rollback.",
            caveat: Some("Higher idle power use and temperatures; on laptops this shortens battery life. Some PCs (Modern Standby devices) don't offer this plan — NOCTURN will tell you if it's unavailable."),
            risk: Risk::Moderate,
            takes: Takes::Immediately,
            action: Action::PowerPlan { target: PowerTarget::HighPerformance },
        },
        Tweak {
            id: "gaming.hardware_gpu_scheduling",
            category: Category::Gaming,
            title: "Enable hardware-accelerated GPU scheduling",
            summary: "Lets the graphics card manage its own memory scheduling, which can reduce latency on supported GPUs.",
            details: "Sets HwSchMode=2 (Settings › Display › Graphics › Default graphics settings). Required for DLSS Frame Generation on NVIDIA GPUs.",
            caveat: Some("Needs a supported GPU and driver (NVIDIA GTX 10-series or newer, AMD RX 5000 or newer, recent Intel Arc/Iris Xe). Results vary by game — some see smoother frame pacing, some see no change. Requires a restart."),
            risk: Risk::Advanced,
            takes: Takes::Restart,
            action: reg(vec![dw(Hklm, r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode", 2)]),
        },
        Tweak {
            id: "gaming.fullscreen_optimizations_off",
            category: Category::Gaming,
            title: "Disable fullscreen optimisations (global)",
            summary: "Makes exclusive-fullscreen games use true exclusive fullscreen instead of Windows' optimised borderless mode.",
            details: "Sets the GameConfigStore FSE behaviour flags that the per-game \"Disable fullscreen optimizations\" checkbox uses, but for all games.",
            caveat: Some("Only worth trying if a specific game stutters or shows input lag in fullscreen. On Windows 11 it often has no measurable effect, and it can break Auto HDR and make Alt+Tab slower."),
            risk: Risk::Advanced,
            takes: Takes::AppRestart,
            action: reg(vec![
                dw(Hkcu, GAMECFG, "GameDVR_FSEBehaviorMode", 2),
                dw(Hkcu, GAMECFG, "GameDVR_FSEBehavior", 2),
                dw(Hkcu, GAMECFG, "GameDVR_HonorUserFSEBehaviorMode", 1),
                dw(Hkcu, GAMECFG, "GameDVR_DXGIHonorFSEWindowsCompatible", 1),
            ]),
        },
        Tweak {
            id: "gaming.system_responsiveness",
            category: Category::Gaming,
            title: "Reserve less CPU for background multimedia tasks",
            summary: "Lowers the share of CPU time Windows' multimedia scheduler keeps for low-priority tasks from 20% to 10%.",
            details: "Sets MMCSS SystemResponsiveness=10 (Windows default: 20).",
            caveat: Some("Small, contested benefit. Measure before and after; roll back if you notice audio crackle while gaming."),
            risk: Risk::Advanced,
            takes: Takes::Restart,
            action: reg(vec![dw(Hklm, MMCSS, "SystemResponsiveness", 10)]),
        },
        // ------------------------------------------------------- Performance
        Tweak {
            id: "performance.menu_show_delay",
            category: Category::Performance,
            title: "Open menus faster",
            summary: "Shortens the delay before cascading menus and submenus appear from 400 ms to 100 ms.",
            details: "Sets Control Panel\\Desktop\\MenuShowDelay to 100 (default 400).",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::SignOut,
            action: reg(vec![sz(Hkcu, r"Control Panel\Desktop", "MenuShowDelay", "100")]),
        },
        Tweak {
            id: "performance.startup_delay",
            category: Category::Performance,
            title: "Remove the startup app delay",
            summary: "Stops Windows from deliberately waiting before it launches your startup apps after sign-in.",
            details: "Sets Explorer\\Serialize\\StartupDelayInMSec to 0.",
            caveat: Some("All startup apps launch together, so the first minute after sign-in can feel busier on PCs with hard drives. Reduce startup apps first for the biggest win."),
            risk: Risk::Safe,
            takes: Takes::SignOut,
            action: reg(vec![dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize", "StartupDelayInMSec", 0)]),
        },
        Tweak {
            id: "performance.reduce_animations",
            category: Category::Performance,
            title: "Reduce window animations",
            summary: "Turns off minimise/maximise animations, taskbar animations and Aero Peek so the desktop feels snappier.",
            details: "Sets WindowMetrics\\MinAnimate=0, Explorer\\Advanced\\TaskbarAnimations=0 and DWM\\EnableAeroPeek=0.",
            caveat: Some("Purely cosmetic trade-off; the desktop looks less smooth."),
            risk: Risk::Safe,
            takes: Takes::SignOut,
            action: reg(vec![
                sz(Hkcu, r"Control Panel\Desktop\WindowMetrics", "MinAnimate", "0"),
                dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "TaskbarAnimations", 0),
                dw(Hkcu, r"Software\Microsoft\Windows\DWM", "EnableAeroPeek", 0),
            ]),
        },
        Tweak {
            id: "performance.transparency_off",
            category: Category::Performance,
            title: "Turn off transparency effects",
            summary: "Removes the blurred see-through effect from the taskbar, Start and Settings, which saves a little GPU work.",
            details: "Sets Themes\\Personalize\\EnableTransparency=0 (Settings › Personalisation › Colours).",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: reg(vec![dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "EnableTransparency", 0)]),
        },
        Tweak {
            id: "performance.background_apps_off",
            category: Category::Performance,
            title: "Stop Store apps running in the background",
            summary: "Prevents Microsoft Store apps from running when you're not using them.",
            details: "Sets BackgroundAccessApplications\\GlobalUserDisabled=1. Desktop programs (Steam, Discord, browsers) are not affected.",
            caveat: Some("Store apps such as Mail, Calendar, Phone Link and some widgets won't refresh or send notifications until you open them."),
            risk: Risk::Moderate,
            takes: Takes::Immediately,
            action: reg(vec![dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications", "GlobalUserDisabled", 1)]),
        },
        Tweak {
            id: "performance.power_throttling_off",
            category: Category::Performance,
            title: "Disable power throttling",
            summary: "Stops Windows putting background processes into low-power \"efficiency mode\".",
            details: "Sets Control\\Power\\PowerThrottling\\PowerThrottlingOff=1.",
            caveat: Some("Increases power use and heat. On laptops this noticeably reduces battery life. Mainly useful when a background tool (e.g. streaming or encoding software) is being slowed down."),
            risk: Risk::Advanced,
            takes: Takes::Restart,
            action: reg(vec![dw(Hklm, r"SYSTEM\CurrentControlSet\Control\Power\PowerThrottling", "PowerThrottlingOff", 1)]),
        },
        Tweak {
            id: "performance.clean_temp",
            category: Category::Performance,
            title: "Clean temporary files",
            summary: "Deletes files older than 24 hours from your temporary folder to free disk space.",
            details: "Removes files from %TEMP% that haven't been modified for 24 hours. Files in use are skipped. Personal folders (Documents, Desktop, Downloads, Pictures) are never touched.",
            caveat: Some("Deleted temporary files cannot be restored. This is normally harmless — apps recreate what they need."),
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: Action::CleanTempFiles,
        },
        // ----------------------------------------------------------- Network
        Tweak {
            id: "network.flush_dns",
            category: Category::Network,
            title: "Flush the DNS cache",
            summary: "Clears remembered website addresses so Windows looks them up fresh.",
            details: "Runs ipconfig /flushdns. Useful when a site won't load after its address changed, or after changing DNS servers.",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: Action::FlushDns,
        },
        Tweak {
            id: "network.throttling_index",
            category: Category::Network,
            title: "Disable multimedia network throttling",
            summary: "Stops Windows limiting non-multimedia network traffic while audio or video is playing.",
            details: "Sets MMCSS NetworkThrottlingIndex=0xFFFFFFFF (Windows default: 10).",
            caveat: Some("Benefit is mostly seen on older systems with heavy network use while media plays. Can slightly raise CPU use under very high throughput. Measure ping/throughput before and after."),
            risk: Risk::Advanced,
            takes: Takes::Restart,
            action: reg(vec![dw(Hklm, MMCSS, "NetworkThrottlingIndex", 0xFFFF_FFFF)]),
        },
        Tweak {
            id: "network.delivery_optimization_http_only",
            category: Category::Network,
            title: "Stop sharing updates with other PCs",
            summary: "Windows Update downloads only from Microsoft instead of uploading to and downloading from other PCs.",
            details: "Sets the Delivery Optimization policy DODownloadMode=0 (HTTP only). Windows Update keeps working normally.",
            caveat: Some("This is a policy setting, so Windows Settings will show \"Some settings are managed by your organisation\" for Delivery Optimization until rolled back."),
            risk: Risk::Moderate,
            takes: Takes::Restart,
            action: reg(vec![dw(Hklm, r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", "DODownloadMode", 0)]),
        },
        // ----------------------------------------------------------- Privacy
        Tweak {
            id: "privacy.advertising_id_off",
            category: Category::Privacy,
            title: "Turn off the advertising ID",
            summary: "Stops apps using a Windows-wide ID to show you personalised ads.",
            details: "Settings › Privacy & security › General › \"Let apps show me personalised ads\".",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: reg(vec![dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0)]),
        },
        Tweak {
            id: "privacy.tailored_experiences_off",
            category: Category::Privacy,
            title: "Turn off tailored experiences",
            summary: "Stops Microsoft using your diagnostic data to personalise tips, ads and recommendations.",
            details: "Settings › Privacy & security › Diagnostics & feedback › Tailored experiences.",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: reg(vec![dw(Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", 0)]),
        },
        Tweak {
            id: "privacy.suggestions_off",
            category: Category::Privacy,
            title: "Turn off suggestions and promoted apps",
            summary: "Removes app suggestions in Start, tips, and Settings recommendations, and stops promoted apps installing automatically.",
            details: "Disables ContentDeliveryManager suggestion channels: Start suggestions, Windows tips, Settings suggested content and silent installation of promoted apps.",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::SignOut,
            action: reg(vec![
                dw(Hkcu, CDM, "SystemPaneSuggestionsEnabled", 0),
                dw(Hkcu, CDM, "SubscribedContent-338388Enabled", 0),
                dw(Hkcu, CDM, "SubscribedContent-338389Enabled", 0),
                dw(Hkcu, CDM, "SubscribedContent-353694Enabled", 0),
                dw(Hkcu, CDM, "SubscribedContent-353696Enabled", 0),
                dw(Hkcu, CDM, "SilentInstalledAppsEnabled", 0),
            ]),
        },
        Tweak {
            id: "privacy.feedback_prompts_off",
            category: Category::Privacy,
            title: "Stop feedback requests",
            summary: "Windows will no longer pop up asks for feedback.",
            details: "Settings › Privacy & security › Diagnostics & feedback › Feedback frequency: Never.",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Immediately,
            action: reg(vec![dw(Hkcu, r"Software\Microsoft\Siuf\Rules", "NumberOfSIUFInPeriod", 0)]),
        },
        Tweak {
            id: "privacy.web_search_in_start_off",
            category: Category::Privacy,
            title: "Turn off web results in Start search",
            summary: "Start menu search shows only your apps, files and settings, without Bing results.",
            details: "Sets the per-user policy Explorer\\DisableSearchBoxSuggestions=1.",
            caveat: Some("You'll need to open a browser for web searches."),
            risk: Risk::Safe,
            takes: Takes::SignOut,
            action: reg(vec![dw(Hkcu, r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", 1)]),
        },
        Tweak {
            id: "privacy.activity_history_off",
            category: Category::Privacy,
            title: "Turn off activity history",
            summary: "Stops Windows recording and uploading the apps and files you use.",
            details: "Sets the PublishUserActivities and UploadUserActivities policies to 0.",
            caveat: None,
            risk: Risk::Safe,
            takes: Takes::Restart,
            action: reg(vec![
                dw(Hklm, r"SOFTWARE\Policies\Microsoft\Windows\System", "PublishUserActivities", 0),
                dw(Hklm, r"SOFTWARE\Policies\Microsoft\Windows\System", "UploadUserActivities", 0),
            ]),
        },
        Tweak {
            id: "privacy.diagnostic_data_required",
            category: Category::Privacy,
            title: "Limit diagnostic data to Required",
            summary: "Sends Microsoft only the minimum diagnostic data needed to keep Windows secure and up to date.",
            details: "Sets the AllowTelemetry policy to 1 (Required). Windows Update and Microsoft Defender continue to work normally.",
            caveat: Some("This is a policy, so the Diagnostics page in Settings will say it's managed by your organisation until rolled back."),
            risk: Risk::Moderate,
            takes: Takes::Restart,
            action: reg(vec![dw(Hklm, r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", "AllowTelemetry", 1)]),
        },
        Tweak {
            id: "privacy.location_off",
            category: Category::Privacy,
            title: "Turn off location access",
            summary: "Blocks apps and Windows from using your device's location.",
            details: "Sets the device-wide location consent to Deny (Settings › Privacy & security › Location).",
            caveat: Some("Maps, Weather, Find my device and automatic time zone will stop using your location."),
            risk: Risk::Moderate,
            takes: Takes::Immediately,
            action: reg(vec![sz(Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location", "Value", "Deny")]),
        },
    ]
}

pub fn all() -> &'static [Tweak] {
    static CATALOG: OnceLock<Vec<Tweak>> = OnceLock::new();
    CATALOG.get_or_init(build)
}

pub fn get(id: &str) -> Option<&'static Tweak> {
    all().iter().find(|t| t.id == id)
}

// ------------------------------------------------------------------ profiles

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub id: &'static str,
    pub name: &'static str,
    /// "system" (Tweak page profiles) or "gaming_mode" (Gaming page modes).
    pub kind: &'static str,
    pub description: &'static str,
    pub tweaks: Vec<&'static str>,
}

pub fn profiles() -> Vec<Profile> {
    let balanced = vec![
        "gaming.game_mode",
        "performance.menu_show_delay",
        "performance.startup_delay",
        "privacy.advertising_id_off",
        "privacy.tailored_experiences_off",
        "privacy.suggestions_off",
        "privacy.feedback_prompts_off",
    ];
    let mut gaming = balanced.clone();
    gaming.extend([
        "gaming.disable_background_recording",
        "gaming.mouse_precision_off",
        "gaming.power_high_performance",
        "gaming.hardware_gpu_scheduling",
    ]);
    let mut maximum = gaming.clone();
    maximum.extend([
        "performance.reduce_animations",
        "performance.transparency_off",
        "performance.background_apps_off",
        "performance.power_throttling_off",
        "gaming.system_responsiveness",
        "network.throttling_index",
    ]);
    let competitive = vec![
        "gaming.game_mode",
        "gaming.disable_background_recording",
        "gaming.mouse_precision_off",
        "gaming.power_high_performance",
        "gaming.hardware_gpu_scheduling",
        "performance.background_apps_off",
        "privacy.feedback_prompts_off",
    ];
    let quality = vec!["gaming.game_mode", "gaming.disable_background_recording", "gaming.power_high_performance", "gaming.hardware_gpu_scheduling"];
    let balanced_gaming = vec!["gaming.game_mode", "gaming.disable_background_recording", "gaming.hardware_gpu_scheduling"];
    let streaming = vec!["gaming.game_mode", "gaming.power_high_performance", "gaming.hardware_gpu_scheduling", "privacy.feedback_prompts_off"];
    vec![
        Profile {
            id: "balanced",
            name: "Balanced",
            kind: "system",
            description: "Safe everyday improvements: snappier menus, faster startup apps and fewer distractions. Nothing that costs battery life.",
            tweaks: balanced,
        },
        Profile {
            id: "gaming",
            name: "Gaming",
            kind: "system",
            description: "Balanced, plus settings that favour games: no background recording, raw mouse input, High performance power plan and GPU scheduling.",
            tweaks: gaming,
        },
        Profile {
            id: "maximum",
            name: "Maximum Performance",
            kind: "system",
            description: "Gaming, plus trade-offs that favour speed over looks and power efficiency. Includes Advanced settings — review each one.",
            tweaks: maximum,
        },
        Profile {
            id: "mode.competitive",
            name: "Competitive",
            kind: "gaming_mode",
            description: "For fast shooters and esports: raw mouse input, no background recording, apps kept from running in the background, and the CPU held at full clocks. Pair with low in-game settings for the highest frame rate your PC can deliver.",
            tweaks: competitive,
        },
        Profile {
            id: "mode.quality",
            name: "Maximum Quality",
            kind: "gaming_mode",
            description: "Keeps the GPU fed for high in-game settings: High performance power plan, GPU scheduling and no background recording. Windows can't raise image quality itself — set that in the game.",
            tweaks: quality,
        },
        Profile {
            id: "mode.balanced",
            name: "Balanced Gaming",
            kind: "gaming_mode",
            description: "The light touch: Game Mode, GPU scheduling and no background recording. Leaves power settings alone, so it's fine for laptops on battery.",
            tweaks: balanced_gaming,
        },
        Profile {
            id: "mode.streaming",
            name: "Streaming",
            kind: "gaming_mode",
            description: "For playing while streaming or recording with OBS or similar: full CPU clocks and GPU scheduling, but background apps and mouse settings are left alone so your capture and chat tools keep working.",
            tweaks: streaming,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_namespaced() {
        let mut seen = HashSet::new();
        for t in all() {
            assert!(seen.insert(t.id), "duplicate id {}", t.id);
            let prefix = match t.category {
                Category::Gaming => "gaming.",
                Category::Performance => "performance.",
                Category::Network => "network.",
                Category::Privacy => "privacy.",
            };
            assert!(t.id.starts_with(prefix), "{} has wrong prefix", t.id);
            assert!(!t.summary.is_empty() && !t.details.is_empty());
        }
    }

    #[test]
    fn profiles_reference_existing_tweaks() {
        for p in profiles() {
            for id in &p.tweaks {
                assert!(get(id).is_some(), "profile {} references unknown {}", p.id, id);
            }
        }
    }

    #[test]
    fn non_safe_tweaks_explain_their_downside() {
        for t in all() {
            if t.risk != Risk::Safe {
                assert!(t.caveat.is_some(), "{} needs a caveat", t.id);
            }
        }
    }

    #[test]
    fn catalogue_never_touches_security_features() {
        let forbidden = [
            "windows defender",
            "windowsdefender",
            "disableantispyware",
            "windowsupdate\\au",
            "smartscreen",
            "enablelua",
            "firewall",
            "wuauserv",
            "\\services\\",
        ];
        for t in all() {
            if let Action::Registry { settings } = &t.action {
                for s in settings {
                    let p = format!("{}\\{}", s.path, s.name).to_ascii_lowercase();
                    for f in forbidden {
                        assert!(!p.contains(f), "{} touches forbidden path {}", t.id, p);
                    }
                }
            }
        }
    }

    #[test]
    fn no_catalogue_text_promises_fps() {
        for t in all() {
            let text = format!("{} {} {}", t.title, t.summary, t.details).to_ascii_lowercase();
            assert!(!text.contains("fps boost") && !text.contains("more fps"), "{}", t.id);
        }
    }
}
