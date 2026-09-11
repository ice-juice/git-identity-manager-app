//! Linux 首期不提供指纹解锁（无统一硬件密钥库）。

use crate::error::{AppError, Result};

use super::BiometricAvailability;

pub fn availability() -> BiometricAvailability {
    BiometricAvailability {
        available: false,
        kind: "none",
        strong: false,
    }
}

pub fn enroll(_key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    Err(unsupported())
}

pub fn derive(_key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    Err(unsupported())
}

pub fn verify_presence(_prompt: &str) -> Result<()> {
    Err(unsupported())
}

pub fn remove(_key_ref: &str) -> Result<()> {
    Ok(())
}

fn unsupported() -> AppError {
    AppError::Invalid("当前系统不支持指纹解锁".into())
}
