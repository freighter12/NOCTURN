//! YouTube Music, read without signing in, straight from the phone.
//!
//! - Browsing (search, home, albums, artists, playlists, radio) uses
//!   music.youtube.com's own web API (client `WEB_REMIX`).
//! - Audio links come from the player API with the `VISIONOS` client, which
//!   hands out direct AAC links (itag 140) without JavaScript decoding or
//!   tokens, as long as a visitor ID is sent. (Same approach yt-dlp uses when
//!   it has no JavaScript runtime, checked 2026-11-11.)
//!
//! Only search text, 11-character video IDs and YouTube browse IDs come from
//! the app; everything else in the requests is fixed.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

const MUSIC_API: &str = "https://music.youtube.com/youtubei/v1";
const PLAYER_API: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
const MUSIC_CLIENT_VERSION: &str = "1.20251015.03.00";
const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const VISIONOS_VERSION: &str = "1.02";
const MAX_BODY: usize = 8 * 1024 * 1024;

pub type YtResult<T> = Result<T, String>;

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub duration_ms: Option<i64>,
    pub artwork: Option<String>,
    pub explicit: bool,
    /// Music videos and user uploads (16:9 artwork) rather than album audio.
    pub video: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// "album" | "playlist" | "artist" | "song"
    pub kind: String,
    /// Browse ID (album/playlist/artist) or video ID (song).
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub title: String,
    pub tracks: Vec<Track>,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub top: Option<Card>,
    pub songs: Vec<Track>,
    pub videos: Vec<Track>,
    pub albums: Vec<Card>,
    pub artists: Vec<Card>,
    pub playlists: Vec<Card>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    /// "album" | "playlist"
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub artwork: Option<String>,
    pub description: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtistPage {
    pub id: String,
    pub name: String,
    pub artwork: Option<String>,
    pub top_songs: Vec<Track>,
    pub shelves: Vec<Shelf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub url: String,
    pub mime: String,
    pub bitrate: i64,
    pub content_length: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Seconds since the Unix epoch after which the link stops working.
    pub expires_at: Option<i64>,
    /// Earliest moment (ms since epoch) YouTube serves the link; earlier requests get 403.
    pub available_at_ms: Option<i64>,
    pub loudness_db: Option<f64>,
}

// ---------------------------------------------------------------- http

fn client() -> YtResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(SAFARI_UA)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("network setup failed: {e}"))
}

async fn read_json(res: reqwest::Response, who: &str) -> YtResult<Value> {
    let status = res.status();
    if status.as_u16() == 429 {
        return Err(format!("{who} is limiting requests right now — try again in a few minutes"));
    }
    if !status.is_success() {
        return Err(format!("{who} answered {status}"));
    }
    let bytes = res.bytes().await.map_err(|e| format!("{who} download failed: {}", e.without_url()))?;
    if bytes.len() > MAX_BODY {
        return Err(format!("{who} sent far more than expected"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches(")]}'").trim_start();
    serde_json::from_str(text).map_err(|_| format!("{who}'s answer couldn't be read"))
}

async fn music_post(endpoint: &str, mut body: Value) -> YtResult<Value> {
    body["context"] = json!({ "client": { "clientName": "WEB_REMIX", "clientVersion": MUSIC_CLIENT_VERSION, "hl": "en", "gl": "GB" } });
    let res = client()?
        .post(format!("{MUSIC_API}/{endpoint}{}prettyPrint=false", if endpoint.contains('?') { "&" } else { "?" }))
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/")
        .header("Cookie", "CONSENT=YES+1; SOCS=CAI")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("couldn't reach YouTube Music: {}", e.without_url()))?;
    read_json(res, "YouTube Music").await
}

// ---------------------------------------------------------------- helpers

pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid_browse_id(id: &str) -> bool {
    (4..=80).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Calls `f` for every key/value pair in the tree; when `f` returns true the
/// value is treated as handled and not descended into.
fn walk<'a>(v: &'a Value, f: &mut dyn FnMut(&'a str, &'a Value) -> bool) {
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

fn find_first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let mut found = None;
    walk(v, &mut |k, x| {
        if found.is_some() {
            return true;
        }
        if k == key {
            found = Some(x);
            return true;
        }
        false
    });
    found
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(t) => {
            if let Some(runs) = t.get("runs").and_then(Value::as_array) {
                runs.iter().filter_map(|r| r.get("text").and_then(Value::as_str)).collect()
            } else {
                t.get("simpleText").and_then(Value::as_str).unwrap_or_default().to_string()
            }
        }
        None => String::new(),
    }
}

fn runs(v: Option<&Value>) -> &[Value] {
    v.and_then(|t| t.get("runs")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn page_type(endpoint: &Value) -> &str {
    endpoint
        .pointer("/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn browse_id(endpoint: &Value) -> Option<&str> {
    endpoint.pointer("/browseEndpoint/browseId").and_then(Value::as_str)
}

/// "4:51" / "1:02:03" → milliseconds.
pub fn parse_len(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || !s.contains(':') {
        return None;
    }
    let mut total = 0i64;
    for part in s.split(':') {
        total = total * 60 + part.trim().parse::<i64>().ok()?;
    }
    Some(total * 1000)
}

fn is_explicit(r: &Value) -> bool {
    r.get("badges").map(|b| b.to_string().contains("MUSIC_EXPLICIT_BADGE")).unwrap_or(false)
}

/// Largest thumbnail, upsized when it's a Google image URL that takes size parameters.
fn artwork(v: &Value) -> Option<String> {
    let thumbs = find_first(v, "thumbnails")?.as_array()?;
    let url = thumbs.iter().max_by_key(|t| t.get("width").and_then(Value::as_i64).unwrap_or(0))?.get("url")?.as_str()?;
    Some(upsize(url))
}

pub fn upsize(url: &str) -> String {
    let url = if url.starts_with("//") { format!("https:{url}") } else { url.to_string() };
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        if let Some(i) = url.rfind('=') {
            if url[i..].contains('w') || url[i..].contains('s') {
                return format!("{}=w544-h544-l90-rj", &url[..i]);
            }
        }
    }
    url
}

const TYPE_LABELS: [&str; 9] = ["Song", "Video", "Episode", "Album", "Single", "EP", "Playlist", "Artist", "Podcast"];

/// Builds a track from a list row (search, album, playlist, artist top songs, home quick picks).
fn track_from_row(r: &Value, fallback_artist: &str, fallback_art: Option<&String>) -> Option<Track> {
    let video_id = r
        .pointer("/playlistItemData/videoId")
        .or_else(|| r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .and_then(Value::as_str)?;
    if !valid_video_id(video_id) {
        return None;
    }
    let cols: Vec<&Value> = r
        .get("flexColumns")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text")).collect())
        .unwrap_or_default();
    let title = text(cols.first().copied());
    if title.is_empty() {
        return None;
    }

    let mut artist = String::new();
    let mut artist_id = None;
    let mut album = None;
    let mut album_id = None;
    let mut duration_ms = None;
    let mut video = false;
    let mut first_plain: Option<String> = None;

    for col in cols.iter().skip(1) {
        let rs = runs(Some(col));
        // artists: the run span from the first to the last artist link (keeps " & " between them)
        let artist_idx: Vec<usize> = rs
            .iter()
            .enumerate()
            .filter(|(_, x)| x.get("navigationEndpoint").is_some_and(|e| matches!(page_type(e), "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL")))
            .map(|(i, _)| i)
            .collect();
        if artist.is_empty() {
            if let (Some(&a), Some(&b)) = (artist_idx.first(), artist_idx.last()) {
                artist = rs[a..=b].iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect();
                artist_id = rs[a].get("navigationEndpoint").and_then(browse_id).map(str::to_string);
            }
        }
        for x in rs {
            let t = x.get("text").and_then(Value::as_str).unwrap_or("").trim();
            if let Some(e) = x.get("navigationEndpoint") {
                if page_type(e) == "MUSIC_PAGE_TYPE_ALBUM" && album.is_none() {
                    album = Some(t.to_string());
                    album_id = browse_id(e).map(str::to_string);
                }
            } else if duration_ms.is_none() && parse_len(t).is_some() {
                duration_ms = parse_len(t);
            } else if t == "Video" || t == "Episode" {
                video = true;
            } else if first_plain.is_none() && !t.is_empty() && t != "•" && !TYPE_LABELS.contains(&t) && !t.ends_with(" plays") && !t.ends_with(" views") {
                first_plain = Some(t.to_string());
            }
        }
    }
    if duration_ms.is_none() {
        duration_ms = r
            .get("fixedColumns")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .map(|c| text(c.pointer("/musicResponsiveListItemFixedColumnRenderer/text")))
            .and_then(|s| parse_len(&s));
    }
    if artist.is_empty() {
        artist = first_plain.unwrap_or_else(|| fallback_artist.to_string());
    }
    let art = artwork(r.get("thumbnail").unwrap_or(&Value::Null)).or_else(|| fallback_art.cloned());
    if art.as_deref().is_some_and(|u| u.contains("i.ytimg.com/vi")) {
        video = true;
    }
    Some(Track { video_id: video_id.into(), title, artist, artist_id, album, album_id, duration_ms, artwork: art, explicit: is_explicit(r), video })
}

fn kind_of(page: &str) -> Option<&'static str> {
    match page {
        "MUSIC_PAGE_TYPE_ALBUM" => Some("album"),
        "MUSIC_PAGE_TYPE_PLAYLIST" => Some("playlist"),
        "MUSIC_PAGE_TYPE_ARTIST" => Some("artist"),
        _ => None,
    }
}

/// A browsable list row (album / playlist / artist in search results).
fn card_from_row(r: &Value) -> Option<Card> {
    let e = r.get("navigationEndpoint")?;
    let kind = kind_of(page_type(e))?;
    let id = browse_id(e)?;
    let cols: Vec<String> = r
        .get("flexColumns")
        .and_then(Value::as_array)
        .map(|c| c.iter().map(|c| text(c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))).collect())
        .unwrap_or_default();
    let subtitle = cols.get(1).cloned().unwrap_or_default();
    let subtitle = strip_label(&subtitle);
    Some(Card { kind: kind.into(), id: id.into(), title: cols.first().cloned().unwrap_or_default(), subtitle, artwork: artwork(r.get("thumbnail").unwrap_or(&Value::Null)) })
}

/// "Album • The Weeknd • 2016" → "The Weeknd • 2016"; keeps "Single"/"EP" as useful info.
fn strip_label(s: &str) -> String {
    let mut parts: Vec<&str> = s.split(" • ").collect();
    if parts.len() > 1 && matches!(parts[0], "Album" | "Playlist" | "Artist" | "Song" | "Video") {
        parts.remove(0);
    }
    parts.join(" • ")
}

/// A two-row tile (carousels on home and artist pages).
fn card_from_tile(r: &Value) -> Option<Card> {
    let title = text(r.get("title"));
    let subtitle = strip_label(&text(r.get("subtitle")));
    let art = artwork(r.get("thumbnailRenderer").unwrap_or(&Value::Null));
    let e = r.get("navigationEndpoint")?;
    if let Some(vid) = e.pointer("/watchEndpoint/videoId").and_then(Value::as_str) {
        return valid_video_id(vid).then(|| Card { kind: "song".into(), id: vid.into(), title, subtitle, artwork: art });
    }
    let kind = kind_of(page_type(e))?;
    Some(Card { kind: kind.into(), id: browse_id(e)?.into(), title, subtitle, artwork: art })
}

fn shelves(v: &Value) -> Vec<Shelf> {
    let mut out = Vec::new();
    walk(v, &mut |k, x| {
        if k != "musicCarouselShelfRenderer" && k != "musicShelfRenderer" {
            return false;
        }
        let title = text(x.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title")).trim().to_string();
        let title = if title.is_empty() { text(x.get("title")) } else { title };
        let mut shelf = Shelf { title, ..Default::default() };
        for item in x.get("contents").and_then(Value::as_array).into_iter().flatten() {
            if let Some(r) = item.get("musicResponsiveListItemRenderer") {
                if let Some(t) = track_from_row(r, "", None) {
                    shelf.tracks.push(t);
                } else if let Some(c) = card_from_row(r) {
                    shelf.cards.push(c);
                }
            } else if let Some(r) = item.get("musicTwoRowItemRenderer") {
                if let Some(c) = card_from_tile(r) {
                    shelf.cards.push(c);
                }
            }
        }
        if !shelf.tracks.is_empty() || !shelf.cards.is_empty() {
            out.push(shelf);
        }
        true
    });
    out
}

// ---------------------------------------------------------------- parsers

pub fn parse_search(v: &Value) -> SearchResults {
    let mut out = SearchResults::default();
    walk(v, &mut |k, x| match k {
        "musicCardShelfRenderer" => {
            let e = x.pointer("/title/runs/0/navigationEndpoint").unwrap_or(&Value::Null);
            let title = text(x.get("title"));
            let subtitle = strip_label(&text(x.get("subtitle")));
            let art = artwork(x.get("thumbnail").unwrap_or(&Value::Null));
            if let (Some(kind), Some(id)) = (kind_of(page_type(e)), browse_id(e)) {
                out.top = Some(Card { kind: kind.into(), id: id.into(), title, subtitle, artwork: art });
            } else if let Some(vid) = e.pointer("/watchEndpoint/videoId").and_then(Value::as_str) {
                out.top = Some(Card { kind: "song".into(), id: vid.into(), title, subtitle, artwork: art });
            }
            // Songs under an artist's top card don't repeat the artist name.
            let card_artist = match &out.top {
                Some(c) if c.kind == "artist" => c.title.clone(),
                Some(c) => c.subtitle.split(" • ").next().unwrap_or("").to_string(),
                None => String::new(),
            };
            for c in x.get("contents").and_then(Value::as_array).into_iter().flatten() {
                if let Some(t) = c.get("musicResponsiveListItemRenderer").and_then(|r| track_from_row(r, &card_artist, None)) {
                    if !out.songs.iter().any(|s| s.video_id == t.video_id) {
                        out.songs.push(t);
                    }
                }
            }
            true
        }
        "musicResponsiveListItemRenderer" => {
            if let Some(t) = track_from_row(x, "", None) {
                let list = if t.video { &mut out.videos } else { &mut out.songs };
                if !list.iter().any(|s| s.video_id == t.video_id) {
                    list.push(t);
                }
            } else if let Some(c) = card_from_row(x) {
                match c.kind.as_str() {
                    "album" => out.albums.push(c),
                    "artist" => out.artists.push(c),
                    "playlist" => out.playlists.push(c),
                    _ => {}
                }
            }
            true
        }
        _ => false,
    });
    out
}

pub fn parse_collection(v: &Value, id: &str) -> Collection {
    let header = find_first(v, "musicResponsiveHeaderRenderer").or_else(|| find_first(v, "musicDetailHeaderRenderer")).unwrap_or(&Value::Null);
    let kind = if id.starts_with("MPREb_") { "album" } else { "playlist" };
    let artist = text(header.get("straplineTextOne"));
    let art = artwork(header.get("thumbnail").unwrap_or(&Value::Null));
    let subtitle = [strip_label(&text(header.get("subtitle"))), text(header.get("secondSubtitle"))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let description = text(find_first(header, "description").and_then(|d| d.get("musicDescriptionShelfRenderer")).and_then(|d| d.get("description")));
    let title = text(header.get("title"));
    let mut tracks = Vec::new();
    walk(v, &mut |k, x| {
        if k != "musicResponsiveListItemRenderer" {
            return false;
        }
        if let Some(mut t) = track_from_row(x, &artist, art.as_ref()) {
            if kind == "album" {
                t.album.get_or_insert_with(|| title.clone());
                t.album_id.get_or_insert_with(|| id.to_string());
            }
            if !tracks.iter().any(|s: &Track| s.video_id == t.video_id) {
                tracks.push(t);
            }
        }
        true
    });
    Collection { id: id.into(), kind: kind.into(), title, subtitle, artist, artwork: art, description, tracks }
}

pub fn parse_artist(v: &Value, id: &str) -> ArtistPage {
    let header = v.get("header").and_then(|h| h.as_object()).and_then(|h| h.values().next()).unwrap_or(&Value::Null);
    let name = text(header.get("title"));
    let art = artwork(header.get("thumbnail").unwrap_or(&Value::Null));
    let mut all = shelves(v.get("contents").unwrap_or(&Value::Null));
    let top_songs = all.iter().position(|s| !s.tracks.is_empty()).map(|i| all.remove(i).tracks).unwrap_or_default();
    let top_songs = top_songs.into_iter().map(|mut t| { if t.artist.is_empty() { t.artist = name.clone(); } t }).collect();
    ArtistPage { id: id.into(), name, artwork: art, top_songs, shelves: all }
}

pub fn parse_radio(v: &Value) -> Vec<Track> {
    let mut out: Vec<Track> = Vec::new();
    walk(v, &mut |k, x| {
        if k != "playlistPanelVideoRenderer" {
            return false;
        }
        let vid = x.get("videoId").and_then(Value::as_str).unwrap_or("");
        let title = text(x.get("title"));
        if !valid_video_id(vid) || title.is_empty() || out.iter().any(|s| s.video_id == vid) {
            return true;
        }
        let by = runs(x.get("longBylineText"));
        let mut artist = String::new();
        let mut artist_id = None;
        let mut album = None;
        let mut album_id = None;
        for r in by {
            let t = r.get("text").and_then(Value::as_str).unwrap_or("");
            if let Some(e) = r.get("navigationEndpoint") {
                match page_type(e) {
                    "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => {
                        if artist_id.is_none() {
                            artist_id = browse_id(e).map(str::to_string);
                        }
                        if !artist.is_empty() && album.is_none() {
                            artist.push_str(" & ");
                        }
                        if album.is_none() {
                            artist.push_str(t);
                        }
                    }
                    "MUSIC_PAGE_TYPE_ALBUM" => {
                        album = Some(t.to_string());
                        album_id = browse_id(e).map(str::to_string);
                    }
                    _ => {}
                }
            }
        }
        if artist.is_empty() {
            artist = text(x.get("longBylineText")).split(" • ").next().unwrap_or("").to_string();
        }
        let art = artwork(x.get("thumbnail").unwrap_or(&Value::Null));
        let video = art.as_deref().is_some_and(|u| u.contains("i.ytimg.com/vi"));
        out.push(Track {
            video_id: vid.into(),
            title,
            artist,
            artist_id,
            album,
            album_id,
            duration_ms: parse_len(&text(x.get("lengthText"))),
            artwork: art,
            explicit: is_explicit(x),
            video,
        });
        true
    });
    out
}

pub fn parse_suggestions(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(v, &mut |k, x| {
        if k != "searchSuggestionRenderer" {
            return false;
        }
        let s = text(x.get("suggestion"));
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
        true
    });
    out
}

/// Picks the audio link the iPhone plays natively: AAC itag 140, else any AAC.
pub fn parse_stream(v: &Value) -> YtResult<Stream> {
    let status = v.pointer("/playabilityStatus/status").and_then(Value::as_str).unwrap_or("");
    if status != "OK" {
        let reason = text(v.pointer("/playabilityStatus/reason")).trim().to_string();
        let reason = if reason.is_empty() { v.pointer("/playabilityStatus/reason").and_then(Value::as_str).unwrap_or("").to_string() } else { reason };
        return Err(match status {
            "LOGIN_REQUIRED" => format!("YouTube wants a sign-in for this song ({reason})"),
            "UNPLAYABLE" | "ERROR" => format!("This song can't be played ({reason})"),
            _ => format!("YouTube said {status} {reason}").trim().to_string(),
        });
    }
    let formats = v.pointer("/streamingData/adaptiveFormats").and_then(Value::as_array).ok_or("YouTube sent no audio formats")?;
    let aac = |f: &&Value| f.get("mimeType").and_then(Value::as_str).is_some_and(|m| m.starts_with("audio/mp4")) && f.get("url").is_some();
    let pick = formats
        .iter()
        .filter(aac)
        .find(|f| f.get("itag").and_then(Value::as_i64) == Some(140))
        .or_else(|| formats.iter().filter(aac).max_by_key(|f| f.get("bitrate").and_then(Value::as_i64).unwrap_or(0)))
        .ok_or("YouTube sent no AAC audio for this song")?;
    let url = pick.get("url").and_then(Value::as_str).unwrap_or_default().to_string();
    let query = |key: &str| url.split(['?', '&']).find_map(|kv| kv.strip_prefix(key).and_then(|r| r.strip_prefix('='))).and_then(|s| s.parse::<i64>().ok());
    let num = |f: &Value, key: &str| f.get(key).and_then(|x| x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_i64()));
    Ok(Stream {
        mime: pick.get("mimeType").and_then(Value::as_str).unwrap_or("audio/mp4").to_string(),
        bitrate: pick.get("bitrate").and_then(Value::as_i64).unwrap_or(0),
        content_length: num(pick, "contentLength"),
        duration_ms: num(pick, "approxDurationMs"),
        expires_at: query("expire"),
        available_at_ms: query("available_at").map(|s| s * 1000),
        loudness_db: v.pointer("/playerConfig/audioConfig/loudnessDb").and_then(Value::as_f64),
        url,
    })
}

// ---------------------------------------------------------------- API

pub async fn search(query: &str) -> YtResult<SearchResults> {
    let q: String = query.trim().chars().take(150).collect();
    if q.is_empty() {
        return Ok(SearchResults::default());
    }
    Ok(parse_search(&music_post("search", json!({ "query": q })).await?))
}

/// Album songs only (YouTube Music's "Songs" filter) — used to match imported playlists.
pub async fn search_songs(query: &str) -> YtResult<Vec<Track>> {
    let q: String = query.trim().chars().take(150).collect();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let r = parse_search(&music_post("search", json!({ "query": q, "params": "EgWKAQIIAWoKEAkQBRAKEAMQBA==" })).await?);
    Ok(r.songs)
}

pub async fn suggestions(input: &str) -> YtResult<Vec<String>> {
    let q: String = input.chars().take(100).collect();
    if q.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(parse_suggestions(&music_post("music/get_search_suggestions", json!({ "input": q })).await?))
}

pub async fn home() -> YtResult<Vec<Shelf>> {
    Ok(shelves(&music_post("browse", json!({ "browseId": "FEmusic_home" })).await?))
}

pub async fn collection(id: &str) -> YtResult<Collection> {
    if !valid_browse_id(id) {
        return Err("not a YouTube Music album or playlist".into());
    }
    let browse = if id.starts_with("MPREb_") || id.starts_with("VL") { id.to_string() } else { format!("VL{id}") };
    let first = music_post("browse", json!({ "browseId": browse })).await?;
    let mut c = parse_collection(&first, id);
    // Long playlists come in pages of ~100; follow them (up to ~1,000 songs).
    let mut token = next_continuation(&first);
    let mut pages = 0;
    while let Some(t) = token.take() {
        pages += 1;
        if pages > 10 {
            break;
        }
        let req = match &t {
            Cont::New(tok) => music_post("browse", json!({ "continuation": tok })).await,
            Cont::Old(tok) => music_post(&format!("browse?ctoken={tok}&continuation={tok}&type=next"), json!({})).await,
        };
        let Ok(more) = req else { break };
        let before = c.tracks.len();
        let page = parse_collection(&more, id);
        for tr in page.tracks {
            if !c.tracks.iter().any(|x| x.video_id == tr.video_id) {
                c.tracks.push(tr);
            }
        }
        if c.tracks.len() == before {
            break;
        }
        token = next_continuation(&more);
    }
    Ok(c)
}

enum Cont {
    /// `continuationItemRenderer` → POST browse {"continuation": token}
    New(String),
    /// `nextContinuationData` → browse?ctoken=…&type=next
    Old(String),
}

/// The "next page" token of a playlist's song list.
fn next_continuation(v: &Value) -> Option<Cont> {
    let ok = |t: &str| t.len() < 2000 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"%-_=".contains(&b));
    if let Some(t) = find_first(v, "continuationItemRenderer").and_then(|c| c.pointer("/continuationEndpoint/continuationCommand/token")).and_then(Value::as_str) {
        if ok(t) {
            return Some(Cont::New(t.to_string()));
        }
    }
    let t = find_first(v, "nextContinuationData")?.get("continuation")?.as_str()?;
    ok(t).then(|| Cont::Old(t.to_string()))
}

pub async fn artist(id: &str) -> YtResult<ArtistPage> {
    if !valid_browse_id(id) {
        return Err("not a YouTube Music artist".into());
    }
    Ok(parse_artist(&music_post("browse", json!({ "browseId": id })).await?, id))
}

/// The "up next" radio YouTube Music builds from a song (the song itself first).
pub async fn radio(video_id: &str) -> YtResult<Vec<Track>> {
    if !valid_video_id(video_id) {
        return Err("not a YouTube video ID".into());
    }
    Ok(parse_radio(&music_post("next", json!({ "videoId": video_id, "playlistId": format!("RDAMVM{video_id}"), "isAudioOnly": true })).await?))
}

// ---- audio links

struct Visitor {
    id: String,
    fetched: Instant,
}

static VISITOR: Mutex<Option<Visitor>> = Mutex::new(None);
const VISITOR_TTL: Duration = Duration::from_secs(6 * 3600);

async fn visitor_id(force_new: bool) -> YtResult<String> {
    if !force_new {
        if let Some(v) = VISITOR.lock().unwrap().as_ref() {
            if v.fetched.elapsed() < VISITOR_TTL {
                return Ok(v.id.clone());
            }
        }
    }
    let res = client()?
        .get("https://www.youtube.com/sw.js_data")
        .send()
        .await
        .map_err(|e| format!("couldn't reach YouTube: {}", e.without_url()))?;
    let v = read_json(res, "YouTube").await?;
    let id = v.pointer("/0/2/0/0/13").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() < 2000).ok_or("YouTube sent no visitor ID")?.to_string();
    *VISITOR.lock().unwrap() = Some(Visitor { id: id.clone(), fetched: Instant::now() });
    Ok(id)
}

async fn player(video_id: &str, visitor: &str) -> YtResult<Value> {
    let body = json!({
        "context": { "client": {
            "clientName": "VISIONOS", "clientVersion": VISIONOS_VERSION,
            "deviceMake": "Apple", "deviceModel": "RealityDevice17,1",
            "userAgent": SAFARI_UA, "osName": "visionOS", "osVersion": "26.5.23O471",
            "hl": "en", "gl": "GB", "visitorData": visitor,
        }},
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let res = client()?
        .post(PLAYER_API)
        .header("X-YouTube-Client-Name", "101")
        .header("X-YouTube-Client-Version", VISIONOS_VERSION)
        .header("X-Goog-Visitor-Id", visitor)
        .header("Origin", "https://www.youtube.com")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("couldn't reach YouTube: {}", e.without_url()))?;
    read_json(res, "YouTube").await
}

/// A playable AAC link for a song. Retries once with a fresh visitor ID when
/// YouTube asks for a sign-in (its bot check).
pub async fn stream(video_id: &str) -> YtResult<Stream> {
    if !valid_video_id(video_id) {
        return Err("not a YouTube video ID".into());
    }
    let visitor = visitor_id(false).await?;
    match parse_stream(&player(video_id, &visitor).await?) {
        Ok(s) => Ok(s),
        Err(e) if e.contains("sign-in") => {
            let fresh = visitor_id(true).await?;
            parse_stream(&player(video_id, &fresh).await?)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Option<Value> {
        let dir = std::env::var("YTM_FIXTURES").ok()?;
        Some(serde_json::from_str(&std::fs::read_to_string(format!("{dir}/{name}.json")).ok()?).unwrap())
    }

    #[test]
    fn lengths_and_artwork() {
        assert_eq!(parse_len("4:51"), Some(291_000));
        assert_eq!(parse_len("1:02:03"), Some(3_723_000));
        assert_eq!(parse_len("2016"), None);
        assert_eq!(upsize("https://lh3.googleusercontent.com/abc=w120-h120-l90-rj"), "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj");
        assert_eq!(upsize("https://i.ytimg.com/vi/x/hqdefault.jpg"), "https://i.ytimg.com/vi/x/hqdefault.jpg");
        assert!(valid_video_id("dQw4w9WgXcQ"));
        assert!(!valid_video_id("dQw4w9WgXc"));
    }

    /// Runs against saved real responses when YTM_FIXTURES points at them.
    #[test]
    fn real_responses() {
        let Some(s) = fixture("search") else { return };
        let r = parse_search(&s);
        eprintln!("top {:?}\nsongs {:#?}\nalbums {:?}\nartists {:?}\nplaylists {:?}\nvideos {}", r.top, &r.songs[..3.min(r.songs.len())], r.albums.iter().map(|a| &a.title).collect::<Vec<_>>(), r.artists.iter().map(|a| &a.title).collect::<Vec<_>>(), r.playlists.len(), r.videos.len());
        assert!(r.top.is_some());
        assert!(r.songs.len() >= 5);
        assert!(r.songs.iter().all(|s| !s.artist.is_empty()));
        assert!(!r.albums.is_empty() && !r.artists.is_empty());

        let a = parse_collection(&fixture("album").unwrap(), "MPREb_mym2n89O4wG");
        eprintln!("album {} by {} ({}) {} tracks, first {:?}", a.title, a.artist, a.subtitle, a.tracks.len(), a.tracks.first());
        assert_eq!(a.tracks.len(), 12);
        assert!(a.tracks.iter().all(|t| t.duration_ms.is_some() && t.artwork.is_some()));

        let p = parse_collection(&fixture("playlist").unwrap(), "VLPL1uK1eZ4ySp4RTxKlCHTLUhMiN1uuxnHC");
        eprintln!("playlist {} {} tracks, first {:?}", p.title, p.tracks.len(), p.tracks.first());
        assert!(p.tracks.len() >= 50);
        assert!(p.tracks[0].album.is_some());

        let ar = parse_artist(&fixture("artist").unwrap(), "UClYV6hHlupm_S_ObS1W-DYw");
        eprintln!("artist {} top {:?} shelves {:?}", ar.name, ar.top_songs.iter().map(|t| &t.title).collect::<Vec<_>>(), ar.shelves.iter().map(|s| format!("{} ({})", s.title, s.cards.len())).collect::<Vec<_>>());
        assert_eq!(ar.name, "The Weeknd");
        assert_eq!(ar.top_songs.len(), 5);

        let h = shelves(&fixture("home").unwrap());
        eprintln!("home {:?}", h.iter().map(|s| format!("{} ({} tracks, {} cards)", s.title, s.tracks.len(), s.cards.len())).collect::<Vec<_>>());
        assert!(!h.is_empty());

        let n = parse_radio(&fixture("next").unwrap());
        eprintln!("radio {} first {:?}", n.len(), n.iter().take(3).map(|t| format!("{} — {} [{:?}]", t.artist, t.title, t.album)).collect::<Vec<_>>());
        assert!(n.len() >= 10);

        let sg = parse_suggestions(&fixture("suggest").unwrap());
        eprintln!("suggestions {sg:?}");
        assert!(!sg.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn live_long_playlist() {
        let r = search("top 500 songs of all time playlist").await.unwrap();
        let p = r.playlists.first().expect("a playlist");
        let c = collection(&p.id).await.unwrap();
        eprintln!("{} ({}) -> {} tracks", c.title, c.subtitle, c.tracks.len());
        assert!(c.tracks.len() > 100, "pages were followed");
    }

    #[tokio::test]
    #[ignore]
    async fn live_stream() {
        for id in ["kJQP7kiw5Fk", "fJ9rUzIMcZQ", "XXYlFuWEuKI"] {
            let s = stream(id).await.unwrap();
            eprintln!("{id}: {} {} kbps, expires {:?}, loudness {:?}", s.mime, s.bitrate / 1000, s.expires_at, s.loudness_db);
            assert!(s.url.starts_with("https://"));
        }
        let home = home().await.unwrap();
        assert!(!home.is_empty());
    }
}
