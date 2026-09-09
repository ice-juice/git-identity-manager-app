//! 云端同步与本地离线备份 IPC 命令（M6 + v1.1）

use crate::app_config::{self, clamp_auto_sync_minutes};
use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::sync::backup::{self, BackupSummary};
use crate::sync::engine::{self, CloudSyncStatus, SnapshotMeta, SyncResult};
use crate::sync::s3::{S3Client, S3Config};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};

// ==================== M6 本地加密备份导出/导入 ====================

#[tauri::command]
pub fn export_vault_backup(
    state: State<AppState>,
    dest_path: String,
    password: String,
) -> Result<BackupSummary> {
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;
    backup::export_backup(vault, Path::new(&dest_path), &password)
}

#[tauri::command]
pub fn inspect_vault_backup(
    src_path: String,
    password: String,
) -> Result<BackupSummary> {
    let payload = backup::inspect_backup(Path::new(&src_path), &password)?;
    Ok(BackupSummary {
        workspace_id: payload.workspace_id,
        created_at: payload.created_at,
        identity_count: payload.data.identities.len(),
        key_count: payload.data.keys.len(),
        repo_count: payload.data.repos.len(),
        has_github_pat: payload.secrets.github_pat.is_some(),
    })
}

#[tauri::command]
pub fn import_vault_backup(
    app: AppHandle,
    state: State<AppState>,
    src_path: String,
    password: String,
    merge: bool,
) -> Result<BackupSummary> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;
    let summary = backup::import_backup(vault, Path::new(&src_path), &password, merge)?;
    drop(vault_guard);
    crate::sync::scheduler::kick_publish(app);
    Ok(summary)
}

// ==================== v1.1 云端同步 ====================

#[tauri::command]
pub fn get_cloud_sync_config(state: State<AppState>) -> Result<Option<S3Config>> {
    let config = state.config.lock().unwrap();
    Ok(config.cloud_sync.clone())
}

#[tauri::command]
pub fn save_cloud_sync_config(
    state: State<AppState>,
    sync_config: Option<S3Config>,
) -> Result<()> {
    let mut config = state.config.lock().unwrap();
    config.cloud_sync = sync_config;
    config.save()
}

#[tauri::command]
pub fn test_cloud_sync_config(sync_config: S3Config) -> Result<u128> {
    let client = S3Client::new(sync_config)?;
    client.test_connection()
}

#[tauri::command]
pub fn get_cloud_sync_status(state: State<AppState>) -> Result<CloudSyncStatus> {
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;

    let sync_config = {
        let config = state.config.lock().unwrap();
        config.cloud_sync.clone()
    };

    let sync_config = match sync_config {
        Some(c) => c,
        None => {
            let data = crate::store::load_data(vault)?;
            return Ok(CloudSyncStatus {
                remote_exists: false,
                remote_updated_at: None,
                remote_workspace_id: None,
                local_identity_count: data.identities.len(),
                local_key_count: data.keys.len(),
                local_repo_count: data.repos.len(),
                status: "unconfigured".into(),
            });
        }
    };

    let client = S3Client::new(sync_config)?;
    engine::get_sync_status(vault, &client)
}

#[tauri::command]
pub fn cloud_sync_push(state: State<AppState>) -> Result<SyncResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;

    let sync_config = {
        let config = state.config.lock().unwrap();
        config
            .cloud_sync
            .clone()
            .ok_or_else(|| AppError::Invalid("尚未配置云存储连接参数".into()))?
    };

    let client = S3Client::new(sync_config)?;
    engine::push_to_cloud(vault, &client)
}

#[tauri::command]
pub fn cloud_sync_pull(state: State<AppState>) -> Result<SyncResult> {
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;

    let sync_config = {
        let config = state.config.lock().unwrap();
        config
            .cloud_sync
            .clone()
            .ok_or_else(|| AppError::Invalid("尚未配置云存储连接参数".into()))?
    };

    let client = S3Client::new(sync_config)?;
    engine::pull_from_cloud(vault, &client)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoSyncSettings {
    pub minutes: u32,
    pub last_auto_sync_at: Option<String>,
    pub last_auto_sync_message: Option<String>,
    pub default_minutes: u32,
}

#[tauri::command]
pub fn get_auto_sync_settings(state: State<AppState>) -> AutoSyncSettings {
    let cfg = state.config.lock().unwrap();
    AutoSyncSettings {
        minutes: cfg.auto_sync_minutes,
        last_auto_sync_at: cfg.last_auto_sync_at.clone(),
        last_auto_sync_message: cfg.last_auto_sync_message.clone(),
        default_minutes: app_config::DEFAULT_AUTO_SYNC_MINUTES,
    }
}

#[tauri::command]
pub fn set_auto_sync_minutes(state: State<AppState>, minutes: u32) -> Result<u32> {
    let minutes = clamp_auto_sync_minutes(minutes);
    let mut cfg = state.config.lock().unwrap();
    cfg.auto_sync_minutes = minutes;
    cfg.save()?;
    Ok(minutes)
}

fn require_client(state: &AppState) -> Result<S3Client> {
    let sync_config = state
        .config
        .lock()
        .unwrap()
        .cloud_sync
        .clone()
        .ok_or_else(|| AppError::Invalid("尚未配置云存储连接参数".into()))?;
    S3Client::new(sync_config)
}

#[tauri::command]
pub fn list_cloud_snapshots(state: State<AppState>) -> Result<Vec<SnapshotMeta>> {
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;
    let client = require_client(&state)?;
    engine::list_snapshots(vault, &client)
}

#[tauri::command]
pub fn restore_cloud_snapshot(state: State<AppState>, snapshot_id: String) -> Result<SyncResult> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault_guard = state.vault.lock().unwrap();
    let vault = vault_guard.as_ref().ok_or(AppError::Locked)?;
    let client = require_client(&state)?;
    let result = engine::restore_snapshot(vault, &client, &snapshot_id)?;
    // 直接推送恢复结果，避免再拉一次把当前云端较新数据合并回来。
    if let Ok(pushed) = engine::push_to_cloud(vault, &client) {
        return Ok(SyncResult {
            message: format!("{}；已推送到云端：{}", result.message, pushed.message),
            ..pushed
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn run_auto_sync_now(app: AppHandle) -> Result<Option<SyncResult>> {
    crate::sync::scheduler::run(&app, "manual")
}
