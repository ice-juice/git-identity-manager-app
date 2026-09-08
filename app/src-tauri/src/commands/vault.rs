//! vault 相关 IPC 命令。

use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::vault::header::KdfParams;
use crate::vault::kdf::{calibrate, DEFAULT_TARGET_MS};
use crate::vault::Vault;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub workspace_path: Option<String>,
    pub workspace_id: Option<String>,
    pub auto_lock_minutes: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitResult {
    pub recovery_key: String,
    pub workspace_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCheck {
    /// 命中同步盘时的中文告警，None 表示安全。
    pub warning: Option<String>,
}

/// 查询当前状态（前端启动时首先调用）。
#[tauri::command]
pub fn vault_status(state: State<AppState>) -> VaultStatus {
    let cfg = state.config.lock().unwrap();
    let vault = state.vault.lock().unwrap();
    let initialized = cfg
        .workspace_path
        .as_ref()
        .map(|p| Vault::exists(&PathBuf::from(p)))
        .unwrap_or(false);
    VaultStatus {
        initialized,
        unlocked: vault.as_ref().map(|v| v.is_unlocked()).unwrap_or(false),
        workspace_path: cfg.workspace_path.clone(),
        workspace_id: vault.as_ref().map(|v| v.workspace_id().to_string()),
        auto_lock_minutes: cfg.auto_lock_minutes,
    }
}

/// 检测路径是否位于常见同步盘目录下（应避免）。
#[tauri::command]
pub fn check_workspace_path(path: String) -> PathCheck {
    let lower = path.to_lowercase();
    let hits = [
        ("onedrive", "OneDrive"),
        ("dropbox", "Dropbox"),
        ("google drive", "Google Drive"),
        ("googledrive", "Google Drive"),
        ("icloud", "iCloud"),
        ("坚果云", "坚果云"),
        ("nutstore", "坚果云"),
        ("百度网盘", "百度网盘"),
        ("baidunetdisk", "百度网盘"),
    ];
    for (needle, label) in hits {
        if lower.contains(needle) {
            return PathCheck {
                warning: Some(format!(
                    "该路径疑似位于「{}」同步盘内。端到端加密备份应走应用内的云同步通道，请勿让第三方同步盘接管整个工作空间目录。",
                    label
                )),
            };
        }
    }
    PathCheck { warning: None }
}

/// 初始化工作空间。会做 Argon2id 运行时标定（略慢，属正常）。
#[tauri::command]
pub fn vault_init(state: State<AppState>, path: String, password: String) -> Result<InitResult> {
    let root = PathBuf::from(&path);
    let kdf: KdfParams = calibrate(DEFAULT_TARGET_MS);
    let (vault, recovery_key) = Vault::init(&root, &password, kdf)?;
    let workspace_id = vault.workspace_id().to_string();

    // 记录工作空间路径（非机密）。
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.workspace_path = Some(path);
        cfg.save()?;
    }
    *state.vault.lock().unwrap() = Some(vault);
    state.unlock_guard.lock().unwrap().reset();

    Ok(InitResult {
        recovery_key,
        workspace_id,
    })
}

fn ensure_loaded(state: &State<AppState>) -> Result<()> {
    let cfg_path = {
        let cfg = state.config.lock().unwrap();
        cfg.workspace_path.clone()
    };
    let path = cfg_path.ok_or(AppError::NotInitialized)?;
    let mut vault = state.vault.lock().unwrap();
    if vault.is_none() {
        *vault = Some(Vault::load(&PathBuf::from(path))?);
    }
    Ok(())
}

/// 用访问密码解锁（带限速）。
#[tauri::command]
pub fn vault_unlock(state: State<AppState>, password: String) -> Result<()> {
    {
        let guard = state.unlock_guard.lock().unwrap();
        if guard.remaining_ms() > 0 {
            return Err(AppError::RateLimited);
        }
    }
    ensure_loaded(&state)?;
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::NotInitialized)?;
    match v.unlock_with_password(&password) {
        Ok(()) => {
            state.unlock_guard.lock().unwrap().reset();
            Ok(())
        }
        Err(e) => {
            state.unlock_guard.lock().unwrap().record_failure();
            Err(e)
        }
    }
}

/// 用恢复密钥解锁（忘记密码/换机）。
#[tauri::command]
pub fn vault_unlock_recovery(state: State<AppState>, recovery_key: String) -> Result<()> {
    ensure_loaded(&state)?;
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::NotInitialized)?;
    v.unlock_with_recovery(&recovery_key)
}

/// 锁定：清零内存中的 MK。
#[tauri::command]
pub fn vault_lock(state: State<AppState>) {
    if let Some(v) = state.vault.lock().unwrap().as_mut() {
        v.lock();
    }
}

/// 修改访问密码（需正确旧密码）。
#[tauri::command]
pub fn change_password(state: State<AppState>, old_password: String, new_password: String) -> Result<()> {
    ensure_loaded(&state)?;
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::NotInitialized)?;
    v.change_password(&old_password, &new_password)
}

/// 轮换恢复密钥（需已解锁），返回新恢复密钥。
#[tauri::command]
pub fn rotate_recovery_key(state: State<AppState>) -> Result<InitResult> {
    let mut vault = state.vault.lock().unwrap();
    let v = vault.as_mut().ok_or(AppError::Locked)?;
    let recovery_key = v.rotate_recovery_key()?;
    let workspace_id = v.workspace_id().to_string();
    Ok(InitResult {
        recovery_key,
        workspace_id,
    })
}
