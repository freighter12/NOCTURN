//! Real icons for Library apps, taken from Windows itself.
//!
//! Each Start-menu app is asked for its icon through the shell
//! (`shell:AppsFolder\<AppID>` → `IShellItemImageFactory`), which works the
//! same for desktop programs and Store apps and never runs anything. Only
//! AppIDs that are in the current Start list are accepted. Icons are cached
//! as PNG files in the data folder and handed to the page as data URLs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::library;

/// Pixel size asked from Windows (tiles are drawn at 24–52 px; this stays sharp at 125–150% scaling).
const ICON_PX: i32 = 64;
const MAX_PER_CALL: usize = 300;
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// AppIDs Windows had no icon for this session (not retried until restart).
static MISSING: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);

fn cache_file(dir: &Path, app_id: &str) -> PathBuf {
    let h = Sha256::digest(app_id.as_bytes());
    let name: String = h.iter().take(12).map(|b| format!("{b:02x}")).collect();
    dir.join(format!("{name}.png"))
}

fn fresh(p: &Path) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < CACHE_TTL)
        .unwrap_or(false)
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

/// Icons for the given Start-menu AppIDs: data URL, or None when Windows has none.
/// AppIDs that aren't in the current Start list (e.g. uninstalled apps) get None
/// without ever reaching the shell, so the page can't probe arbitrary paths.
pub fn icons(data_dir: &Path, app_ids: &[String]) -> AppResult<HashMap<String, Option<String>>> {
    if app_ids.len() > MAX_PER_CALL {
        return Err(AppError::Invalid(format!("at most {MAX_PER_CALL} icons at a time")));
    }
    let known: std::collections::HashSet<String> = library::start_apps(false)?.into_iter().map(|a| a.app_id).collect();
    let dir = data_dir.join("icon-cache");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::msg(format!("icon cache: {e}")))?;

    let mut out = HashMap::new();
    let mut todo = Vec::new();
    {
        let missing = MISSING.lock().unwrap_or_else(|e| e.into_inner());
        for id in app_ids {
            if !library::valid_app_id(id) || !known.contains(id) {
                out.insert(id.clone(), None);
                continue;
            }
            let f = cache_file(&dir, id);
            if fresh(&f) {
                if let Ok(png) = std::fs::read(&f) {
                    out.insert(id.clone(), Some(data_url(&png)));
                    continue;
                }
            }
            if missing.as_ref().is_some_and(|m| m.contains(id)) {
                out.insert(id.clone(), None);
            } else {
                todo.push(id.clone());
            }
        }
    }
    if todo.is_empty() {
        return Ok(out);
    }

    // The shell wants a single-threaded COM apartment; use a thread of our own.
    let extracted: Vec<(String, Option<Vec<u8>>)> = std::thread::spawn(move || extract_all(&todo))
        .join()
        .map_err(|_| AppError::msg("icon extraction stopped unexpectedly"))?;
    let mut missing = MISSING.lock().unwrap_or_else(|e| e.into_inner());
    for (id, png) in extracted {
        match png {
            Some(png) => {
                let _ = std::fs::write(cache_file(&dir, &id), &png);
                out.insert(id, Some(data_url(&png)));
            }
            None => {
                missing.get_or_insert_with(Default::default).insert(id.clone());
                out.insert(id, None);
            }
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn extract_all(ids: &[String]) -> Vec<(String, Option<Vec<u8>>)> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let out = ids
        .iter()
        .map(|id| {
            let png = extract(id).map_err(|e| tracing::debug!("no icon for {id}: {e}")).ok().flatten();
            (id.clone(), png)
        })
        .collect();
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    out
}

#[cfg(not(windows))]
fn extract_all(ids: &[String]) -> Vec<(String, Option<Vec<u8>>)> {
    ids.iter().map(|id| (id.clone(), None)).collect()
}

#[cfg(windows)]
fn extract(app_id: &str) -> windows::core::Result<Option<Vec<u8>>> {
    extract_named(&format!(r"shell:AppsFolder\{app_id}"))
}

/// The icon of a file (e.g. a game's .exe) as PNG, from the shell. Runs on its own COM thread.
#[cfg(windows)]
pub fn file_icon_png(path: &str) -> Option<Vec<u8>> {
    let p = path.to_string();
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let r = extract_named(&p).ok().flatten();
        if init.is_ok() {
            unsafe { CoUninitialize() };
        }
        r
    })
    .join()
    .ok()
    .flatten()
}

#[cfg(windows)]
fn extract_named(name: &str) -> windows::core::Result<Option<Vec<u8>>> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};

    let path = HSTRING::from(name);
    let factory: IShellItemImageFactory = unsafe { SHCreateItemFromParsingName(&path, None)? };
    let hbmp = unsafe { factory.GetImage(SIZE { cx: ICON_PX, cy: ICON_PX }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)? };

    let result = (|| {
        let mut bm = BITMAP::default();
        let got = unsafe { GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) };
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmWidth > 1024 || bm.bmHeight > 1024 {
            return Ok(None);
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = unsafe { CreateCompatibleDC(None) };
        let lines = unsafe { GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) };
        unsafe {
            let _ = DeleteDC(dc);
        }
        if lines == 0 {
            return Ok(None);
        }
        Ok(Some(to_png(&mut px, w as u32, h as u32)))
    })();
    unsafe {
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
    }
    result
}

/// BGRA (premultiplied, as the shell hands it over) → straight-alpha RGBA PNG.
/// Bitmaps without any alpha are treated as opaque.
fn to_png(px: &mut [u8], w: u32, h: u32) -> Vec<u8> {
    let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
    for p in px.chunks_exact_mut(4) {
        let (b, g, r, a) = (p[0], p[1], p[2], if has_alpha { p[3] } else { 255 });
        let un = |c: u8| if a == 0 { 0 } else if a == 255 { c } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        p[0] = un(r);
        p[1] = un(g);
        p[2] = un(b);
        p[3] = a;
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut wr) = enc.write_header() {
            let _ = wr.write_image_data(px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_conversion_unpremultiplies_and_handles_no_alpha() {
        // One half-transparent pixel: premultiplied BGRA (50, 100, 25, 128) → RGBA (50, 199, 100, 128).
        let mut px = vec![50, 100, 25, 128];
        let png = to_png(&mut px, 1, 1);
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(px, vec![50, 199, 100, 128]);
        // No alpha anywhere → opaque, channels swapped only.
        let mut px = vec![10, 20, 30, 0, 40, 50, 60, 0];
        to_png(&mut px, 2, 1);
        assert_eq!(px, vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn cache_names_are_stable_and_path_safe() {
        let d = Path::new("x");
        let a = cache_file(d, r"{6D809377}\Blender\blender.exe");
        assert_eq!(a, cache_file(d, r"{6D809377}\Blender\blender.exe"));
        assert_ne!(a, cache_file(d, "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"));
        let name = a.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with(".png") && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'));
    }

    /// Real shell round-trip: Calculator or Notepad should be on every Windows 11 PC.
    #[test]
    #[ignore]
    fn extracts_a_real_icon() {
        let apps = library::start_apps(true).unwrap();
        let a = apps.iter().find(|a| a.name.contains("Calculator") || a.name.contains("Notepad")).expect("a built-in app");
        let got = extract_all(std::slice::from_ref(&a.app_id));
        let png = got[0].1.as_ref().expect("an icon");
        assert!(png.len() > 200, "png too small: {}", png.len());
    }
}
