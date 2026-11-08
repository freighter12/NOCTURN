//! Volume levelling for Nocturn Music: how loud each song is, so the player
//! can turn loud masters down and quiet ones up and every song sits at about
//! the same level.
//!
//! A song's loudness comes from, in order:
//! 1. ReplayGain / R128 tags in the file (instant, and what the user's own
//!    tagger decided);
//! 2. measuring it: decode the audio and run EBU R128 integrated loudness
//!    (the measure streaming services use) plus the sample peak.
//!
//! Measuring runs on one background thread at background priority. Local
//! files are read in place. A streamed song is downloaded once more into
//! memory (a few MB) when it is about to play, then thrown away; only the two
//! numbers are kept.

use std::collections::VecDeque;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use lofty::prelude::*;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tauri::{AppHandle, Emitter, Runtime};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// ReplayGain 2.0 reference level: a gain of 0 dB means -18 LUFS.
const RG_REFERENCE: f64 = -18.0;
/// Opus R128 gain tags are relative to -23 LUFS.
const R128_REFERENCE: f64 = -23.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Loudness {
    /// Integrated loudness, LUFS.
    pub lufs: f64,
    /// Sample peak, linear (1.0 = full scale).
    pub peak: f64,
}

/// What the player hears about when a song has been measured.
#[derive(Debug, Clone, Serialize)]
pub struct Measured {
    pub id: i64,
    pub lufs: Option<f64>,
    pub peak: Option<f64>,
}

// ------------------------------------------------------------------- tags

fn parse_db(s: &str) -> Option<f64> {
    let t = s.trim().trim_end_matches(|c: char| c.is_ascii_alphabetic()).trim();
    t.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() < 60.0)
}

/// Loudness from ReplayGain or R128 tags, if the file has them.
pub fn from_tags(p: &Path) -> Option<Loudness> {
    let f = lofty::read_from_path(p).ok()?;
    for tag in f.tags() {
        if let Some(g) = tag.get_string(ItemKey::ReplayGainTrackGain).and_then(parse_db) {
            let peak = tag.get_string(ItemKey::ReplayGainTrackPeak).and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok()).filter(|p| p.is_finite() && *p > 0.0 && *p < 4.0).unwrap_or(1.0);
            return Some(Loudness { lufs: RG_REFERENCE - g, peak });
        }
        if let Some(q) = ItemKey::from_key(tag.tag_type(), "R128_TRACK_GAIN").and_then(|k| tag.get_string(k)).and_then(|s| s.trim().parse::<i32>().ok()) {
            // Q7.8 fixed point dB.
            return Some(Loudness { lufs: R128_REFERENCE - q as f64 / 256.0, peak: 1.0 });
        }
    }
    None
}

// -------------------------------------------------------------- measuring

/// Decode audio and measure its loudness. `ext` helps the format probe.
pub fn measure(source: Box<dyn MediaSource>, ext: Option<&str>) -> AppResult<Loudness> {
    let mss = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ext {
        hint.with_extension(e);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AppError::msg(format!("unsupported audio: {e}")))?;
    let mut format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or_else(|| AppError::msg("no audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| AppError::msg(format!("no decoder for this audio: {e}")))?;
    let mut meter: Option<(ebur128::EbuR128, usize)> = None;
    let mut buf: Option<SampleBuffer<f32>> = None;
    let mut frames: u64 = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => {
                // A damaged tail: keep what was measured if there's enough.
                if frames > 0 {
                    tracing::debug!("stopped reading audio early: {e}");
                    break;
                }
                return Err(AppError::msg(format!("couldn't read the audio: {e}")));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) | Err(SymError::IoError(_)) => continue,
            Err(e) => return Err(AppError::msg(format!("couldn't decode the audio: {e}"))),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if channels == 0 || spec.rate == 0 {
            continue;
        }
        if meter.is_none() {
            let m = ebur128::EbuR128::new(channels as u32, spec.rate, ebur128::Mode::I | ebur128::Mode::SAMPLE_PEAK).map_err(|e| AppError::msg(format!("loudness meter: {e}")))?;
            meter = Some((m, channels));
        }
        let (m, ch) = meter.as_mut().expect("meter set above");
        if channels != *ch {
            continue; // a channel-count change mid-stream: skip those frames
        }
        let need = decoded.capacity() as u64;
        if buf.as_ref().is_none_or(|b| (b.capacity() as u64) < need * channels as u64) {
            buf = Some(SampleBuffer::<f32>::new(need, spec));
        }
        let b = buf.as_mut().expect("buffer set above");
        b.copy_interleaved_ref(decoded);
        m.add_frames_f32(b.samples()).map_err(|e| AppError::msg(format!("loudness meter: {e}")))?;
        frames += (b.samples().len() / channels) as u64;
    }
    let (m, ch) = meter.ok_or_else(|| AppError::msg("no audio decoded"))?;
    let lufs = m.loudness_global().map_err(|e| AppError::msg(format!("loudness meter: {e}")))?;
    let peak = (0..ch as u32).filter_map(|c| m.sample_peak(c).ok()).fold(0.0f64, f64::max);
    if !lufs.is_finite() {
        // Silence (or nearly): nothing to level.
        return Err(AppError::msg("the song is silent"));
    }
    Ok(Loudness { lufs, peak: if peak > 0.0 { peak } else { 1.0 } })
}

pub fn measure_file(p: &Path) -> AppResult<Loudness> {
    let f = std::fs::File::open(p)?;
    let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    measure(Box::new(f), ext.as_deref())
}

pub fn measure_bytes(bytes: Vec<u8>, ext: Option<&str>) -> AppResult<Loudness> {
    measure(Box::new(Cursor::new(bytes)), ext)
}

// ------------------------------------------------------------------ store

/// The value a stored measurement must match to still be valid: size and
/// modified time for files, the video for streams.
fn key_of(db: &Db, id: i64) -> AppResult<Option<(String, String, String)>> {
    Ok(db
        .conn()
        .query_row(
            "SELECT source, path, CASE WHEN source = 'youtube' THEN path ELSE size || ':' || mtime END FROM music_tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?)
}

fn is_current(db: &Db, id: i64, key: &str) -> AppResult<bool> {
    Ok(db.conn().query_row("SELECT 1 FROM music_loudness WHERE track_id = ?1 AND key = ?2", params![id, key], |_| Ok(())).optional()?.is_some())
}

fn store(db: &Db, id: i64, key: &str, l: Option<Loudness>, how: &str) -> AppResult<()> {
    db.conn().execute(
        "INSERT INTO music_loudness(track_id, key, lufs, peak, how, measured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(track_id) DO UPDATE SET key = excluded.key, lufs = excluded.lufs, peak = excluded.peak, how = excluded.how, measured_at = excluded.measured_at",
        params![id, key, l.map(|x| x.lufs), l.map(|x| x.peak), how, now()],
    )?;
    Ok(())
}

/// Measure one track now (blocking) and store the answer. `None` = it can't
/// be measured (unsupported format, silence); that answer is stored too, so
/// it isn't tried again until the file changes.
pub fn analyse(db: &Db, data_dir: &Path, streaming: bool, id: i64) -> AppResult<Option<Loudness>> {
    let Some((source, path, key)) = key_of(db, id)? else { return Err(AppError::NotFound("track".into())) };
    if is_current(db, id, &key)? {
        return Ok(db.conn().query_row("SELECT lufs, peak FROM music_loudness WHERE track_id = ?1", [id], |r| {
            Ok(match (r.get::<_, Option<f64>>(0)?, r.get::<_, Option<f64>>(1)?) {
                (Some(lufs), Some(peak)) => Some(Loudness { lufs, peak }),
                _ => None,
            })
        })?);
    }
    let started = std::time::Instant::now();
    let (result, how) = if source == "youtube" {
        if !streaming {
            return Err(AppError::msg("streaming is off"));
        }
        let vid = path.strip_prefix("youtube:").unwrap_or_default();
        let (bytes, ext) = crate::music_stream_engine::fetch_all(db, data_dir, vid)?;
        (measure_bytes(bytes, Some(ext)), "measured")
    } else {
        let p = Path::new(&path);
        match from_tags(p) {
            Some(l) => (Ok(l), "tags"),
            None => (measure_file(p), "measured"),
        }
    };
    let l = match result {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::debug!(id, "loudness not measured: {e}");
            None
        }
    };
    store(db, id, &key, l, if l.is_some() { how } else { "failed" })?;
    tracing::debug!(id, how, ms = started.elapsed().as_millis() as u64, lufs = l.map(|x| x.lufs), "loudness");
    Ok(l)
}

// ----------------------------------------------------------------- worker

static QUEUE: Mutex<VecDeque<i64>> = Mutex::new(VecDeque::new());
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Queue tracks for measuring. `urgent` ones (about to play) go first.
pub fn request<R: Runtime>(app: AppHandle<R>, st: Arc<AppState>, ids: &[i64], urgent: bool) {
    {
        let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        for &id in ids {
            if urgent {
                q.retain(|x| *x != id);
                q.push_front(id);
            } else if !q.contains(&id) {
                q.push_back(id);
            }
        }
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        background_priority();
        loop {
            let next = QUEUE.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
            let Some(id) = next else { break };
            let streaming = crate::settings::load(&st.db).music_streaming;
            match analyse(&st.db, &st.data_dir, streaming, id) {
                Ok(l) => {
                    let _ = app.emit("music-loudness", Measured { id, lufs: l.map(|x| x.lufs), peak: l.map(|x| x.peak) });
                }
                Err(e) => tracing::debug!(id, "loudness skipped: {e}"),
            }
        }
        RUNNING.store(false, Ordering::SeqCst);
        // A request may have slipped in between the last pop and the flag.
        if !QUEUE.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
            request(app, st, &[], false);
        }
    });
}

/// Local tracks with no current measurement (streams are only measured when
/// they're about to play, so nothing is downloaded in bulk).
pub fn missing(db: &Db) -> AppResult<Vec<i64>> {
    let conn = db.conn();
    let mut st = conn.prepare(
        "SELECT t.id FROM music_tracks t LEFT JOIN music_loudness l ON l.track_id = t.id AND l.key = t.size || ':' || t.mtime
         WHERE t.source = 'local' AND l.track_id IS NULL ORDER BY t.id",
    )?;
    let ids = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(ids)
}

#[cfg(windows)]
fn background_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
    // Lower CPU and disk priority, so measuring never gets in the way.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}
#[cfg(not(windows))]
fn background_priority() {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16-bit stereo WAV of a sine at `amp` (0–1) for `secs`.
    fn sine_wav(amp: f64, secs: u32) -> Vec<u8> {
        let rate = 44_100u32;
        let n = rate * secs;
        let data_len = n * 4;
        let mut v = Vec::with_capacity(44 + data_len as usize);
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 4).to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..n {
            let s = (amp * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / rate as f64).sin() * 32767.0) as i16;
            v.extend_from_slice(&s.to_le_bytes());
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    #[test]
    fn measures_a_sine_and_halving_it_is_6_db_quieter() {
        let loud = measure_bytes(sine_wav(0.5, 3), Some("wav")).unwrap();
        let quiet = measure_bytes(sine_wav(0.25, 3), Some("wav")).unwrap();
        // A 1 kHz stereo sine at -6 dBFS peak is about -6.0 LUFS (K-weighting is ~0 dB at 1 kHz, +3 dB for two channels).
        assert!((loud.lufs - -6.0).abs() < 1.0, "loud = {}", loud.lufs);
        assert!(((loud.lufs - quiet.lufs) - 6.02).abs() < 0.2, "difference = {}", loud.lufs - quiet.lufs);
        assert!((loud.peak - 0.5).abs() < 0.01, "peak = {}", loud.peak);
    }

    #[test]
    fn silence_and_junk_are_not_measured() {
        assert!(measure_bytes(sine_wav(0.0, 1), Some("wav")).is_err());
        assert!(measure_bytes(b"not audio at all".to_vec(), None).is_err());
    }

    #[test]
    fn replaygain_text_parses() {
        assert_eq!(parse_db("-6.50 dB"), Some(-6.5));
        assert_eq!(parse_db("+2,1 dB"), Some(2.1));
        assert_eq!(parse_db("loud"), None);
    }

    #[test]
    fn analysis_is_stored_and_invalidated_when_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.wav");
        std::fs::write(&file, sine_wav(0.5, 2)).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO music_tracks(path, folder, title, artist, album, album_artist, duration_ms, size, mtime, added_at) VALUES (?1, ?2, 'a', 'b', 'c', 'b', 2000, 1, 1, ?3)",
                params![file.display().to_string(), dir.path().display().to_string(), now()],
            )
            .unwrap();
        let id: i64 = db.conn().query_row("SELECT id FROM music_tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(missing(&db).unwrap(), vec![id]);
        let l = analyse(&db, dir.path(), false, id).unwrap().unwrap();
        assert!(missing(&db).unwrap().is_empty());
        assert_eq!(analyse(&db, dir.path(), false, id).unwrap(), Some(l), "second call is the stored answer");
        db.conn().execute("UPDATE music_tracks SET mtime = 2", []).unwrap();
        assert_eq!(missing(&db).unwrap(), vec![id], "a changed file is measured again");
    }
}
