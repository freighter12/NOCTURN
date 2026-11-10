//! "Set up AI for me": a free, private assistant that runs on this PC.
//!
//! Uses Ollama (open source, MIT). If it isn't installed, the official
//! installer is downloaded from Ollama's GitHub releases, checked against the
//! SHA-256 list published with the same release, and run silently as a
//! per-user install (no admin prompt). Then a model sized for this PC's
//! memory is pulled through Ollama's own local API and NOCTURN's AI settings
//! are pointed at it. Nothing is sent anywhere except the downloads.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::exec::{self, SafeCommand};

pub const OLLAMA_URL: &str = "http://127.0.0.1:11434";
pub const OLLAMA_BASE_URL: &str = "http://localhost:11434/v1";
const SETUP_URL: &str = "https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe";
const SUMS_URL: &str = "https://github.com/ollama/ollama/releases/latest/download/sha256sum.txt";
/// The installer is ~1.9 GB (it bundles the GPU runtimes); refuse anything absurd.
const SETUP_MAX: u64 = 4 * 1024 * 1024 * 1024;

static CANCEL: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    /// Download size in GB (approximate).
    pub size_gb: f32,
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Detect {
    pub ollama_running: bool,
    pub ollama_version: Option<String>,
    pub ollama_installed: bool,
    /// Models already downloaded in Ollama that can call tools.
    pub models: Vec<String>,
    pub ram_gb: f32,
    pub vram_gb: Option<f32>,
    pub recommended: ModelChoice,
    pub options: Vec<ModelChoice>,
    /// Free space on the drive Ollama stores models on (or the system drive).
    pub free_gb: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    /// download | verify | install | start | pull | configure | done
    pub stage: String,
    pub label: String,
    pub fraction: Option<f32>,
    pub detail: Option<String>,
}

fn choices() -> Vec<ModelChoice> {
    let c = |id: &str, label: &str, size_gb: f32| ModelChoice { id: id.into(), label: label.into(), size_gb, why: String::new() };
    vec![
        c("qwen3:14b", "Qwen3 14B — smartest, needs a strong graphics card", 9.3),
        c("qwen3:8b", "Qwen3 8B — balanced (recommended for most gaming PCs)", 5.2),
        c("qwen3:4b", "Qwen3 4B — light, good on laptops", 2.5),
        c("qwen3:1.7b", "Qwen3 1.7B — smallest, for older PCs", 1.4),
    ]
}

/// Pick the model that will answer at a comfortable speed on this hardware:
/// the whole model should fit in graphics memory with room for the context,
/// otherwise it runs on the processor and smaller is much faster.
pub fn recommend(ram_gb: f32, vram_gb: Option<f32>) -> ModelChoice {
    let all = choices();
    let pick = |id: &str| all.iter().find(|m| m.id == id).cloned().unwrap();
    let v = vram_gb.unwrap_or(0.0);
    let (id, why) = if v >= 14.0 && ram_gb >= 16.0 {
        ("qwen3:14b", format!("Your graphics card has {v:.0} GB of memory, enough for the larger model."))
    } else if v >= 7.0 {
        ("qwen3:8b", format!("Fits in your graphics card's {v:.0} GB, so answers come quickly."))
    } else if ram_gb >= 16.0 && v >= 4.0 {
        ("qwen3:4b", format!("Fits in your graphics card's {v:.0} GB; the 8B model would be noticeably slower here."))
    } else if ram_gb >= 15.0 {
        ("qwen3:4b", format!("Runs on your processor with {ram_gb:.0} GB of memory; a bigger model would be slow."))
    } else if ram_gb >= 7.0 {
        ("qwen3:1.7b", format!("Light enough for {ram_gb:.0} GB of memory."))
    } else {
        ("qwen3:1.7b", "The smallest model; this PC has little memory, so expect slower answers.".into())
    };
    ModelChoice { why, ..pick(id) }
}

fn http() -> AppResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("NOCTURN/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

async fn ollama_version() -> Option<String> {
    #[derive(Deserialize)]
    struct V {
        version: String,
    }
    let r = http().ok()?.get(format!("{OLLAMA_URL}/api/version")).timeout(Duration::from_secs(2)).send().await.ok()?;
    r.json::<V>().await.ok().map(|v| v.version)
}

/// Downloaded models that support tool calling (NOCTURN's assistant needs it).
async fn ollama_models() -> Vec<String> {
    #[derive(Deserialize)]
    struct M {
        name: String,
        #[serde(default)]
        capabilities: Vec<String>,
    }
    #[derive(Deserialize)]
    struct T {
        models: Vec<M>,
    }
    let Ok(c) = http() else { return vec![] };
    let Ok(r) = c.get(format!("{OLLAMA_URL}/api/tags")).timeout(Duration::from_secs(4)).send().await else { return vec![] };
    let Ok(t) = r.json::<T>().await else { return vec![] };
    t.models
        .into_iter()
        // Older Ollama versions don't list capabilities: keep those too.
        .filter(|m| m.capabilities.is_empty() || m.capabilities.iter().any(|c| c == "tools"))
        .map(|m| m.name)
        .collect()
}

/// Where Ollama's tray app is installed, if it is: the per-user default, a
/// folder on PATH (the installer adds one, also read from the registry so a
/// just-finished install is found without restarting NOCTURN), or the
/// uninstall entry's install location.
pub fn find_ollama() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Programs").join("Ollama"));
    }
    let mut paths: Vec<String> = std::env::var("PATH").ok().into_iter().collect();
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    if let Ok(env) = hkcu.open_subkey("Environment") {
        if let Ok(p) = env.get_value::<String, _>("Path") {
            paths.push(p);
        }
    }
    for p in paths {
        for part in p.split(';') {
            let part = part.trim();
            if part.to_ascii_lowercase().contains("ollama") {
                dirs.push(PathBuf::from(part));
            }
        }
    }
    if let Ok(k) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{44E83376-CE68-45EB-8FC1-393500EB558C}_is1") {
        if let Ok(loc) = k.get_value::<String, _>("InstallLocation") {
            dirs.push(PathBuf::from(loc));
        }
    }
    dirs.into_iter().find_map(|d| {
        let app = d.join("ollama app.exe");
        let cli = d.join("ollama.exe");
        if app.is_file() {
            Some(app)
        } else if cli.is_file() {
            Some(cli)
        } else {
            None
        }
    })
}

fn free_gb(p: &Path) -> Option<f32> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(p.as_os_str()), Some(&mut free), None, None).ok()? };
    Some(free as f32 / 1e9)
}

fn models_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("OLLAMA_MODELS") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\"));
    home.join(".ollama")
}

pub async fn detect(ram_bytes: u64, vram_bytes: Option<u64>) -> Detect {
    let version = ollama_version().await;
    let running = version.is_some();
    let models = if running { ollama_models().await } else { vec![] };
    let ram_gb = ram_bytes as f32 / 1024f32.powi(3);
    let vram_gb = vram_bytes.filter(|v| *v > 0).map(|v| v as f32 / 1024f32.powi(3));
    let dir = models_dir();
    let probe = if dir.exists() { dir } else { dir.parent().map(Path::to_path_buf).unwrap_or(dir) };
    Detect {
        ollama_running: running,
        ollama_version: version,
        ollama_installed: running || find_ollama().is_some(),
        models,
        ram_gb,
        vram_gb,
        recommended: recommend(ram_gb, vram_gb),
        options: choices(),
        free_gb: free_gb(&probe),
    }
}

/// Fixed help pages the setup can open in the browser.
pub fn open_page(which: &str) -> AppResult<()> {
    let url = match which {
        "claude_keys" => "https://console.anthropic.com/settings/keys",
        "ollama" => "https://ollama.com",
        _ => return Err(AppError::Invalid("unknown page".into())),
    };
    crate::library::shell_open(url)
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn check_cancel() -> AppResult<()> {
    if CANCEL.load(Ordering::SeqCst) {
        Err(AppError::msg("setup cancelled"))
    } else {
        Ok(())
    }
}

/// Stream the installer to disk (it's too big for memory), hashing as it goes.
async fn download_installer(dest: &Path, progress: &(dyn Fn(Progress) + Sync)) -> AppResult<()> {
    let c = http()?;
    let sums = c.get(SUMS_URL).timeout(Duration::from_secs(30)).send().await.map_err(|e| AppError::msg(format!("couldn't reach GitHub: {}", e.without_url())))?;
    if !sums.status().is_success() {
        return Err(AppError::msg(format!("couldn't read Ollama's checksum list (GitHub answered {})", sums.status())));
    }
    let sums = sums.text().await?;
    let want = sums
        .lines()
        .find_map(|l| {
            let mut it = l.split_whitespace();
            let (h, n) = (it.next()?, it.next()?);
            let n = n.trim_start_matches('*').trim_start_matches("./");
            (h.len() == 64 && n == "OllamaSetup.exe").then(|| h.to_ascii_lowercase())
        })
        .ok_or_else(|| AppError::msg("Ollama's checksum list had no entry for the installer"))?;

    let res = c.get(SETUP_URL).timeout(Duration::from_secs(3 * 3600)).send().await.map_err(|e| AppError::msg(format!("download failed: {}", e.without_url())))?;
    if !res.status().is_success() {
        return Err(AppError::msg(format!("download failed: GitHub answered {}", res.status())));
    }
    let total = res.content_length();
    if total.is_some_and(|t| t > SETUP_MAX) {
        return Err(AppError::msg("the installer is far larger than expected — not downloaded"));
    }
    let tmp = dest.with_extension("partial");
    let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    let mut s = res.bytes_stream();
    let started = Instant::now();
    let mut last = Instant::now() - Duration::from_secs(1);
    while let Some(chunk) = s.next().await {
        if CANCEL.load(Ordering::SeqCst) {
            drop(f);
            let _ = std::fs::remove_file(&tmp);
            return Err(AppError::msg("setup cancelled"));
        }
        let chunk = chunk.map_err(|e| AppError::msg(format!("download interrupted: {}", e.without_url())))?;
        hasher.update(&chunk);
        f.write_all(&chunk)?;
        got += chunk.len() as u64;
        if got > SETUP_MAX {
            drop(f);
            let _ = std::fs::remove_file(&tmp);
            return Err(AppError::msg("download larger than expected — stopped"));
        }
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            let rate = got as f64 / started.elapsed().as_secs_f64().max(0.1);
            progress(Progress {
                stage: "download".into(),
                label: "Downloading Ollama".into(),
                fraction: total.map(|t| got as f32 / t as f32),
                detail: Some(size_detail(got, total, rate)),
            });
        }
    }
    f.flush()?;
    drop(f);
    progress(Progress { stage: "verify".into(), label: "Checking the download".into(), fraction: None, detail: None });
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if hex != want {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::msg("the downloaded installer didn't match Ollama's published checksum — not installed"));
    }
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

fn size_detail(got: u64, total: Option<u64>, rate_bps: f64) -> String {
    let gb = |b: u64| b as f64 / 1e9;
    let mut s = match total {
        Some(t) => format!("{:.2} of {:.2} GB", gb(got), gb(t)),
        None => format!("{:.2} GB", gb(got)),
    };
    if rate_bps > 0.0 {
        s.push_str(&format!(" · {:.1} MB/s", rate_bps / 1e6));
        if let Some(t) = total {
            let left = (t.saturating_sub(got)) as f64 / rate_bps;
            if left >= 60.0 {
                s.push_str(&format!(" · about {} min left", (left / 60.0).ceil() as u64));
            } else {
                s.push_str(&format!(" · {} s left", left.ceil() as u64));
            }
        }
    }
    s
}

async fn wait_for_server(secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if ollama_version().await.is_some() {
            return true;
        }
        if CANCEL.load(Ordering::SeqCst) {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    false
}

/// Pull a model through Ollama's API, reporting byte progress.
async fn pull(model: &str, progress: &(dyn Fn(Progress) + Sync)) -> AppResult<()> {
    #[derive(Deserialize)]
    struct Line {
        #[serde(default)]
        status: String,
        total: Option<u64>,
        completed: Option<u64>,
        error: Option<String>,
    }
    let c = http()?;
    let res = c
        .post(format!("{OLLAMA_URL}/api/pull"))
        .json(&serde_json::json!({ "model": model, "stream": true }))
        .timeout(Duration::from_secs(6 * 3600))
        .send()
        .await
        .map_err(|e| AppError::msg(format!("couldn't talk to Ollama: {}", e.without_url())))?;
    if !res.status().is_success() {
        return Err(AppError::msg(format!("Ollama refused the download ({})", res.status())));
    }
    let mut s = res.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let started = Instant::now();
    let mut last = Instant::now() - Duration::from_secs(1);
    let mut ok = false;
    while let Some(chunk) = s.next().await {
        check_cancel()?;
        let chunk = chunk.map_err(|e| AppError::msg(format!("model download interrupted: {}", e.without_url())))?;
        buf.extend_from_slice(&chunk);
        while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=nl).collect();
            let Ok(l) = serde_json::from_slice::<Line>(&line) else { continue };
            if let Some(e) = l.error {
                return Err(AppError::msg(format!("Ollama couldn't download {model}: {e}")));
            }
            if l.status == "success" {
                ok = true;
            }
            let big = l.total.is_some_and(|t| t > 50_000_000);
            if big || last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                let (label, fraction, detail) = match (l.total, l.completed) {
                    (Some(t), Some(d)) if t > 0 => {
                        let rate = d as f64 / started.elapsed().as_secs_f64().max(0.1);
                        ("Downloading the AI model".to_string(), Some(d as f32 / t as f32), Some(size_detail(d, Some(t), rate)))
                    }
                    _ => (pull_label(&l.status), None, None),
                };
                progress(Progress { stage: "pull".into(), label, fraction, detail });
            }
        }
    }
    if !ok {
        return Err(AppError::msg("the model download ended early — try again"));
    }
    Ok(())
}

fn pull_label(status: &str) -> String {
    let s = status.to_ascii_lowercase();
    if s.contains("manifest") {
        "Preparing the model download".into()
    } else if s.contains("verifying") {
        "Checking the model".into()
    } else if s.contains("writing") {
        "Finishing up".into()
    } else {
        "Downloading the AI model".into()
    }
}

/// The whole local setup. Returns the model now configured.
pub async fn setup_local(db: &Db, data_dir: &Path, model: &str, progress: &(dyn Fn(Progress) + Sync)) -> AppResult<String> {
    if !choices().iter().any(|m| m.id == model) && !model.chars().all(|c| c.is_ascii_alphanumeric() || ":._-/".contains(c)) {
        return Err(AppError::Invalid("unknown model".into()));
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::msg("setup is already running"));
    }
    CANCEL.store(false, Ordering::SeqCst);
    let r = run_setup(db, data_dir, model, progress).await;
    RUNNING.store(false, Ordering::SeqCst);
    if let Err(e) = &r {
        db.audit("warn", "user", "ai.setup.failed", Some(serde_json::json!({ "model": model, "error": e.to_string() })));
    }
    r
}

async fn run_setup(db: &Db, data_dir: &Path, model: &str, progress: &(dyn Fn(Progress) + Sync)) -> AppResult<String> {
    if ollama_version().await.is_none() {
        let exe = match find_ollama() {
            Some(e) => e,
            None => {
                let dir = data_dir.join("downloads");
                std::fs::create_dir_all(&dir)?;
                let installer = dir.join("OllamaSetup.exe");
                download_installer(&installer, progress).await?;
                check_cancel()?;
                progress(Progress { stage: "install".into(), label: "Installing Ollama".into(), fraction: None, detail: Some("This takes a minute or two.".into()) });
                db.audit("info", "user", "ai.setup.install_ollama", None);
                let inst = installer.clone();
                let out = tokio::task::spawn_blocking(move || exec::run(&SafeCommand::OllamaInstall { installer: inst }))
                    .await
                    .map_err(|e| AppError::msg(e.to_string()))??;
                let _ = std::fs::remove_file(&installer);
                if !out.success() {
                    return Err(AppError::msg(format!("the Ollama installer stopped (code {:?})", out.status)));
                }
                find_ollama().ok_or_else(|| AppError::msg("Ollama was installed but couldn't be found afterwards — open it once from the Start menu, then try again"))?
            }
        };
        progress(Progress { stage: "start".into(), label: "Starting Ollama".into(), fraction: None, detail: None });
        // The installer usually starts it; give that a moment before starting it ourselves.
        if !wait_for_server(8).await {
            check_cancel()?;
            exec::spawn_ollama(&exe)?;
            if !wait_for_server(60).await {
                check_cancel()?;
                return Err(AppError::msg("Ollama didn't start — open it from the Start menu, then try again"));
            }
        }
    }
    check_cancel()?;

    let have = ollama_models().await;
    if !have.iter().any(|m| m == model || m.strip_suffix(":latest") == Some(model)) {
        progress(Progress { stage: "pull".into(), label: "Downloading the AI model".into(), fraction: Some(0.0), detail: None });
        pull(model, progress).await?;
    }

    progress(Progress { stage: "configure".into(), label: "Connecting NOCTURN to it".into(), fraction: None, detail: None });
    let mut s = crate::settings::load(db);
    s.ai.provider = "openai_compatible".into();
    s.ai.base_url = OLLAMA_BASE_URL.into();
    s.ai.model = model.into();
    s.ai.max_tokens = s.ai.max_tokens.min(16_384);
    crate::settings::save(db, &s)?;
    db.audit("info", "user", "ai.setup.done", Some(serde_json::json!({ "model": model })));
    progress(Progress { stage: "done".into(), label: "Ready".into(), fraction: Some(1.0), detail: None });
    Ok(model.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_follows_hardware() {
        assert_eq!(recommend(32.0, Some(16.0)).id, "qwen3:14b");
        assert_eq!(recommend(16.0, Some(8.0)).id, "qwen3:8b");
        assert_eq!(recommend(32.0, Some(12.0)).id, "qwen3:8b");
        assert_eq!(recommend(16.0, Some(4.0)).id, "qwen3:4b");
        assert_eq!(recommend(16.0, None).id, "qwen3:4b");
        assert_eq!(recommend(8.0, None).id, "qwen3:1.7b");
        assert_eq!(recommend(4.0, Some(2.0)).id, "qwen3:1.7b");
        assert!(!recommend(16.0, Some(8.0)).why.is_empty());
    }

    #[test]
    fn size_detail_reads_well() {
        assert_eq!(size_detail(500_000_000, Some(2_000_000_000), 10e6), "0.50 of 2.00 GB · 10.0 MB/s · about 3 min left");
        assert_eq!(size_detail(1_990_000_000, Some(2_000_000_000), 10e6), "1.99 of 2.00 GB · 10.0 MB/s · 1 s left");
    }

    /// Against the real Ollama on this PC (if any); changes nothing.
    #[tokio::test]
    #[ignore]
    async fn live_detect() {
        let d = detect(32 * 1024 * 1024 * 1024, Some(8 * 1024 * 1024 * 1024)).await;
        eprintln!("{d:#?}\nexe: {:?}", find_ollama());
    }
}
