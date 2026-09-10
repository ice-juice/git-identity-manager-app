//! 启动后延迟静默检查更新，错开云同步拉取。

use crate::commands::AppState;
use crate::update;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const STARTUP_DELAY: Duration = Duration::from_secs(5);

pub fn start(app: AppHandle) {
    spawn_delayed(app, "gam-update-check");
}

/// 解锁后再次静默检查：启动 5s 时用户往往还在锁屏页。
pub fn kick_after_unlock(app: AppHandle) {
    spawn_delayed(app, "gam-update-check-unlock");
}

fn spawn_delayed(app: AppHandle, name: &str) {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            std::thread::sleep(STARTUP_DELAY);
            silent_check(&app);
        })
        .ok();
}

fn silent_check(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let (enabled, skipped, src, proxy) = match state.config.lock() {
        Ok(cfg) => {
            if !cfg.auto_check_update {
                return;
            }
            (
                true,
                cfg.skipped_update_version.clone(),
                update::source::effective_source(&cfg),
                crate::net::effective(&cfg),
            )
        }
        Err(_) => return,
    };
    if !enabled {
        return;
    }
    if !is_unlocked_with_window(app, &state) {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match update::checker::check(&app, &src, proxy.as_ref()).await {
            Ok(result) => {
                persist_last_check(&app);
                if should_notify(
                    result.available,
                    result.latest_version.as_deref(),
                    skipped.as_deref(),
                ) && is_unlocked_with_window(&app, &app.state::<AppState>())
                {
                    let _ = app.emit("update-available", &result);
                }
            }
            Err(e) => {
                log::info!("静默检查更新失败（忽略）：{e}");
            }
        }
    });
}

fn is_unlocked_with_window(app: &AppHandle, state: &AppState) -> bool {
    let unlocked = state
        .vault
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|v| v.is_unlocked()))
        .unwrap_or(false);
    if !unlocked {
        return false;
    }
    app.get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(true))
        .unwrap_or(false)
}

pub fn should_notify(available: bool, latest: Option<&str>, skipped: Option<&str>) -> bool {
    if !available {
        return false;
    }
    match (latest, skipped) {
        (Some(latest), Some(skipped)) => latest != skipped,
        _ => true,
    }
}

fn persist_last_check(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut cfg) = state.config.lock() {
            cfg.last_update_check_at = Some(
                time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
            );
            let _ = cfg.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_delay_is_five_seconds() {
        assert_eq!(STARTUP_DELAY, Duration::from_secs(5));
    }

    #[test]
    fn notify_only_when_newer_and_not_skipped() {
        assert!(should_notify(true, Some("1.2.0"), None));
        assert!(should_notify(true, Some("1.2.0"), Some("1.1.0")));
        assert!(!should_notify(true, Some("1.2.0"), Some("1.2.0")));
        assert!(!should_notify(false, Some("1.2.0"), None));
    }
}
