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
const MAX_TEXT: usize = 2 * 1024 * 1024;
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
pub(crate) struct RemoteTrack {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) explicit: Option<bool>,
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
                    (!title.is_empty()).then(|| RemoteTrack { title, artist, duration_ms: t.get("duration").and_then(Value::as_i64), explicit: t.get("isExplicit").and_then(Value::as_bool) })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(RemoteList { name, tracks })
}

/// Spotify song IDs in pasted text, when the text is mostly song links (what the
/// Spotify app copies after selecting songs: any number, unlike the 100 its
/// public playlist page shows).
fn spotify_track_ids(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let ids: Vec<String> = lines
        .iter()
        .filter_map(|l| {
            let rest = l.strip_prefix("spotify:track:").or_else(|| l.split("open.spotify.com/").nth(1).and_then(|r| r.split("track/").nth(1)))?;
            let id: String = rest.chars().take_while(char::is_ascii_alphanumeric).collect();
            spotify_id(&id).then_some(id)
        })
        .take(MAX_ITEMS)
        .collect();
    if ids.is_empty() || ids.len() * 2 < lines.len() {
        return Vec::new();
    }
    ids
}

fn parse_spotify_track(html: &str) -> Option<RemoteTrack> {
    let start = html.find("id=\"__NEXT_DATA__\"").and_then(|i| html[i..].find('>').map(|j| i + j + 1))?;
    let json = &html[start..start + html[start..].find("</script>")?];
    let v: Value = serde_json::from_str(json).ok()?;
    let e = v.pointer("/props/pageProps/state/data/entity")?;
    let title = e.get("name").or_else(|| e.get("title")).and_then(Value::as_str)?.trim().to_string();
    let artist = e.get("artists").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    (!title.is_empty()).then(|| RemoteTrack { title, artist, duration_ms: e.get("duration").and_then(Value::as_i64), explicit: e.get("isExplicit").and_then(Value::as_bool) })
}

/// Look up pasted Spotify song links (eight at a time), in order.
async fn fetch_spotify_tracks(app: &AppHandle, ids: Vec<String>) -> Result<RemoteList, String> {
    let total = ids.len();
    let mut found: Vec<Option<RemoteTrack>> = vec![None; total];
    let mut done = 0;
    for chunk in (0..total).collect::<Vec<_>>().chunks(8) {
        let mut set = tokio::task::JoinSet::new();
        for &i in chunk {
            let id = ids[i].clone();
            set.spawn(async move {
                let r = get_text(&format!("https://open.spotify.com/embed/track/{id}")).await;
                (i, r.ok().filter(|(s, _)| *s < 400).and_then(|(_, h)| parse_spotify_track(&h)))
            });
        }
        while let Some(r) = set.join_next().await {
            if let Ok((i, t)) = r {
                found[i] = t;
            }
            done += 1;
        }
        let _ = app.emit("import-progress", Progress { done, total, stage: "reading" });
    }
    let tracks: Vec<RemoteTrack> = found.into_iter().flatten().collect();
    if tracks.is_empty() {
        return Err("couldn't read those Spotify songs — check your connection and try again".into());
    }
    Ok(RemoteList { name: "Spotify songs".into(), tracks })
}

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

/// "Artist - Title", "Title by Artist", "3. Artist – Title", or a CSV export with a header row.
fn parse_text(text: &str) -> Result<RemoteList, String> {
    let lines: Vec<&str> = text.trim_start_matches('\u{feff}').lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let header = lines.first().map(|l| csv_fields(&l.to_lowercase())).unwrap_or_default();
    let col = |names: &[&str]| header.iter().position(|h| names.iter().any(|n| h == n));
    let (ct, ca, cd) = (col(&["track name", "title", "name", "song", "track"]), col(&["artist name(s)", "artist", "artists", "artist name"]), col(&["duration (ms)", "duration_ms", "duration"]));
    let mut tracks = Vec::new();
    if let (Some(ct), true) = (ct, header.len() > 1) {
        for l in lines.iter().skip(1) {
            let f = csv_fields(l);
            let get = |i: Option<usize>| i.and_then(|i| f.get(i)).cloned().unwrap_or_default();
            let title = get(Some(ct));
            if !title.is_empty() {
                let d = get(cd);
                let duration_ms = d.parse::<i64>().ok().map(|n| if n < 10_000 { n * 1000 } else { n }).or_else(|| yt::parse_len(&d));
                tracks.push(RemoteTrack { title, artist: get(ca).replace(';', ", "), duration_ms, explicit: None });
            }
        }
    } else {
        for l in &lines {
            let l = l.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')', '-', ' ', '\t']);
            if l.is_empty() || l.starts_with('#') || l.starts_with("http") {
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
                tracks.push(RemoteTrack { title, artist, duration_ms: None, explicit: None });
            }
        }
    }
    if tracks.is_empty() {
        return Err("couldn't find any songs — paste one per line, like \"Artist - Title\"".into());
    }
    tracks.truncate(MAX_ITEMS);
    Ok(RemoteList { name: "Imported songs".into(), tracks })
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
                explicit: t.get("explicit_lyrics").and_then(Value::as_bool),
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

/// Words that mark a different version of a song. A candidate with one the
/// wanted title doesn't have is never picked (same list as the desktop app).
const VERSION_WORDS: &[&str] = &[
    "live", "cover", "karaoke", "instrumental", "nightcore", "sped up", "speed up", "slowed", "reverb", "8d", "remix", "acoustic", "piano", "reaction", "tutorial", "lesson",
    "hour", "loop", "extended", "edit", "mashup", "bass boosted", "mix", "version", "demo", "concert", "tribute", "parody", "chipmunk", "lofi", "lo fi", "orchestral", "unplugged",
    "session", "rehearsal", "isolated", "acapella", "a cappella", "backing track", "minus one", "shorts", "snippet", "432hz", "528hz", "8 bit", "music box", "ringtone", "reverse",
    "reversed", "clean", "radio edit", "vip", "flip", "bootleg", "rework", "cut", "intro", "outro", "interlude", "skit", "chopped", "screwed",
];

fn lower_words(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has_word(hay: &str, w: &str) -> bool {
    format!(" {hay} ").contains(&format!(" {w} "))
}

/// Version words in `cand` that `want` doesn't have.
fn extra_versions(want: &str, cand: &str) -> bool {
    VERSION_WORDS.iter().any(|w| has_word(cand, w) && !has_word(want, w))
}

/// How well a YouTube Music song fits a wanted track, or None if it's a
/// different song, version or performer.
fn score(want: &RemoteTrack, c: &Track) -> Option<f64> {
    let raw_want = lower_words(&want.title);
    // Fan uploads of unreleased songs label them "(Unreleased)", "OG", "CDQ",
    // "Best Version"…: not a different version of the song, so not checked.
    let raw_cand = strip_upload_labels(&lower_words(&c.title));
    if extra_versions(&raw_want, &raw_cand) || TRIBUTE.iter().any(|w| has_word(&raw_cand, w) && !has_word(&raw_want, w)) {
        return None;
    }
    // Videos are often titled "Artist - Song (Official Video)": take the song part,
    // and count the artist named there.
    let mut title = c.title.as_str();
    let mut named_artist = false;
    if c.video {
        if let Some((a, t)) = c.title.split_once(" - ") {
            if !want.artist.is_empty() && artists_overlap(&want.artist, a) {
                title = t;
                named_artist = true;
            }
        }
    }
    let wt = clean_title(&want.title);
    let mut ct = strip_upload_labels(&clean_title(title));
    if wt.is_empty() || ct.is_empty() {
        return None;
    }
    let mut sim = title_sim(&wt, &ct);
    // "Someone Else - Song (Unreleased)": the part after the dash may be the song
    if c.video && !named_artist {
        if let Some((_, t)) = c.title.split_once(" - ") {
            let alt = strip_upload_labels(&clean_title(t));
            let alt_sim = title_sim(&wt, &alt);
            if alt_sim > sim {
                sim = alt_sim;
                ct = alt;
            }
        }
    }
    // Every wanted word is in the candidate ("Stay" vs "Stay (with Justin Bieber)" after cleaning).
    let contained = has_word(&ct, &wt);
    let dur = match (want.duration_ms, c.duration_ms) {
        (Some(a), Some(b)) => Some((a - b).abs()),
        _ => None,
    };
    // Album audio of the same recording is within a few seconds; more is another cut or song.
    if dur.is_some_and(|d| d > 20_000) {
        return None;
    }
    // Uploads (not album audio) must prove their length when the wanted length is known.
    if c.video && want.duration_ms.is_some() && dur.is_none() {
        return None;
    }
    let close = dur.is_some_and(|d| d <= 3000);
    if !(sim >= 0.8 || (contained && (close || dur.is_none()) && sim >= 0.5)) {
        return None;
    }
    // The performer must match: a same-named song by someone else (or a cover
    // band's copy with the same length) is the wrong song.
    // The wanted artist named anywhere in the upload counts too ("Dome (feat. Xirus)").
    let full = lower_words(&format!("{} {}", c.title, c.artist));
    let named_in_title = want.artist.split([',', '&']).map(lower_words).any(|a| a.len() > 2 && has_word(&full, &a));
    let artist_ok = want.artist.is_empty() || named_artist || named_in_title || artists_overlap(&want.artist, &c.artist);
    // Someone else's upload of an unreleased song (on Spotify under a made-up
    // name, on YouTube as "Real Artist - Song (Unreleased)"): only when it says
    // it's unreleased/leaked AND the title is word-for-word the same AND the
    // length agrees to 3 s. Length alone isn't enough: same-named songs often
    // share a length by chance.
    let labelled = ["unreleased", "leak", "leaked", "og", "cdq"].iter().any(|w| has_word(&lower_words(&c.title), w));
    let reupload = !artist_ok && labelled && ct == wt && dur.is_some_and(|d| d <= 3000);
    if !artist_ok && !reupload {
        return None;
    }
    let mut s = sim * 3.0 + if artist_ok { 2.0 } else { -0.5 };
    if let Some(d) = dur {
        s += if d <= 2000 { 1.5 } else if d <= 5000 { 0.8 } else { 0.0 } - d as f64 / 1000.0 / 8.0;
    }
    if c.video {
        s -= 1.0; // videos carry intros and skits; album audio first
        if !artists_overlap(&want.artist, &c.artist) {
            s -= 1.0; // someone else's upload (lyric channels)
        }
        if has_word(&raw_cand, "lyrics") || has_word(&raw_cand, "lyric") {
            s -= 0.3;
        }
    }
    if let Some(e) = want.explicit {
        if e == c.explicit {
            s += 0.6;
        } else if e && !c.explicit {
            s -= 0.6; // a clean edit of an explicit song
        }
    }
    Some(s)
}

/// Labels fan uploads add that don't change which recording it is.
const UPLOAD_LABELS: &[&str] = &["best version", "full song", "unreleased", "leaked", "leak", "og", "cdq", "hq", "hd", "official audio", "audio", "lyrics", "lyric video"];

fn strip_upload_labels(s: &str) -> String {
    let mut t = format!(" {s} ");
    for w in UPLOAD_LABELS {
        t = t.replace(&format!(" {w} "), " ");
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Phrases karaoke and tribute releases put in their titles.
const TRIBUTE: &[&str] = &["type beat", "made popular by", "originally performed by", "in the style of", "made famous by", "as performed by", "tribute to", "made popular", "originally by"];

/// The best YouTube Music song for a track from another service, if any is close enough.
fn pick(want: &RemoteTrack, candidates: &[Track]) -> Option<Track> {
    candidates
        .iter()
        .take(12)
        .enumerate()
        .filter_map(|(rank, c)| score(want, c).map(|s| (s - rank as f64 * 0.05, c)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c.clone())
}

/// The top result card of a search, as a track (it isn't repeated in the list
/// below it, and is often the one wanted). Subtitle: "Artist • Album • 3:20"
/// for a song, "Artist • 1.2B views • 3:35" for a video.
fn top_track(card: &yt::Card) -> Option<Track> {
    if card.kind != "song" {
        return None;
    }
    let parts: Vec<&str> = card.subtitle.split(" • ").collect();
    Some(Track {
        video_id: card.id.clone(),
        title: card.title.clone(),
        artist: parts.first().copied().unwrap_or("").to_string(),
        artist_id: None,
        album: None,
        album_id: None,
        duration_ms: parts.last().and_then(|p| yt::parse_len(p)),
        artwork: card.artwork.clone(),
        explicit: false,
        video: parts.iter().any(|p| p.ends_with(" views")),
    })
}

/// Find a track on YouTube Music: songs search first, then a broader search
/// with every artist (songs missing from the songs filter, official videos).
pub(crate) async fn find(want: &RemoteTrack) -> Option<Track> {
    let artists: Vec<&str> = want.artist.split([',', '&']).map(str::trim).filter(|a| !a.is_empty()).collect();
    let title = want.title.split(" - ").next().unwrap_or(&want.title).trim();
    let first = artists.first().copied().unwrap_or("");
    // Results vary between searches, so try the words both ways round before
    // settling for a video (some songs are only on YouTube as videos).
    let tries = [
        (format!("{title} {first}"), yt::SONGS),
        (format!("{first} {title}"), yt::SONGS),
        (format!("{first} {title}"), yt::VIDEOS),
        // the artist name may be one only Spotify knows (re-uploads, leaks)
        (title.to_string(), yt::VIDEOS),
        // unreleased songs on Spotify under a made-up name
        (format!("{title} unreleased"), yt::VIDEOS),
    ];
    for (q, filter) in tries {
        let Ok(r) = yt::search_filtered(&q, filter).await else { continue };
        let found: Vec<Track> = r.top.as_ref().and_then(top_track).into_iter().chain(r.songs).chain(r.videos).filter(|t| (filter == yt::VIDEOS) == t.video).collect();
        if let Some(t) = pick(want, &found) {
            return Some(t);
        }
    }
    None
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
    /// The service only showed part of a long playlist (Spotify shows 100).
    pub truncated: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    done: usize,
    total: usize,
    stage: &'static str,
}

pub async fn import(app: &AppHandle, input: &str) -> Result<Imported, String> {
    if input.len() > MAX_TEXT {
        return Err("that's too long to be a playlist".into());
    }
    // A pasted list of songs (several lines, or Spotify song links) rather than one playlist link.
    let ids = spotify_track_ids(input);
    let lines = input.lines().filter(|l| !l.trim().is_empty()).count();
    if !ids.is_empty() || (lines > 1 && parse_link(input).is_err()) {
        let _ = app.emit("import-progress", Progress { done: 0, total: ids.len(), stage: "reading" });
        let list = if ids.is_empty() { parse_text(input)? } else { fetch_spotify_tracks(app, ids).await? };
        return match_all(app, list, "List", String::new(), false).await;
    }
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
        return Ok(Imported { title: c.title, source: "YouTube Music".into(), url: link.page_url(), artwork: c.artwork, tracks: c.tracks, missing: vec![], truncated: false });
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
    // Spotify's public page lists at most 100 songs of a playlist.
    let truncated = matches!(link, Link::Spotify { kind: "playlist", .. }) && list.tracks.len() >= 100;
    match_all(app, list, link.source(), link.page_url(), truncated).await
}

/// Find every song of a list on YouTube Music, a few at a time.
async fn match_all(app: &AppHandle, list: RemoteList, source: &str, url: String, truncated: bool) -> Result<Imported, String> {
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
                let hit = find(&w).await;
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
    Ok(Imported { title: list.name, source: source.into(), url, artwork, tracks, missing, truncated })
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

    #[test]
    fn pasted_lists() {
        let ids = spotify_track_ids("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=1\nhttps://open.spotify.com/intl-de/track/7qiZfU4dY1lWllzX7mPBI3\n");
        assert_eq!(ids, vec!["4cOdK2wGLETKBW3PvgPWqT", "7qiZfU4dY1lWllzX7mPBI3"]);
        assert!(spotify_track_ids("The Beatles - Hey Jude\nAdele - Hello").is_empty());
        let l = parse_text("1. The Beatles - Hey Jude\nBlinding Lights by The Weeknd").unwrap();
        assert_eq!((l.tracks[0].artist.as_str(), l.tracks[0].title.as_str()), ("The Beatles", "Hey Jude"));
        assert_eq!((l.tracks[1].artist.as_str(), l.tracks[1].title.as_str()), ("The Weeknd", "Blinding Lights"));
        let c = parse_text("Track Name,Artist Name(s),Duration (ms)\n\"Hey Jude\",The Beatles,431333").unwrap();
        assert_eq!(c.tracks[0].duration_ms, Some(431333));
    }

    fn t(title: &str, artist: &str, secs: i64, video: bool) -> Track {
        Track { video_id: "x".into(), title: title.into(), artist: artist.into(), artist_id: None, album: None, album_id: None, duration_ms: Some(secs * 1000), artwork: None, explicit: false, video }
    }

    #[test]
    fn picks_the_right_song() {
        let w = RemoteTrack { title: "Stay".into(), artist: "The Kid LAROI, Justin Bieber".into(), duration_ms: Some(141_000), explicit: Some(true) };
        // Same artist, different song; a cover; a sped-up version; the real one.
        let c = vec![t("Stay With Me", "Justin Bieber", 150, false), t("Stay (Cover)", "Someone", 141, false), t("Stay (Sped Up)", "The Kid LAROI", 110, false), t("STAY", "The Kid LAROI & Justin Bieber", 142, false)];
        assert_eq!(pick(&w, &c).unwrap().title, "STAY");
        // Same title by another artist is never taken.
        let w2 = RemoteTrack { title: "Hello".into(), artist: "Adele".into(), duration_ms: Some(295_000), explicit: None };
        assert!(pick(&w2, &[t("Hello", "Lionel Richie", 250, false)]).is_none());
        // Album audio over the music video.
        let w3 = RemoteTrack { title: "Blinding Lights".into(), artist: "The Weeknd".into(), duration_ms: Some(200_000), explicit: None };
        assert_eq!(pick(&w3, &[t("Blinding Lights", "The Weeknd", 202, true), t("Blinding Lights", "The Weeknd", 200, false)]).unwrap().video, false);
    }

    #[test]
    fn reuploads_need_evidence() {
        let w = RemoteTrack { title: "Late Night Thoughts".into(), artist: "Divinity, Xcozse".into(), duration_ms: Some(217_000), explicit: None };
        // the real artist's unreleased upload, same title and length: yes
        assert!(pick(&w, &[t("Juice WRLD - Late Night Thoughts (Unreleased)", "Juicy999", 218, true)]).is_some());
        // same title and length but nothing says it's the same recording: no
        let f = RemoteTrack { title: "Friends".into(), artist: "Haerted".into(), duration_ms: Some(204_000), explicit: None };
        assert!(pick(&f, &[t("Marshmello & Anne-Marie - FRIENDS (Lyrics)", "7clouds", 203, true)]).is_none());
        // words in another order are another song
        let m = RemoteTrack { title: "Me & You".into(), artist: "Xirus".into(), duration_ms: Some(147_000), explicit: None };
        assert!(pick(&m, &[t("you & me (unreleased)", "Someone", 147, true)]).is_none());
        // the wanted artist named in the title counts as the artist
        let d = RemoteTrack { title: "Dome".into(), artist: "Xirus".into(), duration_ms: Some(166_000), explicit: None };
        assert!(pick(&d, &[t("Dome (feat. Xirus)", "mxnmxn", 168, false)]).is_some());
        // type beats are never the song
        let p = RemoteTrack { title: "Perky Conversations".into(), artist: "TopWRLD".into(), duration_ms: Some(181_000), explicit: None };
        assert!(pick(&p, &[t("[FREE] Juice WRLD Type Beat 2024 - \"Perky Conversations\"", "NGHT RYD", 179, true)]).is_none());
    }

    /// cargo test --lib live_match_report -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_match_report() {
        let id = std::env::var("PL").unwrap_or("37i9dQZF1DX5Ejj0EkURtP".into());
        let (_, html) = get_text(&format!("https://open.spotify.com/embed/playlist/{id}")).await.unwrap();
        let list = parse_spotify_embed(&html).unwrap();
        let n = list.tracks.len();
        let mut ok = 0;
        for chunk in list.tracks.chunks(5) {
            let mut set = tokio::task::JoinSet::new();
            for w in chunk.iter().cloned() {
                set.spawn(async move { let p = find(&w).await; (w, p) });
            }
            while let Some(Ok((w, p))) = set.join_next().await {
                let d = |x: Option<i64>| x.map(|m| format!("{}:{:02}", m / 60000, m / 1000 % 60)).unwrap_or("?".into());
                match &p {
                    Some(t) => { ok += 1; eprintln!("OK   {} — {} [{}]  =>  {} — {} [{}]{}", w.artist, w.title, d(w.duration_ms), t.artist, t.title, d(t.duration_ms), if t.video { " (video)" } else { "" }) }
                    None => eprintln!("MISS {} — {} [{}]", w.artist, w.title, d(w.duration_ms)),
                }
            }
        }
        eprintln!("{ok}/{n} found");
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



