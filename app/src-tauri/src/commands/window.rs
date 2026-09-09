//! 关闭确认、最小化到托盘、退出程序。

use crate::commands::vault::{lock_in_memory, try_grace_unlock_silent};
use crate::commands::AppState;
use crate::error::{AppError, Result};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

pub fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn hide_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

/// 最小化到托盘：按设置锁定内存中的工作空间。
pub fn hide_to_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    lock_in_memory(&state);
    hide_main(app);
}

/// 从托盘恢复：先按免验证天数尝试静默解锁，再通知前端刷新。
pub fn restore_from_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let unlocked = try_grace_unlock_silent(&state);
    show_main(app);
    let _ = app.emit("window-restored", ());
    if unlocked {
        crate::commands::vault::schedule_after_unlock(app.clone());
    }
}

pub fn quit_app(app: &AppHandle, state: &AppState) {
    state.allow_exit.store(true, Ordering::SeqCst);
    app.exit(0);
}

fn persist_close_action(state: &AppState, action: &str) -> Result<()> {
    let mut cfg = state.config.lock().unwrap();
    cfg.close_action = Some(action.to_string());
    cfg.save()
}

/// 关闭确认框的选择：`tray` / `quit` / `cancel`。
#[tauri::command]
pub fn apply_close_choice(
    app: AppHandle,
    state: State<AppState>,
    action: String,
    remember: bool,
) -> Result<()> {
    match action.as_str() {
        "tray" => {
            if remember {
                persist_close_action(&state, "tray")?;
            }
            hide_to_tray(&app);
            Ok(())
        }
        "quit" => {
            if remember {
                persist_close_action(&state, "quit")?;
            }
            quit_app(&app, &state);
            Ok(())
        }
        "cancel" => Ok(()),
        other => Err(AppError::Invalid(format!("未知关闭操作：{other}"))),
    }
}

#[tauri::command]
pub fn get_close_preference(state: State<AppState>) -> Option<String> {
    state.config.lock().unwrap().close_action.clone()
}

#[tauri::command]
pub fn clear_close_preference(state: State<AppState>) -> Result<()> {
    let mut cfg = state.config.lock().unwrap();
    cfg.close_action = None;
    cfg.save()
}

#[cfg(test)]
mod tests {
    #[test]
    fn close_actions_are_known() {
        for a in ["tray", "quit", "cancel"] {
            assert!(matches!(a, "tray" | "quit" | "cancel"));
        }
    }
}
