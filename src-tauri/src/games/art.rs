//! Cover art for games in the Library, and starting games.
//!
//! Covers, best first:
//! 1. Steam games: Steam's own portrait art from its local cache (no network),
//!    else Steam's public CDN.
//! 2. Xbox / Store games: the package's own logo.
//! 3. Other games: if a game with exactly the same name is on Steam, its
//!    public portrait art (one search on store.steampowered.com).
//! 4. The game's .exe icon.
//!
//! Every cover is cached in the data folder; misses are remembered for the
//! session so nothing is fetched twice.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use sha2::{Digest, Sha256};

use super::detect::{parse_game_config, Game, Launch};
use crate::error::{AppError, AppResult};

const CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
const MAX_COVER: usize = 3 * 1024 * 1024;

static MISSING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn cache_file(dir: &Path, id: &str) -> PathBuf {
    let h = Sha256::digest(id.as_bytes());
    dir.join(format!("{}.img", h.iter().take(12).map(|b| format!("{b:02x}")).collect::<String>()))
}

fn mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn data_url(bytes: &[u8]) -> String {
    format!("data:{};base64,{}", mime(bytes), base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn is_image(b: &[u8]) -> bool {
    b.len() > 200 && (b.starts_with(&[0xFF, 0xD8]) || b.starts_with(&[0x89, b'P', b'N', b'G']) || (b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP")))
}

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("NOCTURN v", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| AppError::msg(e.to_string()))
}

async fn get_image(url: &str) -> Option<Vec<u8>> {
    let res = client().ok()?.get(url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let b = res.bytes().await.ok()?;
    (b.len() <= MAX_COVER && is_image(&b)).then(|| b.to_vec())
}

/// Steam's local copy of a game's portrait art (`librarycache\<appid>\…library_600x900.jpg`).
fn steam_local(appid: &str) -> Option<Vec<u8>> {
    let root = super::detect::steam_root()?;
    let cache = root.join(r"appcache\librarycache");
    let dir = cache.join(appid);
    let mut candidates = vec![cache.join(format!("{appid}_library_600x900.jpg")), dir.join("library_600x900.jpg")];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                candidates.push(p.join("library_600x900.jpg"));
            }
        }
    }
    candidates.into_iter().find_map(|p| std::fs::read(p).ok().filter(|b| b.len() <= MAX_COVER && is_image(b)))
}

async fn steam_cover(appid: &str) -> Option<Vec<u8>> {
    if !appid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some(b) = steam_local(appid) {
        return Some(b);
    }
    match get_image(&format!("{CDN}/{appid}/library_600x900.jpg")).await {
        Some(b) => Some(b),
        None => get_image(&format!("{CDN}/{appid}/header.jpg")).await,
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A Steam app with exactly this name (ignoring case, symbols and ™/®).
async fn steam_appid_for(name: &str) -> Option<String> {
    let q: String = name.chars().filter(|c| !matches!(c, '™' | '®' | '©')).collect();
    let url = reqwest::Url::parse_with_params("https://store.steampowered.com/api/storesearch/", &[("term", q.as_str()), ("l", "english"), ("cc", "US")]).ok()?;
    let v: serde_json::Value = client().ok()?.get(url).send().await.ok()?.json().await.ok()?;
    let want = norm(&q);
    v.get("items")?.as_array()?.iter().find(|i| i.get("name").and_then(|n| n.as_str()).map(norm).as_deref() == Some(want.as_str())).and_then(|i| i.get("id")?.as_u64()).map(|id| id.to_string())
}

/// The package's own logo for an Xbox/Store game.
fn package_logo(dir: &str) -> Option<Vec<u8>> {
    let cfg = parse_game_config(&std::fs::read_to_string(Path::new(dir).join("MicrosoftGame.config")).ok()?);
    let logo = cfg.logo?;
    let p = Path::new(dir).join(&logo);
    if let Ok(b) = std::fs::read(&p) {
        return Some(b);
    }
    // Scaled variants: Logo.scale-200.png …
    let (parent, stem) = (p.parent()?, p.file_stem()?.to_string_lossy().to_string());
    let mut best: Option<(u64, PathBuf)> = None;
    for e in std::fs::read_dir(parent).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(&format!("{stem}.")) && n.ends_with(".png") {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().is_none_or(|b| size > b.0) {
                best = Some((size, e.path()));
            }
        }
    }
    std::fs::read(best?.1).ok()
}

/// The Microsoft Store's poster for a Store/Xbox game (public catalogue, looked up by package family).
async fn store_poster(family: &str) -> Option<Vec<u8>> {
    if !family.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-') {
        return None;
    }
    let url = format!("https://displaycatalog.mp.microsoft.com/v7.0/products/lookup?alternateId=PackageFamilyName&value={family}&market=US&languages=en-us&fieldsTemplate=details");
    let v: serde_json::Value = client().ok()?.get(url).send().await.ok()?.json().await.ok()?;
    let images = v.pointer("/Products/0/LocalizedProperties/0/Images")?.as_array()?;
    let pick = |purpose: &str| images.iter().find(|i| i.get("ImagePurpose").and_then(|p| p.as_str()) == Some(purpose)).and_then(|i| i.get("Uri")?.as_str());
    let uri = pick("Poster").or_else(|| pick("BoxArt"))?;
    let full = format!("https:{}", uri.trim_start_matches("https:"));
    let host_ok = reqwest::Url::parse(&full).ok()?.host_str()?.ends_with(".s-microsoft.com");
    if !host_ok {
        return None;
    }
    get_image(&format!("{full}?w=600&h=900&q=90&mode=scale&format=jpg")).await
}

async fn find_cover(g: &Game) -> Option<Vec<u8>> {
    if let Some(appid) = g.id.strip_prefix("steam:") {
        if let Some(b) = steam_cover(appid).await {
            return Some(b);
        }
    }
    if let Some(family) = g.id.strip_prefix("xbox:") {
        if let Some(b) = store_poster(family).await {
            return Some(b);
        }
    }
    if let Some(b) = g.package_dir.as_deref().and_then(package_logo) {
        return Some(b);
    }
    if !g.id.starts_with("steam:") && !g.id.starts_with("custom:") {
        if let Some(appid) = steam_appid_for(&g.name).await {
            if let Some(b) = steam_cover(&appid).await {
                return Some(b);
            }
        }
    }
    g.exes.first().and_then(|e| crate::library_icons::file_icon_png(e))
}

/// Covers for detected games (by ID) as data URLs; None when there's nothing better than a placeholder.
pub fn covers(data_dir: &Path, games: &[Game], ids: &[String]) -> HashMap<String, Option<String>> {
    let dir = data_dir.join("covers");
    let _ = std::fs::create_dir_all(&dir);
    let mut out = HashMap::new();
    for id in ids.iter().take(500) {
        let Some(g) = games.iter().find(|g| &g.id == id) else {
            out.insert(id.clone(), None);
            continue;
        };
        let f = cache_file(&dir, id);
        if let Ok(b) = std::fs::read(&f) {
            out.insert(id.clone(), Some(data_url(&b)));
            continue;
        }
        if MISSING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.contains(id)) {
            out.insert(id.clone(), None);
            continue;
        }
        match tauri::async_runtime::block_on(find_cover(g)) {
            Some(b) => {
                let _ = std::fs::write(&f, &b);
                out.insert(id.clone(), Some(data_url(&b)));
            }
            None => {
                MISSING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(id.clone());
                out.insert(id.clone(), None);
            }
        }
    }
    out
}

/// Windows-style argument string: values with spaces quoted after `=`
/// (`--exec="launch Pro"`), whole args quoted otherwise.
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.contains(' ') {
                a.clone()
            } else if let Some((k, v)) = a.split_once('=') {
                format!("{k}=\"{v}\"")
            } else {
                format!("\"{a}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const URL_SCHEMES: &[&str] = &["steam://rungameid/", "com.epicgames.launcher://apps/", "uplay://launch/"];

/// Start a detected game through its launcher.
pub fn launch(g: &Game) -> AppResult<()> {
    match &g.launch {
        Launch::None => Err(AppError::Invalid(format!("NOCTURN can't start {} by itself — open it from its launcher", g.name))),
        Launch::Url(u) => {
            if !URL_SCHEMES.iter().any(|s| u.starts_with(s)) {
                return Err(AppError::Invalid("unexpected launch link".into()));
            }
            crate::library::shell_run(u, None, None)
        }
        Launch::Exe { exe, args } => {
            let p = Path::new(exe);
            if !p.is_file() || !exe.to_ascii_lowercase().ends_with(".exe") {
                return Err(AppError::NotFound(format!("{} (it may have been moved or uninstalled)", g.name)));
            }
            let params = join_args(args);
            crate::library::shell_run(exe, (!params.is_empty()).then_some(params.as_str()), p.parent().and_then(|d| d.to_str()))
        }
        Launch::AppsFolder(id) => crate::library::shell_run(&format!(r"shell:AppsFolder\{id}"), None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_the_way_launchers_expect() {
        assert_eq!(join_args(&["--exec=launch Pro".into()]), r#"--exec="launch Pro""#);
        assert_eq!(join_args(&["--launch-product=valorant".into(), "--launch-patchline=live".into()]), "--launch-product=valorant --launch-patchline=live");
        assert_eq!(join_args(&["two words".into()]), "\"two words\"");
    }

    #[test]
    fn images_are_recognised() {
        let mut jpg = vec![0xFF, 0xD8];
        jpg.resize(300, 0);
        assert!(is_image(&jpg) && mime(&jpg) == "image/jpeg");
        assert!(!is_image(b"<html>not found</html>"));
        assert_eq!(norm("Grand Theft Auto V™: Enhanced"), "grandtheftautovenhanced");
    }

    #[test]
    fn only_known_launch_links_are_opened() {
        let mut g = Game { id: "x".into(), name: "X".into(), launcher: "Steam", install_dir: None, exes: vec![], launch: Launch::Url("https://evil.example/".into()), launch_via: Some("Steam"), package_dir: None };
        assert!(launch(&g).is_err());
        g.launch = Launch::Exe { exe: r"C:\nope\missing.exe".into(), args: vec![] };
        assert!(launch(&g).is_err());
        g.launch = Launch::None;
        assert!(launch(&g).is_err());
    }

    /// Live: covers for the games on this PC (network for non-Steam lookups).
    #[test]
    #[ignore]
    fn live_covers() {
        let games = super::super::detect::detect(&[]);
        let dir = std::env::temp_dir().join("nocturn-covers-test");
        let ids: Vec<String> = games.iter().map(|g| g.id.clone()).collect();
        let c = covers(&dir, &games, &ids);
        for g in &games {
            let v = c.get(&g.id).cloned().flatten();
            println!("{:<10} {:<40} {:?} | cover: {}", g.launcher, g.name, g.launch_via, v.map(|u| format!("{} ({} KB)", &u[5..u.find(';').unwrap_or(15)], u.len() * 3 / 4 / 1024)).unwrap_or("none".into()));
        }
    }
}
