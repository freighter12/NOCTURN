//! AI shuffle: keep the music going in the same vein, like Spotify's
//! autoplay / smart shuffle.
//!
//! Seeds are the songs the user just played. For each seed NOCTURN asks
//! YouTube Music for its radio (similar songs: same genre, mood and era),
//! mixes the radios of the last couple of seeds, drops what was just heard
//! and anything explicit unless the user (18+) allowed it, and then:
//! - songs already in the library play from the library;
//! - other songs are streamed (only if streaming is turned on).
//!
//! With streaming off, or offline, it falls back to the library alone: songs
//! sharing the seeds' genre tags and artists, then the on-device picks.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde::Serialize;

use crate::db::Db;
use crate::error::AppResult;
use crate::music::{self, Track};
use crate::music_import::{artist_set, clean_title, Matcher, RemoteTrack};
use crate::music_stream;
use crate::music_ytm::{self, YtmSong};

/// Seed track → video ID (YouTube Music search result), for this session.
static SEED_VIDEOS: Mutex<Option<HashMap<i64, Option<String>>>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct Continue {
    pub track_ids: Vec<i64>,
    /// "radio" (YouTube Music mix) or "library" (on-device fallback).
    pub source: &'static str,
    /// A short reason to show, e.g. "Because you played Without Me".
    pub because: Option<String>,
}

fn key(title: &str, artist: &str) -> String {
    let a = artist_set(artist).into_iter().min().unwrap_or_default();
    format!("{}|{}", clean_title(title), a)
}

fn video_of(t: &Track, path: Option<&str>) -> Option<String> {
    if t.source == "youtube" {
        return path.and_then(|p| p.strip_prefix("youtube:")).filter(|v| music_stream::valid_video_id(v)).map(String::from);
    }
    None
}

async fn seed_video(t: &Track, path: Option<&str>) -> Option<String> {
    if let Some(v) = video_of(t, path) {
        return Some(v);
    }
    if let Some(Some(v)) = SEED_VIDEOS.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&t.id).cloned())) {
        return Some(v);
    }
    let want = RemoteTrack { title: t.title.clone(), artist: t.artist.clone(), album: t.album.clone(), duration_ms: (t.duration_ms > 0).then_some(t.duration_ms), path: None, video_id: None };
    let songs = music_ytm::search(&music_stream::query_for(&want)).await.ok()?;
    let c: Vec<_> = songs.iter().map(|s| s.candidate()).collect();
    // Explicit or clean doesn't matter for a seed; take the best match.
    let v = music_stream::pick_with(&want, &c, true).map(|p| p.candidate.video_id);
    if let Ok(mut m) = SEED_VIDEOS.lock() {
        m.get_or_insert_with(HashMap::new).insert(t.id, v.clone());
    }
    v
}

/// Round-robin merge, so two seeds' radios alternate.
fn interleave(lists: Vec<Vec<YtmSong>>) -> Vec<YtmSong> {
    let mut out = Vec::new();
    let max = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..max {
        for l in &lists {
            if let Some(s) = l.get(i) {
                out.push(s.clone());
            }
        }
    }
    out
}

/// Radio songs worth playing next: not the seeds, not heard recently, not
/// twice, not explicit unless allowed, at most two in a row by one artist.
pub fn filter_radio(songs: Vec<YtmSong>, avoid: &HashSet<String>, explicit_ok: bool) -> Vec<YtmSong> {
    let mut seen = avoid.clone();
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<YtmSong> = Vec::new();
    for s in songs {
        if s.explicit && !explicit_ok {
            continue;
        }
        let k = key(&s.title, &s.artist);
        if !seen.insert(k) {
            continue;
        }
        let a = artist_set(&s.artist).into_iter().min().unwrap_or_default();
        let n = per_artist.entry(a.clone()).or_default();
        // Variety: no more than 3 songs by one artist per batch.
        if *n >= 3 {
            continue;
        }
        *n += 1;
        out.push(s);
    }
    // Avoid back-to-back songs by the same artist where possible.
    let mut arranged: Vec<YtmSong> = Vec::with_capacity(out.len());
    while !out.is_empty() {
        let last = arranged.last().map(|s: &YtmSong| artist_set(&s.artist));
        let i = out.iter().position(|s| last.as_ref().is_none_or(|l| artist_set(&s.artist).is_disjoint(l))).unwrap_or(0);
        arranged.push(out.remove(i));
    }
    arranged
}

/// On-device fallback: songs sharing the seeds' genres or artists, then the
/// usual recommendations; never a song in `exclude`.
pub fn library_continue(all: &[Track], seeds: &[&Track], exclude: &HashSet<i64>, count: usize) -> Vec<i64> {
    let genres: HashSet<String> = seeds.iter().flat_map(|t| t.genre.split([';', '/', ',']).map(|g| g.trim().to_lowercase())).filter(|g| !g.is_empty()).collect();
    let artists: HashSet<String> = seeds.iter().flat_map(|t| artist_set(&t.artist)).collect();
    let mut scored: Vec<(i64, i64)> = all
        .iter()
        .filter(|t| !exclude.contains(&t.id) && t.source == "local")
        .filter_map(|t| {
            let g = t.genre.split([';', '/', ',']).any(|g| genres.contains(&g.trim().to_lowercase()));
            let a = !artist_set(&t.artist).is_disjoint(&artists);
            let s = (g as i64) * 3 + (a as i64) * 2 + (t.liked as i64);
            (s > 0).then_some((t.id, s))
        })
        .collect();
    // Stable per call but varied between calls.
    let salt = chrono::Utc::now().timestamp_millis();
    scored.sort_by_key(|(id, s)| (std::cmp::Reverse(*s), (id.wrapping_mul(2654435761) ^ salt) & 0xffff));
    let mut out: Vec<i64> = scored.into_iter().map(|(id, _)| id).take(count).collect();
    if out.len() < count {
        for id in music::recommended(all, count * 3) {
            if out.len() >= count {
                break;
            }
            if !exclude.contains(&id) && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// Up to `count` songs to play after `seeds` (most recent last).
pub async fn continue_from(db: &Db, seeds: &[i64], exclude: &[i64], count: usize, streaming: bool) -> AppResult<Continue> {
    let count = count.clamp(1, 25);
    let all = music::tracks(db)?;
    let paths: HashMap<i64, String> = crate::music_import::local_paths(db)?.into_iter().collect();
    let stream_paths: HashMap<i64, String> = {
        let conn = db.conn();
        let mut st = conn.prepare("SELECT id, path FROM music_tracks WHERE source = 'youtube'")?;
        let rows: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.filter_map(Result::ok).collect();
        rows.into_iter().collect()
    };
    let by_id: HashMap<i64, &Track> = all.iter().map(|t| (t.id, t)).collect();
    let seed_tracks: Vec<&Track> = seeds.iter().rev().filter_map(|id| by_id.get(id).copied()).take(5).collect();
    let exclude_set: HashSet<i64> = exclude.iter().chain(seeds.iter()).copied().collect();
    let mut avoid: HashSet<String> = exclude_set.iter().filter_map(|id| by_id.get(id)).map(|t| key(&t.title, &t.artist)).collect();
    // Also skip what was played in the last couple of hours.
    {
        let conn = db.conn();
        let since = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let mut st = conn.prepare("SELECT DISTINCT track_id FROM music_plays WHERE played_at > ?1")?;
        let recent: Vec<i64> = st.query_map([since], |r| r.get(0))?.filter_map(Result::ok).collect();
        for id in recent {
            if let Some(t) = by_id.get(&id) {
                avoid.insert(key(&t.title, &t.artist));
            }
        }
    }
    let explicit_ok = music_stream::explicit_ok();

    // Radio from the two most recent seeds.
    let mut lists = Vec::new();
    let mut because = None;
    for t in seed_tracks.iter().take(2) {
        let path = stream_paths.get(&t.id).or_else(|| paths.get(&t.id)).map(String::as_str);
        let Some(v) = seed_video(t, path).await else { continue };
        match music_ytm::radio(&v).await {
            Ok(r) if !r.is_empty() => {
                because.get_or_insert_with(|| format!("Because you played {}", t.title));
                lists.push(r);
            }
            Ok(_) => {}
            Err(e) => tracing::info!("ai shuffle: radio failed: {e}"),
        }
    }
    let radio = filter_radio(interleave(lists), &avoid, explicit_ok);

    let matcher = Matcher::new(&all, &paths.iter().map(|(k, v)| (*k, v.clone())).collect::<Vec<_>>());
    let mut out: Vec<i64> = Vec::new();
    for s in &radio {
        if out.len() >= count {
            break;
        }
        let want = RemoteTrack { title: s.title.clone(), artist: s.artist.clone(), album: s.album.clone(), duration_ms: s.duration_ms, path: None, video_id: Some(s.video_id.clone()) };
        if let Some((id, _)) = matcher.find(&want) {
            if !exclude_set.contains(&id) && !out.contains(&id) {
                out.push(id);
            }
            continue;
        }
        if streaming {
            match crate::music_stream_engine::ensure_track(db, &want, &s.candidate()) {
                Ok(id) if !exclude_set.contains(&id) && !out.contains(&id) => out.push(id),
                Ok(_) => {}
                Err(e) => tracing::warn!("ai shuffle: couldn't add a streaming song: {e}"),
            }
        }
    }
    if !out.is_empty() {
        db.audit("info", "user", "music.ai_shuffle", Some(serde_json::json!({ "added": out.len(), "source": "radio" })));
        return Ok(Continue { track_ids: out, source: "radio", because });
    }
    let ids = library_continue(&all, &seed_tracks, &exclude_set, count);
    Ok(Continue { because: seed_tracks.first().map(|t| format!("More like {}", t.title)), track_ids: ids, source: "library" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, artist: &str, explicit: bool) -> YtmSong {
        YtmSong { video_id: format!("{id:_<11}"), title: title.into(), artist: artist.into(), album: String::new(), duration_ms: Some(200_000), explicit }
    }

    #[test]
    fn radio_filtering() {
        let songs = vec![
            song("a", "Without Me", "Eminem", true),
            song("b", "The Real Slim Shady", "Eminem", true),
            song("c", "Gangsta's Paradise", "Coolio & L.V.", false),
            song("d", "My Name Is", "Eminem", true),
            song("e", "Lose Yourself", "Eminem", true),
            song("f", "Shake That", "Eminem", true),
            song("g", "Gangsta's Paradise", "Coolio", false),
            song("h", "Jump Around", "House Of Pain", false),
        ];
        let avoid: HashSet<String> = [key("Without Me", "Eminem")].into();
        let clean = filter_radio(songs.clone(), &avoid, false);
        assert_eq!(clean.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), vec!["Gangsta's Paradise", "Jump Around"]);
        let all = filter_radio(songs, &avoid, true);
        // Seed skipped, duplicate Gangsta's Paradise skipped, max 3 Eminem, no two Eminem in a row where avoidable.
        assert!(!all.iter().any(|s| s.title == "Without Me"));
        assert_eq!(all.iter().filter(|s| s.artist == "Eminem").count(), 3);
        assert_eq!(all.iter().filter(|s| s.title == "Gangsta's Paradise").count(), 1);
        let consecutive = all.windows(2).filter(|w| w[0].artist == w[1].artist).count();
        assert!(consecutive <= 1, "{:?}", all.iter().map(|s| &s.artist).collect::<Vec<_>>());
    }

    #[test]
    fn library_fallback_follows_genre_and_artist() {
        let mk = |id: i64, artist: &str, genre: &str| Track { id, source: "local".into(), title: format!("t{id}"), artist: artist.into(), album: "a".into(), album_artist: artist.into(), track_no: None, disc_no: None, year: None, genre: genre.into(), duration_ms: 1, has_art: false, liked: false, added_at: String::new(), plays: 0, last_played: None, loudness: None, peak: None };
        let all = vec![mk(1, "A", "Hip-Hop"), mk(2, "B", "Hip-Hop"), mk(3, "C", "Classical"), mk(4, "A", "Pop"), mk(5, "D", "Jazz")];
        let seeds = [&all[0]];
        let ex: HashSet<i64> = [1].into();
        let got = library_continue(&all, &seeds, &ex, 3);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&2) && got.contains(&4));
        assert!(!got.contains(&3) && !got.contains(&1));
    }
}
