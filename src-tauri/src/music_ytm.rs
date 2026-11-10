//! YouTube Music, read without signing in (same unofficial, opt-in streaming
//! as music_stream). Two things the regular YouTube search can't give us:
//!
//! - **Explicit flags.** Album uploads ("Artist - Topic") of a song often come
//!   in an explicit and a clean version with the same title and length. YouTube
//!   Music marks the explicit one, so NOCTURN can pick the version the user
//!   asked for (Customise › Music › Explicit songs, 18+ only).
//! - **Radio.** The "up next" mix YouTube Music builds from a song: similar
//!   songs, same genre and era. AI shuffle uses it to keep the music going.
//!
//! Requests go to music.youtube.com's own web API with a fixed body; only the
//! search text or an 11-character video ID come from NOCTURN.

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::music_import::{parse_len, walk};
use crate::music_stream::{valid_video_id, Candidate};

const API: &str = "https://music.youtube.com/youtubei/v1";
const CLIENT_VERSION: &str = "1.20251015.03.00";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
/// The "Songs" filter of YouTube Music search.
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D";
const MAX_BODY: usize = 6 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct YtmSong {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<i64>,
    pub explicit: bool,
}

impl YtmSong {
    /// As a search candidate for the matcher (album uploads are official audio).
    pub fn candidate(&self) -> Candidate {
        Candidate {
            video_id: self.video_id.clone(),
            title: self.title.clone(),
            channel: format!("{} - Topic", self.artist.split([',', '&']).next().unwrap_or(&self.artist).trim()),
            duration_ms: self.duration_ms,
            official: true,
            explicit: Some(self.explicit),
        }
    }
}

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::msg(format!("network setup failed: {e}")))
}

async fn post(endpoint: &str, mut body: Value) -> AppResult<Value> {
    body["context"] = json!({ "client": { "clientName": "WEB_REMIX", "clientVersion": CLIENT_VERSION, "hl": "en", "gl": "US" } });
    let res = client()?
        .post(format!("{API}/{endpoint}?prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/")
        .header("Cookie", "CONSENT=YES+1; SOCS=CAI")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::msg(format!("couldn't reach YouTube Music: {}", e.without_url())))?;
    let status = res.status();
    if status.as_u16() == 429 {
        return Err(AppError::msg("YouTube Music is limiting requests right now — try again in a few minutes"));
    }
    if !status.is_success() {
        return Err(AppError::msg(format!("YouTube Music answered {status}")));
    }
    let bytes = res.bytes().await.map_err(|e| AppError::msg(format!("YouTube Music download failed: {}", e.without_url())))?;
    if bytes.len() > MAX_BODY {
        return Err(AppError::msg("YouTube Music sent far more than expected"));
    }
    serde_json::from_slice(&bytes).map_err(|_| AppError::msg("YouTube Music's answer couldn't be read"))
}

fn runs_text(v: Option<&Value>) -> String {
    v.and_then(|t| t.get("runs")).and_then(Value::as_array).map(|r| r.iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect::<String>()).unwrap_or_default()
}

fn is_explicit(r: &Value) -> bool {
    r.get("badges").map(|b| b.to_string().contains("MUSIC_EXPLICIT_BADGE")).unwrap_or(false)
}

/// "Artist • Album • 4:51" (search) or "Artist • Album • 2002" (radio).
/// Returns (artist, album, duration) — or None for user videos
/// ("Channel • 18M views • 192K likes"), which aren't album songs.
fn split_byline(s: &str) -> Option<(String, String, Option<i64>)> {
    let mut parts: Vec<&str> = s.split(" • ").map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| p.ends_with(" views") || p.ends_with(" likes") || p.ends_with(" view")) {
        return None;
    }
    if parts.first().is_some_and(|p| matches!(*p, "Song" | "Video" | "Episode")) {
        parts.remove(0);
    }
    let artist = parts.first()?.to_string();
    let mut dur = None;
    if let Some(last) = parts.last() {
        if last.contains(':') {
            dur = parse_len(last);
        }
    }
    let rest: Vec<&str> = parts[1..].iter().copied().filter(|p| !p.contains(':') && !(p.len() == 4 && p.chars().all(|c| c.is_ascii_digit()))).collect();
    let album = rest.first().map(|s| s.to_string()).unwrap_or_default();
    Some((artist, album, dur))
}

pub fn parse_search(v: &Value) -> Vec<YtmSong> {
    let mut out = Vec::new();
    walk(v, &mut |k, x| {
        if k != "musicResponsiveListItemRenderer" {
            return false;
        }
        let vid = x.pointer("/playlistItemData/videoId").and_then(Value::as_str).unwrap_or("");
        let cols: Vec<String> = x
            .get("flexColumns")
            .and_then(Value::as_array)
            .map(|c| c.iter().map(|c| runs_text(c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))).collect())
            .unwrap_or_default();
        if valid_video_id(vid) && cols.len() >= 2 {
            if let Some((artist, album, dur)) = split_byline(&cols[1]) {
                out.push(YtmSong { video_id: vid.into(), title: cols[0].clone(), artist, album, duration_ms: dur, explicit: is_explicit(x) });
            }
        }
        true
    });
    out
}

pub fn parse_radio(v: &Value) -> Vec<YtmSong> {
    let mut out: Vec<YtmSong> = Vec::new();
    walk(v, &mut |k, x| {
        if k != "playlistPanelVideoRenderer" {
            return false;
        }
        let vid = x.get("videoId").and_then(Value::as_str).unwrap_or("");
        let title = runs_text(x.get("title"));
        let by = runs_text(x.get("longBylineText"));
        let len = runs_text(x.get("lengthText"));
        if valid_video_id(vid) && !title.is_empty() && !out.iter().any(|s| s.video_id == vid) {
            if let Some((artist, album, _)) = split_byline(&by) {
                out.push(YtmSong { video_id: vid.into(), title, artist, album, duration_ms: parse_len(&len), explicit: is_explicit(x) });
            }
        }
        true
    });
    out
}

/// Songs (album audio) for a search.
pub async fn search(query: &str) -> AppResult<Vec<YtmSong>> {
    let q: String = query.chars().take(150).collect();
    let v = post("search", json!({ "query": q, "params": urlencoding_decode(SONGS_FILTER) })).await?;
    Ok(parse_search(&v))
}

/// The radio YouTube Music builds from one song (the song itself first).
pub async fn radio(video_id: &str) -> AppResult<Vec<YtmSong>> {
    if !valid_video_id(video_id) {
        return Err(AppError::Invalid("not a YouTube video ID".into()));
    }
    let v = post("next", json!({ "videoId": video_id, "playlistId": format!("RDAMVM{video_id}"), "isAudioOnly": true })).await?;
    Ok(parse_radio(&v))
}

fn urlencoding_decode(s: &str) -> String {
    s.replace("%3D", "=")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bylines() {
        assert_eq!(split_byline("Eminem • The Eminem Show • 4:51"), Some(("Eminem".into(), "The Eminem Show".into(), Some(291_000))));
        assert_eq!(split_byline("Eminem • The Eminem Show • 2002"), Some(("Eminem".into(), "The Eminem Show".into(), None)));
        assert_eq!(split_byline("Song • Halsey • Manic • 3:22"), Some(("Halsey".into(), "Manic".into(), Some(202_000))));
        assert_eq!(split_byline("VIBE4U2 • 18M views • 192K likes"), None);
    }

    #[test]
    fn parses_search_and_radio_shapes() {
        let search = json!({"contents":[{"musicResponsiveListItemRenderer":{
            "playlistItemData":{"videoId":"tqxRidAWER8"},
            "badges":[{"musicInlineBadgeRenderer":{"icon":{"iconType":"MUSIC_EXPLICIT_BADGE"}}}],
            "flexColumns":[
                {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Without Me"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer":{"text":{"runs":[{"text":"Eminem"},{"text":" • "},{"text":"The Eminem Show"},{"text":" • "},{"text":"4:51"}]}}}
            ]}}]});
        let s = parse_search(&search);
        assert_eq!(s.len(), 1);
        assert!(s[0].explicit);
        assert_eq!(s[0].artist, "Eminem");
        assert_eq!(s[0].candidate().explicit, Some(true));

        let radio = json!({"x":[
            {"playlistPanelVideoRenderer":{"videoId":"-5qqrMu_AZc","title":{"runs":[{"text":"Gangsta's Paradise"}]},"longBylineText":{"runs":[{"text":"Coolio & L.V. • Gangsta's Paradise • 1995"}]},"lengthText":{"runs":[{"text":"4:01"}]}}},
            {"playlistPanelVideoRenderer":{"videoId":"36ydFET_1SE","title":{"runs":[{"text":"Some remix"}]},"longBylineText":{"runs":[{"text":"VIBE4U2 • 18M views • 192K likes"}]},"lengthText":{"runs":[{"text":"3:20"}]}}}
        ]});
        let r = parse_radio(&radio);
        assert_eq!(r.len(), 1, "user videos are skipped");
        assert_eq!(r[0].duration_ms, Some(241_000));
        assert!(!r[0].explicit);
    }

    #[tokio::test]
    #[ignore]
    async fn live_ytm() {
        let s = search("Eminem Without Me").await.unwrap();
        eprintln!("search: {:#?}", &s[..s.len().min(3)]);
        assert!(s.iter().any(|x| x.explicit));
        let r = radio(&s[0].video_id).await.unwrap();
        eprintln!("radio: {} songs, first {:?}", r.len(), r.iter().take(5).map(|x| format!("{} — {}", x.artist, x.title)).collect::<Vec<_>>());
        assert!(r.len() >= 10);
    }
}
