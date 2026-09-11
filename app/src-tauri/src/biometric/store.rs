//! 本机指纹信封 `biometric.env`：不进 vault.json，不云同步。

use crate::error::{AppError, Result};
use crate::vault::header::Envelope;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const FILE_NAME: &str = "biometric.env";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiometricEnvFile {
    pub version: u32,
    pub platform: String,
    pub workspace_id: String,
    pub key_ref: String,
    pub challenge: String,
    pub envelope: Envelope,
    pub created_at: String,
}

pub fn file_path() -> PathBuf {
    crate::app_config::config_dir().join(FILE_NAME)
}

pub fn current_platform() -> &'static str {
    #[cfg(windows)]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        "linux"
    }
}

pub fn encode_challenge(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn decode_challenge(s: &str) -> Result<Vec<u8>> {
    B64.decode(s)
        .map_err(|_| AppError::Serde("biometric.env challenge 不是合法 base64".into()))
}

pub fn load() -> Result<BiometricEnvFile> {
    let path = file_path();
    if !path.is_file() {
        return Err(AppError::NotInitialized);
    }
    let raw = std::fs::read_to_string(&path)?;
    let file: BiometricEnvFile = serde_json::from_str(&raw)?;
    if file.version != VERSION {
        return Err(AppError::Invalid("不支持的指纹信封版本".into()));
    }
    Ok(file)
}

pub fn save(file: &BiometricEnvFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file)?;
    crate::vault::atomic_write(&file_path(), json.as_bytes())
}

pub fn clear() {
    let _ = std::fs::remove_file(file_path());
}

pub fn binding_matches(file: &BiometricEnvFile, workspace_id: &str) -> bool {
    file.workspace_id == workspace_id && file.platform == current_platform()
}

/// 校验工作空间与平台；不匹配则清文件并返回错误。
pub fn load_for_workspace(workspace_id: &str) -> Result<BiometricEnvFile> {
    let file = match load() {
        Ok(f) => f,
        Err(AppError::NotInitialized) => return Err(AppError::BiometricStale),
        Err(e) => return Err(e),
    };
    if !binding_matches(&file, workspace_id) {
        clear();
        return Err(AppError::BiometricStale);
    }
    Ok(file)
}

pub fn is_enrolled_for(workspace_id: &str) -> bool {
    load_for_workspace(workspace_id).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::header::Envelope;

    fn sample(workspace_id: &str) -> BiometricEnvFile {
        BiometricEnvFile {
            version: VERSION,
            platform: current_platform().into(),
            workspace_id: workspace_id.into(),
            key_ref: "gam-vault-ws-1".into(),
            challenge: encode_challenge(&[1, 2, 3, 4]),
            envelope: Envelope {
                salt: "c2FsdA==".into(),
                nonce: "bm9uY2U=".into(),
                wrapped_key: "d3JhcA==".into(),
            },
            created_at: "2026-09-10T00:00:00Z".into(),
        }
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let file = sample("ws-1");
        let json = serde_json::to_string(&file).unwrap();
        let back: BiometricEnvFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, VERSION);
        assert_eq!(back.workspace_id, "ws-1");
        assert_eq!(back.key_ref, file.key_ref);
        assert_eq!(decode_challenge(&back.challenge).unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(back.envelope.wrapped_key, "d3JhcA==");
    }

    #[test]
    fn workspace_mismatch_rejected() {
        let file = sample("ws-expected");
        assert!(binding_matches(&file, "ws-expected"));
        assert!(!binding_matches(&file, "ws-other"));
        let mut other_os = file.clone();
        other_os.platform = "not-this-os".into();
        assert!(!binding_matches(&other_os, "ws-expected"));
    }
}
