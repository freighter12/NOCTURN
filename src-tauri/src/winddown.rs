//! Wind-down: a calmer last half hour before bed.
//!
//! On a schedule (or "Wind down now") NOCTURN slowly warms the screen colour,
//! the player fades the music down, NOCTURN's own notifications go quiet, and
//! at bedtime it can turn the screen off or put the PC to sleep (after a
//! one-minute warning that can be cancelled). The warm screen stays until the
//! morning time or until the user stops it; everything is put back then.
//!
//! The runner lives here, not in the page, so it works with the window hidden
//! in the tray. The page carries out the music parts (it owns playback) from
//! the `winddown` event.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const CONFIG_KEY: &str = "winddown.config";
const DIRTY_KEY: &str = "winddown.gamma_dirty";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Start by itself every scheduled night.
    pub scheduled: bool,
    /// "HH:MM", local time.
    pub bedtime: String,
    /// Minutes of winding down before bedtime (10–120).
    pub lead_min: u32,
    /// Nights it runs (0 = Monday … 6 = Sunday), by the evening it starts.
    pub days: Vec<u8>,
    /// When the night ends and the screen goes back to normal, "HH:MM".
    pub morning: String,
    pub warm: bool,
    /// 0 (none) – 1 (very warm, about 2700 K).
    pub warm_strength: f64,
    /// Fade the music down over the wind-down.
    pub music_fade: bool,
    /// Volume at bedtime as a share of the user's volume (0.1–1).
    pub music_floor: f64,
    /// Fade out and pause at bedtime.
    pub music_stop: bool,
    /// Playlist to start when winding down begins (none = keep what's on).
    pub playlist: Option<String>,
    /// NOCTURN's own desktop notifications stay quiet.
    pub quiet: bool,
    /// "nothing" | "screen_off" | "sleep"
    pub bedtime_action: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            scheduled: false,
            bedtime: "23:30".into(),
            lead_min: 30,
            days: (0..7).collect(),
            morning: "07:00".into(),
            warm: true,
            warm_strength: 0.6,
            music_fade: true,
            music_floor: 0.4,
            music_stop: true,
            playlist: None,
            quiet: true,
            bedtime_action: "nothing".into(),
        }
    }
}

fn hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

pub fn sanitise(mut c: Config) -> Config {
    let d = Config::default();
    if hhmm(&c.bedtime).is_none() {
        c.bedtime = d.bedtime;
    }
    if hhmm(&c.morning).is_none() {
        c.morning = d.morning;
    }
    c.lead_min = c.lead_min.clamp(10, 120);
    c.days.retain(|x| *x < 7);
    c.days.sort_unstable();
    c.days.dedup();
    c.warm_strength = if c.warm_strength.is_finite() { c.warm_strength.clamp(0.0, 1.0) } else { d.warm_strength };
    c.music_floor = if c.music_floor.is_finite() { c.music_floor.clamp(0.1, 1.0) } else { d.music_floor };
    if !["nothing", "screen_off", "sleep"].contains(&c.bedtime_action.as_str()) {
        c.bedtime_action = d.bedtime_action;
    }
    c.playlist = c.playlist.filter(|p| !p.is_empty() && p.len() <= 80);
    c
}

pub fn load(db: &Db) -> Config {
    sanitise(db.get_setting::<Config>(CONFIG_KEY).ok().flatten().unwrap_or_default())
}

// --------------------------------------------------------------- schedule

#[derive(Debug, Clone, PartialEq)]
struct Night {
    start: DateTime<Local>,
    bedtime: DateTime<Local>,
    morning: DateTime<Local>,
}

fn at(date: chrono::NaiveDate, t: NaiveTime) -> Option<DateTime<Local>> {
    // `earliest` handles the hour that repeats when clocks go back; a time
    // skipped when they go forward just has no night that day.
    Local.from_local_datetime(&date.and_time(t)).earliest()
}

/// The first `t` after `after`.
fn next_at(after: DateTime<Local>, t: NaiveTime) -> Option<DateTime<Local>> {
    let today = at(after.date_naive(), t)?;
    if today > after {
        Some(today)
    } else {
        at(after.date_naive().succ_opt()?, t)
    }
}

/// The scheduled night `now` falls in (from wind-down start until morning).
fn scheduled_night(c: &Config, now: DateTime<Local>) -> Option<Night> {
    if !c.scheduled {
        return None;
    }
    let bed = hhmm(&c.bedtime)?;
    let morning = hhmm(&c.morning)?;
    for back in [1i64, 0] {
        let evening = now.date_naive() - chrono::Duration::days(back);
        if !c.days.contains(&(evening.weekday().num_days_from_monday() as u8)) {
            continue;
        }
        // A bedtime before noon (00:30) belongs to the evening before.
        let bed_date = if bed < NaiveTime::from_hms_opt(12, 0, 0)? { evening.succ_opt()? } else { evening };
        let bedtime = at(bed_date, bed)?;
        let start = bedtime - chrono::Duration::minutes(c.lead_min as i64);
        let morning = next_at(bedtime, morning)?;
        if start <= now && now < morning {
            return Some(Night { start, bedtime, morning });
        }
    }
    None
}

// ------------------------------------------------------------------ state

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    /// "off" | "winding" | "night"
    pub phase: String,
    /// 0 → 1 across the wind-down (1 from bedtime on).
    pub progress: f64,
    pub started_at: Option<String>,
    pub bedtime: Option<String>,
    pub morning: Option<String>,
    /// Started by hand (not the schedule).
    pub manual: bool,
    /// The PC will sleep at this time unless cancelled.
    pub sleep_at: Option<String>,
    /// The next scheduled start, when off.
    pub next_start: Option<String>,
    /// Warmth actually applied (0–1); less than asked when the display refuses.
    pub warm_applied: f64,
    /// Why nothing happened at bedtime ("a game is running"), if so.
    pub note: Option<String>,
    pub config: Config,
}

#[derive(Default)]
struct Run {
    night: Option<Night>,
    manual: bool,
    bedtime_done: bool,
    sleep_at: Option<DateTime<Local>>,
    /// The user stopped tonight's scheduled run: don't restart before this.
    dismissed_until: Option<DateTime<Local>>,
    warm_applied: f64,
    note: Option<String>,
    last: Option<Status>,
}

static RUN: Mutex<Option<Run>> = Mutex::new(None);

fn with_run<T>(f: impl FnOnce(&mut Run) -> T) -> T {
    f(RUN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Run::default))
}

fn iso(t: DateTime<Local>) -> String {
    t.to_rfc3339()
}

fn progress(n: &Night, now: DateTime<Local>) -> f64 {
    let total = (n.bedtime - n.start).num_milliseconds().max(1) as f64;
    ((now - n.start).num_milliseconds() as f64 / total).clamp(0.0, 1.0)
}

fn next_start(c: &Config, now: DateTime<Local>) -> Option<DateTime<Local>> {
    if !c.scheduled || c.days.is_empty() {
        return None;
    }
    // Look a minute at a time would be silly; try each of the next 8 evenings.
    let bed = hhmm(&c.bedtime)?;
    (0..8)
        .filter_map(|d| {
            let evening = now.date_naive() + chrono::Duration::days(d);
            if !c.days.contains(&(evening.weekday().num_days_from_monday() as u8)) {
                return None;
            }
            let bed_date = if bed < NaiveTime::from_hms_opt(12, 0, 0)? { evening.succ_opt()? } else { evening };
            Some(at(bed_date, bed)? - chrono::Duration::minutes(c.lead_min as i64))
        })
        .find(|s| *s > now)
}

fn status_of(r: &Run, c: &Config, now: DateTime<Local>) -> Status {
    let (phase, prog) = match &r.night {
        None => ("off", 0.0),
        Some(n) if now < n.bedtime => ("winding", progress(n, now)),
        Some(_) => ("night", 1.0),
    };
    Status {
        phase: phase.into(),
        progress: (prog * 1000.0).round() / 1000.0,
        started_at: r.night.as_ref().map(|n| iso(n.start)),
        bedtime: r.night.as_ref().map(|n| iso(n.bedtime)),
        morning: r.night.as_ref().map(|n| iso(n.morning)),
        manual: r.manual,
        sleep_at: r.sleep_at.map(iso),
        next_start: if r.night.is_none() { next_start(c, now).map(iso) } else { None },
        warm_applied: (r.warm_applied * 100.0).round() / 100.0,
        note: r.note.clone(),
        config: c.clone(),
    }
}

pub fn status(db: &Db) -> Status {
    let c = load(db);
    with_run(|r| status_of(r, &c, Local::now()))
}

/// Quiet hours for NOCTURN's own notifications.
pub fn quiet(db: &Db) -> bool {
    let c = load(db);
    c.quiet && with_run(|r| r.night.is_some())
}

/// Smoothstep, so the change is gentle at both ends.
fn ease(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// One step of the runner. Returns the status if it changed.
fn tick<R: Runtime>(app: &AppHandle<R>, st: &AppState, force_emit: bool) -> Option<Status> {
    let c = load(&st.db);
    let now = Local::now();
    let mut sleep_now = false;
    let mut screen_off = false;
    let mut announce_sleep = false;
    let warm_target = with_run(|r| {
        // End of the night.
        if r.night.as_ref().is_some_and(|n| now >= n.morning) {
            *r = Run { dismissed_until: r.dismissed_until, ..Run::default() };
        }
        if r.dismissed_until.is_some_and(|d| now >= d) {
            r.dismissed_until = None;
        }
        // The schedule starts a night by itself.
        if r.night.is_none() && r.dismissed_until.is_none() {
            if let Some(n) = scheduled_night(&c, now) {
                // Started after bedtime (PC was off)? Skip straight past the bedtime action.
                r.bedtime_done = now >= n.bedtime + chrono::Duration::minutes(2);
                r.night = Some(n);
                r.manual = false;
                tracing::info!("wind-down started (schedule)");
            }
        }
        let warm_target = match &r.night {
            Some(n) if c.warm => c.warm_strength * ease(progress(n, now)),
            _ => 0.0,
        };
        // Bedtime.
        if let Some(n) = &r.night {
            if now >= n.bedtime && !r.bedtime_done {
                r.bedtime_done = true;
                r.note = None;
                match c.bedtime_action.as_str() {
                    "screen_off" => screen_off = true,
                    "sleep" => {
                        r.sleep_at = Some(now + chrono::Duration::seconds(60));
                        announce_sleep = true;
                    }
                    _ => {}
                }
            }
        }
        if r.sleep_at.is_some_and(|s| now >= s) {
            r.sleep_at = None;
            if let Some(game) = running_game(st) {
                r.note = Some(format!("Didn't sleep: {game} is running."));
            } else {
                sleep_now = true;
            }
        }
        warm_target
    });
    let applied = gamma::warm(&st.db, warm_target);
    if announce_sleep {
        crate::tray::notify_always(app, "Going to sleep in a minute", "Wind-down: your PC will sleep at bedtime. Open NOCTURN to cancel.");
    }
    if screen_off {
        power::screen_off();
    }
    if sleep_now {
        tracing::info!("wind-down: sleeping the PC");
        if let Err(e) = power::sleep() {
            with_run(|r| r.note = Some(format!("Couldn't sleep the PC: {e}")));
        }
    }
    with_run(|r| {
        r.warm_applied = applied;
        let s = status_of(r, &c, now);
        // Progress changes every tick; only tell the page about whole-percent steps.
        let changed = force_emit || r.last.as_ref().is_none_or(|l| l.phase != s.phase || (l.progress - s.progress).abs() >= 0.01 || l.sleep_at != s.sleep_at || l.note != s.note || l.config != s.config || l.warm_applied != s.warm_applied);
        if changed {
            r.last = Some(s.clone());
            let _ = app.emit("winddown", &s);
            Some(s)
        } else {
            None
        }
    })
}

fn running_game(st: &AppState) -> Option<String> {
    st.games.live.lock().ok().and_then(|l| l.as_ref().map(|s| s.summary().game_name))
}

/// Start the runner. Called once from setup.
pub fn start<R: Runtime>(app: AppHandle<R>, st: Arc<AppState>) {
    // A previous run that ended without putting the screen back (crash, power cut).
    if st.db.get_setting::<bool>(DIRTY_KEY).ok().flatten().unwrap_or(false) {
        gamma::reset_all();
        let _ = st.db.set_setting(DIRTY_KEY, &false);
    }
    std::thread::spawn(move || loop {
        tick(&app, &st, false);
        std::thread::sleep(Duration::from_secs(5));
    });
}

// --------------------------------------------------------------- commands

pub fn save<R: Runtime>(app: &AppHandle<R>, st: &AppState, c: Config) -> AppResult<Status> {
    let c = sanitise(c);
    st.db.set_setting(CONFIG_KEY, &c)?;
    // Turning the schedule off ends a scheduled night in progress.
    if !c.scheduled {
        with_run(|r| {
            if r.night.is_some() && !r.manual {
                *r = Run::default();
            }
        });
    }
    Ok(tick(app, st, true).unwrap_or_else(|| status(&st.db)))
}

/// "Wind down now": bedtime in `minutes` (0 = straight to bedtime).
pub fn start_now<R: Runtime>(app: &AppHandle<R>, st: &AppState, minutes: u32) -> AppResult<Status> {
    let c = load(&st.db);
    let now = Local::now();
    let minutes = minutes.min(180) as i64;
    let bedtime = now + chrono::Duration::minutes(minutes);
    let morning = hhmm(&c.morning).and_then(|m| next_at(bedtime, m)).ok_or_else(|| AppError::msg("the morning time isn't valid"))?;
    with_run(|r| {
        *r = Run { night: Some(Night { start: now - chrono::Duration::milliseconds(1), bedtime, morning }), manual: true, ..Run::default() };
    });
    tracing::info!(minutes, "wind-down started by hand");
    st.db.audit("info", "user", "winddown.start", Some(serde_json::json!({ "minutes": minutes })));
    Ok(tick(app, st, true).unwrap_or_else(|| status(&st.db)))
}

pub fn stop<R: Runtime>(app: &AppHandle<R>, st: &AppState) -> AppResult<Status> {
    with_run(|r| {
        let until = r.night.as_ref().filter(|_| !r.manual).map(|n| n.morning);
        *r = Run { dismissed_until: until.or(r.dismissed_until), ..Run::default() };
    });
    gamma::warm(&st.db, 0.0);
    st.db.audit("info", "user", "winddown.stop", None);
    Ok(tick(app, st, true).unwrap_or_else(|| status(&st.db)))
}

/// Push bedtime back (and cancel a pending sleep).
pub fn snooze<R: Runtime>(app: &AppHandle<R>, st: &AppState, minutes: u32) -> AppResult<Status> {
    let minutes = minutes.clamp(5, 120) as i64;
    with_run(|r| {
        if let Some(n) = r.night.as_mut() {
            let now = Local::now();
            let base = if n.bedtime > now { n.bedtime } else { now };
            n.bedtime = base + chrono::Duration::minutes(minutes);
            if n.morning <= n.bedtime {
                n.morning = n.bedtime + chrono::Duration::hours(1);
            }
            r.bedtime_done = false;
            r.sleep_at = None;
            r.note = None;
        }
    });
    Ok(tick(app, st, true).unwrap_or_else(|| status(&st.db)))
}

pub fn cancel_sleep<R: Runtime>(app: &AppHandle<R>, st: &AppState) -> AppResult<Status> {
    with_run(|r| {
        if r.sleep_at.take().is_some() {
            r.note = Some("Sleep cancelled.".into());
        }
    });
    Ok(tick(app, st, true).unwrap_or_else(|| status(&st.db)))
}

/// Show a warmth for a few seconds (the settings slider), then go back.
pub fn preview_warm(st: Arc<AppState>, strength: f64) {
    let s = if strength.is_finite() { strength.clamp(0.0, 1.0) } else { 0.0 };
    gamma::warm(&st.db, s);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        // Back to what the runner wants (it re-asserts within 5 s anyway).
        let c = load(&st.db);
        let target = with_run(|r| match &r.night {
            Some(n) if c.warm => c.warm_strength * ease(progress(n, Local::now())),
            _ => 0.0,
        });
        gamma::warm(&st.db, target);
    });
}

// ------------------------------------------------------------------ gamma

/// Screen warmth through each display's gamma ramp (like f.lux). The ramp
/// that was there before is kept and put back exactly.
pub mod gamma {
    use super::*;

    /// RGB multipliers for a colour temperature, 1.0 at 6500 K.
    pub fn kelvin_rgb(k: f64) -> (f64, f64, f64) {
        fn raw(k: f64) -> (f64, f64, f64) {
            let t = k / 100.0;
            let r = if t <= 66.0 { 255.0 } else { 329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2) };
            let g = if t <= 66.0 { 99.470_802_586_1 * t.ln() - 161.119_568_166_1 } else { 288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2) };
            let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { 138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7 };
            (r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0))
        }
        let (r0, g0, b0) = raw(6500.0);
        let (r, g, b) = raw(k.clamp(1900.0, 6500.0));
        ((r / r0).min(1.0), (g / g0).min(1.0), (b / b0).min(1.0))
    }

    /// Strength 0–1 → kelvin (1 = 2700 K).
    pub fn kelvin_for(strength: f64) -> f64 {
        6500.0 - strength.clamp(0.0, 1.0) * (6500.0 - 2700.0)
    }

    pub fn ramp(base: &[u16; 768], m: (f64, f64, f64)) -> [u16; 768] {
        let mut out = [0u16; 768];
        for (c, f) in [m.0, m.1, m.2].into_iter().enumerate() {
            for i in 0..256 {
                out[c * 256 + i] = (base[c * 256 + i] as f64 * f).round().clamp(0.0, 65535.0) as u16;
            }
        }
        out
    }

    pub fn identity() -> [u16; 768] {
        let mut out = [0u16; 768];
        for c in 0..3 {
            for i in 0..256 {
                out[c * 256 + i] = (i * 257) as u16;
            }
        }
        out
    }

    struct Saved {
        originals: HashMap<String, [u16; 768]>,
        current: f64,
    }
    static SAVED: Mutex<Option<Saved>> = Mutex::new(None);

    /// Apply `strength` (0 = put the original back). Returns the strength
    /// actually applied (Windows refuses ramps it thinks are too extreme).
    pub fn warm(db: &Db, strength: f64) -> f64 {
        let mut g = SAVED.lock().unwrap_or_else(|e| e.into_inner());
        if strength <= 0.001 {
            if let Some(s) = g.take() {
                for (dev, orig) in &s.originals {
                    sys::set(dev, orig);
                }
                let _ = db.set_setting(DIRTY_KEY, &false);
            }
            return 0.0;
        }
        let s = g.get_or_insert_with(|| Saved { originals: HashMap::new(), current: 0.0 });
        let devices = sys::displays();
        for d in &devices {
            if !s.originals.contains_key(d) {
                // A ramp that's already far from normal is left over from something
                // else (or a crash): go back to plain identity at the end.
                let orig = sys::get(d).filter(near_identity).unwrap_or_else(identity);
                s.originals.insert(d.clone(), orig);
            }
        }
        let _ = db.set_setting(DIRTY_KEY, &true);
        let mut applied: f64 = 0.0;
        for d in &devices {
            let orig = s.originals[d];
            // Step down until the display accepts it.
            let mut k = strength;
            loop {
                if sys::set(d, &ramp(&orig, kelvin_rgb(kelvin_for(k)))) {
                    applied = applied.max(k);
                    break;
                }
                k -= 0.05;
                if k <= 0.0 {
                    break;
                }
            }
        }
        s.current = applied;
        applied
    }

    fn near_identity(r: &[u16; 768]) -> bool {
        let id = identity();
        r.iter().zip(id.iter()).all(|(a, b)| (*a as i32 - *b as i32).abs() < 6000)
    }

    /// Put every display back to a plain ramp.
    pub fn reset_all() {
        for d in sys::displays() {
            sys::set(&d, &identity());
        }
    }

    #[cfg(windows)]
    mod sys {
        use windows::core::{BOOL, PCWSTR};
        use windows::Win32::Foundation::{LPARAM, RECT};
        use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
        use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};

        unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
            let out = &mut *(data.0 as *mut Vec<String>);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(m, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
                let n = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
                out.push(String::from_utf16_lossy(&info.szDevice[..n]));
            }
            true.into()
        }

        pub fn displays() -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            unsafe {
                let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut out as *mut Vec<String> as isize));
            }
            out.sort();
            out.dedup();
            out
        }

        fn with_dc<T>(dev: &str, f: impl FnOnce(HDC) -> T) -> Option<T> {
            let w: Vec<u16> = dev.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let dc = CreateDCW(PCWSTR::null(), PCWSTR(w.as_ptr()), PCWSTR::null(), None);
                if dc.is_invalid() {
                    return None;
                }
                let r = f(dc);
                let _ = DeleteDC(dc);
                Some(r)
            }
        }

        pub fn get(dev: &str) -> Option<[u16; 768]> {
            with_dc(dev, |dc| {
                let mut r = [0u16; 768];
                unsafe { GetDeviceGammaRamp(dc, r.as_mut_ptr() as *mut core::ffi::c_void).as_bool().then_some(r) }
            })
            .flatten()
        }

        pub fn set(dev: &str, r: &[u16; 768]) -> bool {
            with_dc(dev, |dc| unsafe { SetDeviceGammaRamp(dc, r.as_ptr() as *const core::ffi::c_void).as_bool() }).unwrap_or(false)
        }
    }

    #[cfg(not(windows))]
    mod sys {
        pub fn displays() -> Vec<String> {
            Vec::new()
        }
        pub fn get(_: &str) -> Option<[u16; 768]> {
            None
        }
        pub fn set(_: &str, _: &[u16; 768]) -> bool {
            false
        }
    }
}

// ------------------------------------------------------------------ power

mod power {
    #[cfg(windows)]
    pub fn screen_off() {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND};
        // 2 = off. Posted, so a hung window can't block us.
        unsafe {
            let _ = PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(2));
        }
    }

    #[cfg(windows)]
    pub fn sleep() -> Result<(), String> {
        use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
        use windows::Win32::Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
        use windows::Win32::System::Power::SetSuspendState;
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            // Sleeping needs the shutdown privilege switched on for this process
            // (every user has it; it's just off by default).
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
            let mut luid = LUID::default();
            let res = LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid).and_then(|_| {
                let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
                AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
            });
            let _ = CloseHandle(token);
            res.map_err(|e| e.to_string())?;
            // Sleep (not hibernate), let wake timers work.
            if SetSuspendState(false, false, false) {
                Ok(())
            } else {
                Err(windows::core::Error::from_thread().to_string())
            }
        }
    }

    #[cfg(not(windows))]
    pub fn screen_off() {}
    #[cfg(not(windows))]
    pub fn sleep() -> Result<(), String> {
        Err("not supported".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).earliest().unwrap()
    }

    fn cfg() -> Config {
        Config { scheduled: true, ..Config::default() }
    }

    #[test]
    fn schedule_covers_wind_down_until_morning() {
        let c = cfg(); // 23:30 bedtime, 30 min, morning 07:00, every night
        assert_eq!(scheduled_night(&c, local(2026, 11, 9, 22, 59)), None);
        let n = scheduled_night(&c, local(2026, 11, 9, 23, 10)).unwrap();
        assert_eq!(n.start, local(2026, 11, 9, 23, 0));
        assert_eq!(n.bedtime, local(2026, 11, 9, 23, 30));
        assert_eq!(n.morning, local(2026, 11, 10, 7, 0));
        assert!((progress(&n, local(2026, 11, 9, 23, 15)) - 0.5).abs() < 1e-9);
        // Still the same night after midnight.
        assert_eq!(scheduled_night(&c, local(2026, 11, 10, 3, 0)), Some(n));
        assert_eq!(scheduled_night(&c, local(2026, 11, 10, 7, 0)), None);
        assert_eq!(scheduled_night(&Config { scheduled: false, ..c }, local(2026, 11, 9, 23, 10)), None);
    }

    #[test]
    fn after_midnight_bedtimes_belong_to_the_evening_before() {
        // Monday-night only, bedtime 00:30 → runs from 00:00 Tuesday.
        let c = Config { bedtime: "00:30".into(), days: vec![0], ..cfg() };
        assert_eq!(local(2026, 11, 9, 12, 0).weekday(), chrono::Weekday::Mon);
        let n = scheduled_night(&c, local(2026, 11, 10, 0, 10)).unwrap();
        assert_eq!(n.bedtime, local(2026, 11, 10, 0, 30));
        // Tuesday night isn't scheduled.
        assert_eq!(scheduled_night(&c, local(2026, 11, 11, 0, 10)), None);
        assert_eq!(next_start(&c, local(2026, 11, 10, 12, 0)), Some(local(2026, 11, 17, 0, 0)));
    }

    #[test]
    fn config_is_sanitised() {
        let c = sanitise(Config { bedtime: "25:99".into(), lead_min: 1, days: vec![9, 1, 1, 3], warm_strength: f64::NAN, music_floor: 5.0, bedtime_action: "shutdown".into(), ..Config::default() });
        assert_eq!(c.bedtime, "23:30");
        assert_eq!(c.lead_min, 10);
        assert_eq!(c.days, vec![1, 3]);
        assert_eq!(c.warm_strength, 0.6);
        assert_eq!(c.music_floor, 1.0);
        assert_eq!(c.bedtime_action, "nothing");
    }

    #[test]
    fn warm_colours_cut_blue_first() {
        assert_eq!(gamma::kelvin_rgb(6500.0), (1.0, 1.0, 1.0));
        let (r, g, b) = gamma::kelvin_rgb(gamma::kelvin_for(0.6));
        assert_eq!(r, 1.0);
        assert!(g < 1.0 && b < g, "g={g} b={b}");
        let ramp = gamma::ramp(&gamma::identity(), (1.0, 0.5, 0.25));
        assert_eq!(ramp[255], 65535);
        assert_eq!(ramp[256 + 255], 32768);
        assert_eq!(ramp[512 + 255], 16384);
    }
}
