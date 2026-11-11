//! Synced lyrics from LRCLIB (lrclib.net), a free, open lyrics database.
//! Sends artist, title, album and length — nothing else.

use std::time::Duration;

use serde::{Deserialize, Serialize};

const API: &str = "https://lrclib.net/api";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub ms: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    /// Time-synced lines (empty when only plain words were found).
    pub lines: Vec<Line>,
    pub plain: String,
    pub instrumental: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Hit {
    #[serde(rename = "trackName", default)]
    track_name: String,
    #[serde(rename = "artistName", default)]
    artist_name: String,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    #[serde(rename = "plainLyrics", default)]
    plain: Option<String>,
    #[serde(rename = "syncedLyrics", default)]
    synced: Option<String>,
}

fn nonempty(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| !s.trim().is_empty())
}

/// "[01:02.34] words" → (62340 ms, "words"); several stamps on one line are all kept.
pub fn parse_lrc(s: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in s.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some(end) = r.find(']') else { break };
            let tag = &r[..end];
            rest = r[end + 1..].trim_start();
            let (m, sec) = match tag.split_once(':') {
                Some(x) => x,
                None => continue,
            };
            if let (Ok(m), Ok(sec)) = (m.trim().parse::<f64>(), sec.trim().parse::<f64>()) {
                stamps.push((m * 60.0 + sec) * 1000.0);
            }
        }
        for ms in stamps {
            out.push(Line { ms, text: rest.to_string() });
        }
    }
    out.sort_by(|a, b| a.ms.total_cmp(&b.ms));
    out
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn first_artist(a: &str) -> &str {
    a.split([',', '&', ';']).next().unwrap_or(a).split(" feat").next().unwrap_or(a).trim()
}

/// "Hey Jude (Remastered 2015)" → "Hey Jude"; "Song - Live" → "Song".
fn base_title(t: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split(" - ").next().unwrap_or("").trim().to_string()
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn client() -> Result<reqwest::Client, String> {
    // LRCLIB asks clients to identify themselves.
    reqwest::Client::builder()
        .user_agent(concat!("Nocturn Music iOS v", env!("CARGO_PKG_VERSION"), " (https://github.com/freighter12/NOCTURN)"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("network setup failed: {e}"))
}

async fn fetch_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<Option<T>, String> {
    let res = client()?.get(url).send().await.map_err(|e| format!("couldn't reach LRCLIB: {}", e.without_url()))?;
    match res.status().as_u16() {
        404 => Ok(None),
        200 => res.json::<T>().await.map(Some).map_err(|_| "LRCLIB sent something unexpected".into()),
        s => Err(format!("LRCLIB answered {s}")),
    }
}

fn to_lyrics(h: &Hit, synced_ok: bool) -> Lyrics {
    if h.instrumental {
        return Lyrics { lines: vec![], plain: "♪ Instrumental".into(), instrumental: true };
    }
    let lines = if synced_ok && nonempty(&h.synced) { parse_lrc(h.synced.as_deref().unwrap_or("")) } else { vec![] };
    let plain = if nonempty(&h.plain) {
        h.plain.clone().unwrap_or_default()
    } else {
        parse_lrc(h.synced.as_deref().unwrap_or("")).into_iter().map(|l| l.text).collect::<Vec<_>>().join("\n")
    };
    Lyrics { lines, plain, instrumental: false }
}

pub async fn get(artist: &str, title: &str, album: Option<&str>, duration_ms: Option<i64>) -> Result<Option<Lyrics>, String> {
    let artist = first_artist(artist);
    if artist.is_empty() || title.trim().is_empty() {
        return Ok(None);
    }
    let secs = duration_ms.map(|d| (d as f64 / 1000.0).round() as i64).unwrap_or(0);
    let mut titles = vec![title.trim().to_string()];
    let base = base_title(title);
    if !base.is_empty() && base != titles[0] {
        titles.push(base);
    }
    if secs > 0 {
        for (i, t) in titles.iter().enumerate() {
            let mut url = format!("{API}/get?artist_name={}&track_name={}&duration={secs}", enc(artist), enc(t));
            if let Some(a) = album.filter(|a| !a.trim().is_empty() && i == 0) {
                url.push_str(&format!("&album_name={}", enc(a.trim())));
            }
            if let Some(h) = fetch_json::<Hit>(&url).await? {
                if h.instrumental || nonempty(&h.synced) || nonempty(&h.plain) {
                    return Ok(Some(to_lyrics(&h, true)));
                }
            }
        }
    }
    let want_artist = norm(artist);
    for t in &titles {
        let url = format!("{API}/search?track_name={}&artist_name={}", enc(t), enc(artist));
        let Some(hits) = fetch_json::<Vec<Hit>>(&url).await? else { continue };
        let want_title = norm(&base_title(t));
        let mut ok: Vec<&Hit> = hits
            .iter()
            .filter(|h| h.instrumental || nonempty(&h.synced) || nonempty(&h.plain))
            .filter(|h| norm(&base_title(&h.track_name)) == want_title && norm(&h.artist_name).contains(&want_artist))
            .collect();
        let diff = |h: &Hit| h.duration.map(|d| ((d - secs as f64).abs() * 1000.0) as i64).unwrap_or(60_000);
        ok.sort_by_key(|h| (!nonempty(&h.synced), diff(h)));
        if let Some(h) = ok.first() {
            // timings only line up when the lengths match
            let synced_ok = secs == 0 || diff(h) <= 3000;
            return Ok(Some(to_lyrics(h, synced_ok)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc() {
        let l = parse_lrc("[ar: x]\n[00:01.50] Hello\n[00:03.00][00:10.00] Again\n[00:05.25]");
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], Line { ms: 1500.0, text: "Hello".into() });
        assert_eq!(l[2].ms, 5250.0);
        assert_eq!(l[3].text, "Again");
        assert_eq!(base_title("Hey Jude (Remastered 2015)"), "Hey Jude");
    }

    #[tokio::test]
    #[ignore]
    async fn live_lyrics() {
        let l = get("The Weeknd", "Blinding Lights", Some("After Hours"), Some(200_000)).await.unwrap().unwrap();
        eprintln!("{} synced lines, first {:?}", l.lines.len(), l.lines.get(1));
        assert!(l.lines.len() > 20);
    }
}
