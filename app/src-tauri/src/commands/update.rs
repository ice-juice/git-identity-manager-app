//! 自更新 IPC 命令层：薄封装，动态注入 endpoints。

use crate::app_config::UpdateSource;
use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::platform;
use crate::update::{self, checker::UpdateCheckResult};
use serde_json::json;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn get_update_source(state: State<AppState>) -> Result<UpdateSource> {
    let cfg = state.config.lock().unwrap();
    Ok(update::source::effective_source(&cfg))
}

#[tauri::command]
pub fn save_update_source(state: State<AppState>, source: Option<UpdateSource>) -> Result<()> {
    if let Some(ref src) = source {
        let effective = UpdateSource::effective(Some(src));
        let _ = update::source::resolve_endpoints(&effective)?;
    }
    let mut cfg = state.config.lock().unwrap();
    cfg.update_source = source;
    cfg.save()
}

#[tauri::command]
pub fn get_auto_check_update(state: State<AppState>) -> Result<bool> {
    let cfg = state.config.lock().unwrap();
    Ok(cfg.auto_check_update)
}

#[tauri::command]
pub fn set_auto_check_update(state: State<AppState>, enabled: bool) -> Result<()> {
    let mut cfg = state.config.lock().unwrap();
    cfg.auto_check_update = enabled;
    cfg.save()
}

#[tauri::command]
pub async fn check_update(app: AppHandle, state: State<'_, AppState>) -> Result<UpdateCheckResult> {
    let (src, proxy) = {
        let cfg = state.config.lock().unwrap();
        (
            update::source::effective_source(&cfg),
            crate::net::effective(&cfg),
        )
    };
    let result = update::checker::check(&app, &src, proxy.as_ref()).await?;
    persist_last_check(&state);
    Ok(result)
}

#[tauri::command]
pub async fn download_and_install_update(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    if !platform::self_update_supported() {
        return Err(AppError::Invalid(
            "当前安装方式不支持应用内更新，请使用手动下载".into(),
        ));
    }
    if state.update_busy.swap(true, Ordering::SeqCst) {
        return Err(AppError::Other("正在下载更新，请稍候".into()));
    }
    let outcome = install_inner(&app, &state).await;
    state.update_busy.store(false, Ordering::SeqCst);
    outcome
}

#[tauri::command]
pub fn skip_update_version(state: State<AppState>, version: String) -> Result<()> {
    let version = version.trim().to_string();
    if version.is_empty() {
        return Err(AppError::Invalid("版本号不能为空".into()));
    }
    let mut cfg = state.config.lock().unwrap();
    cfg.skipped_update_version = Some(version);
    cfg.save()
}

#[tauri::command]
pub fn get_last_update_check(state: State<AppState>) -> Result<Option<String>> {
    let cfg = state.config.lock().unwrap();
    Ok(cfg.last_update_check_at.clone())
}

async fn install_inner(app: &AppHandle, state: &AppState) -> Result<()> {
    let (src, proxy) = {
        let cfg = state.config.lock().unwrap();
        (
            update::source::effective_source(&cfg),
            crate::net::effective(&cfg),
        )
    };
    let endpoints = update::checker::resolve_check_endpoints(&src, proxy.as_ref()).await?;
    let updater = update::checker::build_updater(app, endpoints, proxy.as_ref())?;

    let Some(update) = updater
        .check()
        .await
        .map_err(update::checker::map_updater_err)?
    else {
        return Err(AppError::Invalid("当前已是最新版本".into()));
    };
    if !update::checker::is_newer(&update.current_version, &update.version) {
        return Err(AppError::Invalid("不允许安装更低或相同版本".into()));
    }

    let _ = app.emit(
        "update-progress",
        json!({ "phase": "started", "downloaded": 0, "total": null }),
    );
    let mut downloaded = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit(
                    "update-progress",
                    json!({
                        "phase": "downloading",
                        "downloaded": downloaded,
                        "total": total
                    }),
                );
            },
            || {
                let _ = app.emit("update-progress", json!({ "phase": "finished" }));
            },
        )
        .await
        .map_err(update::checker::map_updater_err)?;
    app.restart();
}

fn persist_last_check(state: &AppState) {
    if let Ok(mut cfg) = state.config.lock() {
        cfg.last_update_check_at = Some(iso_now());
        let _ = cfg.save();
    }
}

fn iso_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
