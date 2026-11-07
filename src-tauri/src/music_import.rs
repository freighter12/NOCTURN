//! Playlist import: turn a playlist from another service (or a pasted list,
//! CSV export or M3U file) into a Nocturn playlist of the user's own files.
//!
//! Only the *track list* is read — titles, artists, lengths. Nothing is
//! downloaded or streamed. A pasted link is parsed into a service + ID and
//! NOCTURN builds the request URL itself, for a fixed set of hosts, so a link
//! can't make the app fetch anything else. Songs that aren't in the library
//! are kept as a "missing" list, re-matched whenever the playlist is opened,
//! so adding files later fills the gaps.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::music;

const MAX_ITEMS: usize = 5000;
const MAX_TEXT: usize = 2 * 1024 * 1024;
const MAX_BODY: usize = 12 * 1024 * 1024;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

// ------------------------------------------------------------------ links

#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    Spotify { kind: &'static str, id: String },
    Deezer { kind: &'static str, id: String },
    YouTube { list: String },
}

impl Link {
    pub fn source(&self) -> &'static str {
        match self {
            Link::Spotify { .. } => "Spotify",
            Link::Deezer { .. } => "Deezer",
            Link::YouTube { .. } => "YouTube",
        }
    }
    /// The canonical page for this playlist (what "Open original" opens).
    pub fn page_url(&self) -> String {
        match self {
            Link::Spotify { kind, id } => format!("https://open.spotify.com/{kind}/{id}"),
            Link::Deezer { kind, id } => format!("https://www.deezer.com/{kind}/{id}"),
            Link::YouTube { list } => format!("https://www.youtube.com/playlist?list={list}"),
        }
    }
}

fn spotify_id(s: &str) -> bool {
    s.len() == 22 && s.chars().all(|c| c.is_ascii_alphanumeric())
}
fn yt_list(s: &str) -> bool {
    (2..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
fn deezer_id(s: &str) -> bool {
    (1..=20).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit())
}

/// Short links that point at one of the supported services (resolved with one redirect, not followed blindly).
pub fn is_short_link(s: &str) -> bool {
    reqwest::Url::parse(s.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| matches!(h.as_str(), "spotify.link" | "link.deezer.com" | "deezer.page.link"))
}

pub fn parse_link(input: &str) -> AppResult<Link> {
    let s = input.trim();
    // spotify:playlist:ID
    if let Some(rest) = s.strip_prefix("spotify:") {
        let mut it = rest.split(':');
        if let (Some(kind), Some(id)) = (it.next(), it.next()) {
            let kind = match kind {
                "playlist" => "playlist",
                "album" => "album",
                _ => return Err(AppError::Invalid("only Spotify playlists and albums can be imported".into())),
            };
            if spotify_id(id) {
                return Ok(Link::Spotify { kind, id: id.into() });
            }
        }
        return Err(AppError::Invalid("that Spotify link doesn't look right".into()));
    }
    let url = reqwest::Url::parse(s).map_err(|_| AppError::Invalid("paste a full link, starting with https://".into()))?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(AppError::Invalid("paste a web link (https://…)".into()));
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let segs: Vec<&str> = url.path_segments().map(|p| p.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let query = |k: &str| url.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.to_string());

    match host.as_str() {
        "open.spotify.com" | "play.spotify.com" => {
            // /playlist/ID, /intl-de/playlist/ID, /embed/playlist/ID, /user/x/playlist/ID
            for w in segs.windows(2) {
                let kind = match w[0] {
                    "playlist" => "playlist",
                    "album" => "album",
                    "track" | "artist" | "show" | "episode" => {
                        return Err(AppError::Invalid("that's a Spotify song, artist or podcast — paste a playlist or album link".into()))
                    }
                    _ => continue,
                };
                if spotify_id(w[1]) {
                    return Ok(Link::Spotify { kind, id: w[1].into() });
                }
            }
            Err(AppError::Invalid("couldn't find a Spotify playlist or album in that link".into()))
        }
        "www.deezer.com" | "deezer.com" => {
            for w in segs.windows(2) {
                let kind = match w[0] {
                    "playlist" => "playlist",
                    "album" => "album",
                    _ => continue,
                };
                if deezer_id(w[1]) {
                    return Ok(Link::Deezer { kind, id: w[1].into() });
                }
            }
            Err(AppError::Invalid("couldn't find a Deezer playlist or album in that link".into()))
        }
        "www.youtube.com" | "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" => match query("list") {
            Some(l) if yt_list(&l) => Ok(Link::YouTube { list: l }),
            _ => Err(AppError::Invalid("that YouTube link has no playlist in it (look for ?list=… in the address)".into())),
        },
        "spotify.link" | "link.deezer.com" | "deezer.page.link" => Err(AppError::Invalid("short link".into())),
        _ => Err(AppError::Invalid("links from Spotify, YouTube, YouTube Music and Deezer can be imported. For anything else, paste the song list instead".into())),
    }
}

// ------------------------------------------------------------------ fetching

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteTrack {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    /// File path from an M3U list, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteList {
    pub source: String,
    pub name: String,
    pub url: Option<String>,
    pub tracks: Vec<RemoteTrack>,
    /// The service only showed part of a long playlist.
    pub truncated: bool,
}

fn client(redirects: bool) -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(25))
        .redirect(if redirects { reqwest::redirect::Policy::limited(3) } else { reqwest::redirect::Policy::none() })
        .build()
        .map_err(|e| AppError::msg(format!("network setup failed: {e}")))
}

pub(crate) async fn get_text(url: &str, headers: &[(&str, &str)]) -> AppResult<(u16, String)> {
    let mut req = client(true)?.get(url).header("Accept-Language", "en-GB,en;q=0.9");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = req.send().await.map_err(|e| AppError::msg(format!("couldn't reach the service: {}", e.without_url())))?;
    let status = res.status().as_u16();
    if res.content_length().is_some_and(|n| n as usize > MAX_BODY) {
        return Err(AppError::msg("the service sent far more than a playlist"));
    }
    let bytes = res.bytes().await.map_err(|e| AppError::msg(format!("download failed: {}", e.without_url())))?;
    if bytes.len() > MAX_BODY {
        return Err(AppError::msg("the service sent far more than a playlist"));
    }
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}

/// Follow one redirect of a supported short link and parse where it points.
pub async fn resolve_short(input: &str) -> AppResult<Link> {
    let url = reqwest::Url::parse(input.trim()).map_err(|_| AppError::Invalid("paste a full link".into()))?;
    let res = client(false)?.get(url).send().await.map_err(|e| AppError::msg(format!("couldn't open the short link: {}", e.without_url())))?;
    let loc = res.headers().get("location").and_then(|v| v.to_str().ok()).map(String::from);
    match loc {
        Some(l) => parse_link(&l).map_err(|_| AppError::Invalid("that short link doesn't lead to a playlist NOCTURN can read — open it in your browser and copy the full address".into())),
        None => Err(AppError::Invalid("that short link didn't lead anywhere — open it in your browser and copy the full address".into())),
    }
}

pub async fn fetch(link: &Link) -> AppResult<RemoteList> {
    let list = match link {
        Link::Spotify { kind, id } => {
            let (status, html) = get_text(&format!("https://open.spotify.com/embed/{kind}/{id}"), &[]).await?;
            if status == 404 {
                return Err(AppError::NotFound("Spotify playlist (it may be private or deleted)".into()));
            }
            parse_spotify_embed(&html)?
        }
        Link::Deezer { kind, id } => fetch_deezer(kind, id).await?,
        Link::YouTube { list } => {
            let (status, html) =
                get_text(&format!("https://www.youtube.com/playlist?list={list}"), &[("Cookie", "CONSENT=YES+1; SOCS=CAI")]).await?;
            if status >= 400 {
                return Err(AppError::msg(format!("YouTube answered {status}")));
            }
            parse_youtube(&html)?
        }
    };
    Ok(RemoteList { url: Some(link.page_url()), source: link.source().into(), ..list })
}

pub(crate) fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = match p.parse::<usize>() {
            Ok(i) => cur.get(i)?,
            Err(_) => cur.get(*p)?,
        };
    }
    cur.as_str()
}

pub fn parse_spotify_embed(html: &str) -> AppResult<RemoteList> {
    let start = html.find("id=\"__NEXT_DATA__\"").and_then(|i| html[i..].find('>').map(|j| i + j + 1));
    let json = start.and_then(|s| html[s..].find("</script>").map(|e| &html[s..s + e]));
    let v: Value = json.and_then(|j| serde_json::from_str(j).ok()).ok_or_else(|| AppError::msg("Spotify's page has changed; NOCTURN couldn't read the playlist. Paste the song list instead"))?;
    let e = v.pointer("/props/pageProps/state/data/entity").ok_or_else(|| AppError::NotFound("Spotify playlist (it may be private)".into()))?;
    let name = e.get("name").or_else(|| e.get("title")).and_then(Value::as_str).unwrap_or("Spotify playlist").to_string();
    let album = if e.get("type").and_then(Value::as_str) == Some("album") { name.clone() } else { String::new() };
    let tracks: Vec<RemoteTrack> = e
        .get("trackList")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let title = t.get("title")?.as_str()?.trim().to_string();
                    let artist = t.get("subtitle").and_then(Value::as_str).unwrap_or("").replace('\u{a0}', " ").trim().to_string();
                    (!title.is_empty()).then(|| RemoteTrack { title, artist, album: album.clone(), duration_ms: t.get("duration").and_then(Value::as_i64), path: None })
                })
                .collect()
        })
        .unwrap_or_default();
    // The embed shows at most 100 tracks of a playlist.
    let truncated = tracks.len() >= 100;
    Ok(RemoteList { source: "Spotify".into(), name, url: None, tracks, truncated })
}

async fn fetch_deezer(kind: &str, id: &str) -> AppResult<RemoteList> {
    let (_, body) = get_text(&format!("https://api.deezer.com/{kind}/{id}"), &[]).await?;
    let v: Value = serde_json::from_str(&body).map_err(|_| AppError::msg("Deezer sent something unexpected"))?;
    if v.get("error").is_some() {
        return Err(AppError::NotFound("Deezer playlist (it may be private or deleted)".into()));
    }
    let name = v.get("title").and_then(Value::as_str).unwrap_or("Deezer playlist").to_string();
    let album_name = (kind == "album").then(|| name.clone());
    let mut tracks = Vec::new();
    let mut page = v.get("tracks").cloned();
    let mut pages = 0;
    while let Some(p) = page.take() {
        for t in p.get("data").and_then(Value::as_array).into_iter().flatten() {
            let title = t.get("title").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if title.is_empty() {
                continue;
            }
            tracks.push(RemoteTrack {
                title,
                artist: str_at(t, &["artist", "name"]).unwrap_or("").to_string(),
                album: album_name.clone().or_else(|| str_at(t, &["album", "title"]).map(String::from)).unwrap_or_default(),
                duration_ms: t.get("duration").and_then(Value::as_i64).map(|s| s * 1000),
                path: None,
            });
        }
        pages += 1;
        // Long playlists come in pages; only follow Deezer's own API.
        if let Some(next) = p.get("next").and_then(Value::as_str) {
            if next.starts_with("https://api.deezer.com/") && pages < 25 && tracks.len() < MAX_ITEMS {
                let (_, b) = get_text(next, &[]).await?;
                page = serde_json::from_str(&b).ok();
            }
        }
    }
    Ok(RemoteList { source: "Deezer".into(), name, url: None, tracks, truncated: false })
}

pub(crate) fn walk<'a>(v: &'a Value, f: &mut dyn FnMut(&'a str, &'a Value) -> bool) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if !f(k, x) {
                    walk(x, f);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk(x, f)),
        _ => {}
    }
}

pub(crate) fn parse_len(s: &str) -> Option<i64> {
    let mut total = 0i64;
    for part in s.trim().split(':') {
        total = total * 60 + part.trim().parse::<i64>().ok()?;
    }
    Some(total * 1000)
}

/// "Artist - Title (Official Video)" with channel "ArtistVEVO" → (artist, title).
pub fn split_video_title(title: &str, channel: &str) -> (String, String) {
    for sep in [" - ", " – ", " — ", " | "] {
        if let Some((a, t)) = title.split_once(sep) {
            if !a.trim().is_empty() && !t.trim().is_empty() {
                return (a.trim().to_string(), t.trim().to_string());
            }
        }
    }
    let ch = channel.trim();
    let ch = ch.strip_suffix(" - Topic").unwrap_or(ch);
    let ch = ch.strip_suffix("VEVO").unwrap_or(ch);
    let ch = ch.strip_suffix(" Official").unwrap_or(ch);
    (ch.trim().to_string(), title.trim().to_string())
}

pub fn parse_youtube(html: &str) -> AppResult<RemoteList> {
    let start = html.find("var ytInitialData = ").map(|i| i + "var ytInitialData = ".len()).or_else(|| html.find("ytInitialData\"] = ").map(|i| i + "ytInitialData\"] = ".len()));
    let v: Value = start
        .and_then(|s| html[s..].find(";</script>").map(|e| &html[s..s + e]))
        .and_then(|j| serde_json::from_str(j).ok())
        .ok_or_else(|| AppError::msg("YouTube's page has changed or asked for sign-in; NOCTURN couldn't read the playlist. Paste the song list instead"))?;
    let mut tracks = Vec::new();
    let mut name: Option<String> = None;
    walk(&v, &mut |k, x| {
        match k {
            // Older layout
            "playlistVideoRenderer" => {
                let title = str_at(x, &["title", "runs", "0", "text"]).or_else(|| str_at(x, &["title", "simpleText"])).unwrap_or("");
                let channel = str_at(x, &["shortBylineText", "runs", "0", "text"]).unwrap_or("");
                let dur = x.get("lengthSeconds").and_then(Value::as_str).and_then(|s| s.parse::<i64>().ok()).map(|s| s * 1000);
                if !title.is_empty() {
                    let (artist, title) = split_video_title(title, channel);
                    tracks.push(RemoteTrack { title, artist, album: String::new(), duration_ms: dur, path: None });
                }
                true
            }
            // Current layout
            "lockupViewModel" => {
                if x.get("contentType").and_then(Value::as_str) == Some("LOCKUP_CONTENT_TYPE_VIDEO") {
                    let md = x.pointer("/metadata/lockupMetadataViewModel");
                    let title = md.and_then(|m| str_at(m, &["title", "content"])).unwrap_or("");
                    let channel = md
                        .and_then(|m| m.pointer("/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text/content"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let mut dur = None;
                    walk(x, &mut |k2, y| {
                        if k2 == "thumbnailBadgeViewModel" {
                            if let Some(t) = y.get("text").and_then(Value::as_str) {
                                dur = dur.or(parse_len(t));
                            }
                            return true;
                        }
                        false
                    });
                    if !title.is_empty() {
                        let (artist, title) = split_video_title(title, channel);
                        tracks.push(RemoteTrack { title, artist, album: String::new(), duration_ms: dur, path: None });
                    }
                }
                true
            }
            "playlistHeaderRenderer" => {
                name = name.take().or_else(|| str_at(x, &["title", "simpleText"]).map(String::from));
                false
            }
            "pageHeaderViewModel" => {
                name = name.take().or_else(|| str_at(x, &["title", "dynamicTextViewModel", "text", "content"]).map(String::from));
                false
            }
            _ => false,
        }
    });
    if name.is_none() {
        name = html.split("<meta property=\"og:title\" content=\"").nth(1).and_then(|s| s.split('"').next()).map(|s| s.replace("&amp;", "&").replace("&#39;", "'"));
    }
    if tracks.is_empty() {
        return Err(AppError::NotFound("songs in that YouTube playlist (it may be private, or a mix YouTube makes on the fly)".into()));
    }
    let truncated = tracks.len() >= 100;
    Ok(RemoteList { source: "YouTube".into(), name: name.unwrap_or_else(|| "YouTube playlist".into()), url: None, tracks, truncated })
}

// ------------------------------------------------------------------ pasted text

fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, q) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => q = !q,
            (',', false) => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/// "Artist - Title", "Title by Artist", "3. Artist – Title", CSV exports, M3U lists.
pub fn parse_text(text: &str) -> AppResult<RemoteList> {
    if text.len() > MAX_TEXT {
        return Err(AppError::Invalid("that's too long to be a playlist (2 MB max)".into()));
    }
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut tracks = Vec::new();
    let mut name = "Imported playlist".to_string();

    let is_m3u = lines.first().is_some_and(|l| l.starts_with("#EXTM3U")) || lines.iter().any(|l| l.starts_with("#EXTINF"));
    let header = lines.first().map(|l| csv_fields(&l.to_lowercase())).unwrap_or_default();
    let col = |names: &[&str]| header.iter().position(|h| names.iter().any(|n| h == n));
    let (ct, ca) = (col(&["track name", "title", "name", "song", "track"]), col(&["artist name(s)", "artist", "artists", "artist name"]));

    if is_m3u {
        let mut pending: Option<RemoteTrack> = None;
        for l in &lines {
            if let Some(n) = l.strip_prefix("#PLAYLIST:") {
                name = n.trim().to_string();
            } else if let Some(info) = l.strip_prefix("#EXTINF:") {
                let (dur, label) = info.split_once(',').unwrap_or((info, ""));
                let dur = dur.split_whitespace().next().and_then(|d| d.parse::<i64>().ok()).filter(|d| *d > 0).map(|d| d * 1000);
                let (artist, title) = label.split_once(" - ").map(|(a, t)| (a.trim().to_string(), t.trim().to_string())).unwrap_or((String::new(), label.trim().to_string()));
                pending = Some(RemoteTrack { title, artist, album: String::new(), duration_ms: dur, path: None });
            } else if !l.starts_with('#') {
                let mut t = pending.take().unwrap_or_else(|| {
                    let stem = l.rsplit(['\\', '/']).next().unwrap_or(l);
                    let stem = stem.rsplit_once('.').map(|(a, _)| a).unwrap_or(stem);
                    let (artist, title) = stem.split_once(" - ").map(|(a, t)| (a.trim().to_string(), t.trim().to_string())).unwrap_or((String::new(), stem.to_string()));
                    RemoteTrack { title, artist, album: String::new(), duration_ms: None, path: None }
                });
                t.path = Some(l.to_string());
                tracks.push(t);
            }
        }
    } else if let (Some(ct), true) = (ct, header.len() > 1) {
        let cal = col(&["album name", "album"]);
        let cd = col(&["duration (ms)", "duration_ms", "duration"]);
        for l in lines.iter().skip(1) {
            let f = csv_fields(l);
            let get = |i: Option<usize>| i.and_then(|i| f.get(i)).cloned().unwrap_or_default();
            let title = get(Some(ct));
            if title.is_empty() {
                continue;
            }
            let dur = get(cd);
            let duration_ms = dur.parse::<i64>().ok().map(|n| if n < 10_000 { n * 1000 } else { n }).or_else(|| parse_len(&dur));
            tracks.push(RemoteTrack { title, artist: get(ca), album: get(cal), duration_ms, path: None });
        }
    } else {
        for l in &lines {
            // Strip "1." / "01 -" numbering.
            let l = l.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')', '-', ' ', '\t']);
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let (artist, title) = if let Some((a, t)) = [" - ", " – ", " — ", "\t"].iter().find_map(|s| l.split_once(s)) {
                (a.trim().to_string(), t.trim().to_string())
            } else if let Some((t, a)) = l.rsplit_once(" by ") {
                (a.trim().to_string(), t.trim().to_string())
            } else {
                (String::new(), l.to_string())
            };
            if !title.is_empty() {
                tracks.push(RemoteTrack { title, artist, album: String::new(), duration_ms: None, path: None });
            }
        }
    }
    if tracks.is_empty() {
        return Err(AppError::Invalid("couldn't find any songs — put one per line, like \"Artist - Title\"".into()));
    }
    tracks.truncate(MAX_ITEMS);
    Ok(RemoteList { source: "List".into(), name, url: None, tracks, truncated: false })
}

// ------------------------------------------------------------------ matching

fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'č' | 'ć' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        '’' | '‘' | '`' => '\'',
        c => c,
    }
}

const NOISE: &[&str] = &[
    "official", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "remaster", "remastered", "hd", "4k", "hq", "explicit", "clean", "mv", "m/v", "music video", "feat", "ft", "featuring", "with", "prod", "single version", "album version", "radio edit", "original mix", "color coded",
];

fn is_noise(seg: &str) -> bool {
    let s = seg.trim();
    s.is_empty() || NOISE.iter().any(|n| s == *n || s.starts_with(&format!("{n} ")) || s.starts_with(&format!("{n}.")) || s.ends_with(&format!(" {n}")) || s.contains(&format!(" {n} ")))
        || s.chars().all(|c| c.is_ascii_digit() || c == ' ')
}

/// Normalised title words: lower case, accents folded, bracketed/dashed noise
/// ("(Official Video)", "- Remastered 2011", "feat. X") removed. Version words
/// like live/remix/acoustic/slowed are kept so versions don't get mixed up.
pub fn clean_title(t: &str) -> String {
    let lower: String = t.to_lowercase().chars().map(fold).collect();
    // Drop bracketed segments that are only noise.
    let mut out = String::new();
    let mut depth_seg = String::new();
    let mut depth = 0;
    for c in lower.chars() {
        match c {
            '(' | '[' | '{' => {
                if depth == 0 {
                    depth_seg.clear();
                } else {
                    depth_seg.push(c);
                }
                depth += 1;
            }
            ')' | ']' | '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 && !is_noise(&depth_seg) {
                    out.push(' ');
                    out.push_str(&depth_seg);
                    out.push(' ');
                } else if depth > 0 {
                    depth_seg.push(c);
                }
            }
            _ if depth > 0 => depth_seg.push(c),
            _ => out.push(c),
        }
    }
    // " - Remastered 2011", " - Radio Edit": drop trailing dashed noise parts.
    let mut parts: Vec<&str> = out.split(" - ").collect();
    while parts.len() > 1 && is_noise(parts[parts.len() - 1]) {
        parts.pop();
    }
    let mut s = parts.join(" ");
    // "feat. X" / "ft. X" outside brackets.
    for k in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(i) = s.find(k) {
            s.truncate(i);
        }
    }
    s.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn artist_set(a: &str) -> HashSet<String> {
    let lower: String = a.to_lowercase().chars().map(fold).collect();
    let mut s = lower;
    for sep in [" feat. ", " feat ", " ft. ", " ft ", " featuring ", " & ", " and ", " x ", " with ", ";", "/", ",", "、"] {
        s = s.replace(sep, "|");
    }
    s.split('|')
        .map(|p| p.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .map(|p| p.strip_prefix("the").filter(|x| x.len() > 2).map(String::from).unwrap_or(p))
        .map(|p| p.strip_suffix("vevo").map(String::from).unwrap_or(p))
        .filter(|p| !p.is_empty())
        .collect()
}

fn words(s: &str) -> HashSet<&str> {
    s.split(' ').filter(|w| !w.is_empty()).collect()
}

/// Dice similarity of word sets (1.0 = same words).
pub(crate) fn title_sim(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    let common = wa.intersection(&wb).count() as f64;
    2.0 * common / (wa.len() + wb.len()) as f64
}

struct Local {
    id: i64,
    title: String,
    artists: HashSet<String>,
    duration_ms: i64,
}

pub struct Matcher {
    locals: Vec<Local>,
    by_title: HashMap<String, Vec<usize>>,
    by_path: HashMap<String, i64>,
    by_file: HashMap<String, Option<i64>>,
}

impl Matcher {
    pub fn new(tracks: &[music::Track], paths: &[(i64, String)]) -> Self {
        let locals: Vec<Local> = tracks
            .iter()
            .map(|t| {
                let mut artists = artist_set(&t.artist);
                artists.extend(artist_set(&t.album_artist));
                Local { id: t.id, title: clean_title(&t.title), artists, duration_ms: t.duration_ms }
            })
            .collect();
        let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, l) in locals.iter().enumerate() {
            by_title.entry(l.title.clone()).or_default().push(i);
        }
        let mut by_path = HashMap::new();
        let mut by_file: HashMap<String, Option<i64>> = HashMap::new();
        for (id, p) in paths {
            let lp = p.to_lowercase().replace('/', "\\");
            if let Some(f) = lp.rsplit('\\').next() {
                // A file name shared by two tracks is ambiguous.
                by_file.entry(f.to_string()).and_modify(|v| *v = None).or_insert(Some(*id));
            }
            by_path.insert(lp, *id);
        }
        Matcher { locals, by_title, by_path, by_file }
    }

    /// Best local track for a remote one, with "exact" or "likely".
    pub fn find(&self, r: &RemoteTrack) -> Option<(i64, &'static str)> {
        if let Some(p) = &r.path {
            let lp = p.to_lowercase().replace('/', "\\");
            if let Some(id) = self.by_path.get(&lp) {
                return Some((*id, "exact"));
            }
            if let Some(Some(id)) = lp.rsplit('\\').next().and_then(|f| self.by_file.get(f)) {
                return Some((*id, "exact"));
            }
        }
        let title = clean_title(&r.title);
        if title.is_empty() {
            return None;
        }
        let ra = artist_set(&r.artist);
        let score = |l: &Local, sim: f64| -> Option<f64> {
            let artist_ok = !ra.is_empty() && ra.iter().any(|a| l.artists.iter().any(|b| a == b || (a.len() > 3 && b.contains(a.as_str())) || (b.len() > 3 && a.contains(b.as_str()))));
            let dur = r.duration_ms.filter(|_| l.duration_ms > 0).map(|d| (d - l.duration_ms).abs());
            let dur_ok = dur.is_some_and(|d| d <= 3000);
            let dur_bad = dur.is_some_and(|d| d > 20_000);
            // Need the artist to agree, or (no artist given) the title and length to.
            let accept = if ra.is_empty() { sim >= 0.99 && !dur_bad } else { artist_ok && (sim >= 0.99 || (sim >= 0.8 && !dur_bad)) || (sim >= 0.99 && dur_ok && dur.is_some_and(|d| d <= 1500)) };
            accept.then(|| sim + if artist_ok { 0.5 } else { 0.0 } + if dur_ok { 0.3 } else if dur_bad { -0.3 } else { 0.0 })
        };
        let consider = |best: &mut Option<(f64, i64, bool)>, l: &Local, sim: f64| {
            if let Some(s) = score(l, sim) {
                if best.is_none_or(|(b, _, _)| s > b) {
                    *best = Some((s, l.id, sim >= 0.99));
                }
            }
        };
        let mut best: Option<(f64, i64, bool)> = None;
        if let Some(ix) = self.by_title.get(&title) {
            for &i in ix {
                consider(&mut best, &self.locals[i], 1.0);
            }
        }
        if best.is_none() {
            for l in &self.locals {
                let sim = title_sim(&title, &l.title);
                if sim >= 0.8 {
                    consider(&mut best, l, sim);
                }
            }
        }
        best.map(|(s, id, exact_title)| (id, if exact_title && s >= 1.5 { "exact" } else { "likely" }))
    }
}

fn local_paths(db: &Db) -> AppResult<Vec<(i64, String)>> {
    let conn = db.conn();
    let mut st = conn.prepare("SELECT id, path FROM music_tracks")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ------------------------------------------------------------------ preview / save

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportItem {
    #[serde(flatten)]
    pub remote: RemoteTrack,
    /// Local track it matched, if any.
    pub track_id: Option<i64>,
    /// "exact" | "likely" | null
    pub confidence: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub source: String,
    pub name: String,
    pub url: Option<String>,
    pub truncated: bool,
    pub items: Vec<ImportItem>,
    pub matched: usize,
}

pub fn match_list(db: &Db, list: RemoteList) -> AppResult<ImportPreview> {
    let m = Matcher::new(&music::tracks(db)?, &local_paths(db)?);
    let items: Vec<ImportItem> = list
        .tracks
        .into_iter()
        .take(MAX_ITEMS)
        .map(|r| {
            let hit = m.find(&r);
            ImportItem { track_id: hit.map(|h| h.0), confidence: hit.map(|h| h.1.to_string()), remote: r }
        })
        .collect();
    let matched = items.iter().filter(|i| i.track_id.is_some()).count();
    Ok(ImportPreview { source: list.source, name: list.name.chars().take(80).collect(), url: list.url, truncated: list.truncated, items, matched })
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveImport {
    /// Update this playlist (refresh) instead of creating one.
    #[serde(default)]
    pub playlist_id: Option<String>,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub url: Option<String>,
    pub items: Vec<ImportItem>,
}

/// Create (or refresh) the playlist from the chosen matches and remember the full list.
pub fn save(db: &Db, s: SaveImport) -> AppResult<music::Playlist> {
    if s.items.len() > MAX_ITEMS {
        return Err(AppError::Invalid("too many songs".into()));
    }
    // Only links NOCTURN itself built are stored (they're opened later).
    let url = match s.url.as_deref() {
        Some(u) => Some(parse_link(u)?.page_url()),
        None => None,
    };
    let source: String = match s.source.as_str() {
        "Spotify" | "Deezer" | "YouTube" | "List" => s.source.clone(),
        _ => "List".into(),
    };
    let tracks: Vec<i64> = s.items.iter().filter_map(|i| i.track_id).collect();
    let found = tracks.len();
    let existing = match &s.playlist_id {
        Some(id) => Some(music::playlists(db)?.into_iter().find(|p| &p.id == id).ok_or_else(|| AppError::NotFound("playlist".into()))?),
        None => None,
    };
    // The playlist page shows where it came from and what's missing, live; the description stays the user's.
    let description = existing.as_ref().map(|p| p.description.clone()).unwrap_or_default();
    let p = music::save_playlist(
        db,
        music::PlaylistInput {
            id: existing.as_ref().map(|p| p.id.clone()),
            name: s.name,
            description,
            cover: existing.map(|p| p.cover).unwrap_or(music::Cover { track_id: tracks.first().copied(), glyph: None, colour: None }),
            tracks,
        },
    )?;
    let remote: Vec<&RemoteTrack> = s.items.iter().map(|i| &i.remote).collect();
    db.conn().execute(
        "INSERT INTO music_playlist_imports(playlist_id, source, url, items_json, imported_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(playlist_id) DO UPDATE SET source = excluded.source, url = excluded.url, items_json = excluded.items_json, imported_at = excluded.imported_at",
        params![p.id, source, url, serde_json::to_string(&remote)?, now()],
    )?;
    db.audit("info", "user", "music.import", Some(serde_json::json!({ "source": source, "songs": s.items.len(), "found": found })));
    Ok(p)
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportInfo {
    pub source: String,
    pub url: Option<String>,
    pub imported_at: String,
    pub total: usize,
    /// Songs from the original list not in the playlist — with a match if one has turned up since.
    pub missing: Vec<ImportItem>,
}

/// The stored import for a playlist, with missing songs re-matched against the library as it is now.
pub fn info(db: &Db, playlist_id: &str) -> AppResult<Option<ImportInfo>> {
    let row: Option<(String, Option<String>, String, String)> = db
        .conn()
        .query_row("SELECT source, url, items_json, imported_at FROM music_playlist_imports WHERE playlist_id = ?1", [playlist_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .optional()?;
    let Some((source, url, items_json, imported_at)) = row else { return Ok(None) };
    let remote: Vec<RemoteTrack> = serde_json::from_str(&items_json).unwrap_or_default();
    let in_playlist: HashSet<i64> = music::playlists(db)?.into_iter().find(|p| p.id == playlist_id).map(|p| p.tracks.into_iter().collect()).unwrap_or_default();
    let m = Matcher::new(&music::tracks(db)?, &local_paths(db)?);
    let missing = remote
        .iter()
        .filter_map(|r| {
            let hit = m.find(r);
            match hit {
                Some((id, _)) if in_playlist.contains(&id) => None,
                _ => Some(ImportItem { remote: r.clone(), track_id: hit.map(|h| h.0), confidence: hit.map(|h| h.1.to_string()) }),
            }
        })
        .collect();
    Ok(Some(ImportInfo { source, url, imported_at, total: remote.len(), missing }))
}

pub fn forget(db: &Db, playlist_id: &str) -> AppResult<()> {
    db.conn().execute("DELETE FROM music_playlist_imports WHERE playlist_id = ?1", [playlist_id])?;
    Ok(())
}

/// Open the original playlist page in the browser (only the link NOCTURN stored).
pub fn open_original(db: &Db, playlist_id: &str) -> AppResult<()> {
    let url: Option<String> = db.conn().query_row("SELECT url FROM music_playlist_imports WHERE playlist_id = ?1", [playlist_id], |r| r.get(0)).optional()?.flatten();
    let url = url.ok_or_else(|| AppError::NotFound("original link".into()))?;
    let safe = parse_link(&url)?.page_url();
    crate::library::shell_open(&safe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_parse_to_service_and_id() {
        assert_eq!(parse_link("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc").unwrap(), Link::Spotify { kind: "playlist", id: "37i9dQZF1DXcBWIGoYBM5M".into() });
        assert_eq!(parse_link("https://open.spotify.com/intl-de/album/4aawyAB9vmqN3uQ7FjRGTy").unwrap(), Link::Spotify { kind: "album", id: "4aawyAB9vmqN3uQ7FjRGTy".into() });
        assert_eq!(parse_link("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").unwrap(), Link::Spotify { kind: "playlist", id: "37i9dQZF1DXcBWIGoYBM5M".into() });
        assert_eq!(parse_link("https://www.deezer.com/en/playlist/908622995").unwrap(), Link::Deezer { kind: "playlist", id: "908622995".into() });
        assert_eq!(parse_link("https://music.youtube.com/playlist?list=PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i").unwrap(), Link::YouTube { list: "PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i".into() });
        assert_eq!(parse_link("https://www.youtube.com/watch?v=abc&list=PLx_y-1").unwrap(), Link::YouTube { list: "PLx_y-1".into() });
        // Rejected: other hosts, songs, injected IDs, non-web schemes.
        assert!(parse_link("https://evil.example/playlist/37i9dQZF1DXcBWIGoYBM5M").is_err());
        assert!(parse_link("https://open.spotify.com/track/37i9dQZF1DXcBWIGoYBM5M").is_err());
        assert!(parse_link("https://open.spotify.com/playlist/../../x").is_err());
        assert!(parse_link("https://www.youtube.com/playlist?list=a%2F..%2Fb").is_err());
        assert!(parse_link("file:///C:/Windows/win.ini").is_err());
        assert!(is_short_link("https://spotify.link/abc123"));
        // Canonical page URLs are built from the ID only.
        assert_eq!(parse_link("https://www.deezer.com/fr/album/302127").unwrap().page_url(), "https://www.deezer.com/album/302127");
    }

    #[test]
    fn titles_lose_noise_but_keep_versions() {
        assert_eq!(clean_title("Hey Jude (Remastered 2015)"), "hey jude");
        assert_eq!(clean_title("Tarantino (Official Music Video)"), "tarantino");
        assert_eq!(clean_title("Blinding Lights - Radio Edit"), "blinding lights");
        assert_eq!(clean_title("Here Comes The Sun - Remastered 2009"), "here comes the sun");
        assert_eq!(clean_title("Stay (feat. Justin Bieber)"), "stay");
        assert_eq!(clean_title("Stay ft. Justin Bieber"), "stay");
        assert_eq!(clean_title("Clarity (Live)"), "clarity live");
        assert_eq!(clean_title("Hold On [Slowed + Reverb]"), "hold on slowed reverb");
        assert_eq!(clean_title("Café del Mar"), "cafe del mar");
        assert_eq!(split_video_title("LISA - Tarantino (Official Music Video)", "LLOUD Official"), ("LISA".into(), "Tarantino (Official Music Video)".into()));
        assert_eq!(split_video_title("Tarantino", "LISA - Topic"), ("LISA".into(), "Tarantino".into()));
    }

    #[test]
    fn pasted_lists_csv_and_m3u() {
        let l = parse_text("1. The Beatles - Hey Jude\n\nBlinding Lights by The Weeknd\nJust A Title\n").unwrap();
        assert_eq!(l.tracks.len(), 3);
        assert_eq!((l.tracks[0].artist.as_str(), l.tracks[0].title.as_str()), ("The Beatles", "Hey Jude"));
        assert_eq!((l.tracks[1].artist.as_str(), l.tracks[1].title.as_str()), ("The Weeknd", "Blinding Lights"));
        assert_eq!(l.tracks[2].artist, "");

        let csv = "\u{feff}\"Track URI\",\"Track Name\",\"Artist Name(s)\",\"Album Name\",\"Duration (ms)\"\n\"spotify:track:x\",\"Song, With Comma\",\"A, B\",\"Alb\",\"201000\"\n";
        let l = parse_text(csv).unwrap();
        assert_eq!(l.tracks.len(), 1);
        assert_eq!(l.tracks[0].title, "Song, With Comma");
        assert_eq!(l.tracks[0].artist, "A, B");
        assert_eq!(l.tracks[0].duration_ms, Some(201_000));

        let m3u = "#EXTM3U\n#PLAYLIST:Road trip\n#EXTINF:215,Daft Punk - One More Time\nD:\\Music\\Daft Punk\\01 One More Time.flac\nother\\Song Name.mp3\n";
        let l = parse_text(m3u).unwrap();
        assert_eq!(l.name, "Road trip");
        assert_eq!(l.tracks.len(), 2);
        assert_eq!((l.tracks[0].artist.as_str(), l.tracks[0].duration_ms), ("Daft Punk", Some(215_000)));
        assert_eq!(l.tracks[1].title, "Song Name");
        assert_eq!(l.tracks[1].path.as_deref(), Some("other\\Song Name.mp3"));

        assert!(parse_text("\n\n").is_err());
    }

    fn local(id: i64, title: &str, artist: &str, secs: i64) -> music::Track {
        music::Track {
            id,
            source: "local".into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            album_artist: artist.into(),
            track_no: None,
            disc_no: None,
            year: None,
            genre: String::new(),
            duration_ms: secs * 1000,
            has_art: false,
            liked: false,
            added_at: String::new(),
            plays: 0,
            last_played: None,
        }
    }
    fn remote(title: &str, artist: &str, secs: Option<i64>) -> RemoteTrack {
        RemoteTrack { title: title.into(), artist: artist.into(), album: String::new(), duration_ms: secs.map(|s| s * 1000), path: None }
    }

    #[test]
    fn matching_handles_noise_artists_and_versions() {
        let lib = vec![
            local(1, "Hey Jude", "The Beatles", 431),
            local(2, "Clarity", "Zedd", 271),
            local(3, "Clarity (Live)", "Zedd", 300),
            local(4, "Stay", "The Kid LAROI", 141),
            local(5, "Stay", "Rihanna", 240),
            local(6, "One More Time", "Daft Punk", 320),
        ];
        let m = Matcher::new(&lib, &[(6, r"D:\Music\Daft Punk\01 One More Time.flac".into())]);
        assert_eq!(m.find(&remote("Hey Jude (Remastered 2015)", "The Beatles", Some(429))), Some((1, "exact")));
        assert_eq!(m.find(&remote("Hey Jude", "Beatles", None)).map(|x| x.0), Some(1));
        assert_eq!(m.find(&remote("Clarity", "Zedd, Foxes", Some(271))).map(|x| x.0), Some(2));
        assert_eq!(m.find(&remote("Clarity (Live)", "Zedd", None)).map(|x| x.0), Some(3));
        // Same title, different artist → the right one; unknown artist with a clashing title → no guess.
        assert_eq!(m.find(&remote("STAY (with Justin Bieber)", "The Kid LAROI & Justin Bieber", Some(141))).map(|x| x.0), Some(4));
        assert_eq!(m.find(&remote("Stay", "Someone Else", Some(100))), None);
        // A different song by a known artist is not matched.
        assert_eq!(m.find(&remote("Get Lucky", "Daft Punk", Some(248))), None);
        // M3U path, by full path or (unique) file name.
        assert_eq!(m.find(&RemoteTrack { path: Some(r"d:\music\daft punk\01 one more time.flac".into()), ..remote("x", "", None) }), Some((6, "exact")));
        assert_eq!(m.find(&RemoteTrack { path: Some(r"..\01 One More Time.flac".into()), ..remote("x", "", None) }), Some((6, "exact")));
    }

    #[test]
    fn spotify_and_youtube_pages_parse() {
        let sp = r#"<html><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"type":"playlist","name":"Mix","trackList":[{"title":"Stop The Wedding!","subtitle":"Ashe","duration":198840},{"title":"","subtitle":"x"}]}}}}}}</script></html>"#;
        let l = parse_spotify_embed(sp).unwrap();
        assert_eq!(l.name, "Mix");
        assert_eq!(l.tracks, vec![RemoteTrack { title: "Stop The Wedding!".into(), artist: "Ashe".into(), album: String::new(), duration_ms: Some(198_840), path: None }]);

        let yt = r#"<script>var ytInitialData = {"header":{"pageHeaderViewModel":{"title":{"dynamicTextViewModel":{"text":{"content":"Top 100"}}}}},"items":[{"lockupViewModel":{"contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentImage":{"thumbnailViewModel":{"overlays":[{"thumbnailBottomOverlayViewModel":{"badges":[{"thumbnailBadgeViewModel":{"text":"3:37"}}]}}]}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"LISA - Tarantino (Official Music Video)"},"metadata":{"contentMetadataViewModel":{"metadataRows":[{"metadataParts":[{"text":{"content":"LLOUD Official"}}]}]}}}}}},{"playlistVideoRenderer":{"title":{"runs":[{"text":"Old Song"}]},"shortBylineText":{"runs":[{"text":"Band - Topic"}]},"lengthSeconds":"200"}}]};</script>"#;
        let l = parse_youtube(yt).unwrap();
        assert_eq!(l.name, "Top 100");
        assert_eq!(l.tracks.len(), 2);
        assert_eq!((l.tracks[0].artist.as_str(), l.tracks[0].title.as_str(), l.tracks[0].duration_ms), ("LISA", "Tarantino (Official Music Video)", Some(217_000)));
        assert_eq!((l.tracks[1].artist.as_str(), l.tracks[1].duration_ms), ("Band", Some(200_000)));
        assert!(parse_youtube("<html>consent</html>").is_err());
    }

    /// Live check against the real services (network). Run with --ignored when a service changes its pages.
    #[test]
    #[ignore]
    fn live_sources_return_tracks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        for link in [
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy",
            "https://www.deezer.com/playlist/908622995",
            "https://music.youtube.com/playlist?list=PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i",
        ] {
            let l = rt.block_on(fetch(&parse_link(link).unwrap())).unwrap_or_else(|e| panic!("{link}: {e}"));
            println!("{link}: {} — {} tracks, first: {:?}", l.name, l.tracks.len(), l.tracks.first());
            assert!(l.tracks.len() >= 10, "{link}: only {} tracks", l.tracks.len());
            assert!(l.tracks.iter().filter(|t| !t.artist.is_empty() && t.duration_ms.is_some()).count() * 10 >= l.tracks.len() * 8, "{link}: artists/durations missing");
        }
    }

    #[test]
    fn save_and_info_round_trip_with_rematching() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO music_tracks(id, path, folder, title, artist, album, album_artist, duration_ms, size, mtime, added_at) VALUES (1, 'C:\\m\\a.mp3', 'C:\\m', 'Hey Jude', 'The Beatles', '', 'The Beatles', 431000, 1, 1, 'x')",
                [],
            )
            .unwrap();
        let list = RemoteList { source: "Spotify".into(), name: "Classics".into(), url: Some("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".into()), tracks: vec![remote("Hey Jude", "The Beatles", Some(431)), remote("Let It Be", "The Beatles", Some(243))], truncated: false };
        let pv = match_list(&db, list).unwrap();
        assert_eq!(pv.matched, 1);
        let p = save(&db, SaveImport { playlist_id: None, name: pv.name.clone(), source: pv.source.clone(), url: pv.url.clone(), items: pv.items.clone() }).unwrap();
        assert_eq!(p.tracks, vec![1]);
        assert_eq!(p.description, "");
        let i = info(&db, &p.id).unwrap().unwrap();
        assert_eq!((i.total, i.missing.len(), i.missing[0].track_id), (2, 1, None));
        // The missing song turns up in the library → offered as a match.
        db.conn()
            .execute(
                "INSERT INTO music_tracks(id, path, folder, title, artist, album, album_artist, duration_ms, size, mtime, added_at) VALUES (2, 'C:\\m\\b.mp3', 'C:\\m', 'Let It Be', 'The Beatles', '', 'The Beatles', 243000, 1, 1, 'x')",
                [],
            )
            .unwrap();
        assert_eq!(info(&db, &p.id).unwrap().unwrap().missing[0].track_id, Some(2));
        // Stored URLs must be links NOCTURN can parse.
        assert!(save(&db, SaveImport { playlist_id: None, name: "x".into(), source: "Spotify".into(), url: Some("https://evil.example/".into()), items: vec![] }).is_err());
        forget(&db, &p.id).unwrap();
        assert!(info(&db, &p.id).unwrap().is_none());
    }
}
