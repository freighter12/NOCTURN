//! App Library: the user's own collection of installed apps, connected
//! services and websites, with categories, favourites, pins and recents.
//!
//! Launching is deliberately narrow:
//! * installed apps open through Windows' own `shell:AppsFolder\<AppID>`,
//!   and only if the AppID is still in Windows' Start menu list;
//! * connected services open through their official app link when that app
//!   is installed, otherwise their official website — both from a fixed list;
//! * websites the user added open in the default browser (https only).
//!
//! Nothing is embedded, scraped or automated inside another service.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::security::exec::{self, PsScript};

// ------------------------------------------------------- connected services

pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    /// URL scheme of the official desktop app, if it has one.
    pub scheme: Option<&'static str>,
    /// Link that opens the app (uses `scheme`).
    pub app_link: Option<&'static str>,
    /// Official website / web app.
    pub web: &'static str,
    /// Extra deep links into specific places in the app.
    pub deep_links: &'static [(&'static str, &'static str)],
}

/// Services NOCTURN knows how to open. Each opens through the official app
/// (if installed) or the official website — never embedded.
pub const SERVICES: &[Service] = &[
    Service { id: "discord", name: "Discord", category: "Communication", description: "Voice, video and text chat.", scheme: Some("discord"), app_link: Some("discord://-/channels/@me"), web: "https://discord.com/app", deep_links: &[("Friends", "discord://-/channels/@me"), ("Settings", "discord://-/settings/account")] },
    Service { id: "steam", name: "Steam", category: "Games", description: "Games library and store.", scheme: Some("steam"), app_link: Some("steam://open/main"), web: "https://store.steampowered.com", deep_links: &[("Library", "steam://open/games"), ("Friends", "steam://open/friends"), ("Downloads", "steam://open/downloads")] },
    Service { id: "epic", name: "Epic Games", category: "Games", description: "Epic Games launcher and store.", scheme: Some("com.epicgames.launcher"), app_link: Some("com.epicgames.launcher://apps"), web: "https://store.epicgames.com", deep_links: &[("Library", "com.epicgames.launcher://library")] },
    Service { id: "spotify", name: "Spotify", category: "Media", description: "Music streaming.", scheme: Some("spotify"), app_link: Some("spotify:"), web: "https://open.spotify.com", deep_links: &[] },
    Service { id: "whatsapp", name: "WhatsApp", category: "Communication", description: "Messaging.", scheme: Some("whatsapp"), app_link: Some("whatsapp:"), web: "https://web.whatsapp.com", deep_links: &[] },
    Service { id: "telegram", name: "Telegram", category: "Communication", description: "Messaging.", scheme: Some("tg"), app_link: Some("tg://"), web: "https://web.telegram.org", deep_links: &[] },
    Service { id: "slack", name: "Slack", category: "Communication", description: "Team chat.", scheme: Some("slack"), app_link: Some("slack://open"), web: "https://app.slack.com", deep_links: &[] },
    Service { id: "teams", name: "Microsoft Teams", category: "Communication", description: "Meetings and chat.", scheme: Some("msteams"), app_link: Some("msteams:"), web: "https://teams.microsoft.com", deep_links: &[] },
    Service { id: "outlook", name: "Outlook", category: "Productivity", description: "Mail and calendar.", scheme: None, app_link: None, web: "https://outlook.live.com", deep_links: &[("Calendar", "https://outlook.live.com/calendar")] },
    Service { id: "gmail", name: "Gmail", category: "Productivity", description: "Google mail.", scheme: None, app_link: None, web: "https://mail.google.com", deep_links: &[] },
    Service { id: "notion", name: "Notion", category: "Productivity", description: "Notes and docs.", scheme: Some("notion"), app_link: Some("notion://"), web: "https://www.notion.so", deep_links: &[] },
    Service { id: "youtube", name: "YouTube", category: "Media", description: "Video.", scheme: None, app_link: None, web: "https://www.youtube.com", deep_links: &[("Subscriptions", "https://www.youtube.com/feed/subscriptions")] },
    Service { id: "twitch", name: "Twitch", category: "Media", description: "Live streams.", scheme: None, app_link: None, web: "https://www.twitch.tv", deep_links: &[("Following", "https://www.twitch.tv/directory/following")] },
    Service { id: "xbox", name: "Xbox", category: "Games", description: "Xbox app and Game Pass.", scheme: Some("xbox"), app_link: Some("xbox:"), web: "https://www.xbox.com/play", deep_links: &[] },
];

pub fn service(id: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|s| s.id == id)
}

/// Whether a URL scheme has a registered handler (the app is installed).
pub fn scheme_installed(scheme: &str) -> bool {
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return false;
    }
    winreg::RegKey::predef(winreg::enums::HKEY_CLASSES_ROOT).open_subkey(format!(r"{scheme}\shell\open\command")).is_ok()
}

// ------------------------------------------------------------ installed apps

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StartApp {
    pub name: String,
    pub app_id: String,
    /// Suggested category from the name.
    pub category: String,
}

static START_APPS: Mutex<Option<(Instant, Vec<StartApp>)>> = Mutex::new(None);

pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && !id.chars().any(|c| c.is_control() || c == '"')
}

/// A best-effort category from the app's name.
pub fn guess_category(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| n.contains(k));
    if has(&["steam", "epic games", "battle.net", "ubisoft", "ea app", "gog galaxy", "riot", "minecraft", "xbox", "game"]) {
        "Games"
    } else if has(&["discord", "teams", "slack", "whatsapp", "telegram", "zoom", "skype", "signal", "outlook", "thunderbird", "mail"]) {
        "Communication"
    } else if has(&["chrome", "firefox", "edge", "brave", "opera", "vivaldi"]) {
        "Browsers"
    } else if has(&["word", "excel", "powerpoint", "onenote", "notion", "obsidian", "libreoffice", "acrobat", "notepad", "calculator"]) {
        "Productivity"
    } else if has(&["photoshop", "premiere", "after effects", "blender", "davinci", "resolve", "gimp", "krita", "obs", "audacity", "fl studio", "ableton", "figma", "illustrator", "lightroom", "blackmagic"]) {
        "Creative"
    } else if has(&["spotify", "vlc", "media player", "itunes", "netflix", "music", "movies", "photos"]) {
        "Media"
    } else if has(&["visual studio", "code", "terminal", "powershell", "git", "python", "android studio", "intellij", "pycharm"]) {
        "Development"
    } else {
        "Utilities"
    }
}

/// Windows' Start menu list (cached for a minute).
pub fn start_apps(refresh: bool) -> AppResult<Vec<StartApp>> {
    if let (false, Ok(g)) = (refresh, START_APPS.lock()) {
        if let Some((at, list)) = g.as_ref() {
            if at.elapsed() < Duration::from_secs(60) {
                return Ok(list.clone());
            }
        }
    }
    let rows = exec::as_array(exec::powershell_json(PsScript::StartApps)?);
    let mut list: Vec<StartApp> = rows
        .iter()
        .filter_map(|r| {
            let name = r.get("Name")?.as_str()?.trim().to_string();
            let app_id = r.get("AppID")?.as_str()?.trim().to_string();
            (valid_app_id(&app_id) && !name.is_empty()).then(|| StartApp { category: guess_category(&name).into(), name, app_id })
        })
        // Uninstallers and readmes aren't apps people want in a library.
        .filter(|a| {
            let n = a.name.to_ascii_lowercase();
            !(n.starts_with("uninstall") || n.contains("readme") || n.ends_with(" website") || n.contains("release notes"))
        })
        .collect();
    list.sort_by_key(|a| a.name.to_lowercase());
    list.dedup_by(|a, b| a.app_id == b.app_id);
    if let Ok(mut g) = START_APPS.lock() {
        *g = Some((Instant::now(), list.clone()));
    }
    Ok(list)
}

// ------------------------------------------------------------------- items

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Installed,
    Connected,
    Website,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Installed => "installed",
            Kind::Connected => "connected",
            Kind::Website => "website",
        }
    }
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "installed" => Kind::Installed,
            "connected" => Kind::Connected,
            "website" => Kind::Website,
            _ => return None,
        })
    }
}

/// How an item looks: a monogram or emoji on a colour. Validated on save.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Icon {
    #[serde(default)]
    pub glyph: Option<String>,
    #[serde(default)]
    pub colour: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    /// AppID (installed), service ID (connected) or URL (website).
    pub target: String,
    pub category: String,
    pub favourite: bool,
    pub pinned: bool,
    pub icon: Icon,
    pub added_at: String,
    pub last_opened_at: Option<String>,
    pub open_count: i64,
    /// For connected services: whether the official app is installed.
    pub app_installed: Option<bool>,
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

fn valid_url(u: &str) -> bool {
    u.len() <= 2048 && u.starts_with("https://") && !u.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"') && u.len() > "https://x".len()
}

fn validate_icon(i: &Icon) -> AppResult<Icon> {
    let glyph = i.glyph.as_deref().map(|g| g.trim().chars().take(2).collect::<String>()).filter(|g| !g.is_empty());
    let colour = match i.colour.as_deref() {
        None => None,
        Some(c) if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit()) => Some(c.to_lowercase()),
        Some(_) => return Err(AppError::Invalid("icon colour must be #rrggbb".into())),
    };
    Ok(Icon { glyph, colour })
}

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<Option<Item>> {
    let kind: String = r.get(1)?;
    let Some(kind) = Kind::parse(&kind) else { return Ok(None) };
    let icon_json: String = r.get(7)?;
    let target: String = r.get(3)?;
    let app_installed = (kind == Kind::Connected).then(|| service(&target).and_then(|s| s.scheme).is_some_and(scheme_installed));
    Ok(Some(Item {
        id: r.get(0)?,
        kind,
        name: r.get(2)?,
        target,
        category: r.get(4)?,
        favourite: r.get::<_, i64>(5)? != 0,
        pinned: r.get::<_, i64>(6)? != 0,
        icon: serde_json::from_str(&icon_json).unwrap_or_default(),
        added_at: r.get(8)?,
        last_opened_at: r.get(9)?,
        open_count: r.get(10)?,
        app_installed,
    }))
}

const COLS: &str = "id, kind, name, target, category, favourite, pinned, icon_json, added_at, last_opened_at, open_count";

pub fn list(db: &Db) -> AppResult<Vec<Item>> {
    let conn = db.conn();
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM library_items ORDER BY name COLLATE NOCASE"))?;
    let rows = st.query_map([], row_to_item)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().flatten().collect())
}

pub fn get(db: &Db, id: &str) -> AppResult<Option<Item>> {
    let conn = db.conn();
    Ok(conn.query_row(&format!("SELECT {COLS} FROM library_items WHERE id = ?1"), [id], row_to_item).optional()?.flatten())
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewItem {
    pub kind: Kind,
    pub name: String,
    pub target: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub icon: Icon,
}

/// Add items. Installed apps must be in Windows' Start list; services must be
/// known; websites must be https. Already-added targets are skipped.
pub fn add(db: &Db, items: &[NewItem]) -> AppResult<Vec<Item>> {
    if items.is_empty() || items.len() > 300 {
        return Err(AppError::Invalid("add between 1 and 300 items".into()));
    }
    let start = if items.iter().any(|i| i.kind == Kind::Installed) { start_apps(false)? } else { vec![] };
    let mut added = Vec::new();
    for it in items {
        let (name, target, category) = match it.kind {
            Kind::Installed => {
                let a = start
                    .iter()
                    .find(|a| a.app_id == it.target)
                    .ok_or_else(|| AppError::Invalid(format!("'{}' isn't in Windows' Start menu", clean_text(&it.name, 60))))?;
                (clean_text(if it.name.trim().is_empty() { &a.name } else { &it.name }, 80), a.app_id.clone(), a.category.clone())
            }
            Kind::Connected => {
                let s = service(&it.target).ok_or_else(|| AppError::Invalid(format!("unknown service '{}'", it.target)))?;
                (s.name.to_string(), s.id.to_string(), s.category.to_string())
            }
            Kind::Website => {
                let url = it.target.trim().to_string();
                if !valid_url(&url) {
                    return Err(AppError::Invalid("website addresses must start with https://".into()));
                }
                let name = clean_text(&it.name, 80);
                (if name.is_empty() { url.trim_start_matches("https://").split('/').next().unwrap_or("Website").to_string() } else { name }, url, "Websites".to_string())
            }
        };
        let category = it.category.as_deref().map(|c| clean_text(c, 40)).filter(|c| !c.is_empty()).unwrap_or(category);
        let exists: Option<String> = db.conn().query_row("SELECT id FROM library_items WHERE kind = ?1 AND target = ?2", params![it.kind.as_str(), target], |r| r.get(0)).optional()?;
        if exists.is_some() {
            continue;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let icon = validate_icon(&it.icon)?;
        db.conn().execute(
            "INSERT INTO library_items(id, kind, name, target, category, icon_json, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, it.kind.as_str(), name, target, category, serde_json::to_string(&icon)?, now()],
        )?;
        if let Some(i) = get(db, &id)? {
            added.push(i);
        }
    }
    db.audit("info", "user", "library.add", Some(serde_json::json!({ "count": added.len() })));
    Ok(added)
}

#[derive(Debug, Clone, Deserialize)]
pub struct Edit {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub favourite: Option<bool>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub icon: Option<Icon>,
}

pub fn edit(db: &Db, e: &Edit) -> AppResult<Item> {
    let cur = get(db, &e.id)?.ok_or_else(|| AppError::NotFound("library item".into()))?;
    let name = e.name.as_deref().map(|n| clean_text(n, 80)).filter(|n| !n.is_empty()).unwrap_or(cur.name);
    let category = e.category.as_deref().map(|c| clean_text(c, 40)).unwrap_or(cur.category);
    let icon = match &e.icon {
        Some(i) => validate_icon(i)?,
        None => cur.icon,
    };
    db.conn().execute(
        "UPDATE library_items SET name = ?2, category = ?3, favourite = ?4, pinned = ?5, icon_json = ?6 WHERE id = ?1",
        params![e.id, name, category, e.favourite.unwrap_or(cur.favourite) as i64, e.pinned.unwrap_or(cur.pinned) as i64, serde_json::to_string(&icon)?],
    )?;
    get(db, &e.id)?.ok_or_else(|| AppError::NotFound("library item".into()))
}

pub fn remove(db: &Db, id: &str) -> AppResult<()> {
    db.conn().execute("DELETE FROM library_items WHERE id = ?1", [id])?;
    Ok(())
}

// ------------------------------------------------------------------ launch

/// What `launch` opened, for the UI to explain the transition.
#[derive(Debug, Clone, Serialize)]
pub struct Launched {
    /// app, browser
    pub via: &'static str,
    pub detail: String,
}

pub(crate) fn shell_open(target: &str) -> AppResult<()> {
    #[cfg(windows)]
    unsafe {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let r = ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
        if r.0 as isize <= 32 {
            return Err(AppError::msg("Windows couldn't open it"));
        }
    }
    Ok(())
}

/// Resolve what an item would open, without opening it.
pub fn resolve(item: &Item, deep_link: Option<&str>, start: &dyn Fn() -> AppResult<Vec<StartApp>>) -> AppResult<(String, &'static str)> {
    match item.kind {
        Kind::Installed => {
            if !start()?.iter().any(|a| a.app_id == item.target) {
                return Err(AppError::NotFound(format!("{} is no longer in the Start menu — it may have been uninstalled", item.name)));
            }
            Ok((format!(r"shell:AppsFolder\{}", item.target), "app"))
        }
        Kind::Connected => {
            let s = service(&item.target).ok_or_else(|| AppError::Invalid("unknown service".into()))?;
            let installed = s.scheme.is_some_and(scheme_installed);
            if let Some(d) = deep_link {
                let (_, url) = s.deep_links.iter().find(|(label, _)| *label == d).ok_or_else(|| AppError::Invalid(format!("{} has no '{d}' link", s.name)))?;
                if url.starts_with("https://") {
                    return Ok((url.to_string(), "browser"));
                }
                if !installed {
                    return Err(AppError::Invalid(format!("{} isn't installed, so '{d}' can't open", s.name)));
                }
                return Ok((url.to_string(), "app"));
            }
            match (installed, s.app_link) {
                (true, Some(link)) => Ok((link.to_string(), "app")),
                _ => Ok((s.web.to_string(), "browser")),
            }
        }
        Kind::Website => {
            if !valid_url(&item.target) {
                return Err(AppError::Invalid("not an https address".into()));
            }
            Ok((item.target.clone(), "browser"))
        }
    }
}

pub fn launch(db: &Db, id: &str, deep_link: Option<&str>) -> AppResult<Launched> {
    let item = get(db, id)?.ok_or_else(|| AppError::NotFound("library item".into()))?;
    let (target, via) = resolve(&item, deep_link, &|| start_apps(false))?;
    shell_open(&target)?;
    db.conn().execute("UPDATE library_items SET last_opened_at = ?2, open_count = open_count + 1 WHERE id = ?1", params![id, now()])?;
    db.audit("info", "user", "library.launch", Some(serde_json::json!({ "item": item.name, "via": via })));
    Ok(Launched { via, detail: if via == "browser" { format!("Opened {} in your browser", item.name) } else { format!("Opened {}", item.name) } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_start() -> AppResult<Vec<StartApp>> {
        Ok(vec![StartApp { name: "Blender 5.2".into(), app_id: r"{6D809377}\Blender\blender.exe".into(), category: "Creative".into() }])
    }

    fn item(kind: Kind, target: &str) -> Item {
        Item { id: "x".into(), kind, name: "X".into(), target: target.into(), category: String::new(), favourite: false, pinned: false, icon: Icon::default(), added_at: String::new(), last_opened_at: None, open_count: 0, app_installed: None }
    }

    #[test]
    fn categories_and_ids() {
        assert_eq!(guess_category("Steam"), "Games");
        assert_eq!(guess_category("DaVinci Resolve"), "Creative");
        assert_eq!(guess_category("Brave"), "Browsers");
        assert_eq!(guess_category("Some Tool"), "Utilities");
        assert!(valid_app_id("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        assert!(!valid_app_id("a\"b"));
        assert!(!valid_app_id(""));
        assert!(valid_url("https://example.com/x"));
        assert!(!valid_url("http://example.com"));
        assert!(!valid_url("javascript:alert(1)"));
        assert!(!valid_url("https://exa mple.com"));
        for s in SERVICES {
            assert!(s.web.starts_with("https://"), "{}", s.id);
            for (_, l) in s.deep_links {
                assert!(l.starts_with("https://") || s.scheme.is_some_and(|sc| l.starts_with(sc)), "{} deep link {l}", s.id);
            }
        }
    }

    #[test]
    fn resolve_only_known_targets() {
        let ok = resolve(&item(Kind::Installed, r"{6D809377}\Blender\blender.exe"), None, &fake_start).unwrap();
        assert_eq!(ok, (r"shell:AppsFolder\{6D809377}\Blender\blender.exe".to_string(), "app"));
        assert!(resolve(&item(Kind::Installed, r"C:\Windows\System32\cmd.exe"), None, &fake_start).is_err(), "not in Start list");
        assert!(resolve(&item(Kind::Connected, "evil"), None, &fake_start).is_err());
        assert_eq!(resolve(&item(Kind::Connected, "gmail"), None, &fake_start).unwrap().1, "browser");
        assert!(resolve(&item(Kind::Website, "file:///C:/x.exe"), None, &fake_start).is_err());
        assert_eq!(resolve(&item(Kind::Connected, "youtube"), Some("Subscriptions"), &fake_start).unwrap().0, "https://www.youtube.com/feed/subscriptions");
        assert!(resolve(&item(Kind::Connected, "youtube"), Some("Nope"), &fake_start).is_err());
    }

    #[test]
    fn add_edit_remove() {
        let db = Db::open_in_memory().unwrap();
        let added = add(&db, &[
            NewItem { kind: Kind::Connected, name: String::new(), target: "discord".into(), category: None, icon: Icon::default() },
            NewItem { kind: Kind::Website, name: "Docs".into(), target: "https://docs.rs".into(), category: Some("Dev".into()), icon: Icon { glyph: Some("📘".into()), colour: Some("#334455".into()) } },
        ]).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(added[0].name, "Discord");
        assert!(add(&db, &[NewItem { kind: Kind::Website, name: "x".into(), target: "http://insecure.example".into(), category: None, icon: Icon::default() }]).is_err());
        assert!(add(&db, &[NewItem { kind: Kind::Website, name: "x".into(), target: "https://docs.rs".into(), category: None, icon: Icon::default() }]).unwrap().is_empty(), "duplicate skipped");
        let docs = list(&db).unwrap().into_iter().find(|i| i.name == "Docs").unwrap();
        let e = edit(&db, &Edit { id: docs.id.clone(), name: Some("Rust docs".into()), category: None, favourite: Some(true), pinned: Some(true), icon: None }).unwrap();
        assert!(e.favourite && e.pinned && e.name == "Rust docs" && e.category == "Dev");
        assert!(edit(&db, &Edit { id: docs.id.clone(), name: None, category: None, favourite: None, pinned: None, icon: Some(Icon { glyph: None, colour: Some("red".into()) }) }).is_err());
        remove(&db, &docs.id).unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn real_start_menu_reads() {
        let apps = start_apps(true).unwrap();
        assert!(!apps.is_empty());
        assert!(apps.iter().all(|a| valid_app_id(&a.app_id)));
    }
}
