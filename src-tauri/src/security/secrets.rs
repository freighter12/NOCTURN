//! API key storage in Windows Credential Manager (generic credentials,
//! per-user, encrypted by DPAPI). Keys never touch SQLite, log files or the
//! frontend — the UI only ever learns whether a key is set.

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

use crate::error::{AppError, AppResult};

const PREFIX: &str = "NOCTURN/ai/";

fn target_name(name: &str) -> AppResult<String> {
    if name.is_empty()
        || name.len() > 64
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::Invalid("invalid credential name".into()));
    }
    Ok(format!("{PREFIX}{name}"))
}

fn target(name: &str) -> AppResult<HSTRING> {
    Ok(HSTRING::from(target_name(name)?))
}

pub fn set(name: &str, secret: &str) -> AppResult<()> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(AppError::Invalid("API key is empty".into()));
    }
    if secret.len() > 2048 {
        return Err(AppError::Invalid("API key is unexpectedly long".into()));
    }
    let mut target_w: Vec<u16> = target_name(name)?.encode_utf16().chain(Some(0)).collect();
    let mut user_w: Vec<u16> = "nocturn".encode_utf16().chain(Some(0)).collect();
    let mut blob = secret.as_bytes().to_vec();

    let cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user_w.as_mut_ptr()),
        ..Default::default()
    };
    let res = unsafe { CredWriteW(&cred, 0) };
    // Best-effort scrub of our copy of the secret.
    blob.iter_mut().for_each(|b| *b = 0);
    res.map_err(|e| AppError::msg(format!("could not save API key to Credential Manager: {e}")))
}

pub fn get(name: &str) -> AppResult<Option<String>> {
    let target = target(name)?;
    let mut pcred: *mut CREDENTIALW = std::ptr::null_mut();
    let res = unsafe { CredReadW(&target, CRED_TYPE_GENERIC, None, &mut pcred) };
    match res {
        Ok(()) => {
            let value = unsafe {
                let c = &*pcred;
                let bytes =
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
                let s = String::from_utf8_lossy(bytes).into_owned();
                CredFree(pcred as *const _);
                s
            };
            Ok(Some(value))
        }
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
        Err(e) => Err(AppError::msg(format!("could not read API key: {e}"))),
    }
}

pub fn delete(name: &str) -> AppResult<()> {
    let target = target(name)?;
    match unsafe { CredDeleteW(&target, CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(e) => Err(AppError::msg(format!("could not delete API key: {e}"))),
    }
}

pub fn exists(name: &str) -> bool {
    matches!(get(name), Ok(Some(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_names() {
        assert!(target("").is_err());
        assert!(target("a/b").is_err());
        assert!(target("anthropic").is_ok());
    }

    #[test]
    fn roundtrip() {
        let name = "unit-test-credential";
        set(name, "sk-test-123").unwrap();
        assert_eq!(get(name).unwrap().as_deref(), Some("sk-test-123"));
        delete(name).unwrap();
        assert_eq!(get(name).unwrap(), None);
        delete(name).unwrap(); // idempotent
    }
}
