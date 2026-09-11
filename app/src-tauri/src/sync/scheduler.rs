//! 打开时拉取 + 按间隔自动「先拉后推」。

use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::sync::engine::{self, SyncResult};
use crate::sync::s3::S3Client;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const STARTUP_DEDUP: Duration = Duration::from_secs(60);

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("gam-autosync".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            tick(&app);
        })
        .ok();
}

/// 解锁或从托盘恢复后触发：即使关闭了定时同步，打开时仍拉取一次。
pub fn kick_startup(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let _ = run(&app, "startup");
    });
}

/// 本地编辑保存后立刻合并并推送，降低多端改同一条记录的冲突窗口。
pub fn kick_publish(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        let state = app.state::<AppState>();
        if state.auto_sync_busy.load(Ordering::SeqCst) {
            state.pending_edit_publish.store(true, Ordering::SeqCst);
            return;
        }
        match run(&app, "edit") {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                let configured = state
                    .config
                    .lock()
                    .ok()
                    .and_then(|c| c.cloud_sync.clone())
                    .is_some();
                if configured {
                    state.pending_edit_publish.store(true, Ordering::SeqCst);
                }
            }
        }
    });
}

fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.pending_edit_publish.swap(false, Ordering::SeqCst) {
        let _ = run(app, "edit");
        return;
    }
    let minutes = match state.config.lock() {
        Ok(cfg) => cfg.auto_sync_minutes,
        Err(_) => return,
    };
    if minutes == 0 {
        return;
    }
    let due = match state.last_periodic_sync.lock() {
        Ok(last) => match *last {
            Some(t) => t.elapsed() >= Duration::from_secs(u64::from(minutes) * 60),
            None => false,
        },
        Err(_) => return,
    };
    if due {
        let _ = run(app, "periodic");
    }
}

pub fn run(app: &AppHandle, trigger: &str) -> Result<Option<SyncResult>> {
    let state = app.state::<AppState>();
    if state.auto_sync_busy.swap(true, Ordering::SeqCst) {
        return Ok(None);
    }
    let outcome = run_locked(&state, trigger);
    state.auto_sync_busy.store(false, Ordering::SeqCst);

    match &outcome {
        Ok(Some(res)) => {
            persist_sync_note(&state, &res.synced_at, &res.message);
            let _ = app.emit(
                "cloud-auto-sync",
                serde_json::json!({
                    "ok": true,
                    "trigger": trigger,
                    "message": res.message,
                    "syncedAt": res.synced_at,
                }),
            );
        }
        Ok(None) => {}
        Err(e) => {
            persist_sync_note(&state, &engine_now(), &e.to_string());
            let _ = app.emit(
                "cloud-auto-sync",
                serde_json::json!({
                    "ok": false,
                    "trigger": trigger,
                    "message": e.to_string(),
                }),
            );
        }
    }
    outcome
}

fn run_locked(state: &AppState, trigger: &str) -> Result<Option<SyncResult>> {
    if trigger == "startup" {
        if let Ok(last) = state.last_periodic_sync.lock() {
            if last.is_some_and(|t| t.elapsed() < STARTUP_DEDUP) {
                return Ok(None);
            }
        }
    }

    let (sync_config, app_cfg) = {
        let cfg = state.config.lock().map_err(|_| AppError::Other("配置锁损坏".into()))?;
        (cfg.cloud_sync.clone(), cfg.clone())
    };
    let Some(sync_config) = sync_config else {
        return Ok(None);
    };

    let vault = {
        let guard = state.vault.lock().map_err(|_| AppError::Other("工作空间锁损坏".into()))?;
        let Some(v) = guard.as_ref() else {
            return Ok(None);
        };
        if !v.is_unlocked() {
            return Ok(None);
        }
        v.clone()
    };

    let client = S3Client::from_app(sync_config, &app_cfg)?;
    let result = if trigger == "edit" {
        engine::publish_after_edit(&vault, &client)?
    } else {
        engine::pull_then_maybe_push(&vault, &client)?
    };
    crate::commands::sync::remember_view_after_sync(&vault, &client);
    if let Ok(mut last) = state.last_periodic_sync.lock() {
        *last = Some(Instant::now());
    }
    Ok(Some(result))
}

fn persist_sync_note(state: &AppState, at: &str, message: &str) {
    if let Ok(mut cfg) = state.config.lock() {
        cfg.last_auto_sync_at = Some(at.to_string());
        cfg.last_auto_sync_message = Some(message.to_string());
        let _ = cfg.save();
    }
}

fn engine_now() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".into())
}
