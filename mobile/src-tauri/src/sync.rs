//! Sync with NOCTURN on the PC over the home Wi-Fi (no accounts).
//!
//! Pairing: the PC shows a QR code with its address, port and a secret key
//! (Music › Sources › Sync with iPhone); the phone scans it once. Then:
//! `pull` reads the PC's playlists and liked songs (songs that are files on
//! the PC are found on YouTube Music, like an imported playlist), and `push`
//! sends the phone's own playlists and liked songs. Only private network
//! addresses are accepted, so a code can't point the phone anywhere else.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::import::{self, RemoteTrack};
use crate::yt::{self, Track};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub host: String,
    pub port: u16,
    pub key: String,
    #[serde(default)]
    pub name: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncLibrary {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub playlists: Vec<SyncPlaylist>,
    #[serde(default)]
    pub liked: Vec<SyncTrack>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PulledPlaylist {
    /// The PC's playlist id (the phone keeps one playlist per PC playlist).
    pub id: String,
    pub name: String,
    pub tracks: Vec<Track>,
    pub missing: Vec<import::Missing>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pulled {
    pub device: String,
    pub playlists: Vec<PulledPlaylist>,
    pub liked: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pushed {
    pub device: String,
    pub playlists: usize,
    pub songs: usize,
}

#[derive(Clone, Serialize)]
struct Progress {
    done: usize,
    total: usize,
    stage: &'static str,
}

/// Reads a scanned QR code: {"t":"nocturn-sync","h":"192.168.1.20","p":47821,"k":"…","n":"DESKTOP"}.
pub fn parse_code(code: &str) -> Result<Pairing, String> {
    let v: serde_json::Value = serde_json::from_str(code.trim()).map_err(|_| "that isn't NOCTURN's sync code — open Music › Sources › Sync with iPhone on your PC".to_string())?;
    if v.get("t").and_then(|t| t.as_str()) != Some("nocturn-sync") {
        return Err("that isn't NOCTURN's sync code — open Music › Sources › Sync with iPhone on your PC".into());
    }
    let host = v.get("h").and_then(|h| h.as_str()).unwrap_or("").to_string();
    let port = v.get("p").and_then(|p| p.as_u64()).filter(|p| (1024..=65535).contains(p)).ok_or("the code has no port")? as u16;
    let key = v.get("k").and_then(|k| k.as_str()).filter(|k| (32..=128).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric())).ok_or("the code has no key")?.to_string();
    let name = v.get("n").and_then(|n| n.as_str()).unwrap_or("PC").chars().take(60).collect();
    let p = Pairing { host, port, key, name };
    check_host(&p.host)?;
    Ok(p)
}

/// Only addresses on a home network (192.168.x.x, 10.x.x.x, 172.16–31.x.x, link-local).
fn check_host(host: &str) -> Result<IpAddr, String> {
    let ip: IpAddr = host.parse().map_err(|_| "the code has no address".to_string())?;
    let ok = match ip {
        IpAddr::V4(v) => v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => {
            let seg = v.segments()[0];
            (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    };
    ok.then_some(ip).ok_or_else(|| "that code points outside your home network".into())
}

fn base(p: &Pairing) -> Result<String, String> {
    Ok(match check_host(&p.host)? {
        IpAddr::V6(v) => format!("http://[{v}]:{}", p.port),
        IpAddr::V4(v) => format!("http://{v}:{}", p.port),
    })
}

fn client(timeout: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

fn explain(e: reqwest::Error, p: &Pairing) -> String {
    if e.is_connect() || e.is_timeout() {
        format!("couldn't reach {} — make sure NOCTURN is open on it, iPhone sync is on, and both are on the same Wi-Fi", if p.name.is_empty() { "your PC" } else { &p.name })
    } else {
        format!("sync failed: {}", e.without_url())
    }
}

async fn check(res: reqwest::Response) -> Result<reqwest::Response, String> {
    match res.status().as_u16() {
        200 => Ok(res),
        401 => Err("your PC doesn't recognise this phone any more — scan the code again".into()),
        s => Err(res.json::<serde_json::Value>().await.ok().and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from)).unwrap_or(format!("your PC answered {s}"))),
    }
}

/// Is the PC there, and does it accept our key? Returns its name.
pub async fn hello(p: &Pairing) -> Result<String, String> {
    let res = client(8)?.get(format!("{}/sync/hello", base(p)?)).bearer_auth(&p.key).send().await.map_err(|e| explain(e, p))?;
    let v: serde_json::Value = check(res).await?.json().await.map_err(|_| "your PC sent something unexpected".to_string())?;
    Ok(v.get("name").and_then(|n| n.as_str()).unwrap_or("PC").to_string())
}

fn art(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")
}

/// The PC's playlists and liked songs as phone songs.
pub async fn pull(app: &AppHandle, p: &Pairing) -> Result<Pulled, String> {
    let res = client(60)?.get(format!("{}/sync/library", base(p)?)).bearer_auth(&p.key).send().await.map_err(|e| explain(e, p))?;
    let lib: SyncLibrary = check(res).await?.json().await.map_err(|_| "your PC sent something unexpected".to_string())?;

    // Every distinct song once: streaming songs carry their video, files are looked up.
    let key = |t: &SyncTrack| format!("{}\u{1}{}", t.title.to_lowercase(), t.artist.to_lowercase());
    let mut wanted: Vec<SyncTrack> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for t in lib.playlists.iter().flat_map(|pl| pl.tracks.iter()).chain(lib.liked.iter()) {
        if !seen.contains_key(&key(t)) {
            seen.insert(key(t), wanted.len());
            wanted.push(t.clone());
        }
    }
    let total = wanted.len();
    let mut found: Vec<Option<Track>> = vec![None; total];
    let mut lookups: Vec<usize> = Vec::new();
    for (i, t) in wanted.iter().enumerate() {
        match t.video_id.as_deref().filter(|v| yt::valid_video_id(v)) {
            Some(v) => {
                found[i] = Some(Track {
                    video_id: v.to_string(),
                    title: t.title.clone(),
                    artist: t.artist.clone(),
                    artist_id: None,
                    album: (!t.album.is_empty()).then(|| t.album.clone()),
                    album_id: None,
                    duration_ms: t.duration_ms,
                    artwork: Some(art(v)),
                    explicit: false,
                    video: false,
                })
            }
            None => lookups.push(i),
        }
    }
    let mut done = total - lookups.len();
    let _ = app.emit("sync-progress", Progress { done, total, stage: "matching" });
    for chunk in lookups.chunks(5) {
        let mut set = tokio::task::JoinSet::new();
        for &i in chunk {
            let t = &wanted[i];
            let want = RemoteTrack { title: t.title.clone(), artist: t.artist.clone(), duration_ms: t.duration_ms, explicit: None };
            set.spawn(async move { (i, import::find(&want).await) });
        }
        while let Some(r) = set.join_next().await {
            if let Ok((i, hit)) = r {
                found[i] = hit;
            }
            done += 1;
        }
        let _ = app.emit("sync-progress", Progress { done, total, stage: "matching" });
    }

    let resolve = |t: &SyncTrack| seen.get(&key(t)).and_then(|&i| found[i].clone());
    let playlists = lib
        .playlists
        .iter()
        .map(|pl| {
            let mut tracks: Vec<Track> = Vec::new();
            let mut missing = Vec::new();
            for t in &pl.tracks {
                match resolve(t) {
                    Some(x) if !tracks.iter().any(|y| y.video_id == x.video_id) => tracks.push(x),
                    Some(_) => {}
                    None => missing.push(import::Missing { title: t.title.clone(), artist: t.artist.clone() }),
                }
            }
            PulledPlaylist { id: pl.id.clone(), name: pl.name.clone(), tracks, missing }
        })
        .collect();
    let liked = lib.liked.iter().filter_map(resolve).collect();
    Ok(Pulled { device: if lib.device.is_empty() { p.name.clone() } else { lib.device }, playlists, liked })
}

/// Send the phone's playlists and liked songs to the PC.
pub async fn push(p: &Pairing, lib: &SyncLibrary) -> Result<Pushed, String> {
    let res = client(120)?.post(format!("{}/sync/push", base(p)?)).bearer_auth(&p.key).json(lib).send().await.map_err(|e| explain(e, p))?;
    check(res).await?.json().await.map_err(|_| "your PC sent something unexpected".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        let k = "a".repeat(64);
        let p = parse_code(&format!(r#"{{"t":"nocturn-sync","v":1,"h":"192.168.1.20","p":47821,"k":"{k}","n":"DESKTOP"}}"#)).unwrap();
        assert_eq!((p.host.as_str(), p.port, p.name.as_str()), ("192.168.1.20", 47821, "DESKTOP"));
        assert!(parse_code(&format!(r#"{{"t":"nocturn-sync","h":"8.8.8.8","p":47821,"k":"{k}"}}"#)).is_err());
        assert!(parse_code(&format!(r#"{{"t":"other","h":"192.168.1.2","p":47821,"k":"{k}"}}"#)).is_err());
        assert!(parse_code("https://example.com").is_err());
        assert!(parse_code(r#"{"t":"nocturn-sync","h":"10.0.0.2","p":47821,"k":"short"}"#).is_err());
    }
}
