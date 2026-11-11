//! AI Shuffle — Nocturn's smart shuffle (not a random one).
//!
//! It plans what plays next from four signals:
//! - **Mood / genre / era**: YouTube Music's radio for the current song (and
//!   for one or two songs you've played a lot this session) — similar songs,
//!   in similarity order. Songs that show up in more than one radio fit the
//!   session best.
//! - **Your behaviour**: artists and songs you've liked or keep playing rank
//!   higher; songs you skipped this session (and, less, their artists) sink.
//! - **Freshness**: songs you heard recently are held back; a few new finds
//!   are mixed in with what you queued.
//! - **Flow**: never the same artist twice in a row, at most two songs by one
//!   artist in any ten, and each pick leans toward what fits the song before it.
//!
//! The planning is a pure function (`plan`) so it can be tested without the network.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::textmatch::{artist_set, clean_title};
use crate::yt::{self, Track};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub current: Option<Track>,
    /// What's queued after the current song.
    #[serde(default)]
    pub upcoming: Vec<Track>,
    /// Recently played, newest first.
    #[serde(default)]
    pub recent: Vec<Track>,
    #[serde(default)]
    pub liked: Vec<Track>,
    /// Video IDs skipped (within 30 s) this session.
    #[serde(default)]
    pub skipped: Vec<String>,
    /// How many songs to plan (at least the queued ones are kept).
    #[serde(default)]
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub upcoming: Vec<Track>,
    /// Short explanation to show, e.g. "Picked 25 songs that fit Blinding Lights".
    pub because: String,
    pub discoveries: usize,
}

/// Same song, different upload ("Song (Official Video)" vs the album audio).
fn song_key(t: &Track) -> String {
    format!("{}|{}", clean_title(&t.title), artist_set(&t.artist).into_iter().min().unwrap_or_default())
}

fn main_artist(t: &Track) -> String {
    let mut a: Vec<String> = artist_set(&t.artist).into_iter().collect();
    a.sort();
    a.into_iter().next().unwrap_or_default()
}

struct Cand {
    track: Track,
    score: f64,
    queued: bool,
    /// Best (lowest) position in any radio, 0..1 (0 = most similar).
    similarity_rank: f64,
}

/// Plans the upcoming songs. `radios` are YouTube Music radios, the first one
/// for the current song.
pub fn plan(input: &Input, radios: &[Vec<Track>]) -> Plan {
    let want = input.count.unwrap_or(25).clamp(5, 100).max(input.upcoming.len());
    let skipped: HashSet<&str> = input.skipped.iter().map(String::as_str).collect();
    let current_key = input.current.as_ref().map(song_key);

    // behaviour signals
    let mut artist_love: HashMap<String, f64> = HashMap::new();
    for t in &input.liked {
        *artist_love.entry(main_artist(t)).or_default() += 1.0;
    }
    for (i, t) in input.recent.iter().take(60).enumerate() {
        // recent plays count, newer ones a bit more
        *artist_love.entry(main_artist(t)).or_default() += 0.5 * (1.0 - i as f64 / 80.0);
    }
    let max_love = artist_love.values().cloned().fold(1.0, f64::max);
    let liked_keys: HashSet<String> = input.liked.iter().map(song_key).collect();
    let recent_keys: HashMap<String, usize> = input.recent.iter().take(40).enumerate().map(|(i, t)| (song_key(t), i)).collect();
    let mut skipped_artists: HashMap<String, f64> = HashMap::new();
    for t in input.recent.iter().chain(input.upcoming.iter()) {
        if skipped.contains(t.video_id.as_str()) {
            *skipped_artists.entry(main_artist(t)).or_default() += 1.0;
        }
    }

    // candidates: what you queued + the radios
    let mut cands: Vec<Cand> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut add = |t: &Track, queued: bool, rank: f64, cands: &mut Vec<Cand>| {
        let k = song_key(t);
        if Some(&k) == current_key.as_ref() || skipped.contains(t.video_id.as_str()) {
            return;
        }
        match index.get(&k) {
            Some(&i) => {
                let c = &mut cands[i];
                c.queued |= queued;
                // showing up in several radios = fits the whole session
                if !queued && rank < 1.0 {
                    c.score += 0.35;
                }
                c.similarity_rank = c.similarity_rank.min(rank);
            }
            None => {
                index.insert(k, cands.len());
                cands.push(Cand { track: t.clone(), score: 0.0, queued, similarity_rank: rank });
            }
        }
    };
    for t in &input.upcoming {
        add(t, true, 1.0, &mut cands);
    }
    for (ri, radio) in radios.iter().enumerate() {
        let n = radio.len().max(1) as f64;
        for (i, t) in radio.iter().enumerate() {
            // the current song's radio counts most
            let rank = (i as f64 / n) * if ri == 0 { 1.0 } else { 1.3 };
            add(t, false, rank.min(0.99), &mut cands);
        }
    }

    for c in &mut cands {
        let k = song_key(&c.track);
        let a = main_artist(&c.track);
        let mut s = c.score;
        if c.queued {
            s += 1.2; // you chose it
        }
        s += (1.0 - c.similarity_rank) * 1.4; // fits the mood/genre
        s += artist_love.get(&a).map(|v| v / max_love).unwrap_or(0.0) * 0.9;
        if liked_keys.contains(&k) {
            s += 0.6;
        }
        if let Some(&i) = recent_keys.get(&k) {
            // heard it recently: hold it back (less so for liked songs)
            let fresh_penalty = 1.6 * (1.0 - i as f64 / 50.0);
            s -= if liked_keys.contains(&k) { fresh_penalty * 0.4 } else { fresh_penalty };
        }
        s -= skipped_artists.get(&a).copied().unwrap_or(0.0).min(3.0) * 0.5;
        if c.track.video {
            s -= 0.25; // prefer album audio over music videos
        }
        c.score = s;
    }

    // Order: greedy, keeping the flow (artist spacing) and leaning toward
    // songs that are similar to the last pick (similar radio rank).
    let queued_total = cands.iter().filter(|c| c.queued).count();
    let max_discoveries = (want.saturating_sub(queued_total)).max(want * 2 / 5);
    let mut out: Vec<Track> = Vec::new();
    let mut used = vec![false; cands.len()];
    let mut discoveries = 0;
    let mut last_rank = 0.0;
    let mut last_artist = input.current.as_ref().map(main_artist).unwrap_or_default();
    while out.len() < want {
        let window: Vec<String> = out.iter().rev().take(9).map(main_artist).collect();
        let mut best: Option<(f64, usize)> = None;
        for (i, c) in cands.iter().enumerate() {
            if used[i] || (!c.queued && discoveries >= max_discoveries) {
                continue;
            }
            let a = main_artist(&c.track);
            if a == last_artist && !a.is_empty() {
                continue;
            }
            if window.iter().filter(|x| **x == a).count() >= 2 {
                continue;
            }
            let flow = 1.0 - (c.similarity_rank - last_rank).abs().min(1.0);
            let s = c.score + flow * 0.4;
            if best.is_none_or(|(b, _)| s > b) {
                best = Some((s, i));
            }
        }
        // nothing fits the spacing rules: relax them rather than stop early
        let pick = best.map(|(_, i)| i).or_else(|| (0..cands.len()).find(|&i| !used[i] && (cands[i].queued || discoveries < max_discoveries)));
        let Some(i) = pick else { break };
        used[i] = true;
        if !cands[i].queued {
            discoveries += 1;
        }
        last_rank = cands[i].similarity_rank;
        last_artist = main_artist(&cands[i].track);
        out.push(cands[i].track.clone());
    }
    // never drop a song you queued (unless you skipped it)
    for (i, c) in cands.iter().enumerate() {
        if c.queued && !used[i] {
            out.push(c.track.clone());
        }
    }

    let because = match &input.current {
        Some(t) if discoveries > 0 => format!("Planned {} songs around {} — {} new finds", out.len(), t.title, discoveries),
        Some(t) => format!("Reordered {} songs to flow from {}", out.len(), t.title),
        None => format!("Planned {} songs", out.len()),
    };
    Plan { upcoming: out, because, discoveries }
}

/// Fetches the radios (current song + up to two songs you keep playing) and plans.
pub async fn shuffle(input: Input) -> Result<Plan, String> {
    let mut seeds: Vec<String> = Vec::new();
    if let Some(c) = &input.current {
        seeds.push(c.video_id.clone());
    }
    // Session favourites: the most-played songs in recent history (not the current one).
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in input.recent.iter().take(30) {
        *counts.entry(t.video_id.as_str()).or_default() += 1;
    }
    let mut fav: Vec<&Track> = input.recent.iter().take(30).filter(|t| Some(&t.video_id) != input.current.as_ref().map(|c| &c.video_id)).collect();
    fav.sort_by_key(|t| std::cmp::Reverse(counts.get(t.video_id.as_str()).copied().unwrap_or(0) * 10 + input.liked.iter().any(|l| l.video_id == t.video_id) as usize * 5));
    for t in fav {
        if seeds.len() >= 3 {
            break;
        }
        if !seeds.contains(&t.video_id) && main_artist(t) != input.current.as_ref().map(main_artist).unwrap_or_default() {
            seeds.push(t.video_id.clone());
        }
    }
    let mut radios = Vec::new();
    for s in &seeds {
        if let Ok(r) = yt::radio(s).await {
            radios.push(r);
        }
    }
    if radios.is_empty() && input.upcoming.is_empty() {
        return Err("couldn't reach YouTube Music to plan the next songs".into());
    }
    Ok(plan(&input, &radios))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, title: &str, artist: &str) -> Track {
        Track { video_id: id.into(), title: title.into(), artist: artist.into(), artist_id: None, album: None, album_id: None, duration_ms: Some(200_000), artwork: None, explicit: false, video: false }
    }

    #[test]
    fn plans_with_flow_and_behaviour() {
        let current = t("cur00000000", "Blinding Lights", "The Weeknd");
        let radio = vec![
            t("r1000000000", "Save Your Tears", "The Weeknd"),
            t("r2000000000", "Levitating", "Dua Lipa"),
            t("r3000000000", "Starboy", "The Weeknd"),
            t("r4000000000", "Physical", "Dua Lipa"),
            t("r5000000000", "As It Was", "Harry Styles"),
            t("r6000000000", "Heat Waves", "Glass Animals"),
            t("r7000000000", "Skipped Song", "Some Band"),
        ];
        let input = Input {
            current: Some(current.clone()),
            upcoming: vec![t("q1000000000", "Queued One", "Adele")],
            recent: vec![current.clone(), t("r5000000000", "As It Was", "Harry Styles")],
            liked: vec![t("x", "Something", "Dua Lipa")],
            skipped: vec!["r7000000000".into()],
            count: Some(6),
        };
        let p = plan(&input, &[radio]);
        let ids: Vec<&str> = p.upcoming.iter().map(|x| x.video_id.as_str()).collect();
        eprintln!("{ids:?} — {}", p.because);
        assert!(ids.contains(&"q1000000000"), "keeps what you queued");
        assert!(!ids.contains(&"r7000000000"), "drops skipped songs");
        assert_ne!(main_artist(&p.upcoming[0]), "weeknd", "never the same artist right after the current song");
        for w in p.upcoming.windows(2) {
            assert_ne!(main_artist(&w[0]), main_artist(&w[1]), "no artist twice in a row");
        }
        // just heard "As It Was" → it comes late if at all
        let pos = ids.iter().position(|x| *x == "r5000000000");
        assert!(pos.is_none_or(|p| p >= 3));
    }

    #[tokio::test]
    #[ignore]
    async fn live_ai_shuffle() {
        let cur = yt::search_songs("The Weeknd Blinding Lights").await.unwrap().remove(0);
        let p = shuffle(Input { current: Some(cur), count: Some(15), ..Default::default() }).await.unwrap();
        eprintln!("{}", p.because);
        for x in &p.upcoming {
            eprintln!("  {} — {}", x.artist, x.title);
        }
        assert!(p.upcoming.len() >= 10);
    }
}
