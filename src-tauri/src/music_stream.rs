//! Streaming songs from YouTube (unofficial, opt-in): finding the right video
//! for a song.
//!
//! The common failure of "paste a link and play" apps is picking the wrong
//! video: the first search result is often a music video with a spoken
//! intro, a lyric re-upload cut to a different length, a live recording, a
//! cover or a sped-up edit. Here every candidate is scored on title, artist,
//! length and version words, official uploads ("Artist - Topic", verified
//! artists, "Official Audio") are preferred, and a song is left unmatched
//! rather than guessed when nothing fits.

// Wired into playback once the stream extractor (yt-dlp) is in place.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::music_import::{artist_set, clean_title, get_text, parse_len, split_video_title, str_at, title_sim, walk, RemoteTrack};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub duration_ms: Option<i64>,
    /// Verified artist badge or an auto-generated "Artist - Topic" channel.
    pub official: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pick {
    pub candidate: Candidate,
    /// "exact" | "likely"
    pub confidence: &'static str,
    pub score: f64,
}

pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Words that mark a different version of a song. A candidate containing one
/// that the wanted title doesn't is never picked.
const VERSION_WORDS: &[&str] = &[
    "live", "cover", "karaoke", "instrumental", "nightcore", "sped up", "speed up", "slowed", "reverb", "8d", "remix", "acoustic", "piano", "reaction", "tutorial", "lesson",
    "hour", "loop", "extended", "edit", "mashup", "bass boosted", "mix", "version", "demo", "concert", "tribute", "parody", "chipmunk", "lofi", "lo fi", "orchestral", "unplugged",
    "session", "rehearsal", "isolated", "acapella", "a cappella", "backing track", "minus one", "drum cover", "guitar cover", "bass cover", "shorts", "teaser", "trailer", "snippet",
    "behind the scenes", "making of", "interview", "documentary", "commentary", "explained", "unboxing", "dance practice", "choreography", "fancam",
];

/// Words that make signed-out YouTube search show "Confirm your age" instead of results.
const EXPLICIT: &[&str] = &["stfu", "fuck", "fucking", "fuckin", "shit", "bitch", "bitches", "pussy", "dick", "cock", "sex", "sexy", "nigga", "niggas", "hoe", "hoes", "whore", "slut", "porn", "nude", "naked", "wap", "cum", "ass", "motherfucker"];

fn has_word(hay: &str, w: &str) -> bool {
    // Whole words/phrases only ("live" must not match "alive").
    let hay = format!(" {hay} ");
    hay.contains(&format!(" {w} "))
}

/// Version words in `cand` that `want` doesn't have.
fn extra_versions(want: &str, cand: &str) -> Vec<&'static str> {
    VERSION_WORDS.iter().copied().filter(|w| has_word(cand, w) && !has_word(want, w)).collect()
}

fn lower_words(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Score a candidate for a wanted song, or None if it's the wrong song or version.
pub fn score(want: &RemoteTrack, c: &Candidate) -> Option<(f64, &'static str)> {
    // Titles like "Artist - Song (Official Video)": take the song part.
    let (cand_artist, cand_title) = split_video_title(&c.title, &c.channel);
    let wt = clean_title(&want.title);
    let ct = clean_title(&cand_title);
    let ct_full = clean_title(&c.title);
    if wt.is_empty() || ct.is_empty() {
        return None;
    }
    let sim = title_sim(&wt, &ct).max(title_sim(&wt, &ct_full));
    // The whole wanted title should appear (allows extra words like the artist name).
    let contained = ct_full.contains(&wt) || ct.contains(&wt);
    if sim < 0.6 && !contained {
        return None;
    }
    // Version words are checked on the raw title (clean_title keeps most, drops none of these).
    let raw_want = lower_words(&want.title);
    let raw_cand = lower_words(&c.title);
    if !extra_versions(&raw_want, &raw_cand).is_empty() {
        return None;
    }

    let wa = artist_set(&want.artist);
    let mut ca = artist_set(&cand_artist);
    ca.extend(artist_set(&c.channel));
    let artist_ok = wa.is_empty() || wa.iter().any(|a| ca.iter().any(|b| a == b || (a.len() > 3 && b.contains(a.as_str())) || (b.len() > 3 && a.contains(b.as_str()))) || (a.len() > 3 && raw_cand.replace(' ', "").contains(a.as_str())));
    if !artist_ok && !c.official {
        return None;
    }

    let mut s = sim * 3.0 + if contained { 0.5 } else { 0.0 };
    if artist_ok && !wa.is_empty() {
        s += 2.0;
    }
    if c.official {
        s += 1.0;
    }
    let lc = raw_cand.as_str();
    if c.channel.ends_with(" - Topic") || has_word(lc, "official audio") || has_word(lc, "audio") {
        s += 0.7; // audio uploads have no intros or skits
    }
    if has_word(lc, "lyrics") || has_word(lc, "lyric") {
        s -= 0.3; // re-uploads, often cut or pitched
    }
    if has_word(lc, "performance") || has_word(lc, "music video") || has_word(lc, "official video") || has_word(lc, "mv") {
        s -= 0.4; // may carry intros, dialogue or crowd noise
    }
    let diff = match (want.duration_ms, c.duration_ms) {
        (Some(w), Some(d)) => Some((w - d).abs()),
        _ => None,
    };
    if let Some(d) = diff {
        let w = want.duration_ms.unwrap_or(0).max(1);
        // More than 25 s or 12% off is a different cut (intros, outros, edits).
        if d > 25_000 && d * 100 / w > 12 {
            return None;
        }
        s -= (d as f64 / 1000.0 / 4.0).min(3.0);
    }
    let exact = sim >= 0.9 && artist_ok && diff.is_some_and(|d| d <= 4000);
    Some((s, if exact { "exact" } else { "likely" }))
}

/// The best candidate for a song, if any fits.
pub fn pick(want: &RemoteTrack, cands: &[Candidate]) -> Option<Pick> {
    cands
        .iter()
        .filter_map(|c| score(want, c).map(|(s, conf)| Pick { candidate: c.clone(), confidence: conf, score: s }))
        .max_by(|a, b| a.score.total_cmp(&b.score))
}

/// The search text for a song: first artist + title without bracketed noise.
pub fn query_for(t: &RemoteTrack) -> String {
    let first_artist = t.artist.split([',', '&']).next().unwrap_or("").trim();
    let title: String = {
        let mut out = String::new();
        let mut depth = 0;
        for ch in t.title.chars() {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' if depth > 0 => depth -= 1,
                _ if depth == 0 => out.push(ch),
                _ => {}
            }
        }
        let base = out.split(" - ").next().unwrap_or(&out).trim().to_string();
        if base.is_empty() { t.title.clone() } else { base }
    };
    format!("{first_artist} {title}").trim().chars().take(150).collect()
}

/// What a search page held.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchPage {
    Results(Vec<Candidate>),
    /// "Confirm your age": signed-out search hid every result.
    AgeGate,
}

/// Parse a YouTube search results page (both the older `videoRenderer` and
/// the newer `lockupViewModel` layouts).
pub fn parse_search(html: &str) -> AppResult<SearchPage> {
    let start = html.find("var ytInitialData = ").map(|i| i + "var ytInitialData = ".len());
    let v: Value = start
        .and_then(|s| html[s..].find(";</script>").map(|e| &html[s..s + e]))
        .and_then(|j| serde_json::from_str(j).ok())
        .ok_or_else(|| AppError::msg("YouTube's search page couldn't be read (it may be asking to confirm you're not a bot) — try again later"))?;
    let mut out: Vec<Candidate> = Vec::new();
    let mut age_gate = false;
    walk(&v, &mut |k, x| match k {
        "videoRenderer" => {
            let id = x.get("videoId").and_then(Value::as_str).unwrap_or("");
            let title = str_at(x, &["title", "runs", "0", "text"]).unwrap_or("");
            if valid_video_id(id) && !title.is_empty() {
                let channel = str_at(x, &["ownerText", "runs", "0", "text"]).or_else(|| str_at(x, &["longBylineText", "runs", "0", "text"])).unwrap_or("").to_string();
                let badges = x.get("ownerBadges").and_then(Value::as_array).map(|a| a.iter().filter_map(|b| str_at(b, &["metadataBadgeRenderer", "style"])).collect::<Vec<_>>()).unwrap_or_default();
                let official = channel.ends_with(" - Topic") || badges.iter().any(|b| b.contains("VERIFIED_ARTIST"));
                out.push(Candidate { video_id: id.to_string(), title: title.to_string(), channel, duration_ms: str_at(x, &["lengthText", "simpleText"]).and_then(parse_len), official });
            }
            true
        }
        "lockupViewModel" => {
            if x.get("contentType").and_then(Value::as_str) == Some("LOCKUP_CONTENT_TYPE_VIDEO") {
                let id = x.get("contentId").and_then(Value::as_str).unwrap_or("");
                let md = x.pointer("/metadata/lockupMetadataViewModel");
                let title = md.and_then(|m| str_at(m, &["title", "content"])).unwrap_or("");
                if valid_video_id(id) && !title.is_empty() {
                    let channel = md.and_then(|m| m.pointer("/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text/content")).and_then(Value::as_str).unwrap_or("").to_string();
                    let verified = md.map(|m| m.to_string().contains("AUDIO_BADGE") || m.to_string().contains("CHECK_CIRCLE")).unwrap_or(false);
                    let mut dur = None;
                    walk(x, &mut |k2, y| {
                        if k2 == "thumbnailBadgeViewModel" {
                            dur = dur.or_else(|| y.get("text").and_then(Value::as_str).and_then(parse_len));
                            return true;
                        }
                        false
                    });
                    out.push(Candidate { video_id: id.to_string(), title: title.to_string(), official: channel.ends_with(" - Topic") || verified, channel, duration_ms: dur });
                }
            }
            true
        }
        "backgroundPromoRenderer" => {
            age_gate |= x.to_string().contains("Confirm your age");
            true
        }
        _ => false,
    });
    out.dedup_by(|a, b| a.video_id == b.video_id);
    Ok(if out.is_empty() && age_gate { SearchPage::AgeGate } else { SearchPage::Results(out) })
}

/// The query without words that trigger the signed-out age gate.
pub fn without_explicit(q: &str) -> String {
    q.split_whitespace().filter(|w| !EXPLICIT.contains(&lower_words(w).as_str())).collect::<Vec<_>>().join(" ")
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Search YouTube (videos only). One retry on a network hiccup.
pub async fn search(query: &str) -> AppResult<SearchPage> {
    let url = format!("https://www.youtube.com/results?search_query={}&sp=EgIQAQ%253D%253D", url_encode(query));
    let mut last = None;
    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
        match get_text(&url, &[("Cookie", "CONSENT=YES+1; SOCS=CAI")]).await {
            Ok((429, _)) => return Err(AppError::msg("YouTube is limiting searches right now — try again in a few minutes")),
            Ok((s, html)) if s < 400 => match parse_search(&html) {
                Ok(p) => return Ok(p),
                Err(e) => last = Some(e),
            },
            Ok((s, _)) => last = Some(AppError::msg(format!("YouTube search answered {s}"))),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| AppError::msg("YouTube search failed")))
}

/// Find the video for a song. Tries the plain query, then (if the age gate
/// hid the results or nothing fit) without explicit words, then with "audio".
pub async fn resolve(want: &RemoteTrack) -> AppResult<Option<Pick>> {
    let base = query_for(want);
    let mut queries = vec![base.clone()];
    let clean = without_explicit(&base);
    if clean != base && !clean.is_empty() {
        queries.push(clean.clone());
    }
    queries.push(format!("{clean} audio"));
    let mut seen = std::collections::HashSet::new();
    let mut all: Vec<Candidate> = Vec::new();
    for q in queries {
        if let SearchPage::Results(c) = search(&q).await? {
            all.extend(c.into_iter().filter(|c| seen.insert(c.video_id.clone())));
            if let Some(p) = pick(want, &all) {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(title: &str, artist: &str, secs: Option<i64>) -> RemoteTrack {
        RemoteTrack { title: title.into(), artist: artist.into(), album: String::new(), duration_ms: secs.map(|s| s * 1000), path: None }
    }
    fn cand(id: &str, title: &str, channel: &str, secs: i64, official: bool) -> Candidate {
        Candidate { video_id: format!("{id:_<11}"), title: title.into(), channel: channel.into(), duration_ms: Some(secs * 1000), official }
    }

    #[test]
    fn hey_jude_picks_the_song_not_the_video_with_an_intro() {
        // Real search results (Nov 2026), in YouTube's order.
        let c = vec![
            cand("A", "The Beatles - The Beatles - Hey Jude (Official Music Video) [Remastered 2015]", "The Beatles", 490, true),
            cand("B", "Hey Jude (Remastered 2015)", "The Beatles", 426, true),
            cand("C", "Hey Jude", "The Beatles", 239, true),
            cand("D", "The Beatles- Hey Jude (lyrics)", "Zing and Bow Lyrics", 404, false),
            cand("E", "Hey Jude (Remastered 2009)", "The Beatles", 430, true),
            cand("F", "Paul McCartney - Hey Jude Live at Hyde Park", "Video & Music Clips", 462, false),
        ];
        let p = pick(&want("Hey Jude (Remastered 2015)", "The Beatles", Some(429)), &c).unwrap();
        assert!(p.candidate.video_id.starts_with('B') || p.candidate.video_id.starts_with('E'), "picked {:?}", p.candidate.title);
        assert_eq!(p.confidence, "exact");
    }

    #[test]
    fn versions_covers_and_wrong_songs_are_rejected() {
        let w = want("Blinding Lights", "The Weeknd", Some(200));
        for (t, ch) in [
            ("The Weeknd - Blinding Lights (Live at the Grammys)", "The Weeknd"),
            ("Blinding Lights (Sped Up)", "The Weeknd"),
            ("Blinding Lights - Piano Cover", "Pianist Guy"),
            ("Blinding Lights (Nightcore)", "Nightcore Hub"),
            ("Blinding Lights 1 Hour Loop", "Loops"),
            ("Save Your Tears", "The Weeknd"),
        ] {
            assert!(score(&w, &cand("X", t, ch, 200, ch == "The Weeknd")).is_none(), "{t} should be rejected");
        }
        // …unless the wanted song is that version.
        assert!(score(&want("Blinding Lights (Live)", "The Weeknd", None), &cand("X", "The Weeknd - Blinding Lights (Live at the Grammys)", "The Weeknd", 240, true)).is_some());
        // "alive" isn't "live".
        assert!(score(&want("Alive", "Sia", Some(263)), &cand("X", "Sia - Alive (Official Audio)", "Sia", 263, true)).is_some());
    }

    #[test]
    fn length_and_artist_decide() {
        let w = want("Stay", "The Kid LAROI, Justin Bieber", Some(141));
        // Same title, different song.
        assert!(score(&w, &cand("X", "Rihanna - Stay ft. Mikky Ekko", "Rihanna", 240, true)).is_none());
        // Right song, way too long (music video with intro).
        assert!(score(&w, &cand("X", "The Kid LAROI, Justin Bieber - STAY (Official Video)", "The Kid LAROI", 210, true)).is_none());
        // Right song, right length.
        assert_eq!(score(&w, &cand("X", "The Kid LAROI, Justin Bieber - STAY (Official Audio)", "The Kid LAROI", 142, true)).unwrap().1, "exact");
        // Topic channel upload.
        assert!(score(&w, &cand("X", "STAY", "The Kid LAROI - Topic", 141, true)).is_some());
    }

    #[test]
    fn topic_channel_uploads_match() {
        // Real results for "Los Machucambos Pepito" (Deezer length 187 s).
        let c = vec![
            cand("A", "Los Machucambos - Pepito (Stereo)", "jacob 6657", 187, false),
            cand("B", "Los Machucambos - Pepito", "Jackretro", 171, false),
            cand("C", "Pepito", "LOS MACHUCAMBOS - Topic", 188, true),
            cand("D", "Pepito Mi Corazon - Romantic Sax ChaCha Cover | Los Machucambos Tribute by Sax Romantica", "Sax Romantica", 541, false),
        ];
        let p = pick(&want("Pepito", "Los Machucambos", Some(187)), &c);
        assert_eq!(p.map(|p| p.candidate.video_id.chars().next().unwrap()), Some('C'));
    }

    #[test]
    fn age_gate_layouts_and_extra_rejections() {
        let gate = r#"<script>var ytInitialData = {"contents":{"itemSectionRenderer":{"contents":[{"backgroundPromoRenderer":{"title":{"runs":[{"text":"Confirm your age"}]}}}]}}};</script>"#;
        assert_eq!(parse_search(gate).unwrap(), SearchPage::AgeGate);
        assert_eq!(without_explicit("Drake Janice STFU"), "Drake Janice");
        assert_eq!(without_explicit("Cardi B WAP"), "Cardi B");
        assert_eq!(without_explicit("Class Act"), "Class Act"); // "ass" only as a whole word
        // Newer layout.
        let new = r#"<script>var ytInitialData = {"x":[{"lockupViewModel":{"contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentId":"mQER0A0ej0M","contentImage":{"thumbnailBadgeViewModel":{"text":"7:06"}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"Hey Jude (Remastered 2015)"},"metadata":{"contentMetadataViewModel":{"metadataRows":[{"metadataParts":[{"text":{"content":"The Beatles - Topic"}}]}]}}}}}}]};</script>"#;
        match parse_search(new).unwrap() {
            SearchPage::Results(c) => assert_eq!(c, vec![Candidate { video_id: "mQER0A0ej0M".into(), title: "Hey Jude (Remastered 2015)".into(), channel: "The Beatles - Topic".into(), duration_ms: Some(426_000), official: true }]),
            other => panic!("{other:?}"),
        }
        let w = want("Choosin' Texas", "Ella Langley", Some(220));
        assert!(score(&w, &cand("X", "Ella Langley - Choosin' Texas (Official Behind The Scenes Video)", "Ella Langley", 219, true)).is_none());
        // Audio upload beats the official video of the same length.
        let c = vec![cand("V", "Ella Langley - Choosin' Texas (Official Video)", "Ella Langley", 220, true), cand("A", "Choosin' Texas", "Ella Langley - Topic", 220, true)];
        assert!(pick(&w, &c).unwrap().candidate.video_id.starts_with('A'));
    }

    #[test]
    fn queries_and_ids() {
        assert_eq!(query_for(&want("Hey Jude (Remastered 2015)", "The Beatles", None)), "The Beatles Hey Jude");
        assert_eq!(query_for(&want("Here Comes The Sun - Remastered 2009", "The Beatles, George", None)), "The Beatles Here Comes The Sun");
        assert!(valid_video_id("mQER0A0ej0M"));
        assert!(!valid_video_id("mQER0A0ej0M&x=1"));
        assert_eq!(url_encode("AC/DC & me"), "AC%2FDC+%26+me");
    }

    /// Live: resolve real playlists and print every pick for review.
    #[test]
    #[ignore]
    fn live_resolve_real_playlists() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut total = 0;
        let mut found = 0;
        let mut exact = 0;
        for link in ["https://www.deezer.com/playlist/908622995", "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M"] {
            let list = rt.block_on(crate::music_import::fetch(&crate::music_import::parse_link(link).unwrap())).unwrap();
            for t in list.tracks.iter().take(50) {
                total += 1;
                let r = rt.block_on(resolve(t));
                match r {
                    Ok(Some(p)) => {
                        found += 1;
                        if p.confidence == "exact" {
                            exact += 1;
                        }
                        let d = match (t.duration_ms, p.candidate.duration_ms) {
                            (Some(a), Some(b)) => format!("{:+}s", (b - a) / 1000),
                            _ => "?".into(),
                        };
                        println!("OK  {:<6} {:>5}  {} — {}  =>  {} [{}]", p.confidence, d, t.artist, t.title, p.candidate.title, p.candidate.channel);
                    }
                    Ok(None) => println!("--  none          {} — {}", t.artist, t.title),
                    Err(e) => println!("ERR {e}  {} — {}", t.artist, t.title),
                }
            }
        }
        println!("found {found}/{total}, exact {exact}");
        assert!(found * 10 >= total * 9, "found only {found}/{total}");
    }
}
