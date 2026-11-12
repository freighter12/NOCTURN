//! Lyrics from LRCLIB (lrclib.net), a free, open lyrics database with
//! time-synced lyrics. Used only when a song has no lyrics of its own (an
//! .lrc file next to it or lyrics stored in the file), and only if online
//! lyrics are on. Sends artist, title, album and length — nothing else.
//! Results (including "not found") are cached so each song is asked once.

use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde::Deserialize;

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::music::{parse_lrc, stretch_lrc, Lyrics};
use crate::music_import::{artist_set, clean_title, title_sim};

const API: &str = "https://lrclib.net/api";
/// Ask again after this long when nothing was found (lyrics get added over time).
const RETRY_MISSES_DAYS: i64 = 7;

#[derive(Debug, Clone)]
pub struct Query {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
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
    /// Timings stretched from another version (set by NOCTURN, not LRCLIB).
    #[serde(skip, default)]
    approx: bool,
}

impl Hit {
    fn has_lyrics(&self) -> bool {
        self.instrumental || self.synced.as_deref().is_some_and(|s| !s.trim().is_empty()) || self.plain.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

/// Cache key: first artist + cleaned title + length to the nearest 2 s, so a
/// rescan or a re-import of the same song reuses the answer.
pub fn key(q: &Query) -> String {
    let artist = q.artist.split([',', '&', ';']).next().unwrap_or("").trim().to_lowercase();
    format!("{artist}|{}|{}", clean_title(&q.title), (q.duration_ms + 1000) / 2000)
}

fn first_artist(a: &str) -> &str {
    a.split([',', '&', ';']).next().unwrap_or(a).trim()
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

fn client() -> AppResult<reqwest::Client> {
    // LRCLIB asks clients to identify themselves.
    reqwest::Client::builder()
        .user_agent(concat!("NOCTURN v", env!("CARGO_PKG_VERSION"), " (https://github.com/freighter12/NOCTURN)"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::msg(format!("network setup failed: {e}")))
}

async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> AppResult<Option<T>> {
    for attempt in 0..2 {
        let res = client()?.get(url).send().await.map_err(|e| AppError::msg(format!("couldn't reach LRCLIB: {}", e.without_url())))?;
        match res.status().as_u16() {
            404 => return Ok(None),
            200 => return Ok(Some(res.json::<T>().await.map_err(|_| AppError::msg("LRCLIB sent something unexpected"))?)),
            // Too many requests: wait as asked (at most 3 s) and try once more.
            429 if attempt == 0 => {
                let wait = res.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(2).clamp(1, 3);
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            s => return Err(AppError::msg(format!("LRCLIB answered {s}"))),
        }
    }
    Err(AppError::msg("LRCLIB is busy — try again in a moment"))
}

/// Words that mark a re-edit of a song: same words, different timing.
const VARIANT: &[&str] = &["slowed", "sped up", "speed up", "nightcore", "reverb", "hardstyle", "hardtekk", "remix", "edit", "bass boosted", "phonk", "8d", "daycore", "super slowed", "extended"];

fn variants(title: &str) -> Vec<&'static str> {
    let t = format!(" {} ", clean_title(title));
    VARIANT.iter().copied().filter(|w| t.contains(&format!(" {w} "))).collect()
}

fn without_variants(title: &str) -> String {
    let mut t = format!(" {} ", clean_title(title));
    for w in VARIANT {
        t = t.replace(&format!(" {w} "), " ");
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How a search hit can be used for the song we want.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Use {
    /// Same version, same length (±3 s): timings fit.
    Synced,
    /// A slowed / sped-up version of this exact recording: the original's
    /// timings × (our length / its length) fit closely.
    Stretched,
    /// Same words, but a different version (slowed, sped up, an edit…) or a
    /// few seconds off: show the words without timings.
    PlainOnly,
}

fn usable(q: &Query, h: &Hit) -> Option<Use> {
    let sim = title_sim(&without_variants(&q.title), &without_variants(&h.track_name));
    let wa = artist_set(&q.artist);
    let ha = artist_set(&h.artist_name);
    let artist_ok = wa.is_empty() || wa.iter().any(|a| ha.iter().any(|b| a == b || (a.len() > 3 && b.contains(a.as_str())) || (b.len() > 3 && a.contains(b.as_str()))));
    if sim < 0.9 || !artist_ok {
        return None;
    }
    let mut wv = variants(&q.title);
    let mut hv = variants(&h.track_name);
    wv.sort_unstable();
    hv.sort_unstable();
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

/// Speed-only edits: same recording played slower or faster (or with effects
/// that don't change timing).
const TEMPO_SLOWER: &[&str] = &["slowed", "super slowed", "daycore"];
const TEMPO_FASTER: &[&str] = &["sped up", "speed up", "nightcore"];
const TIMING_SAFE: &[&str] = &["reverb", "8d", "bass boosted"];

/// If our song is a speed-only edit of the hit (and the lengths agree with
/// that), the factor to stretch the hit's timings by.
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
        (false, false) => (0.98..=1.02).contains(&ratio), // reverb/8D only: same timing
        _ => false,
    };
    ok.then_some(ratio)
}

/// The best search hit, adjusted to how it can be used: synced lyrics for the
/// same version and length, otherwise the plain words.
fn best(q: &Query, hits: Vec<Hit>) -> Option<Hit> {
    let mut ok: Vec<(Use, Hit)> = hits.into_iter().filter(|h| h.has_lyrics()).filter_map(|h| usable(q, &h).map(|u| (u, h))).collect();
    ok.sort_by_key(|(u, h)| {
        let synced = h.synced.as_deref().is_some_and(|s| !s.trim().is_empty());
        let diff = h.duration.map(|d| ((d * 1000.0) as i64 - q.duration_ms).abs()).unwrap_or(60_000);
        (*u, !synced, diff)
    });
    let (u, mut h) = ok.into_iter().next()?;
    if u == Use::Stretched {
        let mut wv = variants(&q.title);
        let mut hv = variants(&h.track_name);
        wv.sort_unstable();
        hv.sort_unstable();
        if let (Some(r), Some(s)) = (stretch_ratio(q, &h, &wv, &hv), h.synced.as_deref().filter(|s| !s.trim().is_empty())) {
            h.synced = Some(stretch_lrc(s, r));
            h.approx = true;
            return Some(h);
        }
    }
    if u != Use::Synced {
        // Keep the words, drop timings that wouldn't line up.
        if h.plain.as_deref().is_none_or(|p| p.trim().is_empty()) {
            h.plain = h.synced.as_deref().map(|s| parse_lrc(s).into_iter().map(|x| x.1).collect::<Vec<_>>().join("\n"));
        }
        h.synced = None;
    }
    Some(h)
}

/// Ask LRCLIB: exact lookup (with and without the album, with the cleaned
/// title), then a search filtered by title, artist and length.
async fn fetch(q: &Query) -> AppResult<Option<Hit>> {
    let artist = first_artist(&q.artist);
    if artist.is_empty() || q.title.trim().is_empty() {
        return Ok(None);
    }
    let secs = (q.duration_ms as f64 / 1000.0).round() as i64;
    let mut titles = vec![q.title.trim().to_string()];
    // "Hey Jude (Remastered 2015)" → also try "Hey Jude".
    let base = {
        let mut out = String::new();
        let mut depth = 0;
        for c in q.title.chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' if depth > 0 => depth -= 1,
                _ if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out.split(" - ").next().unwrap_or("").trim().to_string()
    };
    if !base.is_empty() && base != titles[0] {
        titles.push(base);
    }
    let has_sync = |h: &Hit| h.synced.as_deref().is_some_and(|s| !s.trim().is_empty());
    // An exact entry with words but no timings is only a fallback: another entry
    // for the same song often has them, so the search still runs.
    let mut unsynced: Option<Hit> = None;
    if secs > 0 {
        for t in &titles {
            let mut url = format!("{API}/get?artist_name={}&track_name={}&duration={secs}", enc(artist), enc(t));
            if !q.album.trim().is_empty() && t == &titles[0] {
                url.push_str(&format!("&album_name={}", enc(q.album.trim())));
            }
            if let Some(h) = get_json::<Hit>(&url).await? {
                if h.instrumental || has_sync(&h) {
                    return Ok(Some(h));
                }
                if h.has_lyrics() && unsynced.is_none() {
                    unsynced = Some(h);
                }
            }
        }
    }
    for t in &titles {
        let url = format!("{API}/search?track_name={}&artist_name={}", enc(t), enc(artist));
        if let Some(hits) = get_json::<Vec<Hit>>(&url).await? {
            if let Some(h) = best(q, hits) {
                if unsynced.is_none() || h.instrumental || has_sync(&h) {
                    return Ok(Some(h));
                }
            }
        }
    }
    Ok(unsynced)
}

fn to_lyrics(h: &Hit) -> Option<Lyrics> {
    if h.instrumental {
        return Some(Lyrics::plain("♪ Instrumental".into(), "LRCLIB"));
    }
    if let Some(s) = h.synced.as_deref().filter(|s| !s.trim().is_empty()) {
        return Some(Lyrics::from_lrc(s, "LRCLIB", h.approx));
    }
    h.plain.as_deref().filter(|s| !s.trim().is_empty()).map(|p| Lyrics::plain(p.to_string(), "LRCLIB"))
}

/// found, synced, plain, instrumental, fetched_at, approx
type CacheRow = (i64, Option<String>, Option<String>, i64, String, i64);

/// Cached answer: Some(Some) found, Some(None) recently not found, None = ask.
fn cached(db: &Db, k: &str) -> Option<Option<Lyrics>> {
    let row: Option<CacheRow> = db
        .conn()
        .query_row("SELECT found, synced, plain, instrumental, fetched_at, approx FROM music_lyrics_cache WHERE key = ?1", [k], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .optional()
        .ok()
        .flatten();
    let (found, synced, plain, instrumental, at, approx) = row?;
    if found == 0 {
        let fresh = chrono::DateTime::parse_from_rfc3339(&at).map(|t| chrono::Utc::now().signed_duration_since(t) < chrono::Duration::days(RETRY_MISSES_DAYS)).unwrap_or(false);
        return fresh.then_some(None);
    }
    Some(to_lyrics(&Hit { track_name: String::new(), artist_name: String::new(), duration: None, instrumental: instrumental != 0, plain, synced, approx: approx != 0 }))
}

fn store(db: &Db, k: &str, h: Option<&Hit>) -> AppResult<()> {
    db.conn().execute(
        "INSERT INTO music_lyrics_cache(key, found, synced, plain, instrumental, fetched_at, approx) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(key) DO UPDATE SET found = excluded.found, synced = excluded.synced, plain = excluded.plain, instrumental = excluded.instrumental, fetched_at = excluded.fetched_at, approx = excluded.approx",
        params![k, h.is_some() as i64, h.and_then(|h| h.synced.clone()), h.and_then(|h| h.plain.clone()), h.map(|h| h.instrumental as i64).unwrap_or(0), now(), h.map(|h| h.approx as i64).unwrap_or(0)],
    )?;
    Ok(())
}

/// The track's details for a lookup.
pub fn query_for_track(db: &Db, id: i64) -> AppResult<Option<Query>> {
    Ok(db
        .conn()
        .query_row("SELECT title, artist, album, duration_ms FROM music_tracks WHERE id = ?1", [id], |r| Ok(Query { title: r.get(0)?, artist: r.get(1)?, album: r.get(2)?, duration_ms: r.get(3)? }))
        .optional()?)
}

/// Lyrics from LRCLIB for a track (cached). Network trouble isn't cached, so it's retried next time.
pub async fn lyrics(db: &Db, id: i64) -> AppResult<Option<Lyrics>> {
    let Some(q) = query_for_track(db, id)? else { return Ok(None) };
    // A file name used as the title (no tags) and no artist: nothing sensible to ask.
    if q.artist.trim().is_empty() || q.artist.eq_ignore_ascii_case("unknown artist") {
        return Ok(None);
    }
    let k = key(&q);
    if let Some(c) = cached(db, &k) {
        return Ok(c);
    }
    let hit = fetch(&q).await?;
    store(db, &k, hit.as_ref())?;
    Ok(hit.as_ref().and_then(to_lyrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(title: &str, artist: &str, secs: i64) -> Query {
        Query { title: title.into(), artist: artist.into(), album: String::new(), duration_ms: secs * 1000 }
    }
    fn hit(t: &str, a: &str, d: f64, synced: bool) -> Hit {
        Hit { track_name: t.into(), artist_name: a.into(), duration: Some(d), instrumental: false, plain: Some("words".into()), synced: synced.then(|| "[00:01.00] words".into()), approx: false }
    }

    #[test]
    fn keys_survive_noise_and_small_length_differences() {
        assert_eq!(key(&q("Hey Jude (Remastered 2015)", "The Beatles", 429)), key(&q("Hey Jude", "The Beatles", 430)));
        assert_ne!(key(&q("Hey Jude", "The Beatles", 429)), key(&q("Hey Jude", "The Beatles", 300)));
        assert_eq!(key(&q("Stay", "The Kid LAROI, Justin Bieber", 141)), key(&q("Stay", "The Kid LAROI", 141)));
    }

    #[test]
    fn search_hits_must_fit_and_synced_wins() {
        let want = q("Janice STFU", "Drake", 237);
        let hits = vec![
            hit("Janice STFU", "Drake", 211.0, true), // different cut
            hit("Janice STFU", "Drake", 237.0, false),
            hit("Janice STFU", "Drake", 238.0, true),
            hit("Other Song", "Drake", 237.0, true),
            hit("Janice STFU", "Someone Else", 237.0, true),
        ];
        let b = best(&want, hits).unwrap();
        assert_eq!((b.duration, b.synced.is_some()), (Some(238.0), true));
        assert!(best(&want, vec![hit("Janice STFU", "Drake", 300.0, true)]).is_none());
    }

    #[test]
    fn other_versions_and_small_differences_get_plain_words() {
        // A hardstyle edit of a song LRCLIB has in its original form: same words, different structure.
        let want = q("they don't love you more than i love you - hardstyle", "Remika", 128);
        let b = best(&want, vec![hit("they don't love you more than i love you", "Remika", 104.0, true)]).unwrap();
        assert!(b.synced.is_none(), "timings of the original wouldn't fit an edit");
        assert_eq!(b.plain.as_deref(), Some("words"));
        // 5 s off: words, no timings.
        let b = best(&q("Monster", "Xylaris", 196), vec![hit("Monster", "Xylaris", 201.0, true)]).unwrap();
        assert!(b.synced.is_none());
        // Plain text rebuilt from synced lines when the entry has no plain lyrics.
        let only_synced = Hit { plain: None, ..hit("Monster", "Xylaris", 201.0, true) };
        assert_eq!(best(&q("Monster", "Xylaris", 196), vec![only_synced]).unwrap().plain.as_deref(), Some("words"));
        // Same version, same length: synced.
        assert!(best(&q("Monster", "Xylaris", 201), vec![hit("Monster", "Xylaris", 201.0, true)]).unwrap().synced.is_some());
        // A different artist's "Monster" is never used.
        assert!(best(&q("Monster", "Xylaris", 201), vec![hit("Monster", "Kanye West", 201.0, true)]).is_none());
    }

    #[test]
    fn slowed_and_sped_up_versions_get_stretched_timings() {
        let orig = |d: f64| Hit { synced: Some("[00:10.00]first\n[00:40.00]second".into()), ..hit("they don't love you more than i love you", "Remika", d, true) };
        // Slowed: 128 s vs the original 104 s → ×1.2308.
        let b = best(&q("they don't love you more than i love you - slowed", "Remika", 128), vec![orig(104.0)]).unwrap();
        assert!(b.approx);
        let times: Vec<i64> = parse_lrc(b.synced.as_deref().unwrap()).iter().map(|x| x.0).collect();
        // LRC keeps hundredths of a second: 12.3077 s → 12.30 s.
        assert_eq!(times, vec![12_300, 49_230]);
        // Sped up: shorter, ×0.8.
        let b = best(&q("they don't love you more than i love you (sped up)", "Remika", 83), vec![orig(104.0)]).unwrap();
        assert!(b.approx && parse_lrc(b.synced.as_deref().unwrap())[0].0 < 10_000);
        // "Slowed" but longer original? Lengths contradict the label → plain words only.
        let b = best(&q("they don't love you more than i love you - slowed", "Remika", 90), vec![orig(104.0)]).unwrap();
        assert!(b.synced.is_none() && !b.approx);
        // A hardstyle edit changes the structure, not just the speed → plain words.
        let b = best(&q("they don't love you more than i love you - hardstyle", "Remika", 128), vec![orig(104.0)]).unwrap();
        assert!(b.synced.is_none());
        // Reverb only: same timing, kept synced (marked approximate).
        let b = best(&q("they don't love you more than i love you (reverb)", "Remika", 104), vec![orig(104.0)]).unwrap();
        assert!(b.approx && parse_lrc(b.synced.as_deref().unwrap())[0].0 == 10_000);
    }

    #[test]
    fn cache_round_trip_and_misses_expire() {
        let db = Db::open_in_memory().unwrap();
        let h = hit("Hey Jude", "The Beatles", 431.0, true);
        store(&db, "a", Some(&h)).unwrap();
        let l = cached(&db, "a").unwrap().unwrap();
        assert_eq!((l.source, l.synced.len()), ("LRCLIB", 1));
        store(&db, "b", None).unwrap();
        assert!(matches!(cached(&db, "b"), Some(None)), "recent miss is remembered");
        db.conn().execute("UPDATE music_lyrics_cache SET fetched_at = '2000-01-01T00:00:00.000Z' WHERE key = 'b'", []).unwrap();
        assert!(cached(&db, "b").is_none(), "old miss is asked again");
        assert!(cached(&db, "c").is_none());
        let inst = Hit { instrumental: true, plain: None, synced: None, ..h };
        assert_eq!(to_lyrics(&inst).unwrap().text, "♪ Instrumental");
    }

    /// Live: real lookups (network).
    #[test]
    #[ignore]
    fn live_lookups() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        for (t, a, al, s) in [
            ("Hey Jude (Remastered 2015)", "The Beatles", "1 (Remastered)", 429),
            ("Janice STFU", "Drake", "", 237),
            ("Bohemian Rhapsody", "Queen", "A Night At The Opera", 355),
            ("Comment te dire adieu (Remasterisé en 2016)", "Françoise Hardy", "", 147),
            ("Zzqx Not A Real Song", "Nobody Real", "", 200),
        ] {
            let qq = Query { title: t.into(), artist: a.into(), album: al.into(), duration_ms: s * 1000 };
            let r = rt.block_on(fetch(&qq)).unwrap();
            println!("{a} — {t}: {}", match &r {
                Some(h) => format!("found ({}), {}", if h.synced.is_some() { "synced" } else { "plain" }, h.synced.as_deref().or(h.plain.as_deref()).unwrap_or("").lines().find(|l| l.len() > 12).unwrap_or("")),
                None => "not found".into(),
            });
        }
    }
}
