//! 云端同步与本地离线备份 IPC 命令（M6 + v1.1）

use crate::app_config::{self, clamp_auto_sync_minutes};
use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::sync::backup::{self, BackupSummary};
use crate::sync::engine::{self, CloudRestorePreview, CloudSyncStatus, SnapshotMeta, SyncResult};
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
pub fn export_s3_config(dest_path: String, sync_config: S3Config) -> Result<()> {
    crate::sync::s3::write_s3_config_file(Path::new(&dest_path), &sync_config)
}

#[tauri::command]
pub fn import_s3_config(src_path: String) -> Result<S3Config> {
    let cfg = crate::sync::s3::read_s3_config_file(Path::new(&src_path))?;
    validate_s3_config(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub fn test_cloud_sync_config(state: State<AppState>, sync_config: S3Config) -> Result<u128> {
    let proxy = {
        let cfg = state.config.lock().unwrap();
        crate::net::for_cloud_sync(&cfg)
    };
    let client = S3Client::new_with_proxy(sync_config, proxy.as_ref())?;
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
                header_ready: false,
            });
        }
    };

    let client = s3_from_state(&state, sync_config)?;
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

    let client = s3_from_state(&state, sync_config)?;
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

    let client = s3_from_state(&state, sync_config)?;
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
    S3Client::from_app(sync_config, &state.config.lock().unwrap())
}

fn s3_from_state(state: &AppState, sync_config: S3Config) -> Result<S3Client> {
    S3Client::from_app(sync_config, &state.config.lock().unwrap())
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

fn validate_s3_config(cfg: &S3Config) -> Result<()> {
    if cfg.endpoint.trim().is_empty()
        || cfg.bucket.trim().is_empty()
        || cfg.access_key_id.trim().is_empty()
        || cfg.secret_access_key.trim().is_empty()
    {
        return Err(AppError::Invalid(
            "请填写完整的 Endpoint、Bucket、Access Key 与 Secret Key".into(),
        ));
    }
    Ok(())
}

/// 新设备：用恢复密钥验证云端工作空间并预览资产（不落盘）。
#[tauri::command]
pub fn preview_cloud_restore(
    sync_config: S3Config,
    recovery_key: String,
) -> Result<CloudRestorePreview> {
    validate_s3_config(&sync_config)?;
    if recovery_key.trim().is_empty() {
        return Err(AppError::Invalid("请输入恢复密钥".into()));
    }
    let client = S3Client::from_app(sync_config, &crate::app_config::AppConfig::load())?;
    engine::preview_cloud_restore(&client, &recovery_key)
}

/// 新设备：用恢复密钥重建工作空间并拉取云端数据，再设置本机访问密码。
#[tauri::command]
pub fn restore_from_cloud(
    app: AppHandle,
    state: State<AppState>,
    path: String,
    password: String,
    recovery_key: String,
    sync_config: S3Config,
    include_repos: Option<bool>,
) -> Result<SyncResult> {
    validate_s3_config(&sync_config)?;
    if path.trim().is_empty() {
        return Err(AppError::Invalid("请选择工作空间目录".into()));
    }
    if password.len() < 8 {
        return Err(AppError::Invalid("访问密码至少 8 位".into()));
    }
    if recovery_key.trim().is_empty() {
        return Err(AppError::Invalid("请输入恢复密钥".into()));
    }
    let root = std::path::PathBuf::from(&path);
    if crate::vault::Vault::exists(&root) {
        return Err(AppError::AlreadyInitialized(path));
    }

    let client = S3Client::from_app(sync_config.clone(), &state.config.lock().unwrap())?;
    let (vault, result) = engine::restore_from_cloud(
        &root,
        &password,
        &recovery_key,
        &client,
        include_repos.unwrap_or(false),
    )?;
    crate::commands::vault::adopt_unlocked_vault(&state, vault, path, Some(sync_config))?;
    crate::commands::vault::schedule_after_unlock(app);
    Ok(result)
}
