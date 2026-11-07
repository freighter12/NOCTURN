//! Installed-game detection from launcher metadata (read-only):
//! - Steam: `libraryfolders.vdf` + `appmanifest_*.acf` in each library
//! - Epic Games: `*.item` manifests in ProgramData
//! - GOG Galaxy: `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`
//! - Battle.net, Riot Client, EA app: Windows' installed-programs list
//! - Ubisoft Connect: `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs`
//! - Rockstar Games Launcher: `HKLM\SOFTWARE\WOW6432Node\Rockstar Games`
//! - Xbox / Microsoft Store (GDK) games: packages with a `MicrosoftGame.config`
//! - Games the user added by pointing at an .exe
//!
//! Each game records how it is launched, so it opens through its own
//! launcher (overlays, cloud saves and anti-cheat keep working).

use std::path::{Path, PathBuf};

use serde::Serialize;
use winreg::enums::*;
use winreg::RegKey;

/// How to start a game. Built from detection only — never from the page.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Launch {
    #[default]
    None,
    /// A launcher link (steam://, com.epicgames.launcher://, uplay://).
    Url(String),
    /// A program with fixed arguments (Riot Client, Battle.net, the game itself).
    Exe { exe: String, args: Vec<String> },
    /// A Store/Xbox app by its AppUserModelID.
    AppsFolder(String),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Game {
    /// "steam:730", "epic:Fortnite", "gog:1207658924", "riot:valorant",
    /// "bnet:prometheus", "ubisoft:5595", "rockstar:…", "ea:…", "xbox:<family>",
    /// "custom:<exe path>"
    pub id: String,
    pub name: String,
    pub launcher: &'static str,
    pub install_dir: Option<String>,
    /// Likely game executables (largest/most relevant first).
    pub exes: Vec<String>,
    #[serde(skip)]
    pub launch: Launch,
    /// Whether NOCTURN can start it (and through what, for the UI).
    pub launch_via: Option<&'static str>,
    /// Package details for Xbox/Store games (cover art comes from the package).
    #[serde(skip)]
    pub package_dir: Option<String>,
}

impl Game {
    fn new(id: String, name: String, launcher: &'static str, install_dir: Option<String>, exes: Vec<String>, launch: Launch, via: &'static str) -> Game {
        let launch_via = (launch != Launch::None).then_some(via);
        Game { id, name, launcher, install_dir, exes, launch, launch_via, package_dir: None }
    }
}

/// Values of `"key" "value"` pairs in Valve's KeyValues text format, in order.
fn vdf_values<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let needle = format!("\"{key}\"");
    text.lines()
        .filter_map(|l| {
            let t = l.trim();
            let rest = t.strip_prefix(&needle)?.trim();
            let v = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(v)
        })
        .collect()
}

/// Steam library roots from `libraryfolders.vdf` (paths use escaped `\\`).
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf_values(vdf, "path").into_iter().map(|p| PathBuf::from(p.replace("\\\\", "\\"))).collect()
}

/// (appid, name, installdir) from an `appmanifest_*.acf`.
pub fn parse_app_manifest(acf: &str) -> Option<(String, String, String)> {
    let first = |k: &str| vdf_values(acf, k).first().map(|s| s.to_string());
    Some((first("appid")?, first("name")?, first("installdir")?))
}

/// Steam entries that are runtimes/tools rather than games.
fn is_steam_tool(appid: &str, name: &str) -> bool {
    const TOOL_IDS: &[&str] = &["228980", "1070560", "1391110", "1628350", "250820", "1493710"];
    let n = name.to_ascii_lowercase();
    TOOL_IDS.contains(&appid)
        || n.contains("redistributable")
        || n.starts_with("proton")
        || n.contains("steam linux runtime")
        || n.contains("steamvr")
        || n.ends_with(" sdk")
        || n.contains("dedicated server")
}

pub(crate) fn steam_root() -> Option<PathBuf> {
    let k = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let p: String = k.get_value("SteamPath").ok()?;
    Some(PathBuf::from(p.replace('/', "\\")))
}

fn steam_games() -> Vec<Game> {
    let Some(root) = steam_root() else { return vec![] };
    let mut libs = std::fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")).map(|t| parse_library_folders(&t)).unwrap_or_default();
    if !libs.iter().any(|l| l.eq(&root)) {
        libs.push(root);
    }
    let mut out = Vec::new();
    for lib in libs {
        let apps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            let Some((appid, gname, dir)) = parse_app_manifest(&text) else { continue };
            if is_steam_tool(&appid, &gname) {
                continue;
            }
            let install = apps.join("common").join(&dir);
            if !install.is_dir() {
                continue;
            }
            let launch = Launch::Url(format!("steam://rungameid/{appid}"));
            out.push(Game::new(format!("steam:{appid}"), gname, "Steam", Some(install.display().to_string()), vec![], launch, "Steam"));
        }
    }
    out
}

fn epic_games() -> Vec<Game> {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let dir = Path::new(&base).join(r"Epic\EpicGamesLauncher\Data\Manifests");
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out = Vec::new();
    for e in rd.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("item") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
        let (Some(name), Some(loc)) = (s("DisplayName"), s("InstallLocation")) else { continue };
        // Skip engine installs and plug-ins.
        if v.get("bIsApplication").and_then(|b| b.as_bool()) == Some(false) || name.starts_with("Unreal Engine") {
            continue;
        }
        if !Path::new(&loc).is_dir() {
            continue;
        }
        let exe = s("LaunchExecutable").map(|l| Path::new(&loc).join(l).display().to_string()).filter(|p| Path::new(p).is_file());
        let app = s("AppName").unwrap_or_else(|| name.clone());
        // namespace:item:app is what the Epic launcher itself uses.
        let launch = match (s("CatalogNamespace"), s("CatalogItemId"), s("AppName")) {
            (Some(ns), Some(item), Some(a)) if [&ns, &item, &a].iter().all(|x| x.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) => {
                Launch::Url(format!("com.epicgames.launcher://apps/{ns}%3A{item}%3A{a}?action=launch&silent=true"))
            }
            _ => Launch::None,
        };
        out.push(Game::new(format!("epic:{app}"), name, "Epic Games", Some(loc), exe.into_iter().collect(), launch, "Epic Games"));
    }
    out
}

fn gog_games() -> Vec<Game> {
    let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\GOG.com\Games") else { return vec![] };
    k.enum_keys()
        .flatten()
        .filter_map(|id| {
            let g = k.open_subkey(&id).ok()?;
            let name: String = g.get_value("gameName").ok()?;
            let path: String = g.get_value("path").ok()?;
            let exe: Option<String> = g.get_value("exe").ok();
            let exe = exe.filter(|e| Path::new(e).is_file());
            let launch = exe.clone().map(|e| Launch::Exe { exe: e, args: vec![] }).unwrap_or_default();
            Path::new(&path).is_dir().then(|| Game::new(format!("gog:{id}"), name, "GOG", Some(path), exe.into_iter().collect(), launch, "the game itself"))
        })
        .collect()
}

// ---------------------------------------------------------------- installed-programs list

/// One entry of Windows' installed-programs list (Uninstall keys).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Installed {
    pub name: String,
    pub publisher: String,
    pub location: String,
    pub uninstall: String,
}

fn installed_programs() -> Vec<Installed> {
    let mut out = Vec::new();
    for (root, path) in [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        let Ok(k) = RegKey::predef(root).open_subkey(path) else { continue };
        for sub in k.enum_keys().flatten() {
            let Ok(e) = k.open_subkey(&sub) else { continue };
            let get = |n: &str| e.get_value::<String, _>(n).unwrap_or_default();
            let name = get("DisplayName");
            if name.is_empty() {
                continue;
            }
            out.push(Installed { name, publisher: get("Publisher"), location: get("InstallLocation").trim().trim_matches('"').to_string(), uninstall: get("UninstallString") });
        }
    }
    out
}

/// The program at the start of a command line (quoted or not) and the rest.
fn split_command(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(r) = c.strip_prefix('"') {
        if let Some(i) = r.find('"') {
            return (r[..i].to_string(), r[i + 1..].trim().to_string());
        }
    }
    match c.to_ascii_lowercase().find(".exe") {
        Some(i) => (c[..i + 4].to_string(), c[i + 4..].trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// `--name=value` from an argument string.
fn arg(args: &str, name: &str) -> Option<String> {
    let key = format!("--{name}=");
    let i = args.find(&key)? + key.len();
    let v: String = args[i..].trim_start_matches('"').chars().take_while(|c| !c.is_whitespace() && *c != '"').collect();
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')).then_some(v)
}

/// Riot Client games: `"…\RiotClientServices.exe" --uninstall-product=valorant --uninstall-patchline=live`.
pub fn parse_riot(p: &Installed) -> Option<Game> {
    let (exe, args) = split_command(&p.uninstall);
    if !exe.to_ascii_lowercase().ends_with("riotclientservices.exe") {
        return None;
    }
    let product = arg(&args, "uninstall-product")?;
    let patchline = arg(&args, "uninstall-patchline").unwrap_or_else(|| "live".into());
    let launch = Launch::Exe { exe, args: vec![format!("--launch-product={product}"), format!("--launch-patchline={patchline}")] };
    let dir = (!p.location.is_empty()).then(|| p.location.clone());
    Some(Game::new(format!("riot:{product}"), p.name.clone(), "Riot", dir, vec![], launch, "the Riot Client"))
}

/// Battle.net product codes for the uninstall `--uid=` names.
const BNET_CODES: &[(&str, &str)] = &[
    ("prometheus", "Pro"),
    ("wow", "WoW"),
    ("wow_classic", "WoWC"),
    ("wow_classic_era", "WoWCE"),
    ("diablo3", "D3"),
    ("fenris", "Fen"),
    ("osi", "OSI"),
    ("s2", "S2"),
    ("s1", "S1"),
    ("heroes", "Hero"),
    ("hs_beta", "WTCG"),
    ("w3", "W3"),
    ("rtro", "RTRO"),
    ("odin", "ODIN"),
    ("lazarus", "LAZR"),
    ("zeus", "ZEUS"),
    ("viper", "VIPR"),
    ("fore", "FORE"),
    ("auks", "AUKS"),
    ("spot", "SPOT"),
];

/// Battle.net games: `"…\Battle.net\Battle.net.exe" --uid=prometheus`.
pub fn parse_battlenet(p: &Installed) -> Option<Game> {
    let (exe, args) = split_command(&p.uninstall);
    let lower = exe.to_ascii_lowercase();
    if !(lower.contains("battle.net") && lower.ends_with(".exe")) {
        return None;
    }
    let uid = arg(&args, "uid")?;
    if uid == "battle.net" || uid.starts_with("agent") {
        return None;
    }
    // Launch through Battle.net's own launcher (it refuses games started directly).
    let launcher = Path::new(&exe).parent().map(|d| d.join("Battle.net.exe").display().to_string()).unwrap_or(exe.clone());
    let launch = match BNET_CODES.iter().find(|(u, _)| *u == uid) {
        Some((_, code)) => Launch::Exe { exe: launcher, args: vec![format!("--exec=launch {code}")] },
        None => Launch::Exe { exe: launcher, args: vec![] }, // opens Battle.net on its home page
    };
    let dir = (!p.location.is_empty()).then(|| p.location.clone());
    Some(Game::new(format!("bnet:{uid}"), p.name.clone(), "Battle.net", dir, vec![], launch, "Battle.net"))
}

/// EA app games: publisher Electronic Arts with an install folder (the EA app
/// steps in when the game's exe starts).
pub fn parse_ea(p: &Installed) -> Option<Game> {
    let publ = p.publisher.to_ascii_lowercase();
    let n = p.name.to_ascii_lowercase();
    if !publ.contains("electronic arts") || p.location.is_empty() || n == "ea app" || n.starts_with("origin") || n.contains("ea desktop") {
        return None;
    }
    let key: String = p.name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    Some(Game::new(format!("ea:{key}"), p.name.clone(), "EA app", Some(p.location.clone()), vec![], Launch::None, "the EA app"))
}

fn ubisoft_games(programs: &[Installed]) -> Vec<Game> {
    let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs") else { return vec![] };
    k.enum_keys()
        .flatten()
        .filter(|id| id.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|id| {
            let dir: String = k.open_subkey(&id).ok()?.get_value("InstallDir").ok()?;
            let dir = dir.replace('/', "\\").trim_end_matches('\\').to_string();
            if !Path::new(&dir).is_dir() {
                return None;
            }
            // The installed-programs entry has the proper name ("Uplay Install <id>" key).
            let name = programs
                .iter()
                .find(|p| !p.location.is_empty() && p.location.replace('/', "\\").trim_end_matches('\\').eq_ignore_ascii_case(&dir))
                .map(|p| p.name.clone())
                .or_else(|| Path::new(&dir).file_name().map(|f| f.to_string_lossy().to_string()))?;
            Some(Game::new(format!("ubisoft:{id}"), name, "Ubisoft Connect", Some(dir), vec![], Launch::Url(format!("uplay://launch/{id}/0")), "Ubisoft Connect"))
        })
        .collect()
}

fn rockstar_games() -> Vec<Game> {
    let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\Rockstar Games") else { return vec![] };
    k.enum_keys()
        .flatten()
        .filter(|t| !matches!(t.to_ascii_lowercase().as_str(), "launcher" | "rockstar games launcher" | "rockstar games social club" | "social club"))
        .filter_map(|title| {
            let dir: String = k.open_subkey(&title).ok()?.get_value("InstallFolder").ok()?;
            let dir = dir.trim_end_matches('\\').to_string();
            Path::new(&dir).is_dir().then(|| {
                let key: String = title.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
                Game::new(format!("rockstar:{key}"), title.clone(), "Rockstar", Some(dir), vec![], Launch::None, "the Rockstar Games Launcher")
            })
        })
        .collect()
}

/// Executables and art from a GDK game's `MicrosoftGame.config`.
#[derive(Debug, Default, PartialEq)]
pub struct GameConfig {
    pub exes: Vec<(String, String)>,
    pub display_name: Option<String>,
    pub logo: Option<String>,
}

fn xml_attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let end = tag[i..].find('"')?;
    Some(tag[i..i + end].to_string())
}

pub fn parse_game_config(xml: &str) -> GameConfig {
    let mut cfg = GameConfig::default();
    for part in xml.split('<').skip(1) {
        let tag = part.split('>').next().unwrap_or("");
        if tag.starts_with("Executable ") {
            if let (Some(n), Some(id)) = (xml_attr(tag, "Name"), xml_attr(tag, "Id")) {
                cfg.exes.push((n, id));
            }
        } else if tag.starts_with("ShellVisuals ") {
            cfg.display_name = xml_attr(tag, "DefaultDisplayName").filter(|n| !n.starts_with("ms-resource:"));
            cfg.logo = xml_attr(tag, "Square480x480Logo").or_else(|| xml_attr(tag, "Square150x150Logo")).or_else(|| xml_attr(tag, "StoreLogo"));
        }
    }
    cfg
}

fn xbox_games() -> Vec<Game> {
    use crate::security::exec::{as_array, powershell_json, PsScript};
    let Ok(v) = powershell_json(PsScript::GamePackages) else { return vec![] };
    let start = crate::library::start_apps(false).unwrap_or_default();
    as_array(v)
        .into_iter()
        .filter_map(|p| {
            let family = p.get("Family")?.as_str()?.to_string();
            let loc = p.get("Location")?.as_str()?.to_string();
            let cfg = parse_game_config(&std::fs::read_to_string(Path::new(&loc).join("MicrosoftGame.config")).ok()?);
            // The Start-menu entry gives the real name and the ID Windows launches.
            let entry = start.iter().find(|a| a.app_id.starts_with(&format!("{family}!")));
            let name = entry.map(|a| a.name.clone()).or(cfg.display_name.clone())?;
            let aumid = entry.map(|a| a.app_id.clone()).or_else(|| cfg.exes.first().map(|(_, id)| format!("{family}!{id}")))?;
            let exes = cfg.exes.iter().map(|(n, _)| Path::new(&loc).join(n).display().to_string()).collect();
            let mut g = Game::new(format!("xbox:{family}"), name, "Xbox", Some(loc.clone()), exes, Launch::AppsFolder(aumid), "the Xbox app");
            g.package_dir = Some(loc);
            Some(g)
        })
        .collect()
}

const SKIP_EXE: &[&str] = &["unins", "crash", "report", "setup", "install", "redist", "vcredist", "dxsetup", "dotnet", "ue4prereq", "cefprocess", "helper", "easyanticheat", "beservice", "battleye", "updater", "launcherpatcher", "ffmpeg", "7z", "python", "node.exe", "java"];
const SKIP_DIR: &[&str] = &["content", "paks", "movies", "redist", "commonredist", "directx", "support", "localization", "shadercache", "streamingassets", "plugins", "tools", "mods", "_data", "engine"];

fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Find likely game executables under an install folder. Breadth-first so
/// shallow binaries are found before deep asset folders exhaust the budget.
/// Ranking: Unreal "-Shipping" binaries, then names resembling the game or
/// its folder, then executables nearest the root; size breaks ties.
pub fn find_exes(dir: &Path, game_name: &str) -> Vec<String> {
    let want = [alnum(game_name), alnum(&dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())];
    let mut found: Vec<(u8, u8, u64, String)> = Vec::new();
    let mut queue = std::collections::VecDeque::from([(dir.to_path_buf(), 0u8)]);
    let mut visited = 0usize;
    while let Some((d, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            visited += 1;
            if visited > 8000 {
                break;
            }
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            // file_type() comes from the directory listing: no extra disk query.
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                if depth < 5 && !SKIP_DIR.iter().any(|s| name.contains(s)) {
                    queue.push_back((p, depth + 1));
                }
            } else if name.ends_with(".exe") && !SKIP_EXE.iter().any(|s| name.contains(s)) {
                let stem = alnum(name.trim_end_matches(".exe"));
                let similar = !stem.is_empty() && want.iter().any(|w| !w.is_empty() && (w.contains(&stem) || stem.contains(w.as_str()) || (stem.len() >= 4 && w.starts_with(&stem[..4]))));
                let rank = if name.contains("shipping") { 0 } else if similar { 1 } else { 2 };
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                found.push((rank, depth, size, p.display().to_string()));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(b.2.cmp(&a.2)));
    found.into_iter().take(8).map(|(_, _, _, p)| p).collect()
}

/// All detected games, with candidate executables resolved.
pub fn detect(custom_exes: &[String]) -> Vec<Game> {
    let programs = installed_programs();
    let mut games: Vec<Game> = steam_games()
        .into_iter()
        .chain(epic_games())
        .chain(gog_games())
        .chain(programs.iter().filter_map(parse_battlenet))
        .chain(programs.iter().filter_map(parse_riot))
        .chain(programs.iter().filter_map(parse_ea))
        .chain(ubisoft_games(&programs))
        .chain(rockstar_games())
        .chain(xbox_games())
        .collect();
    // One entry per install folder: a Steam copy listed again by Rockstar or
    // the installed-programs list stays the Steam one (listed first).
    let mut seen = std::collections::HashSet::new();
    games.retain(|g| match &g.install_dir {
        Some(d) => seen.insert(d.to_ascii_lowercase().trim_end_matches('\\').to_string()),
        None => true,
    });
    for g in &mut games {
        // Store packages list their own executables (and their folders can't be browsed).
        if g.launcher == "Xbox" {
            continue;
        }
        if let Some(dir) = &g.install_dir {
            if Path::new(dir).is_dir() {
                for exe in find_exes(Path::new(dir), &g.name) {
                    if !g.exes.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
                        g.exes.push(exe);
                    }
                }
            }
        }
        // EA and Rockstar games start through their exe (the launcher steps in).
        if g.launch == Launch::None && matches!(g.launcher, "EA app" | "Rockstar") {
            if let Some(exe) = g.exes.first() {
                g.launch = Launch::Exe { exe: exe.clone(), args: vec![] };
                g.launch_via = Some(if g.launcher == "EA app" { "the EA app" } else { "the Rockstar Games Launcher" });
            }
        }
    }
    for exe in custom_exes {
        let p = Path::new(exe);
        if !p.is_file() {
            continue;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| exe.clone());
        games.push(Game::new(
            format!("custom:{}", exe.to_ascii_lowercase()),
            name,
            "Added by you",
            p.parent().map(|d| d.display().to_string()),
            vec![exe.clone()],
            Launch::Exe { exe: exe.clone(), args: vec![] },
            "the game itself",
        ));
    }
    games.sort_by_key(|a| a.name.to_ascii_lowercase());
    games.dedup_by(|a, b| a.id == b.id);
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steam_metadata() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}";
        let libs = parse_library_folders(vdf);
        assert_eq!(libs, vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]);
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"installdir\"\t\t\"Counter-Strike Global Offensive\"\n}";
        assert_eq!(parse_app_manifest(acf), Some(("730".into(), "Counter-Strike 2".into(), "Counter-Strike Global Offensive".into())));
        assert!(is_steam_tool("228980", "Steamworks Common Redistributables"));
        assert!(!is_steam_tool("730", "Counter-Strike 2"));
    }

    #[test]
    fn finds_and_ranks_exes() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join(r"Game\Binaries\Win64");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("Game-Win64-Shipping.exe"), vec![0u8; 10]).unwrap();
        std::fs::write(d.path().join("Launcher.exe"), vec![0u8; 100]).unwrap();
        std::fs::write(d.path().join("unins000.exe"), vec![0u8; 1000]).unwrap();
        std::fs::create_dir_all(d.path().join("Content")).unwrap();
        std::fs::write(d.path().join(r"Content\hidden.exe"), b"x").unwrap();
        let exes = find_exes(d.path(), "Game");
        assert!(exes[0].ends_with("Game-Win64-Shipping.exe"), "{exes:?}");
        assert!(exes.iter().any(|e| e.ends_with("Launcher.exe")));
        assert!(!exes.iter().any(|e| e.contains("unins") || e.contains("hidden")));
    }

    fn prog(name: &str, publisher: &str, location: &str, uninstall: &str) -> Installed {
        Installed { name: name.into(), publisher: publisher.into(), location: location.into(), uninstall: uninstall.into() }
    }

    #[test]
    fn riot_battlenet_and_ea_entries() {
        // Riot Client (real format).
        let g = parse_riot(&prog("VALORANT", "Riot Games, Inc", r"C:\Riot Games\VALORANT\live", r#""C:\Riot Games\Riot Client\RiotClientServices.exe" --uninstall-product=valorant --uninstall-patchline=live"#)).unwrap();
        assert_eq!((g.id.as_str(), g.launcher, g.launch_via), ("riot:valorant", "Riot", Some("the Riot Client")));
        assert_eq!(g.launch, Launch::Exe { exe: r"C:\Riot Games\Riot Client\RiotClientServices.exe".into(), args: vec!["--launch-product=valorant".into(), "--launch-patchline=live".into()] });
        assert!(parse_riot(&prog("Riot Vanguard", "Riot Games, Inc", "", r#""C:\Program Files\Riot Vanguard\installer.exe" --quiet"#)).is_none());
        // Battle.net (real format), known and unknown product.
        let g = parse_battlenet(&prog("Overwatch", "Blizzard Entertainment", r"C:\Program Files (x86)\Overwatch", r#""C:\Program Files (x86)\Battle.net\Battle.net Launcher.exe" --uid=prometheus"#)).unwrap();
        assert_eq!(g.launch, Launch::Exe { exe: r"C:\Program Files (x86)\Battle.net\Battle.net.exe".into(), args: vec!["--exec=launch Pro".into()] });
        let g = parse_battlenet(&prog("Some New Game", "Blizzard Entertainment", "", r#""C:\Program Files (x86)\Battle.net\Battle.net Launcher.exe" --uid=newthing"#)).unwrap();
        assert_eq!(g.launch, Launch::Exe { exe: r"C:\Program Files (x86)\Battle.net\Battle.net.exe".into(), args: vec![] });
        assert!(parse_battlenet(&prog("Battle.net", "Blizzard Entertainment", "", r#""C:\Program Files (x86)\Battle.net\Battle.net Launcher.exe" --uid=battle.net"#)).is_none());
        // Injected arguments are refused.
        assert!(parse_riot(&prog("X", "", "", r#""C:\R\RiotClientServices.exe" --uninstall-product=val&calc"#)).is_none());
        // EA app games, not the EA app itself.
        assert!(parse_ea(&prog("Apex Legends", "Electronic Arts", r"C:\Program Files\EA Games\Apex", "x")).is_some());
        assert!(parse_ea(&prog("EA app", "Electronic Arts", r"C:\Program Files\Electronic Arts\EA Desktop", "x")).is_none());
        assert!(parse_ea(&prog("Apex Legends", "Electronic Arts", "", "x")).is_none());
    }

    #[test]
    fn gdk_game_config() {
        let xml = r#"<?xml version="1.0"?><Game configVersion="1"><Identity Name="Microsoft.MinecraftUWP" Publisher="CN=Microsoft"/><ExecutableList><Executable Name="Minecraft.Windows.exe" Id="Game" TargetDeviceFamily="PC"/></ExecutableList><ShellVisuals DefaultDisplayName="ms-resource:AppName" Square150x150Logo="Assets\Square150x150Logo.png" Square480x480Logo="Assets\Square480x480Logo.png" StoreLogo="Assets\StoreLogo.png"/></Game>"#;
        let c = parse_game_config(xml);
        assert_eq!(c.exes, vec![("Minecraft.Windows.exe".into(), "Game".into())]);
        assert_eq!(c.display_name, None, "resource names aren't display names");
        assert_eq!(c.logo.as_deref(), Some(r"Assets\Square480x480Logo.png"));
    }

    #[test]
    fn detects_real_games() {
        let games = detect(&[]);
        for g in &games {
            eprintln!("{} | {} | {:?} | {:?}", g.launcher, g.name, g.install_dir, g.exes.first());
            assert!(g.install_dir.as_ref().is_some_and(|d| Path::new(d).is_dir()));
        }
    }
}
