//! 安全自查清单 IPC：同步配置读取，无启动扫描、无事件。

use crate::commands::{recover_lock, AppState};
use crate::security::{self, SecurityChecklist};
use tauri::State;

#[tauri::command(async)]
pub fn security_checklist(state: State<'_, AppState>) -> SecurityChecklist {
    let cfg = recover_lock(&state.config).clone();
    security::build_checklist(&cfg)
}
