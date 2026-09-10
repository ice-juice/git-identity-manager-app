//! 安全自查清单 IPC：同步配置读取，无启动扫描、无事件。

use crate::commands::AppState;
use crate::security::{self, SecurityChecklist};
use tauri::State;

#[tauri::command]
pub fn security_checklist(state: State<AppState>) -> SecurityChecklist {
    let cfg = state.config.lock().unwrap().clone();
    security::build_checklist(&cfg)
}
