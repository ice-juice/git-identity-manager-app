//! 指纹解锁与隐私重认证 IPC。

use crate::biometric;
use crate::commands::vault::{ensure_loaded, resolve_workspace_id};
use crate::commands::{recover_lock, refresh_reveal_grace, AppState};
use crate::error::{AppError, Result};
use tauri::{AppHandle, State};

#[tauri::command(async)]
pub fn biometric_status(state: State<'_, AppState>) -> biometric::BiometricStatus {
    let _ = ensure_loaded(&state);
    let cfg = recover_lock(&state.config).clone();
    let workspace_id = resolve_workspace_id(&state);
    // 不持锁调用系统 API，避免 WinRT 重入把配置锁弄脏。
    biometric::status(workspace_id.as_deref(), &cfg)
}

#[tauri::command(async)]
pub fn biometric_enable(app: AppHandle, state: State<'_, AppState>, password: String) -> Result<()> {
    biometric::bind_window(&app);
    ensure_loaded(&state)?;
    let (workspace_id, mk) = {
        let vault = recover_lock(&state.vault);
        let v = vault.as_ref().ok_or(AppError::Locked)?;
        if !v.is_unlocked() {
            return Err(AppError::Locked);
        }
        v.verify_password(&password)?;
        (v.workspace_id().to_string(), v.master_key()?.clone())
    };
    biometric::enable(&workspace_id, &mk)?;
    let mut cfg = recover_lock(&state.config);
    cfg.biometric_unlock_enabled = true;
    cfg.biometric_reveal_enabled = true;
    cfg.save()
}

#[tauri::command(async)]
pub fn biometric_disable(state: State<'_, AppState>) -> Result<()> {
    biometric::disable()?;
    let mut cfg = recover_lock(&state.config);
    cfg.biometric_unlock_enabled = false;
    cfg.save()
}

#[tauri::command(async)]
pub fn reveal_authorize_biometric(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    biometric::bind_window(&app);
    {
        let vault = recover_lock(&state.vault);
        let v = vault.as_ref().ok_or(AppError::Locked)?;
        if !v.is_unlocked() {
            return Err(AppError::Locked);
        }
    }
    let (enabled, reveal_enabled) = {
        let cfg = recover_lock(&state.config);
        (cfg.biometric_unlock_enabled, cfg.biometric_reveal_enabled)
    };
    if !enabled || !reveal_enabled {
        return Err(AppError::Invalid("未开启指纹重认证".into()));
    }
    let workspace_id = resolve_workspace_id(&state).ok_or(AppError::NotInitialized)?;
    if !biometric::is_enrolled_for(&workspace_id) {
        return Err(AppError::BiometricStale);
    }
    biometric::verify_presence("确认查看敏感信息")?;
    refresh_reveal_grace(&state);
    Ok(())
}

#[tauri::command]
pub fn set_biometric_reveal_enabled(state: State<AppState>, enabled: bool) -> Result<()> {
    let mut cfg = recover_lock(&state.config);
    cfg.biometric_reveal_enabled = enabled;
    cfg.save()
}

#[tauri::command]
pub fn set_biometric_reveal_secret(state: State<AppState>, enabled: bool) -> Result<()> {
    let mut cfg = recover_lock(&state.config);
    cfg.biometric_reveal_secret = enabled;
    cfg.save()
}
