//! Playlist import from a link: Spotify, Deezer, YouTube or YouTube Music.
//!
//! Only the *song list* is read (titles, artists, lengths). A pasted link is
//! parsed into a service + ID and the app builds the request URL itself, for
//! a fixed set of hosts, so a link can't make the app fetch anything else.
//! Spotify/Deezer songs are then found on YouTube Music one by one.
//! (Link parsing and page reading are the desktop app's, music_import.rs.)

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::textmatch::{artists_overlap, clean_title, title_sim};
use crate::yt::{self, Track};

const MAX_ITEMS: usize = 2000;
const MAX_BODY: usize = 12 * 1024 * 1024;
const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1";

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
    pub fn page_url(&self) -> String {
        match self {
            Link::Spotify { kind, id } => format!("https://open.spotify.com/{kind}/{id}"),
            Link::Deezer { kind, id } => format!("https://www.deezer.com/{kind}/{id}"),
            Link::YouTube { list } => format!("https://music.youtube.com/playlist?list={list}"),
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

fn is_short_link(s: &str) -> bool {
    reqwest::Url::parse(s.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| matches!(h.as_str(), "spotify.link" | "link.deezer.com" | "deezer.page.link"))
}

/// The first link in whatever was pasted (share sheets often add text around it).
fn extract_link(input: &str) -> &str {
    let s = input.trim();
    if let Some(i) = s.find("https://").or_else(|| s.find("http://")) {
        let rest = &s[i..];
        return rest.split_whitespace().next().unwrap_or(rest);
    }
    s
}

pub fn parse_link(input: &str) -> Result<Link, String> {
    let s = extract_link(input);
    if let Some(rest) = s.strip_prefix("spotify:") {
        let mut it = rest.split(':');
        if let (Some(kind), Some(id)) = (it.next(), it.next()) {
            let kind = match kind {
                "playlist" => "playlist",
                "album" => "album",
                _ => return Err("only Spotify playlists and albums can be imported".into()),
            };
            if spotify_id(id) {
                return Ok(Link::Spotify { kind, id: id.into() });
            }
        }
        return Err("that Spotify link doesn't look right".into());
    }
    let url = reqwest::Url::parse(s).map_err(|_| "paste a full link, starting with https://".to_string())?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err("paste a web link (https://…)".into());
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let segs: Vec<&str> = url.path_segments().map(|p| p.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let query = |k: &str| url.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.to_string());
    match host.as_str() {
        "open.spotify.com" | "play.spotify.com" => {
            for w in segs.windows(2) {
                let kind = match w[0] {
                    "playlist" => "playlist",
                    "album" => "album",
                    "track" | "artist" | "show" | "episode" => return Err("that's a Spotify song, artist or podcast — paste a playlist or album link".into()),
                    _ => continue,
                };
                if spotify_id(w[1]) {
                    return Ok(Link::Spotify { kind, id: w[1].into() });
                }
            }
            Err("couldn't find a Spotify playlist or album in that link".into())
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
            Err("couldn't find a Deezer playlist or album in that link".into())
        }
        "www.youtube.com" | "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" => match query("list") {
            Some(l) if yt_list(&l) => Ok(Link::YouTube { list: l }),
            _ => Err("that YouTube link has no playlist in it (look for ?list=… in the address)".into()),
        },
        "spotify.link" | "link.deezer.com" | "deezer.page.link" => Err("short link".into()),
        _ => Err("links from Spotify, YouTube Music, YouTube and Deezer can be imported".into()),
    }
}

fn client(redirects: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(25))
        .redirect(if redirects { reqwest::redirect::Policy::limited(3) } else { reqwest::redirect::Policy::none() })
        .build()
        .map_err(|e| format!("network setup failed: {e}"))
}

async fn get_text(url: &str) -> Result<(u16, String), String> {
    let res = client(true)?.get(url).header("Accept-Language", "en-GB,en;q=0.9").send().await.map_err(|e| format!("couldn't reach the service: {}", e.without_url()))?;
    let status = res.status().as_u16();
    let bytes = res.bytes().await.map_err(|e| format!("download failed: {}", e.without_url()))?;
    if bytes.len() > MAX_BODY {
        return Err("the service sent far more than a playlist".into());
    }
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}

async fn resolve_short(input: &str) -> Result<Link, String> {
    let url = reqwest::Url::parse(extract_link(input)).map_err(|_| "paste a full link".to_string())?;
    let res = client(false)?.get(url).send().await.map_err(|e| format!("couldn't open the short link: {}", e.without_url()))?;
    match res.headers().get("location").and_then(|v| v.to_str().ok()) {
        Some(l) => parse_link(l).map_err(|_| "that short link doesn't lead to a playlist — open it in Safari and copy the full address".into()),
        None => Err("that short link didn't lead anywhere — open it in Safari and copy the full address".into()),
    }
}

#[derive(Debug, Clone)]
struct RemoteTrack {
    title: String,
    artist: String,
    duration_ms: Option<i64>,
}

struct RemoteList {
    name: String,
    tracks: Vec<RemoteTrack>,
}

fn parse_spotify_embed(html: &str) -> Result<RemoteList, String> {
    let start = html.find("id=\"__NEXT_DATA__\"").and_then(|i| html[i..].find('>').map(|j| i + j + 1));
    let json = start.and_then(|s| html[s..].find("</script>").map(|e| &html[s..s + e]));
    let v: Value = json.and_then(|j| serde_json::from_str(j).ok()).ok_or("Spotify's page has changed; the playlist couldn't be read")?;
    let e = v.pointer("/props/pageProps/state/data/entity").ok_or("couldn't find that Spotify playlist (it may be private)")?;
    let name = e.get("name").or_else(|| e.get("title")).and_then(Value::as_str).unwrap_or("Spotify playlist").to_string();
    let tracks = e
        .get("trackList")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let title = t.get("title")?.as_str()?.trim().to_string();
                    let artist = t.get("subtitle").and_then(Value::as_str).unwrap_or("").replace('\u{a0}', " ").trim().to_string();
                    (!title.is_empty()).then(|| RemoteTrack { title, artist, duration_ms: t.get("duration").and_then(Value::as_i64) })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(RemoteList { name, tracks })
}

async fn fetch_deezer(kind: &str, id: &str) -> Result<RemoteList, String> {
    let (_, body) = get_text(&format!("https://api.deezer.com/{kind}/{id}")).await?;
    let v: Value = serde_json::from_str(&body).map_err(|_| "Deezer sent something unexpected".to_string())?;
    if v.get("error").is_some() {
        return Err("couldn't find that Deezer playlist (it may be private or deleted)".into());
    }
    let name = v.get("title").and_then(Value::as_str).unwrap_or("Deezer playlist").to_string();
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
                artist: t.pointer("/artist/name").and_then(Value::as_str).unwrap_or("").to_string(),
                duration_ms: t.get("duration").and_then(Value::as_i64).map(|s| s * 1000),
            });
        }
        pages += 1;
        if let Some(next) = p.get("next").and_then(Value::as_str) {
            if next.starts_with("https://api.deezer.com/") && pages < 25 && tracks.len() < MAX_ITEMS {
                let (_, b) = get_text(next).await?;
                page = serde_json::from_str(&b).ok();
            }
        }
    }
    Ok(RemoteList { name, tracks })
}

// ---------------------------------------------------------------- matching on YouTube Music

/// The best YouTube Music song for a track from another service, if any is close enough.
fn pick(want: &RemoteTrack, candidates: &[Track]) -> Option<Track> {
    let wt = clean_title(&want.title);
    let mut best: Option<(f64, &Track)> = None;
    for (rank, c) in candidates.iter().take(8).enumerate() {
        let sim = title_sim(&wt, &clean_title(&c.title));
        if sim < 0.5 {
            continue;
        }
        let artist_ok = want.artist.is_empty() || artists_overlap(&want.artist, &c.artist);
        let dur = match (want.duration_ms, c.duration_ms) {
            (Some(a), Some(b)) => Some((a - b).abs()),
            _ => None,
        };
        if dur.is_some_and(|d| d > 30_000) {
            continue;
        }
        let mut score = sim * 2.0 + if artist_ok { 1.5 } else { 0.0 } - rank as f64 * 0.08;
        if let Some(d) = dur {
            score += if d <= 3000 { 0.8 } else if d <= 10_000 { 0.3 } else { 0.0 };
        }
        if !artist_ok && sim < 0.95 {
            continue;
        }
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, c));
        }
    }
    best.map(|(_, c)| c.clone())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub title: String,
    pub source: String,
    pub url: String,
    pub artwork: Option<String>,
    pub tracks: Vec<Track>,
    pub missing: Vec<Missing>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    done: usize,
    total: usize,
    stage: &'static str,
}

pub async fn import(app: &AppHandle, input: &str) -> Result<Imported, String> {
    let link = match parse_link(input) {
        Ok(l) => l,
        Err(_) if is_short_link(extract_link(input)) => resolve_short(input).await?,
        Err(e) => return Err(e),
    };
    let _ = app.emit("import-progress", Progress { done: 0, total: 0, stage: "reading" });

    // YouTube / YouTube Music: already on YouTube, just read it.
    if let Link::YouTube { list } = &link {
        let c = yt::collection(list).await?;
        if c.tracks.is_empty() {
            return Err("that playlist is empty, private, or a mix YouTube makes on the fly".into());
        }
        return Ok(Imported { title: c.title, source: "YouTube Music".into(), url: link.page_url(), artwork: c.artwork, tracks: c.tracks, missing: vec![] });
    }

    let list = match &link {
        Link::Spotify { kind, id } => {
            let (status, html) = get_text(&format!("https://open.spotify.com/embed/{kind}/{id}")).await?;
            if status == 404 {
                return Err("couldn't find that Spotify playlist (it may be private or deleted)".into());
            }
            parse_spotify_embed(&html)?
        }
        Link::Deezer { kind, id } => fetch_deezer(kind, id).await?,
        Link::YouTube { .. } => unreachable!(),
    };
    if list.tracks.is_empty() {
        return Err("that playlist has no songs (or is private)".into());
    }
    let wanted: Vec<RemoteTrack> = list.tracks.into_iter().take(MAX_ITEMS).collect();
    let total = wanted.len();

    // Find songs a few at a time (fast, without hammering YouTube).
    let mut found: Vec<Option<Track>> = vec![None; total];
    let mut done = 0;
    for chunk in (0..total).collect::<Vec<_>>().chunks(5) {
        let mut set = tokio::task::JoinSet::new();
        for &i in chunk {
            let w = wanted[i].clone();
            set.spawn(async move {
                let first = w.artist.split([',', '&']).next().unwrap_or("").trim().to_string();
                let q = format!("{} {}", w.title, first);
                let hit = yt::search_songs(&q).await.ok().and_then(|c| pick(&w, &c));
                (i, hit)
            });
        }
        while let Some(r) = set.join_next().await {
            if let Ok((i, hit)) = r {
                found[i] = hit;
            }
            done += 1;
        }
        let _ = app.emit("import-progress", Progress { done, total, stage: "matching" });
    }

    let mut tracks = Vec::new();
    let mut missing = Vec::new();
    for (w, f) in wanted.iter().zip(found) {
        match f {
            Some(t) if !tracks.iter().any(|x: &Track| x.video_id == t.video_id) => tracks.push(t),
            Some(_) => {}
            None => missing.push(Missing { title: w.title.clone(), artist: w.artist.clone() }),
        }
    }
    if tracks.is_empty() {
        return Err("none of those songs could be found on YouTube Music".into());
    }
    let artwork = tracks.first().and_then(|t| t.artwork.clone());
    Ok(Imported { title: list.name, source: link.source().into(), url: link.page_url(), artwork, tracks, missing })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        assert_eq!(parse_link("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc").unwrap(), Link::Spotify { kind: "playlist", id: "37i9dQZF1DXcBWIGoYBM5M".into() });
        assert_eq!(parse_link("Check this out https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy?si=x").unwrap(), Link::Spotify { kind: "album", id: "4aawyAB9vmqN3uQ7FjRGTy".into() });
        assert_eq!(parse_link("https://www.deezer.com/en/playlist/908622995").unwrap(), Link::Deezer { kind: "playlist", id: "908622995".into() });
        assert_eq!(parse_link("https://music.youtube.com/playlist?list=PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i&si=1").unwrap(), Link::YouTube { list: "PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i".into() });
        assert!(parse_link("https://evil.example/playlist/37i9dQZF1DXcBWIGoYBM5M").is_err());
        assert!(parse_link("https://open.spotify.com/track/37i9dQZF1DXcBWIGoYBM5M").is_err());
        assert!(is_short_link("https://spotify.link/abc123"));
    }

    #[tokio::test]
    #[ignore]
    async fn live_spotify_match() {
        let (_, html) = get_text("https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M").await.unwrap();
        let list = parse_spotify_embed(&html).unwrap();
        eprintln!("{}: {} songs", list.name, list.tracks.len());
        let mut ok = 0;
        for w in list.tracks.iter().take(10) {
            let c = yt::search_songs(&format!("{} {}", w.title, w.artist)).await.unwrap();
            let p = pick(w, &c);
            eprintln!("{} — {}  =>  {:?}", w.artist, w.title, p.as_ref().map(|t| format!("{} — {}", t.artist, t.title)));
            ok += p.is_some() as usize;
        }
        assert!(ok >= 8);
    }
}
