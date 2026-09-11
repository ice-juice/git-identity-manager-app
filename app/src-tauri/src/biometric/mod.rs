//! 平台原生生物识别：解锁走硬件密钥绑定（路线 B），隐私重认证走存在性确认（路线 A）。

mod store;

#[cfg(windows)]
mod windows;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(windows, target_os = "macos")))]
mod linux;

#[cfg(windows)]
use windows as backend;
#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(not(any(windows, target_os = "macos")))]
use linux as backend;

use crate::error::{AppError, Result};
use crate::vault::crypto::{MasterKey, KEY_LEN};
use crate::vault::envelope;
use serde::Serialize;
use tauri::AppHandle;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use tauri::Manager;

pub use store::{is_enrolled_for, BiometricEnvFile};

#[derive(Debug, Clone, Copy)]
pub struct BiometricAvailability {
    pub available: bool,
    pub kind: &'static str,
    pub strong: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BiometricStatus {
    pub available: bool,
    pub strong: bool,
    pub kind: String,
    pub enabled: bool,
    pub reveal_enabled: bool,
    pub reveal_secret: bool,
    pub stale: bool,
}

#[cfg(windows)]
static PROMPT_HWND: Mutex<Option<isize>> = Mutex::new(None);

pub fn bind_window(app: &AppHandle) {
    #[cfg(windows)]
    {
        if let Some(w) = app.get_webview_window("main") {
            if let Ok(h) = w.hwnd() {
                *PROMPT_HWND.lock().unwrap_or_else(|e| e.into_inner()) = Some(h.0 as isize);
            }
        }
    }
    let _ = app;
}

#[cfg(windows)]
pub(crate) fn prompt_hwnd() -> Option<isize> {
    *PROMPT_HWND.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn availability() -> BiometricAvailability {
    backend::availability()
}

pub fn status(workspace_id: Option<&str>, cfg: &crate::app_config::AppConfig) -> BiometricStatus {
    let avail = availability();
    let enrolled = workspace_id.map(store::is_enrolled_for).unwrap_or(false);
    let stale = cfg.biometric_unlock_enabled && workspace_id.is_some() && !enrolled;
    BiometricStatus {
        available: avail.available,
        strong: avail.strong,
        kind: avail.kind.to_string(),
        enabled: cfg.biometric_unlock_enabled && enrolled,
        reveal_enabled: cfg.biometric_reveal_enabled,
        reveal_secret: cfg.biometric_reveal_secret,
        stale,
    }
}

pub fn enable(workspace_id: &str, mk: &MasterKey) -> Result<()> {
    if !availability().available {
        return Err(AppError::Invalid(
            "本机未检测到可用的指纹 / 人脸 / Windows Hello 硬件".into(),
        ));
    }
    let key_ref = format!("gam-vault-{workspace_id}");
    let mut challenge = vec![0u8; KEY_LEN];
    crate::vault::crypto::fill_random(&mut challenge);
    let raw = match backend::enroll(&key_ref, &challenge) {
        Ok(v) => v,
        Err(e) => {
            let _ = backend::remove(&key_ref);
            return Err(e);
        }
    };
    let envelope = match envelope::wrap_with_biometric(mk, &raw) {
        Ok(env) => env,
        Err(e) => {
            let _ = backend::remove(&key_ref);
            return Err(e);
        }
    };
    let file = BiometricEnvFile {
        version: store::VERSION,
        platform: store::current_platform().into(),
        workspace_id: workspace_id.to_string(),
        key_ref,
        challenge: store::encode_challenge(&challenge),
        envelope,
        created_at: crate::util::now_rfc3339(),
    };
    if let Err(e) = store::save(&file) {
        let _ = backend::remove(&file.key_ref);
        return Err(e);
    }
    Ok(())
}

pub fn disable() -> Result<()> {
    if let Ok(file) = store::load() {
        let _ = backend::remove(&file.key_ref);
    }
    store::clear();
    Ok(())
}

pub fn unlock_master_key(workspace_id: &str) -> Result<MasterKey> {
    let file = store::load_for_workspace(workspace_id)?;
    let challenge = store::decode_challenge(&file.challenge)?;
    let raw = backend::derive(&file.key_ref, &challenge)?;
    envelope::unwrap_with_biometric(&file.envelope, &raw)
}

pub fn verify_presence(prompt: &str) -> Result<()> {
    backend::verify_presence(prompt)
}

pub fn factory_reset_cleanup() {
    if let Ok(file) = store::load() {
        let _ = backend::remove(&file.key_ref);
    }
    store::clear();
}
