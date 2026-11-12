//! Sync with Nocturn Music on the iPhone over the home Wi-Fi. No accounts:
//! the phone scans a QR code once (this PC's address, port and a secret key)
//! and from then on asks this PC for its playlists and liked songs, and sends
//! its own. Off until switched on in Music › Sync with iPhone.
//!
//! The server answers only (1) devices on the local network (private,
//! link-local or loopback addresses) that (2) send the secret key. It serves
//! two JSON calls and nothing else; nothing is ever opened to the internet.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::music;
use crate::music_import::{self, RemoteTrack};
use crate::music_stream::{valid_video_id, Candidate};
use crate::music_stream_engine::ensure_track;
use crate::state::AppState;

pub const PORT: u16 = 47821;
const ENABLED: &str = "phone_sync.enabled";
const KEY: &str = "phone_sync.key";
/// Phone playlist id → the PC playlist made from it (so a second sync updates it).
const MAP: &str = "phone_sync.map";
const LAST: &str = "phone_sync.last";
const MAX_BODY: usize = 20 * 1024 * 1024;

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTrack {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    /// YouTube video, when the song streams (always, from the phone).
    #[serde(default)]
    pub video_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlaylist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<SyncTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncLibrary {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub playlists: Vec<SyncPlaylist>,
    #[serde(default)]
    pub liked: Vec<SyncTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LastSync {
    pub device: String,
    pub at: String,
    pub playlists: usize,
    pub songs: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncInfo {
    pub enabled: bool,
    pub running: bool,
    pub address: Option<String>,
    pub port: u16,
    pub name: String,
    /// The QR code to scan with the phone (SVG), while sync is on.
    pub qr_svg: Option<String>,
    pub last: Option<LastSync>,
}

fn pc_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

/// This PC's address on the home network (the one used to reach other machines).
/// No packet is sent: connecting a UDP socket only picks the route.
fn lan_address() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.1.1:9").or_else(|_| s.connect("10.0.0.1:9")).ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

fn local_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
        IpAddr::V6(v) => {
            if let Some(m) = v.to_ipv4_mapped() {
                return m.is_private() || m.is_loopback() || m.is_link_local();
            }
            let seg = v.segments()[0];
            v.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    }
}

fn key(db: &Db) -> AppResult<String> {
    if let Some(k) = db.get_setting::<String>(KEY)?.filter(|k| k.len() >= 32) {
        return Ok(k);
    }
    let k = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    db.set_setting(KEY, &k)?;
    Ok(k)
}

fn same(a: &str, b: &str) -> bool {
    // length + every byte, without stopping early
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn info(db: &Db) -> AppResult<SyncInfo> {
    let enabled = db.get_setting::<bool>(ENABLED)?.unwrap_or(false);
    let address = lan_address().map(|a| a.to_string());
    let qr_svg = match (enabled, &address) {
        (true, Some(a)) => {
            // What the phone reads: everything it needs to find and talk to this PC.
            let payload = serde_json::json!({ "t": "nocturn-sync", "v": 1, "h": a, "p": PORT, "k": key(db)?, "n": pc_name() }).to_string();
            qrcode::QrCode::new(payload.as_bytes()).ok().map(|q| {
                q.render::<qrcode::render::svg::Color>()
                    .min_dimensions(240, 240)
                    .quiet_zone(true)
                    .dark_color(qrcode::render::svg::Color("#0b0c10"))
                    .light_color(qrcode::render::svg::Color("#ffffff"))
                    .build()
            })
        }
        _ => None,
    };
    Ok(SyncInfo { enabled, running: RUNNING.load(Ordering::SeqCst), address, port: PORT, name: pc_name(), qr_svg, last: db.get_setting(LAST)? })
}

pub fn set_enabled(app: &AppHandle, db: &Db, on: bool) -> AppResult<SyncInfo> {
    db.set_setting(ENABLED, &on)?;
    if on {
        start(app.clone());
    } else {
        RUNNING.store(false, Ordering::SeqCst);
    }
    db.audit("info", "user", "music.phone_sync", Some(serde_json::json!({ "enabled": on })));
    // give the server a moment to bind (or stop) so the status is right
    std::thread::sleep(Duration::from_millis(250));
    info(db)
}

/// A new key: the phone has to scan the code again (for a lost phone).
pub fn reset_key(db: &Db) -> AppResult<SyncInfo> {
    let k = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    db.set_setting(KEY, &k)?;
    info(db)
}

/// Start the server if sync is on (at start-up, or when switched on).
pub fn start(app: AppHandle) {
    let Some(st) = app.try_state::<Arc<AppState>>().map(|s| s.inner().clone()) else { return };
    let _ = APP.set(app.clone());
    if !st.db.get_setting::<bool>(ENABLED).ok().flatten().unwrap_or(false) || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("phone sync couldn't listen on port {PORT}: {e}");
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
        };
        let _ = listener.set_nonblocking(true);
        tracing::info!("phone sync listening on port {PORT}");
        while RUNNING.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    if !local_peer(peer.ip()) {
                        continue; // dropped: not from this network
                    }
                    let (app, st) = (app.clone(), st.clone());
                    std::thread::spawn(move || {
                        let notify = |last: &LastSync| {
                            let _ = app.emit("phone-sync", last);
                        };
                        if let Err(e) = handle(&st.db, &notify, stream) {
                            tracing::debug!("phone sync request failed: {e}");
                        }
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
                Err(_) => std::thread::sleep(Duration::from_millis(500)),
            }
        }
        tracing::info!("phone sync stopped");
    });
}

fn respond(s: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    write!(s, "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())?;
    s.flush()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub track: Option<SyncTrack>,
    /// Where the song is now (moved on by the time since it was saved, while playing).
    pub position_ms: i64,
    pub playing: bool,
    /// How long ago the PC last played it.
    pub age_ms: i64,
}

/// The PC's current song from the saved session (saved every 10 s while playing).
pub fn now_playing(db: &Db) -> AppResult<NowPlaying> {
    let s = music::session(db);
    let none = NowPlaying { track: None, position_ms: 0, playing: false, age_ms: 0 };
    let Some(&id) = s.queue.get(s.index) else { return Ok(none) };
    let Some(t) = music::tracks(db)?.into_iter().find(|t| t.id == id) else { return Ok(none) };
    let path = music_import::local_paths(db)?.into_iter().find(|(i, _)| *i == id).map(|(_, p)| p);
    let age = if s.saved_at > 0 { (chrono::Utc::now().timestamp_millis() - s.saved_at).max(0) } else { i64::MAX / 2 };
    let mut pos = s.position_ms.max(0);
    if s.playing {
        pos += age;
    }
    if t.duration_ms > 0 {
        pos = pos.min(t.duration_ms - 1000).max(0);
    }
    let track = SyncTrack {
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_ms: (t.duration_ms > 0).then_some(t.duration_ms),
        video_id: path.as_deref().and_then(|p| p.strip_prefix("youtube:")).filter(|v| valid_video_id(v)).map(String::from),
    };
    Ok(NowPlaying { track: Some(track), position_ms: pos, playing: s.playing, age_ms: age })
}

/// The app, so a request can tell the window to pause.
static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

fn take_over() {
    if let Some(app) = APP.get() {
        let _ = app.emit("phone-took-over", ());
    }
}

fn handle(db: &Db, notify: &dyn Fn(&LastSync), mut stream: TcpStream) -> std::io::Result<()> {
    let _ = stream.set_nonblocking(false);
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if headers.len() > 50 {
            return respond(&mut stream, 400, r#"{"error":"too many headers"}"#);
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let Ok(k) = key(db) else { return respond(&mut stream, 500, r#"{"error":"no key"}"#) };
    let sent = headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if !same(sent, &k) {
        return respond(&mut stream, 401, r#"{"error":"scan the QR code on your PC again"}"#);
    }
    let len: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return respond(&mut stream, 413, r#"{"error":"too much"}"#);
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;

    let result: AppResult<String> = match (method.as_str(), path.as_str()) {
        ("GET", "/sync/hello") => Ok(serde_json::json!({ "app": "NOCTURN", "name": pc_name(), "version": env!("CARGO_PKG_VERSION") }).to_string()),
        ("GET", "/sync/library") => export(db).and_then(|l| Ok(serde_json::to_string(&l)?)),
        // what's playing, so the phone can carry on from the same spot
        ("GET", "/sync/now") => now_playing(db).and_then(|n| Ok(serde_json::to_string(&n)?)),
        // the phone is taking over: tell the PC to pause
        ("POST", "/sync/take") => now_playing(db).and_then(|n| {
            if n.track.is_some() {
                take_over();
            }
            Ok(serde_json::to_string(&n)?)
        }),
        ("POST", "/sync/push") => serde_json::from_slice::<SyncLibrary>(&body)
            .map_err(|_| AppError::Invalid("that wasn't a library".into()))
            .and_then(|lib| receive(db, lib))
            .map(|last| {
                notify(&last);
                serde_json::to_string(&last).unwrap_or_default()
            }),
        _ => return respond(&mut stream, 404, r#"{"error":"not here"}"#),
    };
    match result {
        Ok(json) => respond(&mut stream, 200, &json),
        Err(e) => respond(&mut stream, 400, &serde_json::json!({ "error": e.to_string() }).to_string()),
    }
}

/// This PC's playlists and liked songs, for the phone. Playlists that came
/// from the phone are left out (the phone has them already).
pub fn export(db: &Db) -> AppResult<SyncLibrary> {
    let tracks = music::tracks(db)?;
    let paths: HashMap<i64, String> = music_import::local_paths(db)?.into_iter().collect();
    let by_id: HashMap<i64, &music::Track> = tracks.iter().map(|t| (t.id, t)).collect();
    let to_sync = |t: &music::Track| SyncTrack {
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_ms: (t.duration_ms > 0).then_some(t.duration_ms),
        video_id: paths.get(&t.id).and_then(|p| p.strip_prefix("youtube:")).filter(|v| valid_video_id(v)).map(String::from),
    };
    let from_phone: Vec<String> = db.get_setting::<HashMap<String, String>>(MAP)?.unwrap_or_default().into_values().collect();
    let playlists = music::playlists(db)?
        .into_iter()
        .filter(|p| !from_phone.contains(&p.id) && !p.tracks.is_empty())
        .map(|p| SyncPlaylist { id: p.id.clone(), name: p.name.clone(), tracks: p.tracks.iter().filter_map(|id| by_id.get(id).map(|t| to_sync(t))).collect() })
        .collect();
    let liked = tracks.iter().filter(|t| t.liked).map(to_sync).collect();
    Ok(SyncLibrary { device: pc_name(), playlists, liked })
}

/// The phone's playlists and liked songs, saved as streaming songs. A second
/// sync updates the same playlists; songs are never unliked here.
pub fn receive(db: &Db, lib: SyncLibrary) -> AppResult<LastSync> {
    let mut map: HashMap<String, String> = db.get_setting(MAP)?.unwrap_or_default();
    let existing: Vec<String> = music::playlists(db)?.into_iter().map(|p| p.id).collect();
    let add = |t: &SyncTrack| -> Option<i64> {
        let vid = t.video_id.as_deref().filter(|v| valid_video_id(v))?;
        let want = RemoteTrack { title: t.title.clone(), artist: t.artist.clone(), album: t.album.clone(), duration_ms: t.duration_ms, path: None, video_id: Some(vid.into()) };
        let c = Candidate { video_id: vid.into(), title: t.title.clone(), channel: t.artist.clone(), duration_ms: t.duration_ms, official: false, explicit: None };
        ensure_track(db, &want, &c).ok()
    };
    let mut songs = 0;
    let mut playlists = 0;
    for p in lib.playlists.iter().take(500) {
        let ids: Vec<i64> = p.tracks.iter().take(music_import::MAX_ITEMS).filter_map(add).collect();
        if ids.is_empty() {
            continue;
        }
        songs += ids.len();
        let id = map.get(&p.id).filter(|id| existing.contains(id)).cloned();
        let saved = music::save_playlist(
            db,
            music::PlaylistInput { id, name: p.name.clone(), description: "From your iPhone".into(), cover: music::Cover { track_id: ids.first().copied(), glyph: None, colour: None }, tracks: ids },
        )?;
        map.insert(p.id.clone(), saved.id);
        playlists += 1;
    }
    for t in lib.liked.iter().take(music_import::MAX_ITEMS) {
        if let Some(id) = add(t) {
            music::set_like(db, id, true)?;
            songs += 1;
        }
    }
    db.set_setting(MAP, &map)?;
    let last = LastSync { device: if lib.device.trim().is_empty() { "iPhone".into() } else { lib.device.chars().take(60).collect() }, at: now(), playlists, songs };
    db.set_setting(LAST, &last)?;
    db.audit("info", "user", "music.phone_sync.received", Some(serde_json::json!({ "playlists": playlists, "songs": songs })));
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn only_local_peers() {
        assert!(local_peer(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 23))));
        assert!(local_peer(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(local_peer(IpAddr::V4(Ipv4Addr::new(172, 20, 1, 1))));
        assert!(!local_peer(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!local_peer(IpAddr::V4(Ipv4Addr::new(172, 40, 1, 1))));
        assert!(local_peer(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(local_peer(IpAddr::V6("fe80::1".parse().unwrap())));
        assert!(!local_peer(IpAddr::V6("2001:4860::8888".parse().unwrap())));
        assert!(local_peer(IpAddr::V6(Ipv4Addr::new(192, 168, 0, 9).to_ipv6_mapped())));
    }

    /// The server end to end, with the same HTTP client the phone uses (reqwest).
    #[tokio::test]
    async fn serves_a_phone() {
        let db = Arc::new(Db::open_in_memory().unwrap());
        let k = key(&db).unwrap();
        // one PC playlist with a streaming song
        let want = RemoteTrack { title: "Blinding Lights".into(), artist: "The Weeknd".into(), album: String::new(), duration_ms: Some(200_000), path: None, video_id: Some("fHI8X4OXluQ".into()) };
        let c = Candidate { video_id: "fHI8X4OXluQ".into(), title: want.title.clone(), channel: want.artist.clone(), duration_ms: want.duration_ms, official: true, explicit: None };
        let id = ensure_track(&db, &want, &c).unwrap();
        music::save_playlist(&db, music::PlaylistInput { id: None, name: "Night drive".into(), description: String::new(), cover: music::Cover::default(), tracks: vec![id] }).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let sdb = db.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().take(5).flatten() {
                let _ = handle(&sdb, &|_: &LastSync| {}, s);
            }
        });
        let base = format!("http://127.0.0.1:{port}");
        let http = reqwest::Client::new();

        let bad = http.get(format!("{base}/sync/hello")).bearer_auth("nope").send().await.unwrap();
        assert_eq!(bad.status().as_u16(), 401);

        let hello: serde_json::Value = http.get(format!("{base}/sync/hello")).bearer_auth(&k).send().await.unwrap().json().await.unwrap();
        assert_eq!(hello["app"], "NOCTURN");

        let lib: SyncLibrary = http.get(format!("{base}/sync/library")).bearer_auth(&k).send().await.unwrap().json().await.unwrap();
        assert_eq!(lib.playlists.len(), 1);
        assert_eq!(lib.playlists[0].tracks[0].video_id.as_deref(), Some("fHI8X4OXluQ"));

        let phone = SyncLibrary {
            device: "iPhone".into(),
            playlists: vec![SyncPlaylist { id: "mine:abc".into(), name: "Gym".into(), tracks: vec![SyncTrack { title: "Starboy".into(), artist: "The Weeknd".into(), album: String::new(), duration_ms: Some(230_000), video_id: Some("34Na4j8AVgA".into()) }] }],
            liked: vec![SyncTrack { title: "Starboy".into(), artist: "The Weeknd".into(), album: String::new(), duration_ms: Some(230_000), video_id: Some("34Na4j8AVgA".into()) }],
        };
        let last: LastSync = http.post(format!("{base}/sync/push")).bearer_auth(&k).json(&phone).send().await.unwrap().json().await.unwrap();
        assert_eq!((last.playlists, last.songs), (1, 2));
        let names: Vec<String> = music::playlists(&db).unwrap().into_iter().map(|p| p.name).collect();
        assert!(names.contains(&"Gym".to_string()));
        assert!(music::tracks(&db).unwrap().iter().any(|t| t.liked && t.title == "Starboy"));
        let took: NowPlaying = http.post(format!("{base}/sync/take")).bearer_auth(&k).send().await.unwrap().json().await.unwrap();
        assert!(took.track.is_none()); // nothing was playing yet
        // the phone's own playlist isn't sent back to it
        assert_eq!(export(&db).unwrap().playlists.len(), 1);

        // carrying on: the PC was playing Blinding Lights 30 s in, saved 2 s ago
        music::save_session(&db, music::Session { queue: vec![id], index: 0, position_ms: 30_000, shuffle: false, repeat: "off".into(), playing: true, saved_at: 0 }).unwrap();
        let now = now_playing(&db).unwrap();
        assert_eq!(now.track.as_ref().and_then(|t| t.video_id.as_deref()), Some("fHI8X4OXluQ"));
        assert!(now.playing && now.position_ms >= 30_000 && now.position_ms < 35_000);
    }

    #[test]
    fn qr_scales() {
        let svg = qrcode::QrCode::new(b"hello").unwrap().render::<qrcode::render::svg::Color>().min_dimensions(240, 240).build();
        assert!(svg.contains("viewBox"), "{}", &svg[..200.min(svg.len())]);
    }

    #[test]
    fn key_compare() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
    }
}
