//! Synced lyrics from LRCLIB (lrclib.net), a free, open lyrics database.
//! Sends artist, title, album and length — nothing else.
//!
//! Same behaviour as the desktop app (src-tauri/src/lyrics_online.rs):
//! enhanced-LRC word timings are kept, and a slowed / sped-up version of a
//! song gets the original's timings stretched to fit ("≈ approximately
//! synced"). The result has the desktop's shape so the phone can reuse the
//! desktop's lyrics timing code (src/music/lyricsTiming.ts) unchanged.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::textmatch::{artists_overlap, clean_title, title_sim};

const API: &str = "https://lrclib.net/api";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub text: String,
    /// Synced lines (ms, text).
    pub synced: Vec<(i64, String)>,
    /// Per-word timings for each synced line (empty when the source has none).
    pub words: Vec<Vec<(i64, String)>>,
    /// Timings stretched from another version of the song (slowed/sped up).
    pub approx: bool,
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
    #[serde(skip, default)]
    approx: bool,
}

fn nonempty(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| !s.trim().is_empty())
}

impl Hit {
    fn has_lyrics(&self) -> bool {
        self.instrumental || nonempty(&self.synced) || nonempty(&self.plain)
    }
}

// ---------------------------------------------------------------- LRC

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedLrc {
    pub lines: Vec<(i64, String)>,
    pub words: Vec<Vec<(i64, String)>>,
}

fn stamp_ms(tag: &str) -> Option<i64> {
    let (m, sec) = tag.split_once(':')?;
    let (m, sec) = (m.trim().parse::<i64>().ok()?, sec.trim().parse::<f64>().ok()?);
    (m >= 0 && sec >= 0.0).then(|| m * 60_000 + (sec * 1000.0).round() as i64)
}

/// LRC, including enhanced word timings (`<mm:ss.xx>word`) and `[offset:±ms]`.
pub fn parse_lrc_full(s: &str) -> ParsedLrc {
    let mut offset = 0i64;
    type Row = (i64, String, Vec<(i64, String)>);
    let mut rows: Vec<Row> = Vec::new();
    for line in s.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some(i) = r.find(']') else { break };
            let tag = &r[..i];
            if let Some(v) = tag.strip_prefix("offset:") {
                offset = v.trim().trim_start_matches('+').parse().unwrap_or(0);
            } else if let Some(t) = stamp_ms(tag) {
                stamps.push(t);
            }
            rest = r[i + 1..].trim_start();
        }
        if stamps.is_empty() {
            continue;
        }
        let mut words = Vec::new();
        let mut text = String::new();
        let mut cur = rest;
        let mut pending: Option<i64> = None;
        while !cur.is_empty() {
            if let Some(r) = cur.strip_prefix('<') {
                if let Some(i) = r.find('>') {
                    if let Some(t) = stamp_ms(&r[..i]) {
                        pending = Some(t);
                        cur = &r[i + 1..];
                        continue;
                    }
                }
            }
            let next = cur.find('<').filter(|&i| i > 0).unwrap_or(cur.len());
            let seg = &cur[..next];
            text.push_str(seg);
            if let Some(t) = pending.take() {
                if !seg.trim().is_empty() {
                    words.push((t, seg.to_string()));
                }
            } else if let Some(last) = words.last_mut() {
                last.1.push_str(seg);
            }
            cur = &cur[next..];
        }
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for t in stamps {
            rows.push((t, text.clone(), words.clone()));
        }
    }
    rows.sort_by_key(|x| x.0);
    let shift = |t: i64| (t - offset).max(0);
    ParsedLrc {
        lines: rows.iter().map(|r| (shift(r.0), r.1.clone())).collect(),
        words: rows.iter().map(|r| r.2.iter().map(|(t, w)| (shift(*t), w.clone())).collect()).collect(),
    }
}

fn stretch(p: &mut ParsedLrc, ratio: f64) {
    let k = |t: i64| (t as f64 * ratio).round() as i64;
    for l in &mut p.lines {
        l.0 = k(l.0);
    }
    for ws in &mut p.words {
        for w in ws {
            w.0 = k(w.0);
        }
    }
}

// ---------------------------------------------------------------- picking a hit

/// Words that mark a re-edit of a song: same words, different timing.
const VARIANT: &[&str] = &["slowed", "sped up", "speed up", "nightcore", "reverb", "hardstyle", "hardtekk", "remix", "edit", "bass boosted", "phonk", "8d", "daycore", "super slowed", "extended"];
const TEMPO_SLOWER: &[&str] = &["slowed", "super slowed", "daycore"];
const TEMPO_FASTER: &[&str] = &["sped up", "speed up", "nightcore"];
const TIMING_SAFE: &[&str] = &["reverb", "8d", "bass boosted"];

fn variants(title: &str) -> Vec<&'static str> {
    let t = format!(" {} ", clean_title(title));
    let mut v: Vec<&str> = VARIANT.iter().copied().filter(|w| t.contains(&format!(" {w} "))).collect();
    v.sort_unstable();
    v
}

fn without_variants(title: &str) -> String {
    let mut t = format!(" {} ", clean_title(title));
    for w in VARIANT {
        t = t.replace(&format!(" {w} "), " ");
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Use {
    /// Same version, same length (±3 s): timings fit.
    Synced,
    /// A slowed / sped-up version of this recording: stretched timings fit.
    Stretched,
    /// Same words, different version or length: show words without timings.
    PlainOnly,
}

struct Query<'a> {
    title: &'a str,
    artist: &'a str,
    duration_ms: i64,
}

fn stretch_ratio(q: &Query, h: &Hit, want_v: &[&str], hit_v: &[&str]) -> Option<f64> {
    let hd = h.duration.filter(|d| *d > 20.0)? * 1000.0;
    if q.duration_ms <= 20_000 || !hit_v.is_empty() || want_v.is_empty() {
        return None;
    }
    let all = |list: &[&str]| want_v.iter().all(|w| list.contains(w) || TIMING_SAFE.contains(w));
    let ratio = q.duration_ms as f64 / hd;
    let slower = want_v.iter().any(|w| TEMPO_SLOWER.contains(w));
    let faster = want_v.iter().any(|w| TEMPO_FASTER.contains(w));
    let ok = match (slower, faster) {
        (true, false) => all(TEMPO_SLOWER) && (1.03..=1.8).contains(&ratio),
        (false, true) => all(TEMPO_FASTER) && (0.55..=0.97).contains(&ratio),
        (false, false) => (0.98..=1.02).contains(&ratio),
        _ => false,
    };
    ok.then_some(ratio)
}

fn usable(q: &Query, h: &Hit) -> Option<Use> {
    let sim = title_sim(&without_variants(q.title), &without_variants(&h.track_name));
    if sim < 0.9 || !artists_overlap(q.artist, &h.artist_name) {
        return None;
    }
    let (wv, hv) = (variants(q.title), variants(&h.track_name));
    if wv != hv {
        return Some(if stretch_ratio(q, h, &wv, &hv).is_some() { Use::Stretched } else { Use::PlainOnly });
    }
    match (q.duration_ms, h.duration) {
        (d, Some(hd)) if d > 0 => {
            let diff = ((hd * 1000.0) as i64 - d).abs();
            if diff <= 3000 {
                Some(Use::Synced)
            } else if diff <= 15_000 {
                Some(Use::PlainOnly)
            } else {
                None
            }
        }
        _ => Some(Use::PlainOnly),
    }
}

fn best(q: &Query, hits: Vec<Hit>) -> Option<Hit> {
    let mut ok: Vec<(Use, Hit)> = hits.into_iter().filter(Hit::has_lyrics).filter_map(|h| usable(q, &h).map(|u| (u, h))).collect();
    ok.sort_by_key(|(u, h)| {
        let diff = h.duration.map(|d| ((d * 1000.0) as i64 - q.duration_ms).abs()).unwrap_or(60_000);
        (*u, !nonempty(&h.synced), diff)
    });
    let (u, mut h) = ok.into_iter().next()?;
    if u == Use::Stretched {
        let (wv, hv) = (variants(q.title), variants(&h.track_name));
        if let (Some(r), true) = (stretch_ratio(q, &h, &wv, &hv), nonempty(&h.synced)) {
            let mut p = parse_lrc_full(h.synced.as_deref().unwrap_or(""));
            stretch(&mut p, r);
            h.synced = Some(to_lrc(&p));
            h.approx = true;
            return Some(h);
        }
    }
    if u != Use::Synced {
        if !nonempty(&h.plain) {
            h.plain = h.synced.as_deref().map(|s| parse_lrc_full(s).lines.into_iter().map(|x| x.1).collect::<Vec<_>>().join("\n"));
        }
        h.synced = None;
    }
    Some(h)
}

fn to_lrc(p: &ParsedLrc) -> String {
    let st = |t: i64| format!("{:02}:{:02}.{:02}", t / 60_000, (t / 1000) % 60, (t % 1000) / 10);
    p.lines
        .iter()
        .enumerate()
        .map(|(i, (t, text))| match p.words.get(i).filter(|w| !w.is_empty()) {
            Some(ws) => format!("[{}]{}", st(*t), ws.iter().map(|(wt, w)| format!("<{}>{}", st(*wt), w)).collect::<String>()),
            None => format!("[{}]{}", st(*t), text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn to_lyrics(h: &Hit) -> Lyrics {
    if h.instrumental {
        return Lyrics { text: "♪ Instrumental".into(), synced: vec![], words: vec![], approx: false, instrumental: true };
    }
    if nonempty(&h.synced) {
        let p = parse_lrc_full(h.synced.as_deref().unwrap_or(""));
        let words = if p.words.iter().any(|w| !w.is_empty()) { p.words.clone() } else { vec![] };
        let text = p.lines.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join("\n");
        return Lyrics { text, synced: p.lines, words, approx: h.approx, instrumental: false };
    }
    Lyrics { text: h.plain.clone().unwrap_or_default(), synced: vec![], words: vec![], approx: false, instrumental: false }
}

// ---------------------------------------------------------------- network

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
    a.split([',', '&', ';']).next().unwrap_or(a).trim()
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

/// Exact lookup (with and without the album, with the cleaned title), then a
/// search filtered by title, artist and length.
pub async fn get(artist: &str, title: &str, album: Option<&str>, duration_ms: Option<i64>) -> Result<Option<Lyrics>, String> {
    let artist_first = first_artist(artist);
    if artist_first.is_empty() || title.trim().is_empty() {
        return Ok(None);
    }
    let q = Query { title, artist, duration_ms: duration_ms.unwrap_or(0) };
    let secs = (q.duration_ms as f64 / 1000.0).round() as i64;
    let mut titles = vec![title.trim().to_string()];
    let base = base_title(title);
    if !base.is_empty() && base != titles[0] {
        titles.push(base);
    }
    // Exact lookups only for the original version (a slowed edit's own entry rarely exists).
    if secs > 0 && variants(title).is_empty() {
        for (i, t) in titles.iter().enumerate() {
            let mut url = format!("{API}/get?artist_name={}&track_name={}&duration={secs}", enc(artist_first), enc(t));
            if let Some(a) = album.filter(|a| !a.trim().is_empty() && i == 0) {
                url.push_str(&format!("&album_name={}", enc(a.trim())));
            }
            if let Some(h) = fetch_json::<Hit>(&url).await? {
                if h.has_lyrics() {
                    return Ok(Some(to_lyrics(&h)));
                }
            }
        }
    }
    let mut search_titles = titles.clone();
    let plain = without_variants(title);
    if !plain.is_empty() && !search_titles.iter().any(|t| clean_title(t) == plain) {
        search_titles.push(plain);
    }
    for t in &search_titles {
        let url = format!("{API}/search?track_name={}&artist_name={}", enc(t), enc(artist_first));
        if let Some(hits) = fetch_json::<Vec<Hit>>(&url).await? {
            if let Some(h) = best(&q, hits) {
                return Ok(Some(to_lyrics(&h)));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_and_words() {
        let p = parse_lrc_full("[ar: x]\n[00:01.50] Hello\n[00:03.00][00:10.00] Again\n[00:05.00]<00:05.00>Hey <00:05.50>Jude");
        assert_eq!(p.lines.len(), 4);
        assert_eq!(p.lines[0], (1500, "Hello".into()));
        assert_eq!(p.words[2], vec![(5000, "Hey ".into()), (5500, "Jude".into())]);
        assert_eq!(base_title("Hey Jude (Remastered 2015)"), "Hey Jude");
        assert_eq!(variants("Stay (Slowed + Reverb)"), vec!["reverb", "slowed"]);
    }

    #[test]
    fn slowed_versions_are_stretched() {
        let q = Query { title: "Blinding Lights (Slowed)", artist: "The Weeknd", duration_ms: 240_000 };
        let hit = Hit { track_name: "Blinding Lights".into(), artist_name: "The Weeknd".into(), duration: Some(200.0), instrumental: false, plain: None, synced: Some("[00:10.00]Yeah\n[00:20.00]I've been".into()), approx: false };
        let h = best(&q, vec![hit]).unwrap();
        assert!(h.approx);
        let l = to_lyrics(&h);
        assert_eq!(l.synced[0].0, 12_000);
    }

    #[tokio::test]
    #[ignore]
    async fn live_lyrics() {
        let l = get("The Weeknd", "Blinding Lights", Some("After Hours"), Some(200_000)).await.unwrap().unwrap();
        eprintln!("{} synced lines, first {:?}", l.synced.len(), l.synced.get(1));
        assert!(l.synced.len() > 20);
    }
}
