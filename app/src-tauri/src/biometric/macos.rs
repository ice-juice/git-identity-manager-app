//! macOS：Keychain ACL 存随机 KEK（路线 B）。读钥匙串时由系统弹出 Touch ID。

use crate::error::{AppError, Result};
use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password_options, AccessControlOptions,
    PasswordOptions,
};

use super::BiometricAvailability;

const SERVICE: &str = "com.jeck.gitkeymaster.biometric";

pub fn availability() -> BiometricAvailability {
    BiometricAvailability {
        available: true,
        kind: "touch-id",
        strong: true,
    }
}

pub fn enroll(key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    let mut raw = vec![0u8; crate::vault::crypto::KEY_LEN];
    crate::vault::crypto::fill_random(&mut raw);
    let _ = delete_generic_password(SERVICE, key_ref);
    let mut opts = PasswordOptions::new_generic_password(SERVICE, key_ref);
    opts.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
    opts.set_access_synchronized(Some(false));
    set_generic_password_options(&raw, opts)
        .map_err(|e| AppError::Other(format!("无法写入钥匙串：{e}")))?;
    Ok(raw)
}

pub fn derive(key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    generic_password(PasswordOptions::new_generic_password(SERVICE, key_ref)).map_err(map_keychain)
}

pub fn verify_presence(_prompt: &str) -> Result<()> {
    let file = super::store::load().map_err(|_| AppError::BiometricStale)?;
    let _ = derive(&file.key_ref, &[])?;
    Ok(())
}

pub fn remove(key_ref: &str) -> Result<()> {
    match delete_generic_password(SERVICE, key_ref) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("-25300") || msg.to_lowercase().contains("not found") {
                Ok(())
            } else {
                Err(AppError::Other(format!("无法删除钥匙串项：{e}")))
            }
        }
    }
}

fn map_keychain(e: security_framework::base::Error) -> AppError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if msg.contains("-128") || lower.contains("cancel") {
        AppError::BiometricCancelled
    } else if msg.contains("-25300") || lower.contains("not found") {
        AppError::BiometricStale
    } else {
        AppError::Other(format!("无法读取钥匙串：{e}"))
    }
}
