//! Raw, faithful registry access for single values.
//!
//! Originals are captured as `(type, bytes)` so rollback restores *exactly*
//! what was there, regardless of value type. All access uses the 64-bit view.

use std::borrow::Cow;
use std::io::ErrorKind;

use serde::{Deserialize, Serialize};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

use super::{Hive, RegData};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawValue {
    pub vtype: u32,
    pub bytes: Vec<u8>,
}

impl RawValue {
    pub fn from_data(d: &RegData) -> Self {
        match d {
            RegData::Dword(v) => RawValue { vtype: REG_DWORD as u32, bytes: v.to_le_bytes().to_vec() },
            RegData::Sz(s) => {
                let mut bytes = Vec::with_capacity(s.len() * 2 + 2);
                for u in s.encode_utf16().chain(Some(0)) {
                    bytes.extend_from_slice(&u.to_le_bytes());
                }
                RawValue { vtype: REG_SZ as u32, bytes }
            }
        }
    }

    /// Whether this stored value is semantically equal to the desired data.
    pub fn matches(&self, d: &RegData) -> bool {
        match d {
            RegData::Dword(v) => self.as_dword() == Some(*v),
            RegData::Sz(s) => self.as_string().as_deref() == Some(s.as_str()),
        }
    }

    pub fn as_dword(&self) -> Option<u32> {
        if self.vtype == REG_DWORD as u32 && self.bytes.len() >= 4 {
            Some(u32::from_le_bytes(self.bytes[..4].try_into().ok()?))
        } else {
            None
        }
    }

    pub fn as_string(&self) -> Option<String> {
        if self.vtype == REG_SZ as u32 || self.vtype == REG_EXPAND_SZ as u32 {
            let words: Vec<u16> = self
                .bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&w| w != 0)
                .collect();
            Some(String::from_utf16_lossy(&words))
        } else {
            None
        }
    }

    /// Human-readable rendering for previews and history.
    pub fn display(&self) -> String {
        if let Some(d) = self.as_dword() {
            return format!("{d} (0x{d:X})");
        }
        if let Some(s) = self.as_string() {
            return format!("\"{s}\"");
        }
        if self.vtype == REG_QWORD as u32 && self.bytes.len() >= 8 {
            let q = u64::from_le_bytes(self.bytes[..8].try_into().unwrap_or_default());
            return format!("{q} (QWORD)");
        }
        format!("<{} bytes, type {}>", self.bytes.len(), self.vtype)
    }
}

pub fn display_data(d: &RegData) -> String {
    RawValue::from_data(d).display()
}

fn root(h: Hive) -> RegKey {
    match h {
        Hive::Hkcu => RegKey::predef(HKEY_CURRENT_USER),
        Hive::Hklm => RegKey::predef(HKEY_LOCAL_MACHINE),
    }
}

fn vtype_from_u32(v: u32) -> AppResult<RegType> {
    Ok(match v {
        x if x == REG_NONE as u32 => REG_NONE,
        x if x == REG_SZ as u32 => REG_SZ,
        x if x == REG_EXPAND_SZ as u32 => REG_EXPAND_SZ,
        x if x == REG_BINARY as u32 => REG_BINARY,
        x if x == REG_DWORD as u32 => REG_DWORD,
        x if x == REG_DWORD_BIG_ENDIAN as u32 => REG_DWORD_BIG_ENDIAN,
        x if x == REG_MULTI_SZ as u32 => REG_MULTI_SZ,
        x if x == REG_QWORD as u32 => REG_QWORD,
        other => return Err(AppError::Invalid(format!("unsupported registry type {other}"))),
    })
}

fn map_access(e: std::io::Error, what: &str) -> AppError {
    if e.kind() == ErrorKind::PermissionDenied {
        AppError::Forbidden(format!("{what}: administrator rights required"))
    } else {
        AppError::msg(format!("{what}: {e}"))
    }
}

/// Read a value. `Ok(None)` when the key or value does not exist.
pub fn read(hive: Hive, path: &str, name: &str) -> AppResult<Option<RawValue>> {
    let key = match root(hive).open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY) {
        Ok(k) => k,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(map_access(e, &format!("open {}\\{path}", hive.label()))),
    };
    match key.get_raw_value(name) {
        Ok(v) => Ok(Some(RawValue { vtype: v.vtype as u32, bytes: v.bytes.into_owned() })),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(map_access(e, &format!("read {}\\{path}\\{name}", hive.label()))),
    }
}

/// Write a value, creating the key if needed.
pub fn write(hive: Hive, path: &str, name: &str, value: &RawValue) -> AppResult<()> {
    let (key, _) = root(hive)
        .create_subkey_with_flags(path, KEY_SET_VALUE | KEY_QUERY_VALUE | KEY_WOW64_64KEY)
        .map_err(|e| map_access(e, &format!("open {}\\{path} for writing", hive.label())))?;
    let rv = RegValue { bytes: Cow::Owned(value.bytes.clone()), vtype: vtype_from_u32(value.vtype)? };
    key.set_raw_value(name, &rv)
        .map_err(|e| map_access(e, &format!("write {}\\{path}\\{name}", hive.label())))
}

/// Delete a value. Succeeds if it is already absent.
pub fn delete(hive: Hive, path: &str, name: &str) -> AppResult<()> {
    let key = match root(hive).open_subkey_with_flags(path, KEY_SET_VALUE | KEY_WOW64_64KEY) {
        Ok(k) => k,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(map_access(e, &format!("open {}\\{path}", hive.label()))),
    };
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(map_access(e, &format!("delete {}\\{path}\\{name}", hive.label()))),
    }
}

/// Restore a captured original (write it back, or delete if it was absent).
pub fn restore(hive: Hive, path: &str, name: &str, original: &Option<RawValue>) -> AppResult<()> {
    match original {
        Some(v) => write(hive, path, name, v),
        None => delete(hive, path, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\NOCTURN\UnitTests\registry";

    #[test]
    fn dword_and_string_encoding() {
        let d = RawValue::from_data(&RegData::Dword(0xffff_ffff));
        assert_eq!(d.as_dword(), Some(0xffff_ffff));
        assert!(d.matches(&RegData::Dword(0xffff_ffff)));
        assert!(!d.matches(&RegData::Dword(0)));

        let s = RawValue::from_data(&RegData::Sz("100".into()));
        assert_eq!(s.as_string().as_deref(), Some("100"));
        assert!(s.matches(&RegData::Sz("100".into())));
        assert!(!s.matches(&RegData::Dword(100)));
    }

    #[test]
    fn write_read_restore_cycle_hkcu() {
        let name = "Value";
        delete(Hive::Hkcu, TEST_KEY, name).unwrap();
        assert_eq!(read(Hive::Hkcu, TEST_KEY, name).unwrap(), None);

        let original = read(Hive::Hkcu, TEST_KEY, name).unwrap();
        let v = RawValue::from_data(&RegData::Dword(7));
        write(Hive::Hkcu, TEST_KEY, name, &v).unwrap();
        assert_eq!(read(Hive::Hkcu, TEST_KEY, name).unwrap(), Some(v));

        restore(Hive::Hkcu, TEST_KEY, name, &original).unwrap();
        assert_eq!(read(Hive::Hkcu, TEST_KEY, name).unwrap(), None);

        // Restoring a pre-existing value of a different type is faithful.
        let pre = RawValue::from_data(&RegData::Sz("before".into()));
        write(Hive::Hkcu, TEST_KEY, name, &pre).unwrap();
        let captured = read(Hive::Hkcu, TEST_KEY, name).unwrap();
        write(Hive::Hkcu, TEST_KEY, name, &RawValue::from_data(&RegData::Dword(1))).unwrap();
        restore(Hive::Hkcu, TEST_KEY, name, &captured).unwrap();
        assert_eq!(read(Hive::Hkcu, TEST_KEY, name).unwrap(), Some(pre));

        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(r"Software\NOCTURN\UnitTests");
    }
}
