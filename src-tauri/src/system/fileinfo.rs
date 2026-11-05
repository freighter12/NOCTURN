//! Facts about an executable: what it says it is (version resource), whether
//! it carries a valid Authenticode signature, and where a shortcut points.
//!
//! The publisher shown comes from the file's own version resource — it is
//! self-reported. A valid signature means the file hasn't been altered since
//! a trusted publisher signed it; it does not prove the signer is the company
//! named in the version resource. The UI labels both accordingly.

use serde::Serialize;
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::WinTrust::{
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY,
    WTD_UI_NONE,
};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct VersionFacts {
    pub company: Option<String>,
    pub description: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Signature {
    /// Signed, and the signature verifies.
    Valid,
    /// No embedded signature. Windows' own files are usually catalog-signed
    /// and show up here too; callers treat files under %WINDIR% separately.
    Unsigned,
    /// A signature is present but doesn't verify (altered file, untrusted or
    /// revoked certificate).
    Invalid,
    /// The file couldn't be checked (missing, access denied).
    Unknown,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn version_facts(path: &str) -> VersionFacts {
    let p = HSTRING::from(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(&p, None);
        if size == 0 {
            return VersionFacts::default();
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(&p, None, size, buf.as_mut_ptr() as _).is_err() {
            return VersionFacts::default();
        }
        // First language/code-page pair; fall back to US English / Unicode.
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let trans = if VerQueryValueW(buf.as_ptr() as _, &HSTRING::from(r"\VarFileInfo\Translation"), &mut ptr, &mut len).as_bool() && len >= 4 {
            let w = std::slice::from_raw_parts(ptr as *const u16, 2);
            format!("{:04x}{:04x}", w[0], w[1])
        } else {
            "040904b0".to_string()
        };
        let get = |field: &str| -> Option<String> {
            let mut p = std::ptr::null_mut();
            let mut n = 0u32;
            let key = HSTRING::from(format!(r"\StringFileInfo\{trans}\{field}"));
            if !VerQueryValueW(buf.as_ptr() as _, &key, &mut p, &mut n).as_bool() || n == 0 {
                return None;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, n as usize));
            let t = s.trim_matches(char::from(0)).trim().to_string();
            (!t.is_empty()).then_some(t)
        };
        VersionFacts { company: get("CompanyName"), description: get("FileDescription"), product: get("ProductName") }
    }
}

/// Authenticode check without network access (no revocation lookup), so it
/// is fast and never phones home.
pub fn signature(path: &str) -> Signature {
    if !std::path::Path::new(path).is_file() {
        return Signature::Unknown;
    }
    let w = wide(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(w.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe { WinVerifyTrust(HWND(-1isize as _), &mut action, &mut data as *mut _ as _) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(HWND(-1isize as _), &mut action, &mut data as *mut _ as _);
    }
    const TRUST_E_NOSIGNATURE: i32 = 0x800B0100u32 as i32;
    const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B0003u32 as i32;
    const TRUST_E_PROVIDER_UNKNOWN: i32 = 0x800B0001u32 as i32;
    match status {
        0 => Signature::Valid,
        TRUST_E_NOSIGNATURE | TRUST_E_SUBJECT_FORM_UNKNOWN | TRUST_E_PROVIDER_UNKNOWN => Signature::Unsigned,
        _ => Signature::Invalid,
    }
}

/// Target of a `.lnk` shortcut, if it can be resolved.
pub fn shortcut_target(lnk: &str) -> Option<String> {
    unsafe {
        // Safe to call repeatedly; returns S_FALSE if already initialised.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]);
        (!s.is_empty()).then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_version_facts_of_a_windows_binary() {
        let v = version_facts(r"C:\Windows\System32\notepad.exe");
        // Notepad may be the Store app stub on Windows 11, but the System32
        // file still carries Microsoft's version resource.
        assert!(v.company.as_deref().unwrap_or("").contains("Microsoft"), "{v:?}");
        assert_eq!(version_facts(r"C:\definitely\missing.exe"), VersionFacts::default());
    }

    #[test]
    fn signature_states() {
        // Windows binaries are catalog-signed: an embedded-signature check
        // reports them as unsigned, which is why callers special-case %WINDIR%.
        let s = signature(r"C:\Windows\System32\notepad.exe");
        assert!(matches!(s, Signature::Valid | Signature::Unsigned), "{s:?}");
        assert_eq!(signature(r"C:\definitely\missing.exe"), Signature::Unknown);
        // A plain text file has no signature.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.exe");
        std::fs::write(&f, b"not a program").unwrap();
        assert_ne!(signature(f.to_str().unwrap()), Signature::Valid);
    }

    #[test]
    fn signed_third_party_binary_verifies_if_present() {
        // Any Authenticode-signed app on the machine; skip if none is found.
        for p in [r"C:\Program Files\Mozilla Firefox\firefox.exe", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", r"C:\Program Files\Google\Chrome\Application\chrome.exe"] {
            if std::path::Path::new(p).exists() {
                assert_eq!(signature(p), Signature::Valid, "{p}");
                return;
            }
        }
    }
}
