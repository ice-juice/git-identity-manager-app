//! 本机限时免验证会话：用 Windows DPAPI 包裹主密钥，过期后必须重新输入访问密码。

use crate::app_config;
use crate::error::{AppError, Result};
use crate::vault::crypto::KEY_LEN;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_GRACE_DAYS: u32 = 30;

fn session_path() -> PathBuf {
    app_config::config_dir().join("grace.dpapi")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 打包：version(1) || expires_le(8) || id_len_le(2) || workspace_id || mk(32)
pub fn pack(workspace_id: &str, mk: &[u8; KEY_LEN], expires_at: u64) -> Vec<u8> {
    let id = workspace_id.as_bytes();
    let mut out = Vec::with_capacity(1 + 8 + 2 + id.len() + KEY_LEN);
    out.push(1);
    out.extend_from_slice(&expires_at.to_le_bytes());
    out.extend_from_slice(&(id.len() as u16).to_le_bytes());
    out.extend_from_slice(id);
    out.extend_from_slice(mk);
    out
}

pub fn unpack(data: &[u8]) -> Result<(String, [u8; KEY_LEN], u64)> {
    if data.len() < 1 + 8 + 2 + KEY_LEN {
        return Err(AppError::Crypto);
    }
    if data[0] != 1 {
        return Err(AppError::Crypto);
    }
    let expires = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let id_len = u16::from_le_bytes(data[9..11].try_into().unwrap()) as usize;
    let id_end = 11 + id_len;
    if data.len() != id_end + KEY_LEN {
        return Err(AppError::Crypto);
    }
    let id = String::from_utf8(data[11..id_end].to_vec()).map_err(|_| AppError::Crypto)?;
    let mut mk = [0u8; KEY_LEN];
    mk.copy_from_slice(&data[id_end..]);
    Ok((id, mk, expires))
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraceInfo {
    pub active: bool,
    pub expires_at: Option<String>,
}

pub fn clamp_days(days: u32) -> u32 {
    days.min(MAX_GRACE_DAYS)
}

pub fn clear() {
    let _ = std::fs::remove_file(session_path());
}

pub fn grant(workspace_id: &str, mk: &[u8; KEY_LEN], days: u32) -> Result<u64> {
    let days = clamp_days(days);
    if days == 0 {
        clear();
        return Ok(0);
    }
    let expires_at = now_unix().saturating_add(u64::from(days) * 86_400);
    let plain = pack(workspace_id, mk, expires_at);
    let blob = protect(&plain)?;
    crate::vault::atomic_write(&session_path(), &blob)?;
    // 用完即清零明文。
    let _ = plain;
    Ok(expires_at)
}

pub fn try_restore(expected_workspace_id: &str) -> Result<[u8; KEY_LEN]> {
    let path = session_path();
    if !path.is_file() {
        return Err(AppError::Locked);
    }
    let blob = std::fs::read(&path)?;
    let plain = unprotect(&blob)?;
    let (id, mk, expires) = match unpack(&plain) {
        Ok(v) => v,
        Err(e) => {
            clear();
            return Err(e);
        }
    };
    if expires <= now_unix() {
        clear();
        return Err(AppError::Invalid("免验证已过期，请输入访问密码".into()));
    }
    if id != expected_workspace_id {
        clear();
        return Err(AppError::Invalid("免验证会话与当前工作空间不匹配".into()));
    }
    Ok(mk)
}

pub fn info(expected_workspace_id: Option<&str>) -> GraceInfo {
    let path = session_path();
    if !path.is_file() {
        return GraceInfo {
            active: false,
            expires_at: None,
        };
    }
    let Ok(blob) = std::fs::read(&path) else {
        return GraceInfo {
            active: false,
            expires_at: None,
        };
    };
    let Ok(plain) = unprotect(&blob) else {
        return GraceInfo {
            active: false,
            expires_at: None,
        };
    };
    let Ok((id, _mk, expires)) = unpack(&plain) else {
        return GraceInfo {
            active: false,
            expires_at: None,
        };
    };
    if expires <= now_unix() || expected_workspace_id.is_some_and(|w| w != id) {
        if expires <= now_unix() {
            clear();
        }
        return GraceInfo {
            active: false,
            expires_at: None,
        };
    }
    GraceInfo {
        active: true,
        expires_at: unix_to_rfc3339(expires),
    }
}

fn unix_to_rfc3339(ts: u64) -> Option<String> {
    time::OffsetDateTime::from_unix_timestamp(ts as i64)
        .ok()
        .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok())
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi::protect(data)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi::unprotect(data)
}

#[cfg(not(windows))]
fn protect(_data: &[u8]) -> Result<Vec<u8>> {
    Err(AppError::Invalid("免验证启动仅支持 Windows".into()))
}

#[cfg(not(windows))]
fn unprotect(_data: &[u8]) -> Result<Vec<u8>> {
    Err(AppError::Invalid("免验证启动仅支持 Windows".into()))
}

#[cfg(windows)]
mod dpapi {
    use crate::error::{AppError, Result};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };

    pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
        crypt(data, true)
    }
    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
        crypt(data, false)
    }

    fn crypt(data: &[u8], wrap: bool) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = unsafe {
            if wrap {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        if ok == 0 {
            return Err(AppError::Crypto);
        }
        let out = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        unsafe {
            LocalFree(output.pbData as _);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_roundtrip() {
        let mk = [7u8; 32];
        let raw = pack("ws-1", &mk, 1_700_000_000);
        let (id, out, exp) = unpack(&raw).unwrap();
        assert_eq!(id, "ws-1");
        assert_eq!(out, mk);
        assert_eq!(exp, 1_700_000_000);
    }

    #[test]
    fn unpack_rejects_truncation() {
        assert!(unpack(&[1, 2, 3]).is_err());
    }
}
