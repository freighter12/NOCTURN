//! Discord Rich Presence: "Listening to <song>" on the user's Discord
//! profile while Nocturn Music plays. Opt-in.
//!
//! Talks to the Discord desktop app over its local pipe
//! (`\\.\pipe\discord-ipc-N`), the same way games do — no login, no token,
//! nothing sent anywhere but the Discord app on this PC. What's shown is
//! exactly the song, artist, album and (for streamed songs) the YouTube
//! thumbnail; the user picks which of those in Customise › Music.

use std::io::{Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// NOCTURN's application on the Discord developer portal. Its name is what
/// Discord shows above the song ("Listening to NOCTURN").
pub const CLIENT_ID: &str = "";
/// The moon logo, served from the public repository.
const LOGO_URL: &str = "https://raw.githubusercontent.com/freighter12/NOCTURN/main/src-tauri/icons/128x128@2x.png";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Presence {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    /// When the song started and will end (unix ms), for Discord's progress bar.
    #[serde(default)]
    pub start_ms: Option<i64>,
    #[serde(default)]
    pub end_ms: Option<i64>,
    /// Cover art: an https URL Discord can fetch (YouTube thumbnails); else the logo.
    #[serde(default)]
    pub image_url: Option<String>,
    /// "Open on YouTube" button for streamed songs.
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Status {
    /// Discord's app ID is set in this build.
    pub configured: bool,
    pub connected: bool,
    /// The Discord account it's showing on.
    pub user: Option<String>,
    pub error: Option<String>,
}

static STATUS: Mutex<Status> = Mutex::new(Status { configured: false, connected: false, user: None, error: None });
static TX: OnceLock<Mutex<Sender<Option<Presence>>>> = OnceLock::new();

pub fn status(client_id: &str) -> Status {
    let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    s.configured = valid_client_id(client_id);
    s
}

pub fn valid_client_id(id: &str) -> bool {
    (17..=21).contains(&id.len()) && id.chars().all(|c| c.is_ascii_digit())
}

fn set_status(f: impl FnOnce(&mut Status)) {
    f(&mut STATUS.lock().unwrap_or_else(|e| e.into_inner()));
}

/// Discord wants 2–128 characters per text field.
fn field(s: &str) -> Option<String> {
    let s: String = s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if s.is_empty() {
        return None;
    }
    let mut s: String = s.chars().take(128).collect();
    if s.chars().count() < 2 {
        s.push(' ');
    }
    Some(s)
}

/// The SET_ACTIVITY payload for a presence.
pub fn activity(p: &Presence) -> Value {
    let mut a = json!({
        "type": 2, // Listening
        "status_display_type": 2, // "Listening to <song>" in the member list
        "details": field(&p.title).unwrap_or_else(|| "Unknown song".into()),
        "state": field(&format!("by {}", p.artist)).unwrap_or_else(|| "Nocturn Music".into()),
        "assets": {
            "large_image": p.image_url.as_deref().filter(|u| u.starts_with("https://") && u.len() <= 256).unwrap_or(LOGO_URL),
            "large_text": field(&p.album).unwrap_or_else(|| "Nocturn Music".into()),
        },
    });
    if p.paused {
        a["state"] = json!(field(&format!("Paused · {}", p.artist)).unwrap_or_else(|| "Paused".into()));
    } else if let (Some(s), Some(e)) = (p.start_ms, p.end_ms) {
        if e > s {
            a["timestamps"] = json!({ "start": s, "end": e });
        }
    }
    if let Some(l) = p.link.as_deref().filter(|u| u.starts_with("https://www.youtube.com/watch?v=") && u.len() <= 64) {
        a["buttons"] = json!([{ "label": "Open on YouTube", "url": l }]);
    }
    a
}

// ------------------------------------------------------------------- pipe

struct Conn {
    pipe: std::fs::File,
    nonce: u64,
}

fn write_frame(w: &mut impl Write, op: u32, payload: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(payload)?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    w.write_all(&frame)?;
    w.flush()
}

fn read_frame(r: &mut impl Read) -> std::io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    r.read_exact(&mut head)?;
    let op = u32::from_le_bytes(head[0..4].try_into().expect("4 bytes"));
    let len = u32::from_le_bytes(head[4..8].try_into().expect("4 bytes")) as usize;
    if len > 1 << 20 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

fn connect(client_id: &str) -> Result<Conn, String> {
    let mut last = "Discord isn't running".to_string();
    for n in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{n}");
        let Ok(mut pipe) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else { continue };
        if let Err(e) = write_frame(&mut pipe, 0, &json!({ "v": 1, "client_id": client_id })) {
            last = format!("couldn't talk to Discord: {e}");
            continue;
        }
        match read_frame(&mut pipe) {
            Ok((1, v)) if v["evt"] == "READY" => {
                let user = v["data"]["user"]["global_name"].as_str().or_else(|| v["data"]["user"]["username"].as_str()).map(String::from);
                set_status(|s| {
                    s.connected = true;
                    s.user = user;
                    s.error = None;
                });
                tracing::info!("connected to Discord (pipe {n})");
                return Ok(Conn { pipe, nonce: 0 });
            }
            Ok((_, v)) => last = v["message"].as_str().map(|m| format!("Discord said: {m}")).unwrap_or_else(|| "Discord refused the connection".into()),
            Err(e) => last = format!("couldn't talk to Discord: {e}"),
        }
    }
    Err(last)
}

fn send(c: &mut Conn, act: Option<&Value>) -> Result<(), String> {
    c.nonce += 1;
    let msg = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": act }, "nonce": c.nonce.to_string() });
    write_frame(&mut c.pipe, 1, &msg).map_err(|e| e.to_string())?;
    match read_frame(&mut c.pipe).map_err(|e| e.to_string())? {
        (1, v) if v["evt"] == "ERROR" => Err(v["data"]["message"].as_str().unwrap_or("Discord rejected the status").to_string()),
        (2, _) => Err("Discord closed the connection".into()),
        _ => Ok(()),
    }
}

/// Show (Some) or clear (None) the presence. Latest call wins; failures are
/// retried quietly every 20 s while there's something to show.
pub fn set(client_id: &str, p: Option<Presence>) {
    if !valid_client_id(client_id) {
        return;
    }
    let client_id = client_id.to_string();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Option<Presence>>();
        std::thread::spawn(move || {
            let mut conn: Option<Conn> = None;
            let mut want: Option<Presence> = None;
            let mut shown: Option<Option<Presence>> = None;
            loop {
                match rx.recv_timeout(Duration::from_secs(20)) {
                    Ok(p) => want = p,
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                while let Ok(p) = rx.try_recv() {
                    want = p;
                }
                if shown.as_ref() == Some(&want) {
                    continue;
                }
                if want.is_none() && conn.is_none() {
                    shown = Some(None);
                    continue;
                }
                if conn.is_none() {
                    match connect(&client_id) {
                        Ok(c) => conn = Some(c),
                        Err(e) => {
                            set_status(|s| {
                                s.connected = false;
                                s.error = Some(e);
                            });
                            continue;
                        }
                    }
                }
                let c = conn.as_mut().expect("connected above");
                let act = want.as_ref().map(activity);
                match send(c, act.as_ref()) {
                    Ok(()) => {
                        shown = Some(want.clone());
                        set_status(|s| s.error = None);
                        if want.is_none() {
                            // Nothing playing: let go, so Discord stops showing NOCTURN at all.
                            conn = None;
                            set_status(|s| s.connected = false);
                        }
                    }
                    Err(e) => {
                        tracing::debug!("discord presence failed: {e}");
                        conn = None;
                        set_status(|s| {
                            s.connected = false;
                            s.error = Some(e);
                        });
                    }
                }
            }
        });
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(p);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Presence {
        Presence { title: "One More Time".into(), artist: "Daft Punk".into(), album: "Discovery".into(), start_ms: Some(1000), end_ms: Some(321_000), image_url: None, link: None, paused: false }
    }

    #[test]
    fn activity_shows_song_artist_album_and_progress() {
        let a = activity(&p());
        assert_eq!(a["type"], 2);
        assert_eq!(a["details"], "One More Time");
        assert_eq!(a["state"], "by Daft Punk");
        assert_eq!(a["assets"]["large_text"], "Discovery");
        assert_eq!(a["assets"]["large_image"], LOGO_URL);
        assert_eq!(a["timestamps"]["end"], 321_000);
        assert!(a.get("buttons").is_none());
    }

    #[test]
    fn paused_has_no_progress_and_bad_links_are_dropped() {
        let a = activity(&Presence { paused: true, link: Some("https://evil.example/x".into()), image_url: Some("http://insecure/x.jpg".into()), ..p() });
        assert!(a.get("timestamps").is_none());
        assert_eq!(a["state"], "Paused · Daft Punk");
        assert!(a.get("buttons").is_none());
        assert_eq!(a["assets"]["large_image"], LOGO_URL);
        let b = activity(&Presence { link: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".into()), ..p() });
        assert_eq!(b["buttons"][0]["url"], "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    }

    #[test]
    fn fields_are_padded_trimmed_and_capped() {
        assert_eq!(field("A").as_deref(), Some("A "));
        assert_eq!(field("   "), None);
        assert_eq!(field(&"x".repeat(300)).unwrap().len(), 128);
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, 1, &json!({ "a": 1 })).unwrap();
        let (op, v) = read_frame(&mut buf.as_slice()).unwrap();
        assert_eq!(op, 1);
        assert_eq!(v["a"], 1);
    }

    /// Needs the Discord app running: a made-up ID must reach Discord and be refused by it.
    #[test]
    #[ignore]
    fn live_discord_pipe_answers() {
        let e = connect("100000000000000001").err().expect("a made-up ID can't be accepted");
        assert!(e.starts_with("Discord said") || e.contains("refused"), "got: {e}");
        println!("Discord answered: {e}");
    }

    #[test]
    fn client_ids_are_snowflakes() {
        assert!(valid_client_id("1234567890123456789"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("12345abc901234567"));
    }
}
