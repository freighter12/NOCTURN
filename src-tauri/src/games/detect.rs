//! Installed-game detection from launcher metadata (read-only):
//! - Steam: `libraryfolders.vdf` + `appmanifest_*.acf` in each library
//! - Epic Games: `*.item` manifests in ProgramData
//! - GOG Galaxy: `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`
//! - Games the user added by pointing at an .exe
//!
//! Microsoft Store / Xbox games live in protected WindowsApps folders and
//! aren't listed yet.

use std::path::{Path, PathBuf};

use serde::Serialize;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Game {
    /// "steam:730", "epic:Fortnite", "gog:1207658924", "custom:<exe path>"
    pub id: String,
    pub name: String,
    pub launcher: &'static str,
    pub install_dir: Option<String>,
    /// Likely game executables (largest/most relevant first).
    pub exes: Vec<String>,
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

fn steam_root() -> Option<PathBuf> {
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
            out.push(Game { id: format!("steam:{appid}"), name: gname, launcher: "Steam", install_dir: Some(install.display().to_string()), exes: vec![] });
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
        let launch = s("LaunchExecutable").map(|l| Path::new(&loc).join(l).display().to_string()).filter(|p| Path::new(p).is_file());
        out.push(Game {
            id: format!("epic:{}", s("AppName").unwrap_or_else(|| name.clone())),
            name,
            launcher: "Epic Games",
            install_dir: Some(loc),
            exes: launch.into_iter().collect(),
        });
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
            Path::new(&path).is_dir().then(|| Game {
                id: format!("gog:{id}"),
                name,
                launcher: "GOG",
                install_dir: Some(path),
                exes: exe.filter(|e| Path::new(e).is_file()).into_iter().collect(),
            })
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
    let mut games: Vec<Game> = steam_games().into_iter().chain(epic_games()).chain(gog_games()).collect();
    for g in &mut games {
        if let Some(dir) = &g.install_dir {
            for exe in find_exes(Path::new(dir), &g.name) {
                if !g.exes.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
                    g.exes.push(exe);
                }
            }
        }
    }
    for exe in custom_exes {
        let p = Path::new(exe);
        if !p.is_file() {
            continue;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| exe.clone());
        games.push(Game {
            id: format!("custom:{}", exe.to_ascii_lowercase()),
            name,
            launcher: "Added by you",
            install_dir: p.parent().map(|d| d.display().to_string()),
            exes: vec![exe.clone()],
        });
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

    #[test]
    fn detects_real_games() {
        let games = detect(&[]);
        for g in &games {
            eprintln!("{} | {} | {:?} | {:?}", g.launcher, g.name, g.install_dir, g.exes.first());
            assert!(g.install_dir.as_ref().is_some_and(|d| Path::new(d).is_dir()));
        }
    }
}
