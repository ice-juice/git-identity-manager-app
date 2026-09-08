//! Tauri 应用入口：注册状态与命令。

pub mod app_config;
pub mod commands;
pub mod error;
pub mod importer;
pub mod model;
pub mod ssh;
pub mod store;
pub mod sys;
pub mod vault;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::vault::vault_status,
            commands::vault::check_workspace_path,
            commands::vault::vault_init,
            commands::vault::vault_unlock,
            commands::vault::vault_unlock_recovery,
            commands::vault::vault_lock,
            commands::vault::change_password,
            commands::vault::rotate_recovery_key,
            commands::assets::read_ssh_config,
            commands::assets::scan_keys,
            commands::assets::detect_toolchain,
            commands::assets::list_keys,
            commands::assets::list_identities,
            commands::assets::import_key,
            commands::assets::import_key_from_path,
            commands::assets::test_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
