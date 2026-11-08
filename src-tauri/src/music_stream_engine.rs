//! Nocturn Music streaming (unofficial, opt-in): tools, stream links, the
//! local relay and streaming tracks.
//!
//! How a streamed song plays:
//! 1. The song is a normal library row (`source = 'youtube'`, `path =
//!    'youtube:<video id>'`), so queues, playlists, likes and history work.
//! 2. The player asks `media.localhost/track/<id>` as for a local file.
//! 3. NOCTURN gets the audio link with yt-dlp (cached until it expires), and
//!    relays the requested byte range with the headers YouTube requires —
//!    the browser can't play the link directly (YouTube answers 403).
//!
//! Failure handling found while testing: links expire after ~6 h and are
//! re-fetched on 403/410; a removed/blocked/age-restricted video is replaced
//! by the next-best match; an outdated yt-dlp is updated and retried once;
//! YouTube's bot check is reported plainly; two requests for the same video
//! share one extraction; extraction without a JavaScript runtime is
//! deprecated, so Deno is installed alongside yt-dlp.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::music::Served;
use crate::music_import::RemoteTrack;
use crate::music_stream::{self, Candidate};
use crate::security::exec::{self, SafeCommand, VideoId};

const TOOLS_KEY: &str = "stream.tools";
const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const YTDLP_SUMS: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
const YTDLP_LATEST: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest";
const DENO_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const DENO_SUM: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip.sha256sum";
/// Bytes relayed per request; the player asks for the rest.
const CHUNK: u64 = 1024 * 1024;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

// ------------------------------------------------------------------ tools

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ToolsRecord {
    ytdlp_sha: String,
    ytdlp_version: String,
    deno_sha: String,
    installed_at: String,
    checked_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub enabled: bool,
    pub installed: bool,
    pub ytdlp_version: Option<String>,
    pub installed_at: Option<String>,
    pub checked_at: Option<String>,
    pub streaming_tracks: i64,
    /// Streaming songs in no playlist and not liked.
    pub unused_tracks: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallProgress {
    pub stage: String,
    /// 0–1 for the current download, if known.
    pub fraction: Option<f32>,
}

pub struct Tools {
    pub ytdlp: PathBuf,
    pub deno: PathBuf,
}

fn tools_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("tools")
}

fn record(db: &Db) -> Option<ToolsRecord> {
    db.get_setting::<ToolsRecord>(TOOLS_KEY).ok().flatten()
}

pub fn status(db: &Db, data_dir: &Path, enabled: bool) -> Status {
    let r = record(db);
    let dir = tools_dir(data_dir);
    let installed = r.is_some() && dir.join("yt-dlp.exe").is_file() && dir.join("deno.exe").is_file();
    let n = db.conn().query_row("SELECT COUNT(*) FROM music_tracks WHERE source = 'youtube'", [], |x| x.get(0)).unwrap_or(0);
    let unused = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM music_tracks WHERE source = 'youtube' AND id NOT IN (SELECT track_id FROM music_playlist_tracks) AND id NOT IN (SELECT track_id FROM music_likes)",
            [],
            |x| x.get(0),
        )
        .unwrap_or(0);
    Status {
        enabled,
        installed,
        ytdlp_version: r.as_ref().map(|r| r.ytdlp_version.clone()),
        installed_at: r.as_ref().map(|r| r.installed_at.clone()),
        checked_at: r.map(|r| r.checked_at),
        streaming_tracks: n,
        unused_tracks: unused,
    }
}

fn hex_sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn file_sha(p: &Path) -> AppResult<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The SHA-256 for `file` in a checksum list ("<hash>  <name>" lines, or
/// PowerShell's "Hash : <HASH>" for a single file when `file` is None).
pub fn checksum_for(list: &str, file: Option<&str>) -> Option<String> {
    let is_hex = |w: &str| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit());
    match file {
        Some(name) => list.lines().find_map(|l| {
            let mut it = l.split_whitespace();
            let (h, n) = (it.next()?, it.next()?);
            (is_hex(h) && n.trim_start_matches('*') == name).then(|| h.to_ascii_lowercase())
        }),
        None => list.split(|c: char| !c.is_ascii_hexdigit()).find(|w| is_hex(w)).map(|h| h.to_ascii_lowercase()),
    }
}

fn http() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::msg(format!("network setup failed: {e}")))
}

/// One shared client for relaying audio (keeps connections to YouTube warm),
/// with no default headers of its own.
fn relay_client() -> AppResult<reqwest::Client> {
    static C: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(c) = C.get() {
        return Ok(c.clone());
    }
    let c = reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).pool_idle_timeout(Duration::from_secs(90)).build().map_err(|e| AppError::msg(format!("network setup failed: {e}")))?;
    Ok(C.get_or_init(|| c).clone())
}

async fn download(url: &str, max: usize, progress: &(dyn Fn(Option<f32>) + Sync)) -> AppResult<Vec<u8>> {
    let res = http()?.get(url).timeout(Duration::from_secs(600)).send().await.map_err(|e| AppError::msg(format!("download failed: {}", e.without_url())))?;
    if !res.status().is_success() {
        return Err(AppError::msg(format!("download failed: GitHub answered {}", res.status())));
    }
    let total = res.content_length();
    let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut s = res.bytes_stream();
    while let Some(chunk) = s.next().await {
        let chunk = chunk.map_err(|e| AppError::msg(format!("download interrupted: {}", e.without_url())))?;
        out.extend_from_slice(&chunk);
        if out.len() > max {
            return Err(AppError::msg("download larger than expected — stopped"));
        }
        progress(total.map(|t| out.len() as f32 / t as f32));
    }
    Ok(out)
}

/// Write a file via a temporary name so a half-written file is never used.
fn write_atomic(p: &Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = p.with_extension("partial");
    std::fs::write(&tmp, bytes)?;
    if p.exists() {
        // A running yt-dlp keeps its exe locked; move it aside first.
        let old = p.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(p, &old)?;
        let _ = std::fs::remove_file(&old);
    }
    std::fs::rename(&tmp, p)?;
    Ok(())
}

async fn fetch_ytdlp(dir: &Path, progress: &(dyn Fn(InstallProgress) + Sync)) -> AppResult<String> {
    progress(InstallProgress { stage: "Downloading yt-dlp".into(), fraction: Some(0.0) });
    let sums = String::from_utf8_lossy(&download(YTDLP_SUMS, 64 * 1024, &|_| {}).await?).into_owned();
    let want = checksum_for(&sums, Some("yt-dlp.exe")).ok_or_else(|| AppError::msg("yt-dlp's checksum list had no entry for yt-dlp.exe"))?;
    let exe = download(YTDLP_URL, 80 * 1024 * 1024, &|f| progress(InstallProgress { stage: "Downloading yt-dlp".into(), fraction: f })).await?;
    let got = hex_sha(&exe);
    if got != want {
        return Err(AppError::msg("the downloaded yt-dlp didn't match its published checksum — not installed"));
    }
    write_atomic(&dir.join("yt-dlp.exe"), &exe)?;
    Ok(got)
}

async fn fetch_deno(dir: &Path, progress: &(dyn Fn(InstallProgress) + Sync)) -> AppResult<String> {
    progress(InstallProgress { stage: "Downloading Deno (JavaScript runtime yt-dlp needs for YouTube)".into(), fraction: Some(0.0) });
    let sum = String::from_utf8_lossy(&download(DENO_SUM, 64 * 1024, &|_| {}).await?).into_owned();
    let want = checksum_for(&sum, None).ok_or_else(|| AppError::msg("Deno's checksum file couldn't be read"))?;
    let zip = download(DENO_URL, 200 * 1024 * 1024, &|f| progress(InstallProgress { stage: "Downloading Deno (JavaScript runtime yt-dlp needs for YouTube)".into(), fraction: f })).await?;
    if hex_sha(&zip) != want {
        return Err(AppError::msg("the downloaded Deno didn't match its published checksum — not installed"));
    }
    progress(InstallProgress { stage: "Unpacking Deno".into(), fraction: None });
    let archive = dir.join("deno.zip");
    let unpack = dir.join("deno-unpack");
    let _ = std::fs::remove_dir_all(&unpack);
    std::fs::create_dir_all(&unpack)?;
    std::fs::write(&archive, &zip)?;
    let out = exec::run(&SafeCommand::TarExtract { archive: archive.clone(), dest: unpack.clone() });
    let _ = std::fs::remove_file(&archive);
    let out = out?;
    if !out.success() {
        let _ = std::fs::remove_dir_all(&unpack);
        return Err(AppError::msg(format!("couldn't unpack Deno: {}", out.stderr.trim())));
    }
    let bytes = std::fs::read(unpack.join("deno.exe"));
    let _ = std::fs::remove_dir_all(&unpack);
    let bytes = bytes.map_err(|_| AppError::msg("the Deno download had no deno.exe"))?;
    write_atomic(&dir.join("deno.exe"), &bytes)?;
    Ok(hex_sha(&bytes))
}

fn ytdlp_version(exe: &Path) -> AppResult<String> {
    let out = exec::run(&SafeCommand::YtDlpVersion { exe: exe.to_path_buf() })?;
    let v = out.stdout.trim().to_string();
    if !out.success() || v.is_empty() || v.len() > 40 {
        return Err(AppError::msg("yt-dlp didn't start"));
    }
    Ok(v)
}

/// Download (or repair) yt-dlp and Deno. Safe to call again.
pub async fn install(db: &Db, data_dir: &Path, progress: &(dyn Fn(InstallProgress) + Sync)) -> AppResult<()> {
    let dir = tools_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let mut rec = record(db).unwrap_or_default();
    let ok = |p: &Path, sha: &str| !sha.is_empty() && p.is_file() && file_sha(p).map(|h| h == sha).unwrap_or(false);
    if !ok(&dir.join("yt-dlp.exe"), &rec.ytdlp_sha) {
        rec.ytdlp_sha = fetch_ytdlp(&dir, progress).await?;
    }
    if !ok(&dir.join("deno.exe"), &rec.deno_sha) {
        rec.deno_sha = fetch_deno(&dir, progress).await?;
    }
    progress(InstallProgress { stage: "Checking".into(), fraction: None });
    rec.ytdlp_version = ytdlp_version(&dir.join("yt-dlp.exe"))?;
    rec.installed_at = now();
    rec.checked_at = now();
    db.set_setting(TOOLS_KEY, &rec)?;
    *VERIFIED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    db.audit("info", "user", "music.stream.install", Some(serde_json::json!({ "yt_dlp": rec.ytdlp_version })));
    Ok(())
}

pub fn remove(db: &Db, data_dir: &Path) -> AppResult<()> {
    let _ = std::fs::remove_dir_all(tools_dir(data_dir));
    db.conn().execute("DELETE FROM settings WHERE key = ?1", [TOOLS_KEY])?;
    *VERIFIED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

/// Tools verified against the recorded checksums (once per session).
static VERIFIED: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn tools(db: &Db, data_dir: &Path) -> Result<Tools, StreamError> {
    let dir = tools_dir(data_dir);
    let t = Tools { ytdlp: dir.join("yt-dlp.exe"), deno: dir.join("deno.exe") };
    let mut v = VERIFIED.lock().unwrap_or_else(|e| e.into_inner());
    if v.as_deref() == Some(dir.as_path()) {
        return Ok(t);
    }
    let rec = record(db).ok_or(StreamError::NotInstalled)?;
    let good = |p: &Path, sha: &str| file_sha(p).map(|h| h == sha).unwrap_or(false);
    if !good(&t.ytdlp, &rec.ytdlp_sha) || !good(&t.deno, &rec.deno_sha) {
        return Err(StreamError::Other("the streaming tools changed on disk since NOCTURN installed them — reinstall them in Music › Sources".into()));
    }
    *v = Some(dir);
    Ok(t)
}

/// Newest yt-dlp release tag (from GitHub's redirect), e.g. "2026.10.26".
async fn latest_ytdlp_tag() -> AppResult<String> {
    let c = reqwest::Client::builder().user_agent(UA).redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build().map_err(|e| AppError::msg(e.to_string()))?;
    let res = c.get(YTDLP_LATEST).send().await.map_err(|e| AppError::msg(format!("couldn't reach GitHub: {}", e.without_url())))?;
    let loc = res.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("");
    loc.rsplit("/tag/").next().filter(|t| !t.is_empty() && t.len() < 40 && *t != loc).map(String::from).ok_or_else(|| AppError::msg("couldn't read yt-dlp's latest version"))
}

/// Update yt-dlp if a newer release exists. Returns true if it changed.
pub async fn update(db: &Db, data_dir: &Path) -> AppResult<bool> {
    let mut rec = record(db).ok_or_else(|| AppError::Invalid("streaming isn't set up".into()))?;
    let tag = latest_ytdlp_tag().await?;
    rec.checked_at = now();
    let changed = tag != rec.ytdlp_version;
    if changed {
        let dir = tools_dir(data_dir);
        rec.ytdlp_sha = fetch_ytdlp(&dir, &|_| {}).await?;
        rec.ytdlp_version = ytdlp_version(&dir.join("yt-dlp.exe"))?;
        *VERIFIED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        tracing::info!(version = %rec.ytdlp_version, "yt-dlp updated");
    }
    db.set_setting(TOOLS_KEY, &rec)?;
    Ok(changed)
}

/// Daily update check (YouTube breaks old yt-dlp versions every few weeks).
pub async fn update_if_due(db: &Db, data_dir: &Path) {
    let Some(rec) = record(db) else { return };
    let due = chrono::DateTime::parse_from_rfc3339(&rec.checked_at).map(|t| chrono::Utc::now().signed_duration_since(t) > chrono::Duration::hours(24)).unwrap_or(true);
    if due {
        if let Err(e) = update(db, data_dir).await {
            tracing::info!("yt-dlp update check: {e}");
        }
    }
}

// ------------------------------------------------------------------ stream links

#[derive(Debug, Clone, PartialEq)]
pub enum StreamError {
    NotInstalled,
    Disabled,
    /// Removed, private, blocked in this country, or taken down.
    Unavailable(String),
    AgeRestricted,
    /// "Sign in to confirm you're not a bot".
    BotCheck,
    Other(String),
}

impl StreamError {
    pub fn message(&self) -> String {
        match self {
            StreamError::NotInstalled => "Streaming isn't set up — turn it on in Music › Sources.".into(),
            StreamError::Disabled => "Streaming is turned off in Music › Sources.".into(),
            StreamError::Unavailable(r) => format!("The YouTube video isn't available ({r}) and no other upload of this song fit."),
            StreamError::AgeRestricted => "YouTube only plays this upload when signed in (age-restricted), and no other upload fit.".into(),
            StreamError::BotCheck => "YouTube is asking to confirm you're not a bot. Wait a while and try again.".into(),
            StreamError::Other(m) => m.clone(),
        }
    }
    fn replaceable(&self) -> bool {
        matches!(self, StreamError::Unavailable(_) | StreamError::AgeRestricted)
    }
}

/// Sort yt-dlp's error output into something actionable.
pub fn classify(stderr: &str) -> StreamError {
    let e = stderr.to_lowercase();
    let line = stderr.lines().rev().find(|l| l.contains("ERROR")).unwrap_or("").trim().trim_start_matches("ERROR:").trim().to_string();
    if e.contains("not a bot") || e.contains("sign in to confirm you") && e.contains("bot") {
        StreamError::BotCheck
    } else if e.contains("age") && (e.contains("confirm your age") || e.contains("age-restricted") || e.contains("inappropriate")) {
        StreamError::AgeRestricted
    } else if e.contains("video unavailable")
        || e.contains("is unavailable")
        || e.contains("private video")
        || e.contains("has been removed")
        || e.contains("not available in your country")
        || e.contains("blocked it")
        || e.contains("copyright")
        || e.contains("terminated")
    {
        let reason = if e.contains("country") { "blocked in your country" } else if e.contains("private") { "private" } else if e.contains("copyright") { "removed for copyright" } else { "removed or unavailable" };
        StreamError::Unavailable(reason.into())
    } else if line.is_empty() {
        StreamError::Other("yt-dlp failed without saying why".into())
    } else {
        StreamError::Other(format!("yt-dlp: {}", line.chars().take(200).collect::<String>()))
    }
}

#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub mime: String,
    pub total: Option<u64>,
    /// Unix seconds.
    pub expires: i64,
    /// YouTube refuses a fresh link (403) until this time (unix seconds) —
    /// yt-dlp's "Sleeping … as required by the site".
    pub available_at: Option<f64>,
}

/// Read yt-dlp's `-J` output for the chosen audio format.
pub fn parse_info(json: &str) -> Result<StreamInfo, StreamError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| StreamError::Other("yt-dlp's output couldn't be read".into()))?;
    let url = v.get("url").and_then(|u| u.as_str()).filter(|u| u.starts_with("https://")).ok_or_else(|| StreamError::Other("yt-dlp found no audio stream".into()))?;
    let host_ok = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.ends_with(".googlevideo.com") || h.ends_with(".youtube.com"))).unwrap_or(false);
    if !host_ok {
        return Err(StreamError::Other("yt-dlp returned an unexpected stream host".into()));
    }
    let headers = v
        .get("http_headers")
        .and_then(|h| h.as_object())
        .map(|m| m.iter().filter_map(|(k, x)| x.as_str().map(|s| (k.clone(), s.to_string()))).filter(|(k, _)| !k.eq_ignore_ascii_case("range")).collect())
        .unwrap_or_default();
    let ext = v.get("ext").and_then(|e| e.as_str()).unwrap_or("");
    let mime = match ext {
        "m4a" | "mp4" => "audio/mp4",
        "webm" => "audio/webm",
        "mp3" => "audio/mpeg",
        "opus" | "ogg" => "audio/ogg",
        _ => "audio/mp4",
    }
    .to_string();
    let q = reqwest::Url::parse(url).ok();
    let param = |k: &str| q.as_ref().and_then(|u| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.to_string()));
    let total = v.get("filesize").and_then(|f| f.as_u64()).or_else(|| param("clen").and_then(|c| c.parse().ok()));
    let expires = param("expire").and_then(|e| e.parse().ok()).unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600);
    let available_at = v.get("available_at").and_then(|a| a.as_f64());
    Ok(StreamInfo { url: url.to_string(), headers, mime, total, expires, available_at })
}

static CACHE: Mutex<Option<HashMap<String, StreamInfo>>> = Mutex::new(None);
static INFLIGHT: Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> = Mutex::new(None);

fn cached(vid: &str) -> Option<StreamInfo> {
    let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.as_ref().and_then(|m| m.get(vid)).filter(|i| i.expires - 600 > chrono::Utc::now().timestamp()).cloned()
}

fn forget(vid: &str) {
    if let Some(m) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(vid);
    }
}

fn extract(t: &Tools, vid: &str) -> Result<StreamInfo, StreamError> {
    let video = VideoId::parse(vid).map_err(|e| StreamError::Other(e.to_string()))?;
    let out = exec::run(&SafeCommand::YtDlpExtract { exe: t.ytdlp.clone(), deno: t.deno.clone(), video }).map_err(|e| StreamError::Other(e.to_string()))?;
    if !out.success() {
        return Err(classify(&out.stderr));
    }
    parse_info(&out.stdout)
}

/// The audio link for a video, from cache or yt-dlp. One extraction per video
/// at a time; an unexplained failure updates yt-dlp and tries once more.
pub fn stream_info(db: &Db, data_dir: &Path, vid: &str) -> Result<StreamInfo, StreamError> {
    if let Some(i) = cached(vid) {
        return Ok(i);
    }
    let lock = INFLIGHT.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).entry(vid.to_string()).or_default().clone();
    let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = cached(vid) {
        return Ok(i);
    }
    let t = tools(db, data_dir)?;
    let started = std::time::Instant::now();
    let info = match extract(&t, vid) {
        Err(StreamError::Other(m)) => {
            tracing::warn!("stream extraction failed ({m}); checking for a yt-dlp update");
            let updated = tauri::async_runtime::block_on(update(db, data_dir)).unwrap_or(false);
            if updated {
                extract(&tools(db, data_dir)?, vid)?
            } else {
                return Err(StreamError::Other(m));
            }
        }
        r => r?,
    };
    tracing::debug!(vid, ms = started.elapsed().as_millis() as u64, "stream link ready");
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(vid.to_string(), info.clone());
    Ok(info)
}

// ------------------------------------------------------------------ relay

/// Sleep until YouTube accepts a fresh link (at most 10 s).
fn wait_until_available(info: &StreamInfo) {
    if let Some(at) = info.available_at {
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        let wait = (at - now + 0.3).clamp(0.0, 10.0);
        if wait > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(wait));
        }
    }
}

/// Fetch one byte range of the stream. `Ok(None)` = the link was refused (expired).
async fn fetch_range(info: &StreamInfo, start: u64, end: u64) -> AppResult<Option<(Vec<u8>, u64, u64, Option<u64>)>> {
    // Exactly the headers yt-dlp says this link needs. (A client-level
    // User-Agent plus this one would send two, and YouTube answers 403.)
    let mut headers = reqwest::header::HeaderMap::new();
    for (k, v) in &info.headers {
        if let (Ok(k), Ok(v)) = (reqwest::header::HeaderName::from_bytes(k.as_bytes()), reqwest::header::HeaderValue::from_str(v)) {
            headers.insert(k, v);
        }
    }
    if !headers.contains_key(reqwest::header::USER_AGENT) {
        headers.insert(reqwest::header::USER_AGENT, reqwest::header::HeaderValue::from_static(UA));
    }
    headers.insert(reqwest::header::RANGE, reqwest::header::HeaderValue::from_str(&format!("bytes={start}-{end}")).map_err(|e| AppError::msg(e.to_string()))?);
    let client = relay_client()?;
    let res = client.get(&info.url).headers(headers).timeout(Duration::from_secs(30)).send().await.map_err(|e| AppError::msg(format!("stream interrupted: {}", e.without_url())))?;
    let st = res.status().as_u16();
    if st == 403 || st == 410 || st == 401 {
        return Ok(None);
    }
    if st == 416 {
        return Err(AppError::msg("range past the end of the stream"));
    }
    if !(200..300).contains(&st) {
        return Err(AppError::msg(format!("YouTube answered {st}")));
    }
    // "bytes 0-1048575/6891519"
    let cr = res.headers().get("content-range").and_then(|v| v.to_str().ok()).map(String::from);
    let total = cr.as_deref().and_then(|c| c.rsplit('/').next()).and_then(|t| t.parse().ok()).or(info.total);
    let body = res.bytes().await.map_err(|e| AppError::msg(format!("stream interrupted: {}", e.without_url())))?.to_vec();
    let (s, e) = match cr.as_deref().and_then(|c| c.strip_prefix("bytes ")).and_then(|c| c.split('/').next()).and_then(|r| r.split_once('-')) {
        Some((a, b)) => (a.parse().unwrap_or(start), b.parse().unwrap_or(start + body.len() as u64 - 1)),
        None => (start, start + body.len().max(1) as u64 - 1),
    };
    Ok(Some((body, s, e, total)))
}

/// Last error per streaming track, for the player's message.
static LAST_ERROR: Mutex<Option<HashMap<i64, String>>> = Mutex::new(None);

fn set_error(id: i64, msg: Option<String>) {
    let mut g = LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    match msg {
        Some(s) => {
            m.insert(id, s);
        }
        None => {
            m.remove(&id);
        }
    }
}

pub fn last_error(id: i64) -> Option<String> {
    LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&id).cloned())
}

fn video_of(path: &str) -> Option<&str> {
    path.strip_prefix("youtube:").filter(|v| music_stream::valid_video_id(v))
}

fn fail(status: u16, msg: &str) -> Served {
    Served { status, mime: "text/plain".into(), body: msg.as_bytes().to_vec(), range: None }
}

/// Serve `/track/<id>` for a streaming track: relay the requested range.
pub fn serve_track(db: &Db, data_dir: &Path, enabled: bool, id: i64, path: &str, range: Option<&str>) -> Served {
    if !enabled {
        set_error(id, Some(StreamError::Disabled.message()));
        return fail(403, "streaming is off");
    }
    let Some(mut vid) = video_of(path).map(String::from) else { return fail(404, "not a stream") };
    let mut replaced = false;
    let mut early_retry_done = false;
    for _attempt in 0..3 {
        let info = match stream_info(db, data_dir, &vid) {
            Ok(i) => i,
            Err(e) if e.replaceable() && !replaced => {
                // The video went away: find another upload of the same song.
                match replace_video(db, id, &vid) {
                    Ok(Some(new_vid)) => {
                        tracing::info!(id, old = %vid, new = %new_vid, "replaced unavailable stream");
                        vid = new_vid;
                        replaced = true;
                        continue;
                    }
                    _ => {
                        set_error(id, Some(e.message()));
                        return fail(502, &e.message());
                    }
                }
            }
            Err(e) => {
                set_error(id, Some(e.message()));
                return fail(502, &e.message());
            }
        };
        let total = info.total.unwrap_or(0);
        let (start, end) = crate::music::parse_range(range, total).unwrap_or((0, total.saturating_sub(1)));
        if total > 0 && start >= total {
            return Served { status: 416, mime: info.mime, body: Vec::new(), range: Some((0, 0, total)) };
        }
        let end = if total > 0 { end.min(start + CHUNK - 1).min(total - 1) } else { start + CHUNK - 1 };
        wait_until_available(&info);
        match tauri::async_runtime::block_on(fetch_range(&info, start, end)) {
            Ok(Some((body, s, e, t))) => {
                set_error(id, None);
                return Served { status: 206, mime: info.mime, body, range: Some((s, e, t.unwrap_or(e + 1))) };
            }
            Ok(None) => {
                // Refused: YouTube may still be early on a fresh link — wait once more
                // before treating it as expired and fetching a new one.
                if !early_retry_done {
                    early_retry_done = true;
                    std::thread::sleep(Duration::from_millis(2000));
                    if let Ok(Some((body, s, e, t))) = tauri::async_runtime::block_on(fetch_range(&info, start, end)) {
                        set_error(id, None);
                        return Served { status: 206, mime: info.mime, body, range: Some((s, e, t.unwrap_or(e + 1))) };
                    }
                }
                forget(&vid);
                continue;
            }
            Err(e) => {
                set_error(id, Some(e.to_string()));
                return fail(502, &e.to_string());
            }
        }
    }
    let msg = "YouTube kept refusing the stream. Try again in a moment.";
    set_error(id, Some(msg.into()));
    fail(502, msg)
}

/// The whole stream in memory (for measuring its loudness), with a file
/// extension for the format. At most 40 MB.
pub fn fetch_all(db: &Db, data_dir: &Path, vid: &str) -> AppResult<(Vec<u8>, &'static str)> {
    const MAX: u64 = 40 * 1024 * 1024;
    let mut info = stream_info(db, data_dir, vid).map_err(|e| AppError::msg(e.message()))?;
    let ext = match info.mime.as_str() {
        "audio/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        _ => "m4a",
    };
    let mut out: Vec<u8> = Vec::new();
    let mut refused = false;
    wait_until_available(&info);
    loop {
        let start = out.len() as u64;
        let total = info.total.unwrap_or(MAX).min(MAX);
        if start >= total {
            break;
        }
        let end = (start + CHUNK - 1).min(total - 1);
        match tauri::async_runtime::block_on(fetch_range(&info, start, end))? {
            Some((body, _, _, t)) => {
                if body.is_empty() {
                    break;
                }
                if info.total.is_none() {
                    info.total = t;
                }
                out.extend_from_slice(&body);
                if t.is_none() && (body.len() as u64) < end - start + 1 {
                    break;
                }
            }
            None if !refused => {
                refused = true;
                forget(vid);
                info = stream_info(db, data_dir, vid).map_err(|e| AppError::msg(e.message()))?;
                wait_until_available(&info);
            }
            None => return Err(AppError::msg("YouTube refused the stream")),
        }
    }
    Ok((out, ext))
}

/// Warm the stream link for a track (called for the next song in the queue).
pub fn prepare(db: &Db, data_dir: &Path, id: i64) {
    let path: Option<String> = db.conn().query_row("SELECT path FROM music_tracks WHERE id = ?1 AND source = 'youtube'", [id], |r| r.get(0)).optional().ok().flatten();
    if let Some(v) = path.as_deref().and_then(video_of) {
        let _ = stream_info(db, data_dir, v);
    }
}

/// Thumbnail as cover art (cached).
pub fn serve_art(data_dir: &Path, path: &str) -> Served {
    let Some(vid) = video_of(path) else { return fail(404, "no art") };
    let dir = data_dir.join("stream-art");
    let file = dir.join(format!("{vid}.jpg"));
    if let Ok(b) = std::fs::read(&file) {
        return Served { status: 200, mime: "image/jpeg".into(), body: b, range: None };
    }
    let url = format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg");
    let got = tauri::async_runtime::block_on(async {
        let res = http()?.get(&url).timeout(Duration::from_secs(15)).send().await.map_err(|e| AppError::msg(e.to_string()))?;
        if !res.status().is_success() {
            return Err(AppError::msg("no thumbnail"));
        }
        Ok::<_, AppError>(res.bytes().await.map_err(|e| AppError::msg(e.to_string()))?.to_vec())
    });
    match got {
        Ok(b) if b.len() < 2 * 1024 * 1024 => {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&file, &b);
            Served { status: 200, mime: "image/jpeg".into(), body: b, range: None }
        }
        _ => fail(404, "no art"),
    }
}

// ------------------------------------------------------------------ streaming tracks

/// Add (or reuse) the library row for a song streamed from a video.
pub fn ensure_track(db: &Db, want: &RemoteTrack, c: &Candidate) -> AppResult<i64> {
    let path = format!("youtube:{}", c.video_id);
    let (vartist, vtitle) = music_stream_split(&c.title, &c.channel);
    let title = if want.title.trim().is_empty() { vtitle } else { want.title.clone() };
    let artist = if want.artist.trim().is_empty() { vartist } else { want.artist.clone() };
    let album_artist = artist.split([',', '&']).next().unwrap_or(&artist).trim().to_string();
    let dur = c.duration_ms.or(want.duration_ms).unwrap_or(0);
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO music_tracks(source, path, folder, title, artist, album, album_artist, duration_ms, has_art, size, mtime, added_at)
         VALUES ('youtube', ?1, 'youtube', ?2, ?3, ?4, ?5, ?6, 1, 0, 0, ?7)",
        params![path, title.chars().take(300).collect::<String>(), artist.chars().take(300).collect::<String>(), want.album.chars().take(300).collect::<String>(), album_artist, dur, now()],
    )?;
    Ok(conn.query_row("SELECT id FROM music_tracks WHERE path = ?1", [&path], |r| r.get(0))?)
}

fn music_stream_split(title: &str, channel: &str) -> (String, String) {
    crate::music_import::split_video_title(title, channel)
}

#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    pub index: usize,
    pub track_id: Option<i64>,
    pub confidence: Option<String>,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveProgress {
    pub done: usize,
    pub total: usize,
    pub found: usize,
}

/// Find a video for each song (4 at a time) and add streaming tracks.
/// Songs from a YouTube playlist already carry their video ID.
pub async fn resolve_many(db: &Db, items: Vec<RemoteTrack>, progress: &(dyn Fn(ResolveProgress) + Sync)) -> Vec<Resolved> {
    let total = items.len();
    let mut out: Vec<Resolved> = Vec::with_capacity(total);
    let mut found = 0;
    let mut stream = futures_util::stream::iter(items.into_iter().enumerate())
        .map(|(i, t)| async move {
            if let Some(v) = t.video_id.as_deref().filter(|v| music_stream::valid_video_id(v)) {
                let c = Candidate { video_id: v.into(), title: t.title.clone(), channel: t.artist.clone(), duration_ms: t.duration_ms, official: false };
                return (i, t, Ok(Some(music_stream::Pick { candidate: c, confidence: "exact", score: 0.0 })));
            }
            let r = music_stream::resolve(&t).await;
            (i, t, r)
        })
        .buffer_unordered(4);
    while let Some((i, t, r)) = stream.next().await {
        let res = match r {
            Ok(Some(p)) => match ensure_track(db, &t, &p.candidate) {
                Ok(id) => {
                    found += 1;
                    Resolved { index: i, track_id: Some(id), confidence: Some(p.confidence.into()), problem: None }
                }
                Err(e) => Resolved { index: i, track_id: None, confidence: None, problem: Some(e.to_string()) },
            },
            Ok(None) => Resolved { index: i, track_id: None, confidence: None, problem: Some("no upload on YouTube fit this song".into()) },
            Err(e) => Resolved { index: i, track_id: None, confidence: None, problem: Some(e.to_string()) },
        };
        out.push(res);
        progress(ResolveProgress { done: out.len(), total, found });
    }
    out.sort_by_key(|r| r.index);
    db.audit("info", "user", "music.stream.resolve", Some(serde_json::json!({ "songs": total, "found": found })));
    out
}

/// "Wrong song": point a streaming track at a different upload (never the
/// current one again). Returns the new video ID, or None if nothing else fits.
pub fn rematch(db: &Db, id: i64) -> AppResult<Option<String>> {
    let path: Option<String> = db.conn().query_row("SELECT path FROM music_tracks WHERE id = ?1 AND source = 'youtube'", [id], |r| r.get(0)).optional()?;
    let path = path.ok_or_else(|| AppError::NotFound("streaming song".into()))?;
    let current = video_of(&path).ok_or_else(|| AppError::NotFound("streaming song".into()))?.to_string();
    forget(&current);
    let r = replace_video(db, id, &current)?;
    db.audit("info", "user", "music.stream.rematch", Some(serde_json::json!({ "from": current, "to": r })));
    Ok(r)
}

/// A streaming track's video stopped working: search again (excluding it)
/// and point the track at the new video. Playlists keep the same track.
fn replace_video(db: &Db, id: i64, bad: &str) -> AppResult<Option<String>> {
    let row: Option<(String, String, String, i64)> =
        db.conn().query_row("SELECT title, artist, album, duration_ms FROM music_tracks WHERE id = ?1 AND source = 'youtube'", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
    let Some((title, artist, album, dur)) = row else { return Ok(None) };
    let want = RemoteTrack { title, artist, album, duration_ms: (dur > 0).then_some(dur), path: None, video_id: None };
    let exclude: HashSet<String> = [bad.to_string()].into();
    let pick = tauri::async_runtime::block_on(music_stream::resolve_excluding(&want, &exclude))?;
    let Some(p) = pick else { return Ok(None) };
    let new_path = format!("youtube:{}", p.candidate.video_id);
    let conn = db.conn();
    // Another track may already use that video: merge into it.
    let existing: Option<i64> = conn.query_row("SELECT id FROM music_tracks WHERE path = ?1", [&new_path], |r| r.get(0)).optional()?;
    match existing {
        Some(other) if other != id => {
            conn.execute("UPDATE music_tracks SET path = ?1 WHERE id = ?2", params![format!("youtube-gone:{bad}:{id}"), id])?;
            conn.execute("UPDATE music_playlist_tracks SET track_id = ?1 WHERE track_id = ?2", params![other, id])?;
            conn.execute("UPDATE OR IGNORE music_likes SET track_id = ?1 WHERE track_id = ?2", params![other, id])?;
            conn.execute("DELETE FROM music_tracks WHERE id = ?1", [id])?;
        }
        _ => {
            conn.execute("UPDATE music_tracks SET path = ?1 WHERE id = ?2", params![new_path, id])?;
        }
    }
    Ok(Some(p.candidate.video_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_from_both_formats() {
        let sums = "0123  yt-dlp\nf5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d  yt-dlp.exe\naaaa  yt-dlp_x86.exe\n";
        assert_eq!(checksum_for(sums, Some("yt-dlp.exe")).as_deref(), Some("f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d"));
        assert_eq!(checksum_for(sums, Some("yt-dlp_min.exe")), None);
        let ps = "\nAlgorithm : SHA256\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\nPath      : C:\\a\\deno.zip\n";
        assert_eq!(checksum_for(ps, None).as_deref(), Some("cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf"));
    }

    #[test]
    fn yt_dlp_errors_are_classified() {
        assert_eq!(classify("ERROR: [youtube] aaaaaaaaaaa: This video is unavailable"), StreamError::Unavailable("removed or unavailable".into()));
        assert_eq!(classify("ERROR: [youtube] x: Video unavailable. The uploader has not made this video available in your country"), StreamError::Unavailable("blocked in your country".into()));
        assert_eq!(classify("ERROR: [youtube] x: Sign in to confirm your age. This video may be inappropriate for some users."), StreamError::AgeRestricted);
        assert_eq!(classify("ERROR: [youtube] x: Sign in to confirm you’re not a bot. Use --cookies-from-browser"), StreamError::BotCheck);
        assert!(matches!(classify("ERROR: [youtube] x: nsig extraction failed"), StreamError::Other(m) if m.contains("nsig")));
        assert!(StreamError::Unavailable("x".into()).replaceable());
        assert!(!StreamError::BotCheck.replaceable());
    }

    #[test]
    fn stream_info_is_parsed_and_checked() {
        let j = r#"{"url":"https://rr3---sn-x.googlevideo.com/videoplayback?expire=1794072899&clen=6891519&itag=140","ext":"m4a","http_headers":{"User-Agent":"UA","Accept":"*/*","Range":"x"}}"#;
        let i = parse_info(j).unwrap();
        assert_eq!((i.mime.as_str(), i.total, i.expires), ("audio/mp4", Some(6_891_519), 1_794_072_899));
        assert_eq!(i.headers.len(), 2); // Range is ours to set
        assert_eq!(parse_info(r#"{"url":"https://r.googlevideo.com/v?expire=1","available_at":1794052098}"#).unwrap().available_at, Some(1_794_052_098.0));
        assert!(parse_info(r#"{"url":"https://evil.example/x","ext":"m4a"}"#).is_err());
        assert!(parse_info(r#"{"url":"http://rr1.googlevideo.com/x"}"#).is_err());
        assert!(parse_info("not json").is_err());
    }

    #[test]
    fn streaming_tracks_are_reused_and_survive_scans() {
        let db = Db::open_in_memory().unwrap();
        let want = RemoteTrack { title: "Hey Jude".into(), artist: "The Beatles".into(), album: String::new(), duration_ms: Some(429_000), path: None, video_id: None };
        let c = Candidate { video_id: "mQER0A0ej0M".into(), title: "Hey Jude (Remastered 2015)".into(), channel: "The Beatles - Topic".into(), duration_ms: Some(426_000), official: true };
        let a = ensure_track(&db, &want, &c).unwrap();
        assert_eq!(ensure_track(&db, &want, &c).unwrap(), a);
        let t = crate::music::tracks(&db).unwrap();
        assert_eq!((t[0].source.as_str(), t[0].title.as_str(), t[0].duration_ms, t[0].has_art), ("youtube", "Hey Jude", 426_000, true));
        // (Scans leaving streaming tracks alone is checked in music::tests::scan_library_and_serve.)
        // Unused streaming songs are pruned — but not within the grace period, nor when liked.
        assert_eq!(crate::music::prune_streams(&db, chrono::Duration::hours(1)).unwrap(), 0);
        crate::music::set_like(&db, a, true).unwrap();
        assert_eq!(crate::music::prune_streams(&db, chrono::Duration::seconds(-5)).unwrap(), 0);
        crate::music::set_like(&db, a, false).unwrap();
        assert_eq!(crate::music::prune_streams(&db, chrono::Duration::seconds(-5)).unwrap(), 1);
        assert_eq!(video_of("youtube:mQER0A0ej0M"), Some("mQER0A0ej0M"));
        assert_eq!(video_of("youtube:../x"), None);
        assert_eq!(video_of("C:\\music\\a.mp3"), None);
    }

    /// Live: real extraction and a relayed byte range (needs the tools installed in a temp dir).
    #[test]
    #[ignore]
    fn live_install_extract_and_relay() {
        let dir = std::env::temp_dir().join("nocturn-stream-test");
        let db = Db::open_in_memory().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(install(&db, &dir, &|p| println!("{} {:?}", p.stage, p.fraction.map(|f| (f * 100.0) as i32)))).unwrap();
        println!("status: {:?}", status(&db, &dir, true));
        let t0 = std::time::Instant::now();
        let info = stream_info(&db, &dir, "mQER0A0ej0M").unwrap();
        println!("extract: {:?} total={:?} mime={}", t0.elapsed(), info.total, info.mime);
        let t1 = std::time::Instant::now();
        assert!(stream_info(&db, &dir, "mQER0A0ej0M").is_ok());
        assert!(t1.elapsed() < Duration::from_millis(50), "second lookup should be cached");
        println!("available_at in {:?} s", info.available_at.map(|a| a - chrono::Utc::now().timestamp_millis() as f64 / 1000.0));
        wait_until_available(&info);
        let (body, s, e, total) = rt.block_on(fetch_range(&info, 0, CHUNK - 1)).unwrap().expect("not refused");
        println!("relay: {} bytes {s}-{e}/{total:?}", body.len());
        assert_eq!(body.len() as u64, CHUNK);
        assert_eq!(&body[4..8], b"ftyp", "an MP4/M4A stream");
        assert!(matches!(stream_info(&db, &dir, "aaaaaaaaaaa"), Err(StreamError::Unavailable(_))));

        // Self-repair: a song pointing at a removed video gets another upload, transparently.
        let want = RemoteTrack { title: "Stand By Me".into(), artist: "Ben E. King".into(), album: String::new(), duration_ms: Some(180_000), path: None, video_id: None };
        let dead = Candidate { video_id: "aaaaaaaaaaa".into(), title: "Stand By Me".into(), channel: "Ben E. King".into(), duration_ms: Some(180_000), official: false };
        let id = ensure_track(&db, &want, &dead).unwrap();
        let t2 = std::time::Instant::now();
        let s = serve_track(&db, &dir, true, id, "youtube:aaaaaaaaaaa", Some("bytes=0-1023"));
        let path: String = db.conn().query_row("SELECT path FROM music_tracks WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        println!("self-repair: status {} in {:?}, now {path}", s.status, t2.elapsed());
        assert_eq!(s.status, 206, "{}", String::from_utf8_lossy(&s.body));
        assert!(path.starts_with("youtube:") && path != "youtube:aaaaaaaaaaa");
        assert_eq!(last_error(id), None);
        // Streaming off: refused with a clear reason.
        assert_eq!(serve_track(&db, &dir, false, id, &path, None).status, 403);
        assert!(last_error(id).unwrap().contains("turned off"));
    }
}
